# Contained semantic document provenance

This is the semantic document slice, extracted from
`toml 1.1.2+spec-1.1.0` with `std`, `parse`, and `preserve_order` semantics and the
pinned depth ceiling of 80. It uses this package's contained raw syntax APIs and
the audited `toml_datetime 1.1.1+spec-1.1.0` `Datetime::from_str` closure. Raw
source/cfg/dependency and archive lineage is in `../../provenance.json`.
The MIT and Apache licenses at the package root apply to the extracted source.

The primary local source root is
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`.
The source archive identities and independent initial audit are recorded in
`/private/tmp/rawr-p9-toml-parser-supplement-provenance.json` (SHA-256
`00fb2cd8ecdfabc1cc9e7135256f3a310431d54a765a5b1d20304d9e8f6b8594`).
The governing supplement is
`/private/tmp/rawr-p9-toml-parser-supplement.md` (SHA-256
`0bff526acdc75a441279488d1d1ef7454c866da41105610cb119ac8b825f52bc`).

| Source relative to `toml-1.1.2+spec-1.1.0/` | SHA-256 |
|---|---|
| `src/de/parser/mod.rs` | `fa8b59a71e94ca16da04a6c07af425f64944d4f47d32aa9882e3b67ae19a7d5a` |
| `src/de/parser/document.rs` | `375f952c38c1c64b2420cdc8faa2cac3c5454d81b4d1bbd34a67e74c089ab62b` |
| `src/de/parser/key.rs` | `e591ddab749739f5ae15eecdd55c1fe5bf1bd0f54f2f5aef85b0703cea1344d8` |
| `src/de/parser/array.rs` | `3703713d23fde2f4a0d917e70c20d5aad23129c4783755c4506e2876a9654cff` |
| `src/de/parser/inline_table.rs` | `cc4e1b96824f9b51dc5a780dd7044b664e0c42f21f6fe3366aff3d6ce499164d` |
| `src/de/parser/value.rs` | `5f186d2e7871b0d8140f6356eb8ef053bdc6f3f1c29a80a875fbede00172a53a` |
| `src/de/parser/devalue.rs` | `4d0fca1bdb99279032163baab25e64f8c0340fd713c237d2f860ecc4db01d247` |
| `src/de/parser/detable.rs` | `d6e5498a19046695d8f8447b111a632d41ab7060808753a5b1950a36e0159faa` |
| `src/de/parser/dearray.rs` | `e4e67bb5366da643a49d1a31214fe4f9b746b5016bec439f114062dd6e3e83c8` |
| `src/map.rs` | `e900f48839a0ec7e0a69620c8f8fb1d312e3f4d7f27376648964e05cb03393d0` |

Additional primary files:

- `serde_spanned-1.1.1/src/spanned.rs`, SHA-256
  `63ef6c841a03fd302b260b0af621db0b9677f5fb761fbeb1ac6a3877997b673a`:
  decoded value equality excludes the key span; the contained `Key` and AVL
  comparator preserve that equality without a serde dependency.
- `toml_datetime-1.1.1+spec-1.1.0/src/datetime.rs`, SHA-256
  `398619e45d1430468fa37ca325a7cc2a05c50497115be5bbd4d1b097d351b5e5`:
  only `Datetime::from_str` and its pure parsing closure are used. No `Display`
  call or error-to-string conversion occurs in the admission path.

## Control-flow mapping

`document.rs::parse` preserves the lexical pass, raw event pass with whitespace
validation and outer recursion guard, then semantic pass from upstream
`de/parser/mod.rs:28–46`. Lexer `into_vec` and the assumed token-sized event
capacity are replaced by individually admitted output and geometric fallible
storage. The semantic pass starts only when the raw pass has no failure.

`assemble.rs` corresponds to these upstream functions:

| Contained code | Pinned primary source |
|---|---|
| `document`, `header` | `document.rs:22–174` |
| `insert_unique`, dotted capture | `document.rs:193–257` |
| `finish_table`, `start_table` | `document.rs:260–370` |
| document branch of `descend` | `document.rs:373–518` |
| `key`, `close_key`, lookahead | `key.rs:11–121` |
| `array` | `array.rs:16` onward |
| `inline_table`, inline branch of `descend` | `inline_table.rs:18` onward |
| `scalar` | `value.rs:64–107` |
| integer spelling/radix and float conversion | `devalue.rs:19–101` |

Decorative debug/prefix/suffix/trailing positions that do not enter semantic
output are omitted. On a standard header, the previous table is finished before
the next header is decoded. A permitted implicit predecessor is shift-removed
on opening and appended on finishing. Existing value replacement preserves its
original key and insertion position. Decoded keys govern lookup. Explicit,
implicit, dotted, and inline flags and the latest array-of-tables child govern
extension. Array and inline values are committed on separator or close, in the
same order as upstream. Numeric spelling remains deferred: a syntactically
valid large integer is retained, and float overflow is refused by conversion
unless the spelling explicitly contains `inf`. Date validity is checked while
assembling the scalar, before enclosing duplicate-key checks.

The private ordered AVL table replaces IndexMap/Hashbrown. It stores payloads,
binary-tree links, height, insertion-order links, and a free-list link in one
vector. Search uses decoded UTF-8 byte ordering. Rotations change search links
only. Two-child deletion detaches the successor node structurally, without
swapping payloads or iteration links. Removal unlinks iteration order and makes
the slot reusable; reinsertion appends even when that slot is reused. No key
hash, random seed, uncharged clone, or name-only authority is introduced.

## Ownership and work closure

The caller supplies the existing cumulative account through `Admission`.
`Request::Items` admits actual token/event/key/value/slot ownership and the first
bounded diagnostic. `Request::String` checks prospective full decoded byte
length, including borrowed strings; it is not a byte-allocation charge.
`Request::Allocation` admits the full requested `Layout` before fallible
allocation, including every replacement. Prior allocations remain charged.
There is no filesystem/path/candidate comparison request for pure TOML work.

All growable storage is in `storage.rs`: token/event/path vectors, arrays, AVL
nodes, and owned text. Vectors grow geometrically from four slots. Text borrows
its first input slice, then allocates explicitly when expansion/concatenation
requires ownership and grows geometrically. An admitted-capacity field prevents
allocator slack from authorizing later logical growth. Only `try_reserve_exact`
can increase owned capacity; appends then fit the admitted capacity. Layout
arithmetic is checked. No `Box`, IndexMap, Hashbrown, `Cow::clone`, serde clone,
formatted diagnostic, input `Arc` copy, or unsafe code is used.

For input bytes B, tokens T, events E, semantic slots N, maximum decoded key
bytes K, and retained depth D:

- Raw lexing scans the admitted complete input; a token is admitted before it
  enters owned storage. Raw parsing/scalar recovery scans the admitted token or
  raw-scalar extent. Parser recursion remains limited to the pinned/caller
  ceiling until first failure, after which opens refuse entry.
- The event and semantic cursors advance monotonically. Recovery events that
  are retained are admitted individually; E is never assumed to equal T.
- AVL search/insertion/removal use O(log(N+1)) node visits and at most O(K) byte
  work per comparison. AVL recursion is logarithmic, with no adversarial hash
  degeneration. Order iteration is O(N). Free slots do not require growth.
- Geometric vector/string replacement has linear aggregate copying in each
  container's high-water extent. Each string append scans/copies its actual
  decoded expansion; key copies needed for inserted parent tables are explicit.
- Semantic array/inline recursive calls are bounded by the raw guard. Dotted
  descent is iterative. The separate dotted-key rule is applied after key
  decoding/path assembly, preserving its error precedence.
- Destruction is allocation-free and independent of native stack depth.
  `dispose.rs` uses one cursor already stored in each container, scans every
  physical slot once, and repeatedly descends to destroy a leaf container.
  Every child is replaced only after it has no container descendants; its own
  destructor therefore does not recurse. Work is O(N × D), without a depth
  assertion, overflow fallback, leaked subtree, or added acceptance limit.
  This also covers removed/replaced subtrees and partial semantic failure.

Pinned accepted-input construction gives a conservative retained-depth bound
of 6,719 including the root:
`1 + 2*80 + 2*79 + 80*80`. Header paths have at most 80 components and each
component can traverse an array-of-tables and its last table. Document dotted
descent has at most 79 parents, each possibly traversing such an array/table.
At most 80 nested inline/array opens can each add one container and at most 79
dotted parents. These bounds may overlap, so the sum is conservative. A
detached/current table or rejected duplicate does not create a deeper ancestry.
This is a source-derived work bound, not a new cap or a replacement for the
caller's existing limits. The destructor's safety does not depend on this sum.

`Session` owns a permanent first-failure latch across error callbacks and
builder clearing. A first diagnostic is a static tag plus bounded spans. After
failure, all further ownership requests are suppressed. `syntax::Receiver`
returns false from array/inline opens when admission or any earlier operation
failed; raw recovery then takes its iterative skip path. This is necessary
because malformed pinned recovery can emit closes without corresponding guard
opens and drive the guard's counter negative. No semantic pass is run for that
raw-failure stream. The parser's return value and a scalar-kind result never
clear failure or establish success. Immediate total cancellation is not claimed.

## Boundary and verification

`Document` retains the exact supplied `&str`; constructors do not read files,
establish EOF, mint freshness, select a package, or bind source/Git identity.
The importer must obtain complete bytes from its associated whole-file
observation, enforce its existing byte limit, validate UTF-8, and borrow the
original account. Later typed loader, workspace/discovery, current replay, and
owner joins remain separate required work.

`document::tests` exercises actual contained operations: reorder and removal,
escaped collisions, flags, array-table descent, all scalar classes and deferred
overflow, date/error precedence, ignored-field decoding, Unicode/string growth,
prior-use exact/floor admission, refusal at every request, recovery output,
negative-depth malformed recovery, AVL rotations/free-slot reuse, and compounded
dotted-inline destruction. The root owns compilation and test execution. A
separate pinned-reference harness owns differential evidence; no successful
differential run is inferred from these expected-output unit tests.

## Executed development checks

Root ran the 27 raw/semantic library tests, 13 added whole-file observation
tests with the 38 existing membership tests, and five EOF/UTF-8/document
admission integration tests. The 35-case pinned reference comparison also
passed; see `../../reference/README.md` and its recorded output checksum.
The importer check without default features and the CLI import-baseline check
passed. These are focused
development checks using nightly-2026-07-11, offline, locked after intentional
local dependency additions, and jobs 1. They are not release qualification.

An independent read-only source review of semantic assembly, admission, recovery,
teardown and the importer adapter found no P0/P1 defects or unsupported authority
claims, at confidence 0.88. It compared the pinned TOML archive and nine source
members. The separate source-derived depth review identified the malformed
recovery counterexample addressed by the permanent open-callback refusal.
Final whole-candidate review remains downstream of product integration.
