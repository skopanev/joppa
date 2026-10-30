"""A local stand-in for DigitalOcean Spaces in tests/http_smoke.py: path-style PUT, GET,
HEAD and DELETE of /<bucket>/<key>. It refuses a URL without a presigned AWS4
query or with an ended lifetime. The Rust tests check the signature itself."""
import datetime
import http.server
import threading
import urllib.parse

spaces_files = {}

class SpacesHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass
    def signed(self):
        # Spaces refuses a second auth mechanism next to the presigned query.
        if 'Authorization' in self.headers:
            return None
        url = urllib.parse.urlsplit(self.path)
        query = dict(urllib.parse.parse_qsl(url.query))
        if query.get('X-Amz-Algorithm') != 'AWS4-HMAC-SHA256' or 'X-Amz-Signature' not in query:
            return None
        at = datetime.datetime.strptime(query['X-Amz-Date'], '%Y%m%dT%H%M%SZ').replace(tzinfo=datetime.timezone.utc)
        age = datetime.datetime.now(datetime.timezone.utc) - at
        if age.total_seconds() > int(query['X-Amz-Expires']) or not url.path.startswith('/joppa-smoke/workspaces/'):
            return None
        return url.path.removeprefix('/joppa-smoke/'), query
    def answer(self, status, body=b'', headers=()):
        self.send_response(status)
        for key, value in headers:
            self.send_header(key, value)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        if self.command != 'HEAD':
            self.wfile.write(body)
    def do_PUT(self):
        found = self.signed()
        if not found: return self.answer(403)
        spaces_files[found[0]] = self.rfile.read(int(self.headers['Content-Length']))
        self.answer(200)
    def do_DELETE(self):
        found = self.signed()
        if not found: return self.answer(403)
        spaces_files.pop(found[0], None)
        self.answer(204)
    def do_GET(self):
        found = self.signed()
        if not found: return self.answer(403)
        key, query = found
        if key not in spaces_files: return self.answer(404)
        if self.command == 'HEAD':
            self.send_response(200)
            self.send_header('Content-Length', str(len(spaces_files[key])))
            return self.end_headers()
        self.answer(200, spaces_files[key], [
            ('Content-Type', query.get('response-content-type', 'application/octet-stream')),
            ('Content-Disposition', query.get('response-content-disposition', ''))])
    do_HEAD = do_GET

spaces_server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), SpacesHandler)
threading.Thread(target=spaces_server.serve_forever, daemon=True).start()
env.update({'SPACES_ENDPOINT': f'http://127.0.0.1:{spaces_server.server_address[1]}',
    'SPACES_BUCKET': 'joppa-smoke', 'SPACES_REGION': 'test-1',
    'AWS_ACCESS_KEY_ID': 'smoke-key-id', 'AWS_SECRET_ACCESS_KEY': 'smoke-secret'})
