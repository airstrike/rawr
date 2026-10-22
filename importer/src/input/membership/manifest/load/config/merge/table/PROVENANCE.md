# Closed configuration table

This is the `Table = Map<str>` private standard-map boundary. It stores only `&str` keys and
`usize` flat-node indices, using the adjacent closed rustc-hash 2.1.2 string
hasher. Neither payload has recursive destruction, allocation, user callbacks or
mutable key state. The map cannot escape, unrestrictedly clone or bulk-extend
through this API. Its closed `try_copy` operation is audited below.
This is not a completed ConfigValue merge or a current-input proof.

## Separate closed Path specialization

`PathTable = Map<Path>` shares the reservation mechanism, with a sealed key
domain containing only str and Path. It admits the original native byte length
before hashing or comparison. Compile-time size/alignment assertions establish
that borrowed Path/index entries have the same physical layout as str/index
entries. Path Hash/Eq scans are finite in that admitted byte length and cannot
invoke user callbacks. Keys remain immutable and borrowed from stable storage.

This specialization has new, bounded evidence rather than inheriting the
string-domain qualification: adjacent Fx provenance identifies pinned std Path
hashing and 141 observations from the unchanged dependency, including the exact
usize hash operation. Table tests establish native equality and first-spelling
retention. The target-conflict caller tests exercise grouping followed by
filtered insertion into a second map, warning order and admission refusal.
The original string alias and its allocation/order regression evidence remain
unchanged. Neither specialization establishes a complete manifest or reuse proof.

## Pinned correspondence

Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f` uses FxHashMap in
`util/context/config_value.rs`. The reference toolchain is
nightly-2026-07-11 aarch64-apple-darwin. Its installed std Cargo.toml selects
hashbrown 0.17.1 with rustc-dep-of-std (including nightly). Connection must verify
the complete compiler/stdlib/target identity; a changed pin requires renewed
layout and operation evidence. A post-reservation capacity check diagnoses drift
but does not substitute for that prior identity check.

- std `collections/hash/map.rs:3005`: FromIterator starts empty then extends.
- core `iter/adapters/mod.rs:180`: Result's GenericShunt has zero lower size hint.
- hashbrown `map.rs:4840`: extend reserves the lower bound on an empty map.
- `raw.rs:1120`: ordinary insertion reserves one before searching, even on a
  duplicate. `Table::insert` performs the same reservation fallibly first.
- std `HashMap::entry` uses `rustc_entry.rs:35`: search first, reserve only vacant.
  `Table::merge` adds a closed read-only lookup to find a fallible reserve boundary.
  Subsequent std entry has sufficient capacity and cannot allocate. Occupied
  nodes remain available for later recursive value merging through their index.
- `raw.rs:1324`: usable capacity is items plus growth_left. Removal may introduce
  tombstones; it does not establish a smaller physical allocation. The wrapper
  retains bucket extent separately. `raw.rs:2739` rehashes in place if the required
  item count is at most half the full physical capacity; otherwise it grows.
- `raw.rs:104,196,216`: bucket selection and exact allocation layout.
  The fixed entry is at least 12 bytes, above every small-item exception; Cargo's
  String/ConfigValue entry is also above those exceptions. Changing entry bytes
  therefore changes allocation bytes, not bucket count. Group selection follows
  the pinned cfg: SSE2/LSX 16, aarch64 NEON 8, generic 8 or 4 as specified there.
- alloc `alloc.rs:205`: std Global reports exactly the requested length. The
  hashbrown oversized-block branch is unreachable through this allocator.

## Closed physical copy

`try_copy` follows installed pinned std `collections/hash/map.rs:1443` through
hashbrown `map.rs:195` to `raw.rs:3326`. RawTable's `clone_from` allocates only
when the source is allocated and the destination has a different bucket count.
The wrapper first admits one actual copy API event, then the exact full physical
layout, retaining the original Session and charging old storage as before.
It starts a fresh standard map and calls fallible `try_reserve` with the full
physical capacity, **not** the source's tombstone-reduced `capacity()` or length.
`layout::growth(full)` must recover the retained bucket extent. The fresh map has
no tombstones, so its checked full capacity uniquely establishes equal physical
bucket extent before the otherwise infallible clone boundary is entered.

The zero-allocation singleton takes `raw.rs:3327`'s singleton branch and requests
no allocation. An allocated but empty source must still reserve its complete
physical extent and execute the ordinary clone branch; emptiness is not a reason
to reset its allocation, controls or growth_left. The pinned nightly
TrivialClone specialization (`raw.rs:3410`) copies all physical data buckets and
control bytes, then copies items and growth_left. The fallback (`raw.rs:3433`)
also copies controls unchanged, writes each cloned entry into its original
bucket, and copies items/growth_left. Both preserve tombstones, iteration order
and subsequent growth behavior. Borrowed str keys, usize indices and the inert
Fx builder have no allocating or user-provided clone behavior. No rehashing,
insertion reconstruction, unsafe code or raw-table recreation is used here.

Copy work is finite in B physical buckets plus the trailing G controls; it does
not hash or compare keys. As with growth, Items represents the actual operation,
not guessed bucket probes; physical allocation admission bounds the bulk copy.
`replace` admits the actual lookup and key bound before `get_mut`, changes only
an existing usize, and latches State for a missing key. This lets flat subtree
copy remap arbitrary child indices without touching keys, controls or growth.

`layout.rs` adapts only these arithmetic rules, using safe checked Layout
construction. Hashbrown's MIT/Apache licenses are adjacent. Full replacement
layouts include entry bytes, alignment padding, controls and the trailing group.
They are admitted before try_reserve; old storage remains charged. Allocator
slack is never used to authorize more logical items. Allocation/layout/refusal
errors latch in the same Session. No catch_unwind or fallback container is used.

## Finite work and account mapping

Items charges actual API operations/iteration events; String enforces the actual
key's per-string bound. It is not a cumulative byte debit. Stored keys have
already passed that bound. Allocation charges full actual layouts. No guessed
probe count is translated into extra logical items or observation-hash events.

Let B be physical buckets, G the selected group width, N occupied entries and L
the largest admitted key length. The fixed Fx hasher scans a key linearly and
allocates nothing. String equality checks length then at most L bytes. Hashbrown's
triangular probe (`raw.rs:65`) visits every group once; its load-factor invariant
retains an empty bucket. Thus a search scans at most max(B,G) controls and that
many candidate comparisons, including tiny-table replicated controls. An ordinary
operation has a fixed number of such searches; merge's extra lookup doubles a
search at most. This proves finite collision-heavy work without assuming average
constant time or charging nonexistent comparisons.

Growth (`raw.rs:2885`) visits existing physical buckets, hashes each occupied key
once and performs finite insertion probes into an empty replacement. In-place
rehash (`raw.rs:2987`) marks remaining entries DELETED, then each inner iteration
turns one such entry into FULL or moves it to EMPTY and ends that chain; swaps
also reduce the remaining DELETED count. The outer scan visits B buckets. Hence
the rehash has at most N inner iterations and finite probes, with no auxiliary
allocation. The closed hasher/key operations cannot panic through user code.

Iteration scans the finite physical table, with each next event admitted before
advancing. Drop releases one table allocation and inert entries, independently of
configuration nesting depth. Node ownership and recursive merge continuations
remain the eventual caller's separate flat-storage responsibility.

## Evidence and limits

`importer/reference/manifest/config-table/reference.rs` is a standalone harness
linked to the unchanged cached rustc-hash 2.1.2 rlib, built using edition 2021 and
std enabled. A test-only global allocator observes exact requested table layouts
while a fixed scenario executes direct std insert/entry/remove. Keys and operation
lists are built before observation; printing happens afterwards. No production
allocator is changed. Capture stdout as observations.txt on the pinned aarch64
toolchain. Tests compare every ordered map state and requested layout with this
fixture; separate owned Result collections check compact-entry bucket history.

These tests do not yet establish full nested ConfigValue conversion/merging,
include traversal, recursive clone requirements, selected getter provenance or original
source failure order. Those routes must use audited operations and provide their
own complete caller evidence before this table can participate in reuse.
