// `node --test` in this directory.

const assert = require("node:assert");
const path = require("node:path");
const test = require("node:test");

const { programFor } = require("./program");

/** A file system holding only `manifests`. */
function holding(...manifests) {
  return (candidate) => manifests.includes(candidate);
}

test("a marked file runs the crate of the nearest Cargo.toml above it", () => {
  const exists = holding("/w/Cargo.toml", "/w/examples/gallery/Cargo.toml");
  const file = path.join("/w/examples/gallery/src/main.rs");
  assert.deepStrictEqual(programFor(file, "// rawr", file, exists), {
    program: "/w/examples/gallery",
  });
});

test("a marked file with no Cargo.toml above it runs alone", () => {
  assert.deepStrictEqual(programFor("/tmp/hello.rs", "// rawr", "hello.rs", holding()), {
    program: "/tmp/hello.rs",
  });
});

test("an unmarked file says it isn't marked and runs nothing", () => {
  const exists = holding("/w/Cargo.toml");
  assert.deepStrictEqual(
    programFor("/w/examples/gallery/src/main.rs", "use iced;", "examples/gallery/src/main.rs", exists),
    { error: "`examples/gallery/src/main.rs` isn't marked: its first line isn't `// rawr`" },
  );
});

test("the marker may follow a byte order mark and trailing space", () => {
  assert.ok(programFor("/a.rs", "﻿// rawr  ", "a.rs", holding()).program);
});
