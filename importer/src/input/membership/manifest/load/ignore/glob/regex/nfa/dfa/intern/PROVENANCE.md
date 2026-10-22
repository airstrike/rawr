# Closed DFA state interner

This private specialization is selected:
`HashMap<State, u32, BuildHasherDefault<DefaultHasher>>`. It is part of the
private dense constructor. Its finishing continuation is described separately
in `../finish/PROVENANCE.md`. This interner proof alone does not qualify Core,
membership, resource fit or measured performance.

## Pins and operation vocabulary

Regex source root:
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/regex-automata-0.4.14/src/`.
Pinned Rust source root:
`~/.rustup/toolchains/nightly-2026-07-11-aarch64-apple-darwin/lib/rustlib/src/rust/library/`.
The applicable standard library uses vendored hashbrown 0.17.1 and std Global.
Renew this evidence when compiler, standard library, target, cfg or either pin
changes. The post-reservation capacity assertion detects some drift; it is not
a substitute for checking the pin before connection.

Upstream `dfa/determinize.rs` constructs the map at line 52, inserts at 63 and
545, looks up at 509, clears at 266 and reads length at 594. It never removes,
iterates, clones, drains or exposes the map. Our concrete wrapper has only that
vocabulary and no generic hasher, callback, mutable map or key escape. Keys are
internally encoded immutable `Arc<[u8]>`; values are inert u32 state IDs.

The dead key is inserted; the identical quit key is deliberately not inserted.
Every later insertion follows a missing-key lookup with unchanged bytes.
State IDs come from dense-row append, not map placement. Canonical `builder_states`
drives transition construction and the ordered match extraction at line 269.
Replacing the hash initialization therefore preserves each equality result,
cache hit, new ID, frontier order and match order by induction over those calls.

`Runner::add_state` publishes the row, shared canonical key and map entry before
checking DFA size and then auxiliary size. Its semantic memory formula at
589–598 uses map length, not bucket capacity, hasher size or placement. Those
source projections remain separate from actual physical allocation admission.

## Fixed hash and exact equality

Installed `std/src/hash/random.rs:83` constructs SipHasher13 from RandomState's
keys. `DefaultHasher::new` at 106 instead uses the same algorithm with zero keys.
The fixed builder invokes that constructor without `RandomState::new`, native
TLS or RNG. Seed/TLS/entropy effects are expressly classified as replaceable
only here; native RNG failure behavior is not claimed to be reproduced.

`alloc/src/sync.rs:3869` delegates `Arc<T>::hash` to T. The derived State hash has
only this single field. `core/src/hash/mod.rs:931` hashes a byte slice by writing
its usize length prefix, then its bytes; `write_length_prefix` at 483 delegates
to `write_usize`. Thus owned State and borrowed `[u8]` use the same stream.
`core/src/hash/sip.rs:256–300` consumes full eight-byte chunks and a bounded tail
using fixed stack state, and `finish` performs fixed rounds. It allocates
nothing, invokes no user callbacks and performs no admission. The internally
owned byte slice's valid length bounds the loops and length arithmetic.

State equality and borrowed lookup equality compare byte content, not Arc
addresses. `core/src/slice/cmp.rs:14` checks lengths; its byte specialization at
151 uses `compare_bytes` over that exact valid length. Unequal lengths or
contents stay distinct even when hashes, control tags or starting buckets
collide. Equal encoded keys recover the existing ID without another key Arc.

Pinned hashbrown `map.rs:1806` hashes the incoming key before its table reserve
and search. Our prospective fallible reserve precedes `insert`'s incoming hash.
This movement crosses no allocating, failing, callback or admission operation
inside the established concrete hash closure. It does not reorder source
semantic errors or prospective resource refusal.

## Physical ownership and finite table work

The layout arithmetic specializes the configuration-table source proof
to `(State, u32)`, without adopting the configuration table's distinct key/hasher qualification.
`vendor/hashbrown-0.17.1/src/raw.rs:104,196,216` selects buckets and the requested
layout. This entry is at least 12 bytes on supported pointer widths, above all
tiny-entry exceptions. The calculation includes entry storage, alignment
padding, control bytes and the trailing control group. Group cfg matches the
pinned SSE2/LSX, aarch64/generic implementations. `alloc/src/alloc.rs:205`'s
Global route reports the requested allocation length, excluding allocator
bonus capacity from the growth calculation.

Only insertion grows the table. No removal introduces tombstones and final
clear is followed by no further insertion. Thus the usable capacities are
3, 7, 14, 28, 56, and successive doubled bucket extents, independently of hash
placement. `raw.rs:2739`'s in-place tombstone rehash branch is unreachable in
this vocabulary. Before every growth we admit the full replacement layout,
then call `try_reserve(1)` and check the resulting capacity. The following
insert cannot allocate. Old allocation remains charged even after its storage
is freed; overflow, admission and allocation failures latch terminally.

Let B be physical buckets, G group width, N occupied entries and L the largest
admitted key length. `raw.rs:65`'s triangular sequence visits every group;
`find_inner` at 2008 stops upon an empty bucket, guaranteed by the load factor.
Even complete hash collisions therefore yield finite group scans and at most
the finite table's candidate byte comparisons, each bounded by L. The
tiny-table replicated controls are covered by max(B, G). No expected constant
time assumption or collision-resistance claim is required.

`resize_inner` at 2885 scans old occupied buckets, hashes each of N keys once,
and performs finite insertion probes in the replacement before moving inert
entries. There is no additional scratch allocation. `clear` at 833 drops the
flat entries and resets controls while retaining the table allocation. Each
Arc payload is a byte slice, so successful and partial-failure teardown has
no recursive graph destruction. Rehash/drop traversal order has no callbacks,
admissions, refunds or observable state-ID effects.

## Original request mapping and evidence boundary

`Items(1)` precedes each actual get/insert; `Items(len)` precedes the actual
entry releases at clear. The empty fixed constructor allocates nothing.
Binary keys issue no String request and no opaque `Items(keylen)` hash-scan
charge. Encoded-byte construction, explicit copies, slots, Arc layouts and
canonical storage keep their separate existing prospective admissions. The
finite bounds above justify the closed operations; they are not extra debits
or filesystem/observation-hash events.

`tests.rs` exercises the actual fixed hasher for owned/borrowed keys, zero/high
bytes, lengths and common prefixes, duplicate lookup, growth/capacity requests,
and every interner refusal before mutation. Its collision fixture uses actual
fixed-hasher keys sharing initial bucket and control tag across the reached
tables (`control/tag.rs:35`); it does not inject a different production hasher
or claim to manufacture equal 64-bit Sip hashes. The source bound above also
covers full hash collisions. Existing subset tests cover raw state/transition/
match order, semantic limit precedence and cumulative account floors.

Root executed all104 NFA tests on nightly-2026-07-11/aarch64-apple-darwin with
import-baseline enabled; they passed. Capacity/layout-model checks are not a new
claim of allocator-trace observations. No full matcher or production membership
connection follows from this interner alone.
