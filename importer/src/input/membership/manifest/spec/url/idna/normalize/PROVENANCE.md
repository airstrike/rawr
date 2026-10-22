# Contained ICU UTS46 normalization

This is the complete mapping/normalization component reached by idna_adapter
1.2.2 with icu_normalizer 2.2.0. `map_normalize` and `normalize_validate` construct
a fallible `Normalizer<I>` with the original Session. `next(&mut Session)` emits
one transformed scalar or EOF; it carries no IDNA, DNS, URL or package authority.
The private continuation and actual transformed output implement
`smart-constructor-newtype`; no unvalidated raw-success marker is returned.

## Exact source and build closure

Registry source root:
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f`.
Root verified every relevant source/data member against its pinned archive in
`/private/tmp/rawr-p9-idna-source-verification.json`, SHA-256
`eb9b9edd2df181523d72d38561a4c982ec762a7c7acc464b19efc1c21eae5aa9`.
That record includes the complete transitive archive identities, member hashes,
and exact data-crate `build.rs` archive comparisons;
the reached members below were also read directly for this extraction.

| Member | SHA-256 |
|---|---|
| `icu_normalizer-2.2.0/src/lib.rs` | `a4ddf07687cf76033c8cff7f29550f5688f55e6363572881dd250a812ac6b39f` |
| `icu_normalizer-2.2.0/src/uts46.rs` | `b2f7093211a0bf946ee59927fb988a2915643f29823bd5709417b8950747e2eb` |
| `icu_normalizer-2.2.0/src/provider.rs` | `5850afc7ae842c7af74ce029be256944c64f5d0b51d95725a8366f5af22163e9` |
| `icu_collections-2.2.0/src/char16trie/trie.rs` | `4e42e18270fcd8805ccbc589e3d618c7ca3cfefb60da4710e823fb2028db5011` |
| `icu_collections-2.2.0/src/codepointtrie/cptrie.rs` | `3d1c54488035c9e045c568a87d43448c4e980962376e516ba62d02f5d19c387a` |
| `zerovec-0.11.6/src/zerovec/slice.rs` | `b587331ee27282c0bf5364aa806433b9fc4fd9b72a365a34d6ab34856559fe7e` |
| `icu_normalizer_data-2.2.0/data/normalizer_uts46_data_v1.rs.data` | `0b52640d8e0249ae1ce7f103057a53007f4fa51fd0782a62f9c24b5025a1e803` |
| `icu_normalizer_data-2.2.0/data/normalizer_nfd_tables_v1.rs.data` | `08f669c94e57f59894bc6a78ebc52006a3fd71544128a79200a6a04c06df00a5` |
| `icu_normalizer_data-2.2.0/data/normalizer_nfkd_tables_v1.rs.data` | `60286bb59fd862264cd1144dd1dc6f75fe3d911a45af382b8737fdbb8f730501` |
| `icu_normalizer_data-2.2.0/data/normalizer_nfc_v1.rs.data` | `b755f662862b17d1000475f546a8b668ba4494eb05183e4f06b1e26db0e8ee17` |

The ICU4X license is Unicode-3.0. Its complete license is retained as `LICENSE`,
SHA-256 `f367c1b8e1aa262435251e442901da4607b4650e0e63a026f5044473ecfb90f2`.
The direct dependency seam is exactly icu_normalizer 2.2.0 with default features
disabled and `compiled_data`, plus icu_collections 2.2.0 with default features
disabled. No direct zerovec or provider dependency is required.

Root's executed dependency/build evidence is
`/private/tmp/rawr-p9-idna-dependency-check.json`, SHA-256
`6c1152af7d254b0d4b81c878b1f5b2c046892758cd231f45d685f1a1d5a0357a`.
It records exact package/checksum correspondence, absent `ICU4X_DATA_DIR`, and
actual data-crate build outputs without `icu4x_custom_data`. This matters because
`icu_normalizer_data/build.rs` selects alternative data through that environment
variable, and `src/lib.rs:14–17` selects the corresponding include.

The inspected normalizer fingerprints
`target/debug/.fingerprint/icu_normalizer-{755a7ee8a6507be9,5f5bd268888ef3c7}/lib-icu_normalizer.json`
both record only `compiled_data` and empty rustflags. The executed pinned typed
reference feature record is `importer/reference/manifest/features.json`, SHA-256
`744a297f0a45a50063c11d527c6194618417cd616c8bb5dceb4535d6d6f0cc75`:
normalizer `compiled_data`, collections no features, provider `baked`, zerovec
`derive/yoke`. Thus the reached normalizer uses its untyped CodePointTrie and
local u8 CCC representation; utf8/utf16 convenience normalizers, serde/datagen,
HarfBuzz, and the optional icu_properties CCC wrapper are outside this call path.
Root's extended checked/test-compiled feature inventory for all eleven reached
packages is `/private/tmp/rawr-p9-idna-resolved-features.json`; each recorded
feature set matches the pinned reference and each rustflags list is empty.

## Immutable data access proof

`provider.rs:28–50` installs Baked's generated implementations. Each used data
macro declares a public `&'static` singleton (`*.rs.data:24`). The UTS46 trie,
canonical NFD tables, supplementary NFKD tables, and NFC composition trie are
the exact four objects selected by `lib.rs:1769–1812,2472–2505`.

The contained code borrows those constants directly. It does not call provider
`load`, construct or clone a DataPayload, clone a ZeroVec/Char16Trie, or invoke a
stock normalizer iterator. It retains the original compile-time bounds for the
table offsets and passthrough cap. Both actual passthrough bounds are derived
from the UTS46 table's cap, using the same min-with-0xC0/min-with-0x300 logic.

The shared lookup closure performs no allocation:

- `CodePointTrie::get` (`cptrie.rs:672`) uses checked scalar input and fixed
  fast/small indexing (`:504–636`), reading only the immutable index/data slices.
  Small lookup has a fixed number of indexed reads; no traversal container or
  runtime data parsing is constructed.
- `Char16Trie::iter` (`trie.rs:99`) returns only a borrowed ZeroSlice, position and
  remaining-match state. `next`, `next16`, branch/search/value helpers
  (`:200–594`) operate on that fixed state. Composition consumes second then
  starter, at most four UTF-16 units, using the exact pinned branch/value logic.
- `ZeroSlice::get`, `get_subslice`, and `iter` (`slice.rs:192,239,370`) return
  copied scalar values or borrowed subslices/iterators. Their concrete u16/char
  decoding does not allocate. The local generic decomposition helper iterates
  only those exact borrowed subslices; it does not collect them.

Shared safe lookup methods contain upstream audited unsafe implementation, but
this extraction introduces no unsafe block or alternative data representation.
Its local scalar fields eliminate ICU's packed-character unsafe reconstruction.

## Program correspondence

| Local operation | Pinned source and preserved behavior |
|---|---|
| Two constructors | `uts46.rs:109–158`: mapping ignores the ignorable marker; validation converts it to replacement. Both otherwise perform the same mapping and NFC. Validation still requires the caller's input/output comparison. |
| Priming | `lib.rs:627–659`: fixed U+FFFF placeholder, followed by one decomposition step before returning; leading combining marks and one pending starter can already be consumed. |
| Delegate and lookahead | `lib.rs:741–795`: decomposition passthrough, marker treatment, pending take and exact source-consumption order. |
| Decomposition | `lib.rs:798–921`: self mapping, packed two-BMP expansion, Hangul, singleton, FDFA, all four BMP/non-BMP canonical/supplementary table branches. |
| Expansion tails | `lib.rs:661–737`: same first scalar, placeholder CCC for nonstarter-only tails, and last-starter offset for general tails. |
| Combining gather | `lib.rs:930–1001`: identical ordinary CCC path and all nine special nonstarter cases, including Tibetan, Greek and halfwidth kana expansion. |
| Stable ordering | `lib.rs:543–555`: only a combining slice of length at least two resolves placeholder CCCs and sorts. Local `(ccc, ordinal)` heapsort preserves the source stable equal-CCC order. |
| Composition | `lib.rs:1089–1270`: buffered starter versus pending starter, passthrough lookahead, contiguous composition, blocked/discontiguous matching, deferred starter and EOF paths. |
| Pair lookup | `lib.rs:357–420`: identical Hangul range arithmetic and second-first non-Hangul trie lookup. Safe scalar construction replaces unchecked conversion. |

The source debug assertions for impossible provider corruption are not new
input-policy checks. Fixed pinned data is the constructor's data authority;
replacement/None fallbacks remain the source's release behavior. All Unicode
scalars, including supplementary scalars, are in scope. No DNS/Punycode guard,
new combining-run cap, transitional mapping or ASCII deny list is introduced.
Those operations belong to the later complete IDNA consumer.

## Actual ownership, work and failure

`Buffer<Character>` is the only input-dependent retained scratch. Each actual
record contains `char`, CCC and an encounter ordinal. `Buffer::push` admits its
item and full replacement layout before allocation or append, with admitted
capacity tracked independently of allocator slack. Ordinals are assigned to the
existing admitted records immediately before sorting, so equal CCC ordering
requires neither a hidden sort allocation nor a separate uncharged index vector.

Leading scratch is admitted during constructor priming. Later expansion/gather
scratch is admitted during `next`, before the corresponding output can be emitted.
Buffer reuse, prefix removal and clear neither shrink nor refund old layouts.
Prefix drain and individual removal shift only already admitted records, preserve
order and allocate nothing. An empty prefix drain is a constant-time no-op.
There is no scalar/String/output-only precharge and no fresh budget: returned
chars are fixed values; the consumer admits its actual subsequent char/String
storage and expansion through the same Session. Pure transformation charges no
filesystem path, hashing or candidate-comparison event.

Each call checks the shared terminal latch before touching the delegate. Any
refused construction returns no continuation. Refusal during `next`, or a shared
Session refusal between emissions, permanently marks the continuation refused.
No further delegate read or output can follow. This boundary has no semantic
trial rejection: invalid IDNA statuses map to the pinned replacement output for
the consumer to judge, while resource failure remains `Stop::Refused`.

For finite input of `n` scalars, the pinned data encoding expands a scalar to at
most 18 scalars: FDFA has 18, BMP table lengths have at most 15+2, non-BMP lengths
at most 15+1, Hangul at most three, and special nonstarters at most two. Write
`e <= 18n` for this conservative total. Delegate progress is monotone; pending
lookahead is taken once, and every buffer record is either emitted or removed
by composition. No recursive decomposition or normalization call exists.

Stable ordering costs at most `O(e log e)` across combining runs. Pair lookup has
a fixed bound from the immutable two-scalar trie key. Prefix and discontiguous
removals cost at most `O(e²)` in the conservative worst case, as each successful
removal or emitted prefix reduces the finite retained sequence and each shift
touches at most `e` records. This preserves the source's work domain; it is not
an invented linear-work credit. Actual source and scratch cardinality remain
governed by the original cumulative account. All retained dynamic storage is one
flat primitive-record buffer; success and every partial-failure teardown have
constant call depth.

## Validation prepared for root execution

Seven focused tests exercise literal mappings, distinct ignorable modes, stable
CCC ties, special expansions, composition, constructor priming, scratch reuse,
all refusal positions and exact original item/layout floors after prior usage.
The original-account fixture exhausts unrelated GraphBudget operations first and
uses a zero string ceiling for this char-only component; the caller's later
output materialization is a separate real storage operation.

The direct pinned reference uses `Uts46MapperBorrowed::{map_normalize,
normalize_validate}` from exact icu_normalizer 2.2.0. It compares both emitted
scalars and delegate poll counts after construction and every `next`, including
repeated EOF. A streamed sweep covers every Unicode scalar with separators in
both modes, without collecting a fixture or expected output. Context fixtures
cover combining stability, discontiguous composition, Hangul, multi-starter
expansions, supplementary values, ignored characters, and runs far beyond the
stock inline buffer. Ordinary fixture/reference allocations are test-only.

Source readiness and root's dependency check are not claims that these tests have
run. Root owns compilation, exact-reference execution and the resulting logs;
complete IDNA/URL integration and acceptance remain separate.
