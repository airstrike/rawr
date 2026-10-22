# Live manifest acquisition

This boundary produces acquisition data only. `manifest` receives the caller's
original `cargo::Limits` and `membership::Budget`, reads one newly opened stream,
and returns `Acquired` with a checked `observed::Record` and optional identity
observations. It does not implement a current selector, cold Cargo association,
workspace discovery, source authentication, or a gate-spanning freshness proof.
The surrounding membership module remains disconnected from production input
authority until the complete selector is implemented and qualified.

## Pinned behavior and native closure

The independent source audit is
`/private/tmp/rawr-p9-live-file-io-audit.md`, prepared against repository checkpoint
`e66ec347`. It checks the exact reached APIs and registry source members; it does
not claim installed libc/kernel binary correspondence or native test results.
Its SHA-256 is
`204f11980c4cba470a4c4920fc3bcf563893081835dbca13dae310d30c5491a4`.

- Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`,
  `crates/cargo-util/src/paths.rs:167–182`: `read` obtains `read_bytes` through
  `std::fs::read` before UTF-8 conversion. Read errors precede decoding errors.
- Rust `375b1431b7d89d1c2e2bc168c011848ae12b7d14`, installed rust-src for
  `nightly-2026-07-11-aarch64-apple-darwin`, `std/src/fs.rs:340–349`: opening is
  followed by one optional metadata sizing hint, then complete reading.
  `std/src/io/mod.rs:418–528` continues positive short reads and retries EINTR.
  Neither metadata length nor exhausted destination capacity establishes EOF.
- `std/src/sys/fs/unix.rs:1377–1391` opens with `O_RDONLY | O_CLOEXEC` and mode
  `0o666` (ignored without creation). Its internal `cvt_r` hides retries; the
  importer instead exposes them through existing safe one-shot nix calls.
- nix 0.27.1, `src/fcntl.rs:207–219`, `src/unistd.rs:1099–1114`, and
  `src/sys/stat.rs:239–269`: one imported libc open/read/stat/fstat/close call per
  invocation. These are invocation counts, not kernel-internal work claims.
  Only open/read EINTR is retried; each attempt is independently admitted.
- nix `src/lib.rs:271–286`: `NixPath for CStr` borrows the existing C spelling
  without a pathname allocation. A prospectively charged terminated `Vec<u8>`
  is borrowed using `CStr::from_bytes_with_nul`; there is no CString conversion,
  boxed-slice shrink, normalization, Unicode conversion, or no-follow flag.
- nix `src/errno.rs:144–146,1562–1673` maps unknown native codes to enum value
  zero. Native error 107 is therefore captured immediately through
  `nix::errno::errno()` on Err, before any callback, budget operation or cleanup.
  `native::raw_with` is the exact extraction branch tested with code107; the test
  does not write thread-local errno or claim the host emitted that code.
- Pinned `std/src/os/fd/owned.rs:200–225`: one close attempt with ignored result,
  without retry. The private descriptor guard preserves this behavior on every
  exit. No raw descriptor is exported, cloned, sought, or retargeted.

The audit verified these registry archives against their locked checksums and
the cited members against the archives:

| Package | Archive SHA-256 |
|---|---|
| nix 0.27.1 | `2eb04e9c688eff1c89d72b407f168cf79bb9e867a9d3323ed6c01519eb9cc053` |
| libc 0.2.189 | `3eaf3ede3fee6db1a4c2ee091bf8a8b4dccdc6d17f656fb07896ee72867612f2` |

Selected exact source hashes from that audit:

| Source | SHA-256 |
|---|---|
| Cargo `crates/cargo-util/src/paths.rs` | `77ce5172eed1315ff302e112b2f87bec8ba43cd1bc5111394ea1815b1ac2b34e` |
| std `src/fs.rs` | `7d8e4c97fcd69e3dd74acf9d2cf58c634623f9103b3c2e370be08f74793224da` |
| std `src/io/mod.rs` | `b08c437f6405d75d3ddee29515ec6a9e1a2ce75ef025e6b705df84e9e7d6a53e` |
| nix `src/lib.rs` | `571f418caf6a9646939ae4976ec4bde5f9325e9b4320715c9586590b9d8d4963` |
| nix `src/fcntl.rs` | `8cda1abc82c562b2340b3d27176b674b65d0e1c4bf888e7700c6b50e63c3bf0d` |
| nix `src/unistd.rs` | `e9d24b6490d0578fc3e31200023e131df9471c09b7553b452a9ad76399eea753` |
| nix `src/sys/stat.rs` | `3928598e6428d7e44b42d36aeb59ac353eaf0270801a6ac72c511804c5fdf358` |
| nix `src/errno.rs` | `3c2935cc3238c13a545ab8ceb6a8aa9fd03a9cbf72be041e9ea032f6ee19c2f4` |

The audit compares the reached libc0.2.185 declarations vendored by pinned std
with product libc0.2.189. Open/read/stat-field/fstat/close declarations and
relevant target cfg agree. This is a reached-path source comparison, not
whole-libc equivalence. The audit's installed rust-src member hashes identify
the inspected source; its distribution archive was not fetched or reverified.

## Optional identity observations

Before-path, opened-descriptor, after-descriptor, and after-path probes use
following `stat`/`fstat`, not `lstat` or canonicalization. A probe errno remains
optional data and never changes the file's read result. Open failure leaves
both descriptor observations explicitly `None`; no probe or identity fact is
fabricated. Only open ENOENT maps to `Value::Absent`; ENOTDIR and all other
open errors remain `Value::Error`. A later read ENOENT remains a read error.

`live::Metadata` retains native fields without imposing the separate serialized
Metadata schema: signed nanoseconds remain signed, even outside its normalized
range. Device and length casts match pinned std's `as u64`, including signed
Darwin `dev_t` and `off_t`. Type extraction masks `S_IFMT` then uses exact
equality; S_IF encodings are not independent bit flags. No file-type or length
predicate changes whether the stream may be read.

Comparison is `Changed` if any available metadata disagree, `Matched` only if
all four probes succeeded and agree, and otherwise `Unavailable`. Changed takes
precedence over missing facts. Comparison never compares metadata length with
content length. All three comparison results remain acquisition data; the
containing full witness must interpret them alongside complete path, symlink,
origin, selection and current-gate evidence. Matched metadata is not snapshot
isolation, complete concurrent-mutation detection, or protection against ABA.

## Read and work bounds

The private descriptor is born immediately after one successful open. Every
read uses that descriptor and its advancing offset, beginning at offset zero.
The one initialized stack scratch is fixed at8192 bytes, below Darwin's INT_MAX
read extent. Every requested extent is positive and no larger than scratch.
Positive short reads continue; metadata is not used for sizing or completion.
At the manifest cap, a separately admitted one-byte probe distinguishes EOF,
growth and error. A positive overflow probe is never copied into retained data.
Only `Ok(0)` from an admitted positive extent produces `End::Eof`.

The exact original accounts are retained:

| Operation/storage | Prospective charge |
|---|---|
| Native query path | Original per-path string limit, one owned item, exact bytes through `observed::Path::from_bytes` |
| Terminated syscall spelling | One owned item and path length+1 before exact reserve/copy |
| File vector | One owned item; each full requested geometric replacement layout before reserve/copy |
| stat/fstat/open/read | One graph path operation before each invocation, including failed attempts, retries and EOF |
| Descriptor cleanup | One graph path operation reserved before the first open attempt; close does not debit again |
| Fixed scratch and inline Acquired/Identity | Statically bounded stack storage, no invented heap allocation charge |
| Copies and metadata comparisons | Bounded by admitted extents/layouts and four fixed observations; no fictitious hash, decode or candidate-comparison charge |
| Subsequent record collection/encoding/document | Their actual additional ownership remains charged by the same caller account |

For a successful open with O open attempts and R read attempts, the path debit
is O+R+5: before-path, reserved close, opened fstat, after fstat, after-path, plus
the open/read attempts. A failed open spends O+3, including the unused cleanup
reservation. Neither failure nor disposal refunds prior charges. A budget
failure permits only an already-admitted descriptor cleanup and cannot return
an Acquired value or continue later I/O.

The file vector tracks admitted capacity separately from allocator capacity.
Growth admits max(required, min(2×admitted, manifest limit)); initial capacity is
the actual first successful read count. Full prior layouts remain spent. Each
request is at least geometric except the final cap-clamped request, so aggregate
requested layouts and copy work stay linear in the largest admitted capacity.
Allocator bonus cannot bypass the next admission. No shrink or boxed conversion
occurs. The EOF probe and fixed scratch require no growing heap buffer.

This buffer changes ownership mechanics, not the accepted TOML/source grammar.
Its prospective bound is the caller's original manifest limit and aggregate
ownership account; neither stored data nor a fresh local account can select a
weaker allowance.

## Focused verification

The deterministic syscall seam drives the same `file::acquire` state machine as
the native adapter. Cases cover zero/exact/over cap, short reads, fixed chunks,
open/read EINTR and exhaustion, raw first/partial errors, optional metadata
failures and misleading hints, mutation/replacement/unavailable comparisons,
every operation-budget floor, ownership exact/floor/prior usage, allocation
refusal and allocator bonus, and actual Stored→Host→Document integration.

Native fixtures cover a regular file, symlink, missing path, ENOTDIR, directory,
`/dev/null`, and exact non-UTF8 filename bytes. If the filesystem rejects creation
of the invalid spelling with EPERM, that fact is reported and acquisition still
executes against the exact native query; deterministic data-path coverage is
retained. This is a fixture limitation, not a product source exclusion.

Execution owner command, using the existing single Cargo slot:

```sh
cargo +nightly-2026-07-11 test -p rawr-import --offline --locked --jobs 1 --features rustc-driver,test-fixtures --lib input::membership::live -- --test-threads=1 --nocapture
```

All 22 tests passed on 2026-09-05. The native invalid-UTF8 filename creation
returned EPERM on this APFS filesystem; acquisition of the exact query still ran,
but a positive native read of that filename was not established. The test log
is `/private/tmp/rawr-p9-live-file-focused-test.log`, SHA-256
`e729d1ddfcca405690d9bddb648a7e6dbc0502350aa44305009a7e2fd8ad93e3`.

Independent review of the complete source boundary and provenance found no
P0/P1 findings, confidence 0.92. Scoped stable format and diff checks passed.
The reviewer checked source, not an installed syscall trace, the full selector,
or cold/current binding. This focused boundary review does not satisfy T11.

No full-selector, release, installation or readiness claim follows from this
focused suite.
