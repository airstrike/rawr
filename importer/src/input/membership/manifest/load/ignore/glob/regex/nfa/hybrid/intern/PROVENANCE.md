# Closed hybrid table containment

The adapter is exactly
`HashMap<dfa::key::State, u32, BuildHasherDefault<DefaultHasher>>`.
The regex, Rust, std, hashbrown 0.17.1, target/cfg and Global allocator pins are
those recorded in `../../dfa/intern/PROVENANCE.md`. The exact encoded State type
and layout arithmetic are shared; the dense interner's behavior is not changed.
This adapter has only new, get, insert/replacement, len and clear. No production
iteration, individual removal, callback, key/map escape, Clone or Debug exists.

## Hashing, equality and reserve order

`std/src/hash/random.rs:106` constructs SipHasher13 with zero keys. There is no
RandomState constructor, lazy TLS or native entropy call. `alloc/src/sync.rs:3869`
hashes Arc by payload; `core/src/hash/mod.rs:931` hashes slices by length and
contents. Owned and borrowed binary keys therefore use identical streams.
The derived single-field key Eq and slice comparison compare exact content,
including zero/high bytes, lengths and common prefixes. Hash/tag/bucket
collisions never decide equality. Fixed SipHasher13 consumes finite byte chunks
and a fixed tail on the stack (`core/src/hash/sip.rs:256–300`); there are no
allocations, admissions, callbacks or recoverable failures inside hashing.

Pinned hashbrown `map.rs:1806` hashes then calls
`raw.rs:1120::find_or_find_insert_index`, which reserves one slot before any
equality search. Accordingly, even replacement at exhausted capacity admits the
full next layout and fallibly reserves before insert. Moving the incoming hash
across that prospective reserve crosses no effect in the established concrete
hash closure. The resulting capacity must match the pin, proving that the
following std insert cannot allocate. Standard replacement keeps the existing
stored key and drops the incoming equal key; only its tagged u32 value changes.

The layout arithmetic follows `raw.rs:104,196,216` for the exact
`(State,u32)` entry: bucket storage, padding, alignment, controls and trailing
group. This fat-Arc entry takes no tiny-entry exception. Each replacement extent
is charged fully, with no refund for the old table. Allocation error or capacity
drift latches the original Session terminally before insertion.

## Repeated epochs and finite work

`raw.rs:833::clear` releases entries without freeing buckets.
`clear_no_drop:3184` fills controls with EMPTY, sets items to zero and restores
growth-left to full capacity. No tombstone survives a clear and no individual
deletion can create one. Between clears only new insertion and value replacement
occur. Thus reserve is necessary precisely at exhausted capacity; the tombstone
in-place rehash branch in `reserve_rehash_inner:2739` is unreachable. Retained
empty buckets authorize reuse, not a fresh allocation request or account refund.

Let B be retained buckets, G the pinned control-group width, N live entries,
L the maximum admitted encoded key length, and E the number of reached epochs.
`raw.rs:65`'s triangular probing visits every group; load rules guarantee an empty
bucket. Even complete collisions give finite hash/equality/probe work bounded
by admitted B/G/N/L, including replicated controls in tiny tables. No expected
constant-time assumption or hash-resistance premise is used.

`resize_inner:2885` scans old occupied buckets, rehashes each finite key and
inserts into the larger table, with no further scratch allocation. Nonempty
clear scans control storage as well as dropping N entries, so its bound depends
on retained B, not just current N. Across epochs the bound includes E times the
retained high-water extent. Each actual clear and sentinel reconstruction is
reached through the same finite original account; the source's reset counters
cannot replenish that account. Empty clear returns before scanning controls.
These bounds justify closed operations; they are not invented extra debits.

`Items(1)` precedes each actual get/insert/replacement; `Items(len)` precedes
entry release at clear. Binary keys emit no String, opaque hash-byte-scan,
filesystem or observation-hash request. Actual encoded-byte work, Vec/Arc
allocations, canonical slots, copies and Arc shares remain separately admitted.

## State identity, lifetime and refusal

Hybrid state IDs derive from transition length after semantic fit/possible
clear (`hybrid/dfa.rs:2285–2344`). The canonical vector, not table order,
determines state lookup. Sentinel insertion intentionally replaces one equal
key three times and finally restores dead at2570. Source memory usage at2021
uses map length; retained bucket capacity never selects fit or efficiency
failures. Predicted one-more-state memory remains charged semantically even
when the next source insertion is a sentinel replacement.

Canonical/vector/map/incoming/saved Arc owners are distinct. Save clones the
canonical State; vector then map clear leaves that saved owner alive; init
recreates sentinels before saved-state insertion and tag restoration. Reset
discards that obligation before clear. Key payloads are flat byte slices, so
every partial-failure teardown is flat and requires no new admitted allocation.
Any Session refusal stops before subsequent mutation/admission; it never becomes
a semantic cache error or permits retry on a new account.

The private wrapper does not derive cache/map Clone or Debug. Upstream
`meta/regex.rs:1911` clones Regex with a new pool; it does not require cloning
populated hybrid caches. Any separately reached clone, debug dump, iteration,
seed-sensitive behavior, callback, key mutation or pin/cfg change reopens the
closure before connection. This containment does not claim native RNG failures or speed.

Tests use the actual fixed hasher and the real State builder, including a
deterministically found equal-tag/equal-start-bucket group. They do not inject
a replacement hasher or claim manufactured equal 64-bit hashes. Source bounds
cover complete collisions. Growth/replacement/clear tests check retained
capacity, old-key versus incoming-key Weak lifetimes and refusal before table
mutation. Cache tests check tagged canonical results, saved Arc lifetimes,
source semantic failures and original-account terminal behavior.
