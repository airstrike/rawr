# Fixed trailing Unicode whitespace

This private helper is not activated and does not establish
the enclosing modeled Cargo process, its prelude, or whether the producer was
cold. `Owner` is caller-retained storage; it must eventually be shared by all
reached users of the source static within that complete process, including Git
path-file parsing and repository configuration. It must not be reset at a
Session, Host, package, discovery, or nested-manifest boundary.

## Sources

- bstr 1.12.3, crates.io checksum
  `5cee35f73844aa3014bb606320a6c1f010249dbdf43342fe54b5a4f6a8ed4b79`:
  `src/unicode/fsm/whitespace_anchored_rev.rs`, `src/unicode/whitespace.rs:18–24`,
  and `src/ext_slice.rs` (`trim_end`). MIT OR Apache-2.0; exact license files are
  retained as `bstr-LICENSE-*`.
- regex-automata 0.4.14, crates.io checksum
  `6e1dd4122fc1595e8162618945476892eefca7b88c52820e74af6262213cae8f`:
  `src/dfa/dense.rs` (complete representation, deserialization and validation),
  `src/dfa/{special,accel,start,search,automaton}.rs`,
  `src/util/{wire,alphabet,start,primitives,lazy}.rs`, and
  `src/util/prefilter/mod.rs`. MIT OR Apache-2.0; exact licenses are retained as
  `regex-automata-LICENSE-*`.
- The selected dependency versions belong to pinned Cargo
  `59800466c5c41c444d264b1010b4d57e85a7117f`.

`whitespace_anchored_rev.littleendian.dfa` is copied byte-for-byte from bstr's
`src/unicode/fsm/` directory. It is 3232 bytes, SHA256
`791825312415cbd24aca8b8262ded3e1fabc785e3eba3660f65019296f8e5f58`.
The artifact remains aligned static storage. Safe little-endian scalar reads
replace casting the bytes to borrowed u32 slices; no table heap copy occurs.

## Containment

Runtime decoding retains all enabled source fields, including both inline
256-entry maps, all optional start IDs, flags, quit buckets, the accelerator
count slice and the complete `Option<Prefilter>` representation (fat Arc,
boolean and usize). The full inline owner layout is available for inclusion in
an eventual containing owner's actual storage layout. Construction admits its
real logical slots and all decoding/validation/search work through the original
Session. Inline storage causes no fictional heap or removed upstream Box debit.

The safe decoder preserves the source validation order: wire structure and
special ranges, accelerators, match table, alphabet transitions, start IDs,
then state/accelerator correspondence. It checks all source tables at runtime;
neither an artifact hash nor compile-time validation substitutes for this work.
The fixed-table closure is checked before publication. Source ByteSet decoding
reads both buckets from the same initial slice; the fixed table has two zero
buckets and the contained operation preserves that detail.

Search implements whole-input anchored reverse matching with earliest=false.
It uses the anchored Start::Text entry, delayed match offsets, last-match
retention, dead-state termination without EOI, and an EOI transition on input
exhaustion (including empty input). It traverses bytes without UTF-8 conversion.
The source's four-transition unrolling is expressed as individually admitted
safe transitions, with identical state order and special-state dispatch.
No new regex compilation, forward search, ASCII trim, allocator or mutex is
introduced.

This replaces only this immutable helper's original global atomic publication,
Box indirection and equivalent racing initialization. The private owner publishes
only a fully validated helper; any decode or search refusal poisons it across
fresh Sessions. Cleanup allocates nothing, spends nothing and refunds nothing.
The original Session retains the first failure and all work already spent.

The direct consumer seam is `Owner::trim_end(&[u8], &mut Session) -> Step<&[u8]>`.
Git prefix validation, full-input semantic error copies, file buffer/descriptor
lifetimes, path conversion and the actual process/history association remain
the caller's separate required operations. No repository proof follows here.

## Located caller boundary

The actual command boundary is `cargo.rs::cargo_package_files` (4318–4430 at this
checkpoint), invoked by `complete_harness_parts` (2427–2451) once per
fallback package. Each reached non-registry package list launches a distinct
Cargo child and therefore needs an independent logical helper owner; the
enclosing graph/allocation account remains cumulative. The owner must not be
shared merely because those children occur in the same fallback loop.

Within one command, the same source static is reached by gix-discover 0.53.0
`path.rs:51–70` and `parse.rs:21–36`, from `is.rs:54,91,106–111`. Upward discovery
(`upwards/mod.rs:97–174`) can retain prior trim use through failed candidates.
Opening the found repository rereads commondir (`gix 0.85.0
open/repository.rs:187`). Repository config initialization also reaches the
same helper through `open/repository.rs:247–260` → `config/cache/init.rs:144` →
`config/cache/util.rs:137–146` → `config/tree/sections/core.rs:400–412`, when an
existing `core.abbrev` value is trimmed. Nested repository discovery must share
this history as well (pinned Cargo `sources/path.rs:570–635,641–644,822–827`).

The contained `manifest/load/driver/run.rs:26–56` currently takes separate source,
configuration and reader owners, not a complete modeled-process wrapper. The
source-ID `History` is not that wrapper and explicitly leaves command-entry
prelude association open. The helper therefore remains an explicit caller-owned
argument; neither an empty owner nor a successful local trim establishes that
the producer's first relevant use was cold. These are concrete connection
conditions, not a request for new prelude reconstruction in this helper slice.
