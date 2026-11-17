#!/usr/bin/env python3
"""Build a verified local Joppa export archive from raw database streams and Spaces files."""
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timezone
import hashlib
import hmac
import json
import os
from pathlib import Path
import shutil
import sys
import tarfile
import time
import urllib.error
import urllib.parse
import urllib.request

STORAGE_KEYS = (
    "SPACES_BUCKET",
    "SPACES_ENDPOINT",
    "SPACES_REGION",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
)


# Build URL openers on the main thread to avoid macOS _scproxy thread contention.
DIRECT_OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))
DEFAULT_OPENER = urllib.request.build_opener()


def open_url(url: str):
    host = urllib.parse.urlparse(url).hostname
    opener = DIRECT_OPENER if host in ("127.0.0.1", "localhost", "::1") else DEFAULT_OPENER
    return opener.open(urllib.request.Request(url, method="GET"), timeout=120)
def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def parse_env_text(text: str) -> dict[str, str]:
    values: dict[str, str] = {}
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        value = value.strip()
        if len(value) >= 2 and value[0] == value[-1] and value[0] in ("'", '"'):
            value = value[1:-1]
        if key in STORAGE_KEYS:
            values[key] = value
    missing = [key for key in STORAGE_KEYS if not values.get(key)]
    if missing:
        raise RuntimeError(f"Missing Spaces configuration keys: {', '.join(missing)}")
    return values


def s3_encode(text: str, keep_slash: bool = False) -> str:
    safe = "-._~/" if keep_slash else "-._~"
    return urllib.parse.quote(text, safe=safe)


def presign_get(cfg: dict[str, str], object_key: str, ttl: int = 900) -> str:
    endpoint = urllib.parse.urlparse(cfg["SPACES_ENDPOINT"])
    if endpoint.scheme not in ("https", "http") or not endpoint.netloc:
        raise RuntimeError("Invalid SPACES_ENDPOINT")
    if endpoint.scheme == "http" and endpoint.hostname not in ("127.0.0.1", "localhost", "::1"):
        raise RuntimeError("SPACES_ENDPOINT must use HTTPS")
    now = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    date = now[:8]
    region = cfg["SPACES_REGION"]
    scope = f"{date}/{region}/s3/aws4_request"
    path = f"/{cfg['SPACES_BUCKET']}/{object_key.lstrip('/')}"
    params = [
        ("X-Amz-Algorithm", "AWS4-HMAC-SHA256"),
        ("X-Amz-Credential", f"{cfg['AWS_ACCESS_KEY_ID']}/{scope}"),
        ("X-Amz-Date", now),
        ("X-Amz-Expires", str(ttl)),
        ("X-Amz-SignedHeaders", "host"),
    ]
    params.sort()
    query = "&".join(f"{s3_encode(k)}={s3_encode(v)}" for k, v in params)
    canonical = (
        f"GET\n{s3_encode(path, keep_slash=True)}\n{query}\n"
        f"host:{endpoint.netloc}\n\nhost\nUNSIGNED-PAYLOAD"
    )
    string_to_sign = (
        f"AWS4-HMAC-SHA256\n{now}\n{scope}\n"
        f"{hashlib.sha256(canonical.encode('utf-8')).hexdigest()}"
    )
    signing_key = hmac.new(
        f"AWS4{cfg['AWS_SECRET_ACCESS_KEY']}".encode("utf-8"),
        date.encode("utf-8"),
        hashlib.sha256,
    ).digest()
    for part in (region, "s3", "aws4_request"):
        signing_key = hmac.new(signing_key, part.encode("utf-8"), hashlib.sha256).digest()
    signature = hmac.new(
        signing_key, string_to_sign.encode("utf-8"), hashlib.sha256
    ).hexdigest()
    return f"{endpoint.scheme}://{endpoint.netloc}{s3_encode(path, keep_slash=True)}?{query}&X-Amz-Signature={signature}"


class WriterPool:
    """Keep a bounded set of append handles open while demultiplexing JSONL rows."""

    def __init__(self, limit: int = 24):
        self.limit = limit
        self.handles: dict[Path, object] = {}

    def write_line(self, path: Path, line: str) -> None:
        handle = self.handles.get(path)
        if handle is None:
            if len(self.handles) >= self.limit:
                oldest_path, oldest = next(iter(self.handles.items()))
                oldest.close()
                del self.handles[oldest_path]
            path.parent.mkdir(parents=True, exist_ok=True)
            handle = path.open("a", encoding="utf-8")
            self.handles[path] = handle
        handle.write(line if line.endswith("\n") else line + "\n")

    def close_all(self) -> None:
        for handle in self.handles.values():
            handle.close()
        self.handles.clear()


def valid_token(value: str, length: int | None = None) -> bool:
    if not value or any(ch not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_." for ch in value):
        return False
    return length is None or len(value) == length


def valid_sha256(value: str) -> bool:
    return len(value) == 64 and all(ch in "0123456789abcdef" for ch in value)


def fetch_blob(
    cfg: dict[str, str],
    cache_root: Path,
    workspace: str,
    digest_hex: str,
    active: bool,
) -> tuple[str, Path | None, int]:
    cache_file = cache_root / workspace / digest_hex
    if cache_file.is_file():
        if sha256_file(cache_file) == digest_hex:
            return ("cached", cache_file, cache_file.stat().st_size)
        cache_file.unlink(missing_ok=True)

    object_key = f"workspaces/{workspace}/{digest_hex}"
    last_error: Exception | None = None
    for attempt in range(3):
        url = presign_get(cfg, object_key)
        try:
            with open_url(url) as response:
                cache_file.parent.mkdir(parents=True, exist_ok=True)
                tmp_file = cache_file.with_name(f".{digest_hex}.{os.getpid()}.tmp")
                hasher = hashlib.sha256()
                size = 0
                try:
                    with tmp_file.open("wb") as out:
                        while chunk := response.read(1024 * 1024):
                            hasher.update(chunk)
                            out.write(chunk)
                            size += len(chunk)
                    os.chmod(tmp_file, 0o600)
                    actual = hasher.hexdigest()
                    if actual != digest_hex:
                        raise RuntimeError(
                            f"SHA-256 mismatch for {object_key}: expected {digest_hex}, got {actual}"
                        )
                    os.replace(tmp_file, cache_file)
                except Exception:
                    tmp_file.unlink(missing_ok=True)
                    raise
                return ("downloaded", cache_file, size)
        except urllib.error.HTTPError as err:
            if err.code in (403, 404) and not active:
                return ("swept", None, 0)
            if err.code in (400, 401, 403, 404):
                raise RuntimeError(
                    f"Spaces refused active attachment {object_key} with HTTP {err.code}"
                ) from err
            last_error = RuntimeError(f"Spaces HTTP {err.code} for {object_key}")
        except urllib.error.URLError as err:
            last_error = RuntimeError(f"Spaces network error for {object_key}: {err.reason}")
        if attempt < 2:
            time.sleep(1.0 * (attempt + 1))
    assert last_error is not None
    raise last_error


def build_bundle(raw_dir: Path, output_dir: Path, stamp: str, storage_cfg: dict[str, str]) -> dict:
    bundle_name = f"joppa-export-{stamp}"
    stage_parent = raw_dir.parent / "bundle"
    bundle_root = stage_parent / bundle_name
    bundle_root.mkdir(parents=True, exist_ok=True)

    pg_dir = bundle_root / "postgres"
    pg_dir.mkdir(parents=True, exist_ok=True)
    dump_src = raw_dir / "joppa.dump"
    if not dump_src.is_file() or dump_src.stat().st_size == 0:
        raise RuntimeError("Database dump file is missing or empty")
    dump_dst = pg_dir / "joppa.dump"
    shutil.move(str(dump_src), str(dump_dst))
    dump_sha256 = sha256_file(dump_dst)
    dump_size = dump_dst.stat().st_size

    workspaces: dict[str, dict] = {}
    with (raw_dir / "raw_workspaces.jsonl").open("r", encoding="utf-8") as handle:
        for line in handle:
            if not line.strip():
                continue
            row = json.loads(line)
            ws = row["id"]
            if not valid_token(ws):
                raise RuntimeError(f"Invalid workspace ID in export: {ws!r}")
            workspaces[ws] = {
                "id": ws,
                "created_at": row["created_at"],
                "aliases": row.get("aliases") or [],
                "meta": {},
                "tree_order": {},
                "counts": {
                    "objects": 0,
                    "present_objects": 0,
                    "archived_objects": 0,
                    "revisions": 0,
                    "notes": 0,
                    "state_entries": 0,
                    "test_reports": 0,
                    "test_results": 0,
                    "history_marks": 0,
                    "events": 0,
                    "active_files": 0,
                },
            }
            ws_dir = bundle_root / "workspaces" / ws
            ws_dir.mkdir(parents=True, exist_ok=True)
            for filename in (
                "objects.jsonl",
                "revisions.jsonl",
                "notes.jsonl",
                "state_entries.jsonl",
                "test_reports.jsonl",
                "test_results.jsonl",
                "history_marks.jsonl",
                "journal.jsonl",
            ):
                (ws_dir / filename).touch()

    attachments: dict[tuple[str, str], dict] = {}
    pool = WriterPool()
    try:
        with (raw_dir / "raw_state_entries.jsonl").open("r", encoding="utf-8") as handle:
            for line in handle:
                if not line.strip():
                    continue
                row = json.loads(line)
                ws = row["workspace"]
                info = workspaces[ws]
                info["counts"]["state_entries"] += 1
                ws_dir = bundle_root / "workspaces" / ws
                pool.write_line(ws_dir / "state_entries.jsonl", line)
                section = row["section"]
                key = row["key"]
                value = row["value"]
                if section == "meta":
                    info["meta"][key] = value
                elif section == "tree_order":
                    info["tree_order"][key] = value
                elif section == "notes":
                    info["counts"]["notes"] += 1
                    note_record = {"address": key, **(value if isinstance(value, dict) else {"value": value})}
                    pool.write_line(
                        ws_dir / "notes.jsonl",
                        json.dumps(note_record, ensure_ascii=False, separators=(",", ":")),
                    )
                    att_map = value.get("attachments") if isinstance(value, dict) else None
                    if isinstance(att_map, dict):
                        for att_id, att in att_map.items():
                            if not isinstance(att, dict):
                                continue
                            digest_hex = att.get("hash")
                            if not isinstance(digest_hex, str) or not valid_sha256(digest_hex):
                                continue
                            removed = bool(att.get("removed", False))
                            item = attachments.setdefault(
                                (ws, digest_hex),
                                {
                                    "workspace": ws,
                                    "hash": digest_hex,
                                    "active": False,
                                    "size": att.get("size"),
                                    "names": set(),
                                    "content_types": set(),
                                    "objects": set(),
                                },
                            )
                            if not removed:
                                item["active"] = True
                            if isinstance(att.get("name"), str) and att["name"]:
                                item["names"].add(att["name"])
                            if isinstance(att.get("content_type"), str) and att["content_type"]:
                                item["content_types"].add(att["content_type"])
                            item["objects"].add(f"{key}#{att_id}")

        stream_specs = (
            ("raw_objects.jsonl", "objects.jsonl", "objects"),
            ("raw_revisions.jsonl", "revisions.jsonl", "revisions"),
            ("raw_test_reports.jsonl", "test_reports.jsonl", "test_reports"),
            ("raw_test_results.jsonl", "test_results.jsonl", "test_results"),
            ("raw_history_marks.jsonl", "history_marks.jsonl", "history_marks"),
            ("raw_events.jsonl", "journal.jsonl", "events"),
        )
        for raw_name, target_name, counter_key in stream_specs:
            with (raw_dir / raw_name).open("r", encoding="utf-8") as handle:
                for line in handle:
                    if not line.strip():
                        continue
                    row = json.loads(line)
                    ws = row["workspace"]
                    info = workspaces[ws]
                    info["counts"][counter_key] += 1
                    if counter_key == "objects":
                        if row.get("present"):
                            info["counts"]["present_objects"] += 1
                        else:
                            info["counts"]["archived_objects"] += 1
                    pool.write_line(bundle_root / "workspaces" / ws / target_name, line)
    finally:
        pool.close_all()

    for (ws, _), item in attachments.items():
        if item["active"]:
            workspaces[ws]["counts"]["active_files"] += 1

    for ws, info in sorted(workspaces.items()):
        schema = info["meta"].get("tree_schema") or info["meta"].get("schema")
        payload = {
            "id": info["id"],
            "created_at": info["created_at"],
            "aliases": info["aliases"],
            "schema": schema,
            "meta": info["meta"],
            "tree_order": info["tree_order"],
            "counts": info["counts"],
        }
        (bundle_root / "workspaces" / ws / "workspace.json").write_text(
            json.dumps(payload, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )

    output_dir.mkdir(parents=True, exist_ok=True)
    cache_root = output_dir / ".files-cache"
    cache_root.mkdir(parents=True, exist_ok=True)
    os.chmod(cache_root, 0o700)

    files_root = bundle_root / "files"
    files_root.mkdir(parents=True, exist_ok=True)
    index_rows = []
    swept_rows = []
    downloaded_count = 0
    cached_count = 0
    total_file_bytes = 0

    ordered_attachments = sorted(attachments.values(), key=lambda x: (x["workspace"], x["hash"]))
    if ordered_attachments:
        with ThreadPoolExecutor(max_workers=16) as executor:
            future_map = {
                executor.submit(
                    fetch_blob,
                    storage_cfg,
                    cache_root,
                    item["workspace"],
                    item["hash"],
                    item["active"],
                ): item
                for item in ordered_attachments
            }
            results = {}
            for future in as_completed(future_map):
                item = future_map[future]
                results[(item["workspace"], item["hash"])] = future.result()

        for item in ordered_attachments:
            ws = item["workspace"]
            digest_hex = item["hash"]
            status, cached_path, byte_size = results[(ws, digest_hex)]
            record = {
                "workspace": ws,
                "hash": digest_hex,
                "active": item["active"],
                "size": byte_size if cached_path is not None else item["size"],
                "names": sorted(item["names"]),
                "content_types": sorted(item["content_types"]),
                "objects": sorted(item["objects"]),
            }
            if status == "swept" or cached_path is None:
                swept_rows.append(record)
                continue
            if status == "downloaded":
                downloaded_count += 1
            else:
                cached_count += 1
            total_file_bytes += byte_size
            dst = files_root / ws / digest_hex
            dst.parent.mkdir(parents=True, exist_ok=True)
            try:
                os.link(cached_path, dst)
            except OSError:
                shutil.copy2(cached_path, dst)
            index_rows.append({**record, "path": f"files/{ws}/{digest_hex}"})

    (files_root / "index.json").write_text(
        json.dumps(index_rows, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    (files_root / "swept.json").write_text(
        json.dumps(swept_rows, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )

    totals = {
        "workspaces": len(workspaces),
        "objects": sum(w["counts"]["objects"] for w in workspaces.values()),
        "revisions": sum(w["counts"]["revisions"] for w in workspaces.values()),
        "notes": sum(w["counts"]["notes"] for w in workspaces.values()),
        "events": sum(w["counts"]["events"] for w in workspaces.values()),
        "test_reports": sum(w["counts"]["test_reports"] for w in workspaces.values()),
        "test_results": sum(w["counts"]["test_results"] for w in workspaces.values()),
        "files_exported": len(index_rows),
        "files_downloaded": downloaded_count,
        "files_cached": cached_count,
        "files_swept_detached": len(swept_rows),
        "files_bytes": total_file_bytes,
    }
    manifest = {
        "format_version": 1,
        "exported_at": stamp,
        "postgres_dump": {
            "path": "postgres/joppa.dump",
            "sha256": dump_sha256,
            "size_bytes": dump_size,
        },
        "totals": totals,
        "workspaces": {ws: info["counts"] for ws, info in sorted(workspaces.items())},
    }
    (bundle_root / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )

    archive_path = output_dir / f"{bundle_name}.tar.gz"
    tmp_archive = output_dir / f".{bundle_name}.{os.getpid()}.tar.gz.tmp"
    try:
        with tmp_archive.open("wb") as raw_out:
            os.chmod(tmp_archive, 0o600)
            with tarfile.open(fileobj=raw_out, mode="w:gz", dereference=False) as tar:
                tar.add(bundle_root, arcname=bundle_name)
        with tarfile.open(tmp_archive, mode="r:gz") as check_tar:
            names = set(check_tar.getnames())
            required = {
                f"{bundle_name}/manifest.json",
                f"{bundle_name}/postgres/joppa.dump",
                f"{bundle_name}/files/index.json",
            }
            if not required.issubset(names):
                raise RuntimeError("Archive verification failed: missing required entries")
        os.replace(tmp_archive, archive_path)
    except Exception:
        tmp_archive.unlink(missing_ok=True)
        raise

    return {
        "archive": str(archive_path),
        "archive_size_bytes": archive_path.stat().st_size,
        "archive_sha256": sha256_file(archive_path),
        **totals,
    }


def main() -> int:
    if len(sys.argv) != 4:
        print("Usage: export_bundle.py RAW_DIR OUTPUT_DIR STAMP", file=sys.stderr)
        return 2
    raw_dir = Path(sys.argv[1]).resolve()
    output_dir = Path(sys.argv[2]).resolve()
    stamp = sys.argv[3]
    storage_env = os.environ.get("JOPPA_STORAGE_ENV_TEXT", "")
    storage_cfg = parse_env_text(storage_env)
    summary = build_bundle(raw_dir, output_dir, stamp, storage_cfg)
    print(json.dumps(summary, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
