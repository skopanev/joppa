// Plain geometry for the Flow view checks (flow_*_browser.cjs). A point is [x, y], a path is a
// list of points, and a box is {left, right, top, bottom}. The y axis goes down.

// The column of each node of a snapshot by address: nodes with the same x (1 px) share one.
function columns(nodes) {
  const xs = nodes.map(n => n.x).sort((p, q) => p - q).filter((x, i, all) => i === 0 || x > all[i - 1] + 1);
  return Object.fromEntries(nodes.map(n => [n.address, xs.findIndex(x => Math.abs(x - n.x) <= 1)]));
}

// The y of a path at `x`, on its first segment that spans x, or null.
function yAtX(path, x) {
  for (let i = 1; i < path.length; i++) {
    const [x0, y0] = path[i - 1], [x1, y1] = path[i];
    if (x0 === x1) { if (x0 === x) return y0; continue; }
    if ((x0 - x) * (x1 - x) <= 0) return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
  }
  return null;
}

// The distance from point c to the segment p→q.
function segmentDistance([px, py], [qx, qy], [cx, cy]) {
  const dx = qx - px, dy = qy - py, length = dx * dx + dy * dy;
  const t = length ? Math.max(0, Math.min(1, ((cx - px) * dx + (cy - py) * dy) / length)) : 0;
  return Math.hypot(px + t * dx - cx, py + t * dy - cy);
}

// The smallest distance from a path to point c.
const pathDistance = (path, c) => Math.min(...path.slice(1).map((q, i) => segmentDistance(path[i], q, c)));

// True when the segment p→q meets the box (the clip test of Liang and Barsky).
function segmentHitsBox([x0, y0], [x1, y1], b) {
  let t0 = 0, t1 = 1;
  const dx = x1 - x0, dy = y1 - y0;
  for (const [p, q] of [[-dx, x0 - b.left], [dx, b.right - x0], [-dy, y0 - b.top], [dy, b.bottom - y0]]) {
    if (p === 0) { if (q < 0) return false; continue; }
    const r = q / p;
    if (p < 0) { if (r > t1) return false; t0 = Math.max(t0, r); }
    else { if (r < t0) return false; t1 = Math.min(t1, r); }
  }
  return true;
}
const pathHitsBox = (path, box) => path.slice(1).some((q, i) => segmentHitsBox(path[i], q, box));

// True when two paths cross: a segment of one goes through a segment of the other.
function pathsCross(a, b) {
  const side = (p, q, r) => Math.sign((q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]));
  for (let i = 1; i < a.length; i++) for (let j = 1; j < b.length; j++) {
    const [p, q, r, s] = [a[i - 1], a[i], b[j - 1], b[j]];
    if (side(p, q, r) * side(p, q, s) < 0 && side(r, s, p) * side(r, s, q) < 0) return true;
  }
  return false;
}

// True when two boxes overlap.
const boxesOverlap = (p, q) => p.left < q.right && q.left < p.right && p.top < q.bottom && q.top < p.bottom;

module.exports = {columns, yAtX, segmentDistance, pathDistance, segmentHitsBox, pathHitsBox, pathsCross, boxesOverlap};
