# Closed GlobSet FNV tables

The source is globset 0.4.18 in the verified cached archive, together with
nightly-2026-07-11/aarch64-apple-darwin's std and vendored hashbrown 0.17.1.
`table.rs` exposes only three concrete value families: global `usize` index
lists, `(usize, String)` required-extension builder lists, and
`(usize, glob::regex::nfa::Regex)` compiled-owner lists. The common generic map
and row storage are implementation-private. There is no arbitrary production
hasher, key, value, callback, destructor, allocator, entry guard, Clone or native
pool API. This is a table component, not a compiled GlobSet or membership proof.

## Exact source routes

| Operation | Pinned source | Local boundary |
|---|---|---|
| Literal/basename/extension add | `lib.rs:696–789`, `entry(lit.into_bytes()).or_insert(vec![]).push(global_index)` | `Indices::add` retains owned UTF-8 text admission, transfers its allocation with `into_bytes`, obtains the actual std entry, then appends the index. |
| Required-extension builder add | `lib.rs:988–1002` | `Regexes::add` retains the owned regex String until its group is consumed; it never substitutes an index into unrelated text storage. |
| Builder consumption | `lib.rs:1005–1013` | `Regexes::into_iter`/`Groups::next` use the actual standard-map consuming iterator and yield owned key plus owned text rows in actual bucket order. |
| Destination insertion | `lib.rs:1008` | `Compiled::insert_empty` clones the extension allocation, then executes ordinary map insertion with a new empty list before any regex in the group is compiled. |
| Destination append | `lib.rs:1009–1012` | The caller constructs one actual Regex, then `Compiled::push` does the borrowed `get_mut` and appends `(global_index, Regex)` in source order. Missing keys are an internal invariant error. |
| Queries | `lib.rs:708–722,738–755,771–789,896–931` | `Indices::{contains,get}` and `Compiled::get_mut` use borrowed byte lookup. Candidate empty-extension/basename guards and per-strategy iteration belong to the eventual caller. |

The consuming API keeps the extension key separate from `Texts`: the caller can
insert its cloned destination key before calling `Texts::into_iter`, exactly
as the source does. String values move without cloning through both iterator
layers. No sorting, insertion-order reconstruction, drained side list or
reinserted map changes source compilation order.

## FNV, equality and finite work

`fnv.rs` is the pinned crate's hasher: initial value `0xcbf29ce484222325`, then
XOR each byte and multiply with wrapping arithmetic by `0x100000001b3`.
`BuildHasherDefault<Hasher>` initializes one stack scalar. There is no
RandomState, Sip/Fx substitution, TLS, entropy, native call, allocation or
recoverable failure inside this hasher.

Keys are **Vec<u8>, not String**. Installed `alloc/src/vec/mod.rs:3925` hashes
Vec by its slice; `core/src/hash/mod.rs:931` hashes a slice by length prefix then
elements. `write_length_prefix:483` calls `write_usize:392`, which feeds native
usize bytes to this FNV implementation. Numeric `hash_slice:817` writes the u8
payload directly. The borrowed `[u8]` route therefore has exactly the same
stream, including empty keys, zeros, high bytes and prefix-related keys. Vector
and slice equality compare length then exact byte content; hash/tag/bucket
collisions do not determine equality. Stored keys are never mutated.

Let B be the admitted table's physical bucket count, G its fixed control-group
width, N its live entries, and L the maximum previously admitted owned or
borrowed key extent. FNV takes `size_of::<usize>() + key.len()` byte rounds;
each equality reads at most L bytes. Hashbrown `raw.rs:65` triangular probing
visits each group before repetition, and its load rules retain an EMPTY bucket.
Each lookup therefore examines a finite number of controls/comparisons bounded
by B/G/L, including replicated tiny-table controls. The preliminary lookup in
the fallible Entry adapter adds at most one such search. No average-case or
hash-resistance assumption is needed, including complete collisions.

`raw.rs:2885::resize_inner` scans existing buckets, hashes each occupied key,
and probes the fresh larger table without another scratch allocation. The
domain has no removal, drain or clear, hence no tombstones or in-place rehash
route. Bucket growth is monotonic and the old extent remains charged. Query
keys borrow the eventual admitted Candidate spans; this component adds no
Candidate construction or input ownership shortcut.

## Entry versus ordinary insert

Installed std `collections/hash/map.rs:1013` calls hashbrown
`rustc_entry.rs:35`. That function hashes and searches first, and reserves only
on vacancy. On the occupied route the incoming key is absent from the returned
entry and has dropped before `or_insert` returns the existing value. Thus an
occupied entry at exhausted capacity must not grow the table, and its discarded
equal key must not remain owned through the following row growth/refusal.

The private adapter first does a closed borrowed lookup. Only vacancy reaches
fallible `try_reserve(1)`; it then calls the actual std `entry(key).or_insert`
with enough admitted capacity. The actual occupied entry's key-drop and empty
default-value-drop order is preserved. There is no callback between lookup,
reservation and entry publication.

Ordinary insertion is different: std `map.rs:1333`, hashbrown `map.rs:1806` and
`raw.rs:1120::find_or_find_insert_index` reserve before equality search, including
full-table replacement. `Map::insert_empty` therefore prospectively reserves
before its actual std insert. The old key stays stored, the equal incoming key
drops, and the replaced owned value drops at the statement boundary. Moving
the infallible concrete FNV calculation across the prospective reserve does not
cross an observable callback, native effect or source semantic failure.

## Layout and original admission

The mechanical containment precedents are the configuration table's admitted
std Entry/insert boundary and the dense/hybrid tables' finite binary-key closure.
Their domain-specific policy choices are not generalized: this component uses
globset's **original FNV policy**, owns the original key bytes, has different
concrete values, and actually consumes bucket iteration. Its proof covers those
additional reached operations explicitly.

`layout.rs` adapts hashbrown `raw.rs:104,196,216` for the actual
`(Vec<u8>, Rows<T>)` entry, not `(State,u32)` or a guessed Vec/Regex layout.
The owned Vec key alone exceeds every tiny-item exception. B is 4/8/16 for
the source small-capacity ranges and otherwise the checked 8/7 power-of-two
rule. The full layout includes entry bytes, alignment padding, controls and
the trailing G controls; G follows the pinned cfg (aarch64 8). Std Global
reports the requested length, excluding oversized-block growth. Capacity is
checked after every fallible reserve before the following infallible operation.
Pin/cfg/allocator drift requires renewed evidence, not reliance on that check.

Rows preserve insertion order in one flat Vec and track the admitted extent.
Growth follows installed `alloc/src/raw_vec/mod.rs:158,503` using the actual
closed element layout and full replacement ownership. In particular, a large
exclusive Regex owner may have a different physical size from upstream's pool
wrapper; it is never charged as a small source Regex. Full replacement layouts
and actual moves are admitted before moving values, and allocator slack does
not authorize logical growth. Source map bucket count/order depend on entry
count/hash, not the size or capacity of these pointed-to row vectors.

`Items(1)` represents each actual map/iterator/row operation. String checks
retain real owned literal/extension/regex-text per-string limits at ingress;
they are not cumulative byte charges. Actual extension clone work charges its
full byte allocation and actual copied bytes. Binary lookup/hash/probe work
does not invent `Items(keylen)` or a decoding/string allocation. Candidate
admission and original source text construction remain caller obligations.
Every allocation and work request uses the original Session; refusal latches
before subsequent mutation, cannot trigger another algorithm, and never resets
or refunds spent work.

## Consuming ownership and teardown

Std `map.rs:2096` transfers its actual underlying consuming iterator.
Hashbrown `raw.rs:3523,4087` transfers buckets and reads the next actual entry;
`RawIntoIter::drop:4044` drops remaining entries in that iterator's order, then
deallocates the table. `Groups` wraps that iterator without collecting or
rehashing. `Texts` retains its row Vec until source-order inner consumption;
`TextIter` wraps the actual Vec iterator and preserves remaining String drop
order on early return.

Keys and text are flat owned byte/String allocations. Index values are inert.
Compiled values are the concrete Regex owner: immutable compiled Arcs,
an independent direct-factory Arc and its exclusive optional retained Core
cache. This boundary neither clones nor merges those owners. Their separately
qualified drop graph contains fixed-depth flat vectors/maps, not a user
callback or native pool guard. Partial-failure cleanup drops existing ownership
and makes no fresh admission or allocation. Drop work is finite in admitted
buckets, row lengths and retained compiled-owner extents; it never needs to
admit more work after the first refusal.

## Focused checks

Tests compare actual fixed FNV owned/borrowed streams to the pinned scalar byte
formula, occupied and vacant full-capacity behavior, actual allocation layouts,
old-key retention, genuine equal-tag/initial-bucket collision groups and direct
std/FNV iteration at growth boundaries. Consuming text tests compare actual
std bucket order and original String pointers; controlled test-only value
observers compare early iterator-drop and replacement order. Concrete compiled
rows exercise distinct retained Regex histories and global-index append order.
Refusal tests cover map/row reserve publication, every reached entry/compiled
append admission, exact request prefixes, consuming iterator refusal and text
limits. The test-only observers do not widen the production value domain.

Root owns formatting, builds, execution and qualification. No allocator/native
probe, network operation, package change, algorithm substitution, semantic cap,
new account or partial matcher connection is part of this source batch.
