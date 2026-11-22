'use strict';

// Runs after installation: replaces bin/litemiyu (a Node.js script) with a hard link to the program,
// so that the `litemiyu` command starts the program directly. Whatever goes wrong here, the script
// stays in place and still works, so this never fails the installation.

const fs = require('fs');
const path = require('path');
const { program } = require('./lib/platform.js');

try {
  const from = program();
  const to = path.join(__dirname, 'bin', 'litemiyu');
  const next = `${to}.new`;
  fs.rmSync(next, { force: true });
  try {
    fs.linkSync(from, next);
  } catch {
    fs.copyFileSync(from, next);
  }
  fs.chmodSync(next, 0o755);
  fs.renameSync(next, to);
} catch {
  // Keep the script.
}
