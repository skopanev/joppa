// The import map of the board modules, as the snapshot embeds it: every file of src/board
// as a data URL. A page that the checks build needs no route for the modules.
const {readdirSync, readFileSync} = require('node:fs');
const path = require('node:path');

const dir = path.join(__dirname, '..', 'src', 'board');
const imports = {};
for (const name of readdirSync(dir).sort())
  imports['board/' + name] = 'data:text/javascript;base64,' + readFileSync(path.join(dir, name)).toString('base64');
module.exports = JSON.stringify({imports});
