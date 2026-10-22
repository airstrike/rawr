# Contained local Git configuration

This private connection runs supplied-fact discovery, the distinct opening-cwd
occurrence, the commondir reread, and complete local/worktree StageOne loading.
It is not a complete opened repository, native cwd acquisition, producer-history
association, or production membership authority.

The source is Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f`'s lock
cohort. Restored crate archives were checked against that lock before extraction:

| Crate | Archive SHA-256 |
|---|---|
| gix-config 0.58.0 | `a29bf266c4cdaf759e535c24ad4ce655b987aeb6911075643403cc7cc5ade583` |
| gix-config-value 0.18.1 | `ed42168329552f6c2e5df09665c104199d45d84bedb53683738a49b57fe1baab` |
| unicode-bom 2.0.3 | `7eec5d1121208364f6793f7d2e222bf75a915c19557537745b195b253dd64217` |
| gix-trace 0.1.20 | `44dc45eae785c0eb14173e0f152e6e224dcf4d45b6a6999a3aed22af541ad678` |
| smallvec 1.15.2 | `8ed6a63f02c8539c91a8685a86f4099661ba3da017932f6ebbea6de3f0fa7c90` |

The restored sources live under `/private/tmp/rawr-gix-pinned-sep26`; smallvec
uses the existing exact development dependency with `const_generics`. These
crates are MIT OR Apache-2.0 licensed, except unicode-bom (Apache-2.0). Existing Git provenance identifies the
gix/gix-discover and Rust standard-library pins and attached licenses.

`opening.rs` follows gix `discover.rs:39–45` and
`open/repository.rs:159–197`. StageOne follows
`config/cache/incubate.rs:25–99,120–169`: allocate the shared read buffer, load
local configuration, derive initial bare/version/hash fields, optionally append
worktree configuration, then derive Unicode/protect/reflog settings. The first
bare value is deliberately retained across the append.

The parser retains the complete owned event representation, frontmatter
SmallVec, section order, lookup topology, error cursor, and metadata Arc
relationships from gix-config `parse/events.rs` and `file/init.rs`. It preserves
BOM handling, native bytes, comments, escapes, continuations, implicit values,
case-folded section/key names, and case-sensitive subsections. Lookup,
normalization, Boolean/Integer conversion, and append follow `file/access/raw.rs`,
`file/section/body.rs`, `file/util.rs`, `file/access/mutate.rs`,
`value/normalize.rs`, and gix-config-value's conversion sources. Source-created errors retain their owned key,
path, remaining input, and conversion payloads, including lenient discard paths.

This changes only hashing in the four documented private config map domains.
`table.rs` uses pinned zero-key `DefaultHasher`, retains actual owned key clones,
occupied-entry behavior, growth, removal, and tombstone history, and admits
actual closed map operations and concrete replacement allocation layouts.
Finite hashing/probing/rehash bounds are containment arguments, not invented
executed-item charges. No populated File/map clone is implemented. SmallVec
growth distinguishes its real inline copy from allocator-bounded heap realloc.

The Configuration read role binds the caller's retained requested capacity in
the query and schema-6 transcript. Open failure leaves old buffer contents;
successful open clears them. One shared source-shaped read algorithm executes
and replays exact hints/read events, preserves partial read bytes, and invokes
the parser while the descriptor remains alive. Separate evidence bytes survive
parser clearing. The completion bridge spends the original Budget and latches
the first refusal; there is no new account or guessed capacity authority.

The pinned warning branch is disabled: gix-trace defaults are empty, its
`tracing` feature activates `dep:tracing`, and the exact Cargo lock's gix-trace
entry has no dependencies. `warn!` forwards to `disabled.rs`'s empty event macro,
so its formatting arguments do not execute. Explicit I/O error/path owners
created before that macro still do. This does not claim feature-enabled parity.

The bounded downstream source review found two unordered config consumers:
`is_void` tests each visited body's length, and hasconfig gathers lookup IDs
before restoring explicit section order. The reached stateful `core.worktree`
callback follows explicit reverse per-key IDs, including callbacks for matching
sections without the requested key. Repository/cache clones share `Arc<File>`;
populated File clone, formatting, serialization, mutable snapshot, and raw
multi-value mutation are not reached by Cargo's opening/index/dirwalk route.
Later global-path deduplication and trust repair use separate ordered collections
(a three-candidate BTreeSet and a BTreeMap); their cloned paths and replaced
metadata Arcs remain future connection work. This proves no authority over
unrelated object/index/ignore maps or the complete shared seed cohort.

Focused checks exercise actual codec-to-Host StageOne and opening composition,
worktree precedence, full events, malformed/native errors, retained-buffer
growth, descriptor-scoped completion, fixed-map entry/removal, and original
refusal. They are component evidence, not final RawrGallery qualification.
