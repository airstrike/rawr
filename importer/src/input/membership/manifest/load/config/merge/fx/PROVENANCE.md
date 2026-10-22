# Closed string and Unix path hashing

This module adapts the allocation-free closure reached by `Hash for str` and
the pinned Unix `Hash for Path`
from rustc-hash 2.1.2, the version in Cargo commit
`59800466c5c41c444d264b1010b4d57e85a7117f`'s lockfile.
Upstream crate SHA-256:
`94300abf3f1ae2e2b8ffb7b58043de3d399c73fa6f4b73826402a5c457614dbe`.
The upstream MIT and Apache licenses are adjacent.

The pinned dependency enables `std`, not `nightly`. The standard string Hash
implementation writes the UTF-8 bytes followed by the u8 sentinel. This module
is private and is not a general FxHasher replacement. The path specialization
additionally implements upstream `write_usize` directly; other integer entry
points remain outside the closed domain.
Fixed-width little-endian decoding replaces upstream slice-conversion unwraps.

`importer/reference/manifest/config-fx/reference.rs` records the unchanged
dependency's outputs for lengths 0–1024 and eight configuration/Unicode keys.
The fixture was generated with nightly-2026-07-11 on aarch64-apple-darwin.
Compile upstream src/lib.rs with edition 2021, crate name rustc_hash, rlib crate
type and cfg feature="std"; then compile the harness with that rlib as its
rustc_hash extern and capture stdout as observations.txt. No workspace lockfile
or dependency version changes are needed.

## Separate native Path evidence

Pinned std `library/std/src/path.rs:3796–3847` hashes Unix path components with
byte writes and a final `write_usize`. That final operation must call the pinned
Fx integer mixer, not the Hasher default byte-writing implementation. Native
Path equality and hashing retain redundant-separator/current-directory
equivalence without collapsing parent components.

`importer/reference/manifest/config-fx/path_reference.rs` links the unchanged
rustc-hash 2.1.2 rlib using the same procedure above. Its 141 observations cover
empty, root, relative, equivalent, parent, non-UTF-8 and NUL-containing paths,
plus component lengths 0–128. Tests compare borrowed and owned native paths
against `path-observations.txt`. This is separate Path-domain evidence; the
existing string qualification is unchanged and does not itself qualify Path.

This primitive neither allocates nor establishes bounded map ownership.
Its future table caller must admit key scans and rehash work under the original
Session and preserve the pinned table's allocation and iteration behavior.
