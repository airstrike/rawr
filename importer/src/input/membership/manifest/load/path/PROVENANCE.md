# Lexical normalization paths

This private closure implements the exact native path operations reached by
Cargo package normalization on the supported Darwin/Unix host cfg. It performs
no filesystem, process, environment or canonicalization operation and produces
no source/owner/membership authority. Embedded NUL, invalid UTF-8, repeated
separators, backslashes and parent components remain input data.

## Verified primary sources

On 2026-09-06, a read-only Python archive comparison verified both archive
SHA-256 and complete member equality against the local source used here:

| Archive | SHA-256 | Member | Member SHA-256 |
|---|---|---|---|
| `/private/tmp/rawr-p9-toml-supplement-cargo.tar.gz` | `dc2728d4e9650b9871142cc3f1f0e449647e0b493775931f0b70ee1fa658a321` | `cargo-59800466c5c41c444d264b1010b4d57e85a7117f/crates/cargo-util/src/paths.rs` | `77ce5172eed1315ff302e112b2f87bec8ba43cd1bc5111394ea1815b1ac2b34e` |
| registry `pathdiff-0.2.3.crate` | `df94ce210e5bc13cb6651479fa48d14f601d9858cfe0467f43ae157023b938d3` | `pathdiff-0.2.3/src/lib.rs` | `c44b3fc6053f9a5b0bacc9097257685f6f0edb88dd098f06d4d8fa5575c2b574` |

The pathdiff archive hash equals the pinned Cargo.lock checksum. No dependency
or registry source was changed. `reference.rs` contains the unchanged contiguous
`normalize_path` (:84–120) and `diff_paths` (:43–91) functions, each inside a
test-only module with its imports outside the copied region. Keep that file out
of rustfmt. It is not a production fallback. The attached MIT/Apache licenses
were copied from the verified pathdiff archive; Cargo uses these same licenses.

Pinned installed rust-src was read under
`~/.rustup/toolchains/nightly-2026-07-11-aarch64-apple-darwin/lib/rustlib/src/rust/library`:

- `std/src/path.rs`, SHA-256
  `ca86470ed4960d6afc3b1126a6519a1db327deaaadabc6fc7976be1a38ba2c4d`:
  `PathBuf::_push` :1341–1420, `Path::_join` :3081–3084, and
  `From<OsString> for PathBuf` :1994–2001.
- `std/src/os/unix/ffi/os_str.rs`, SHA-256
  `5db4bdbca5facd9f5df12a17709ca0e7df82acd462bf6e117106752f559f84b3`:
  `OsStringExt::{from_vec,into_vec}` :28–34 move the underlying byte Vec.

These rust-src hashes identify inspected source; they are not a new claim of
installed binary/source equivalence. The importer already uses Unix native-byte
paths; Windows/Cygwin prefix dispatch is not a supported host cfg of this
closure. No native filename subset is added within the supported cfg.

## Correspondence

- `join`: exact Unix `Path::join` bytes, with absolute right path replacement,
  separator insertion if the nonempty retained left side lacks a trailing `/`,
  and no lexical normalization. Joining an empty suffix can add a trailing `/`.
  It directly allocates the final concatenation rather than cloning a root
  which an absolute suffix immediately discards.
- `normalize`: borrowed component stack implements Cargo's `CurDir` omission,
  normal-component pop, retained leading/consecutive parents, and root floor.
  Unix components cannot yield `Prefix`. Each ordinary component is entered and
  popped at most once. Final separators match repeated native PathBuf pushes.
- `diff`: exact pathdiff absolute/relative mismatch, common-prefix,
  base-CurrentDir, base-ParentDir and exhausted-side branches. `None` is a
  semantic result. Do not normalize the base first.
- `rebase`: Cargo `core/workspace.rs:2135–2160` order is old-root join, lexical
  normalize, diff against new root, then checked UTF-8. It can succeed despite
  invalid UTF-8 in both shared prefixes; only a non-UTF-8 final spelling rejects.
- `normalize_text`: the single build-script branch starts as UTF-8 and lexical
  components preserve it; the final native buffer moves into String. Multiple
  build-script normalization policy is outside this lexical helper.

## Ownership and work

All public entries check the same original Session; they do not reconstruct
limits or accounts. Source lengths and every final/temporary owned native
buffer are checked before allocation. Native buffers request their full exact
extent once with fallible reserve; complete intermediate buffers in rebase are
separately accounted. The fallible component Buffer accounts each owned slot
and geometric full replacement Layout before allocation; it never uses allocator
bonus capacity to authorize logical growth. Prior charges remain spent when
normalization pops components or a later diff/UTF-8 conversion rejects.

Components borrow native input; no component string is copied. Final assembly
counts checked capacity before copying into admitted storage. A native buffer
moves through Vec -> OsString -> PathBuf; String conversion consumes the same
Vec without copying or shrink. A UTF-8 error owns only that already admitted
buffer plus the fixed error representation, then drops it. Semantic rebase
failures admit a diagnostic item and permit governed caller handling; any
Session/resource failure remains terminal.

Normalize and diff use monotone iterators, a flat component buffer and bounded
final passes. Join copies each retained byte once. Rebase performs the fixed
join/normalize/diff sequence. Sorting, filesystem, candidate-comparison and
hashing events are not invented for this pure work. Partial output teardown
drops only flat borrowed component arrays or byte buffers, independent of depth.

## Executed validation

Seven focused tests compare raw native bytes, rather than Path equality (which
would hide spelling differences), against the unchanged pinned functions and
std join. The matrix includes empty/current/parent/root/absolute and trailing
separator shapes, Unicode, invalid UTF-8, NUL and Windows-looking Unix names.
Rebase tests execute the complete reference composition and distinguish relative
failure from final UTF-8 failure. Additional tests verify pointer transfer,
exact cumulative original item/byte/string floors with prior usage, refusal
before every allocation, no post-refusal work, and zero heap allocation for an
empty output. The graph's filesystem/hash allowance is already exhausted in
the pure-account fixture.

Root executed all seven focused tests successfully in 0.03s. Independent source
review found no P0/P1 (confidence 0.98); CLI compilation and scoped format/diff
checks pass. Root independently verified that both reference function regions
are unchanged contiguous pinned-source copies. Reference SHA-256:
`0d4cbd315ef3bb32f60e9fa75daee26b24cc2c4d659c25108fd4064ccf1e69ee`.
Logs: `/private/tmp/rawr-p9-loader-path.log` and
`/private/tmp/rawr-p9-loader-path-cli.log`. `reference.rs` remains outside rustfmt.
This is lexical helper evidence, not completed normalization or product reuse.
