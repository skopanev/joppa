// The board script as the server serves it: every part of src/board in file name order.
const {readdirSync, readFileSync} = require('node:fs');
const path = require('node:path');

const dir = path.join(__dirname, '..', 'src', 'board');
module.exports = readdirSync(dir).sort().map(name => readFileSync(path.join(dir, name), 'utf8')).join('');
