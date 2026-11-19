// What F5 runs when no launch.json names a program: the crate the active
// file belongs to, found by the nearest Cargo.toml above it, or the file
// alone when none is. It reads only the file system, so it runs outside
// VS Code too.

const fs = require("fs");
const path = require("path");

const MARKER = "// rawr";

/** Whether `line`, a file's first line, is the marker `// rawr`. */
function isMarker(line) {
  return line.replace(/^﻿/, "").trimEnd() === MARKER;
}

function isFile(candidate) {
  return fs.statSync(candidate, { throwIfNoEntry: false })?.isFile() === true;
}

/**
 * The program for the Rust file `file`, whose first line is `first`:
 * `{ program }`, the directory of the nearest Cargo.toml above it or else
 * the file, or `{ error }` when the file isn't marked. The message names
 * the file as `name`.
 */
function programFor(file, first, name = file, exists = isFile) {
  if (!isMarker(first)) {
    return { error: `\`${name}\` isn't marked: its first line isn't \`${MARKER}\`` };
  }
  let directory = path.dirname(file);
  for (;;) {
    if (exists(path.join(directory, "Cargo.toml"))) {
      return { program: directory };
    }
    const parent = path.dirname(directory);
    if (parent === directory) {
      return { program: file };
    }
    directory = parent;
  }
}

module.exports = { MARKER, isMarker, programFor };
