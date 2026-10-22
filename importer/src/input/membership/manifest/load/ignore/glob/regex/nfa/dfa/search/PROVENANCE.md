# Private dense forward query boundary

This implements only the query shapes reached by globset 0.4.18. It does not
activate Core selection, cache pools, production membership, or qualification.
The finished forward and reverse tables remain owned together; these two query
shapes read the forward table only.

Source roots:

- globset archive: `~/.cargo/registry/cache/index.crates.io-1949cf8c6b5b557f/globset-0.4.18.crate`;
  inspected with `tar -xOf`, without unpacking or running a producer.
- regex-automata: `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/regex-automata-0.4.14/src/`.
- memchr: `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/memchr-2.8.2/src/`.

## Calls and ordering

globset `src/glob.rs:146–148`, `src/lib.rs:860–888,914–931` call either
`meta::Regex::is_match` with the whole candidate path or
`which_overlapping_matches` with a whole-haystack `Input` and a caller-owned
PatternSet. `src/lib.rs:271–305` disables UTF-8 empty handling, keeps byte input,
and selects All for regex sets. `meta/regex.rs:530–539` sets earliest=true only
for is_match; `meta/strategy.rs:765–797,861–899` and
`meta/wrappers.rs:947–956,1024–1033` reach the forward DFA methods. Reverse search,
bounded input spans, arbitrary anchored modes, capture searches, and UTF-8-empty
search continuations are outside these closed producer calls.

The Cargo glob producer uses literal_separator=true, reflected in the closed
emitter `glob/regex.rs:175–176`: `?` emits `[^/]` and `*` emits `[^/]*`.
Consequently slash is a non-loop accelerator needle for a star state. The
actual one/two/three-needle fixtures are `*` (`/`), `*a` (`/a`), and `*{a,b}`
(`/ab`). `*{a,b,c}` has four exit bytes and is not accelerated. These fixture
facts do not change the language or constructor configuration.

`search.rs::first` corresponds to `dfa/search.rs:14–184`, with the four-byte
unroll expanded into the same ordered transitions. It returns a delayed match
before acceleration or later quit bytes. Overlapping state corresponds to
`dfa/search.rs:312–443`: ordered match IDs at one position precede the next input
transition. The caller inserts each ID separately and checks fullness
immediately afterward (`dfa/automaton.rs:1808–1829`). Dense uses
`PatternSet::insert`, whose insufficient-capacity path panics
(`util/search.rs:1225–1228`), unlike hybrid/Pike's fallible try_insert. Globset
allocates the set from the same matcher's pattern_len (`src/lib.rs:971–980`),
so a reached Full here violates the private producer contract. It becomes
terminal internal Error::State, not successful matching, semantic fallback,
or newly authorized source-panic behavior. Existing IDs remain retained.
There is no eager capacity guard; legitimate saturation still stops after an
in-range insertion, and a no-match search does not reach an insertion failure.

Whole-haystack EOI corresponds to `dfa/search.rs:578–609`: synthetic EOI is
always taken after input exhaustion or accelerator miss. A quit state reached
by synthetic EOI is not a byte-quit error. The start lookup is the unanchored
Text entry (`dfa/dense.rs:3264–3280`); the universal-start cache belongs to
prefilter restart and does not replace this source lookup. The private producer
has no prefilter and disables UTF-8 empty handling; other configurations are
internal contract errors, not semantic fallback.

Semantic Quit/UnsupportedAnchored results remain nested under the original
Session Step. Admission refusal, arithmetic failure, invalid retained state,
and unavailable/unfinished engine state never become semantic misses or
fallback. Query scratch is fixed scalar state and borrows the haystack/table;
the caller's existing admitted PatternSet owns result bits.

## Acceleration helper audit

`dfa/accel.rs:94–109` calls `util/memchr.rs:10–30`'s selected perf implementation.
globset's Cargo.toml enables regex-automata std+perf; regex-automata Cargo.toml
perf → perf-literal → perf-literal-substring enables memchr, while std enables
memchr/std. The scalar `not(perf-literal-substring)` branch is not the selected
source. A scalar replacement is not accepted as correspondence.

The pinned Cargo lock selects memchr 2.8.2 with checksum
`88904434abc2901f197fe8cc55f0445e7ded921dba5911dad2e2b39b48e663c4`.
The initial implementation used Rawr's existing 2.8.3 lock entry; that was the
wrong dependency context. This direct dependency now follows pinned Cargo.
Historical 2.8.3 debug fingerprints `bba9ca44e34454df` and
`0c0b63e3c8c69457` report alloc+std; `52e582c870b80eda` and
`d86c394419ae1f59` additionally report default. All four have no dependencies,
logging, or rustflags. These observations do not replace checking the final
build's feature closure. `src/macros.rs:5–15` compiles logging bodies away when
logging is absent. Enabling logging would invalidate this helper closure.

The project's supported target is aarch64-apple-darwin.
`memchr.rs:27–34,92–99,161–171` reaches
`arch/generic/memchr.rs:1125–1142` and the raw aarch64 dispatch in
`memchr.rs:504–657`. `arch/aarch64/memchr.rs:8–32` selects NEON at compile time
when available, otherwise the all-target word implementation. Neither aarch64
branch uses runtime CPU detection, global function-pointer caches, callbacks,
OS calls, native libraries, recursion, or dynamic allocation. The x86/wasm
dispatches do not establish authority for unsupported targets.

- NEON One/Two/Three `find_raw`, `arch/aarch64/neon/memchr.rs:156–175,
  504–524,790–812`: zero length returns immediately; fewer than 16 bytes uses
  `generic::fwd_byte_by_byte`; larger inputs use generic vector search.
- Generic forward loops `arch/generic/memchr.rs:143–228,493–566,765–848`:
  one initial 16-byte load, advancing aligned groups (four vectors for One,
  two for Two/Three), remaining aligned vectors, and at most one overlapping
  final vector. The aligned middle covers each byte at most once. Including
  both boundary vectors gives at most n+32 visited byte lanes for suffix n.
- `vector.rs:298–368,432–457` contains fixed load/splat/compare/or/mask intrinsics
  and trailing-zero offset selection. Searchers retain at most three scalar
  needles and three 16-byte vectors on the stack. No heap storage is reached.
- Without NEON, `arch/all/memchr.rs:127–171,437–478,701–742` uses an initial
  unaligned usize, strictly advancing aligned words, and one final scalar
  confirmation scan. Even charging that final scan as a whole second pass
  gives at most 2n+8 byte lanes. `has_needle`/`confirm` at 281,546,810 and
  `has_zero_byte` at 877 use fixed scalar operations and no ownership.

A common conservative ARM work bound is therefore 2n+32 lanes, with one read
and at most k needle comparisons per lane, k in 1..=3. This proves finite work;
it is not a new cumulative byte/lane debit or a machine-instruction count.
32 is the two possible 16-byte vector boundary loads, not a new capacity.

The existing logical operation vocabulary admits Items(1) at entry to
`table::accelerate` for the actual accelerator-search event, before dispatching
to the exact safe `memchr`, `memchr2`, or `memchr3` helper. No String or
Allocation request is issued: the call borrows its input and all reached
helper storage above is fixed stack data. Refusal prevents entering the helper
and leaves every downstream cursor update unexecuted. The parent adopts this
as routine containment, with the internal finite bound playing the same
evidence role as other proven closed helper operations; no cap or accounting
policy changes. The direct dependency is exact memchr 2.8.2 with
default-features=false and std enabled.

`table/tests.rs` observes the actual helper entry through a test-only thread
local counter. It checks refusal before that entry, single-event accounting,
no requested allocation, empty suffixes, all three needle counts, ordered hits
and misses across vector/alignment boundaries. `search/tests.rs` separately
covers real completed engines, byte classes, match/EOI timing, saturation,
semantic errors, and original-Session refusal through both query APIs.

Renew this closure when the dependency version, unified features (especially
logging), supported target, compiler/target cfg, or helper call vocabulary
changes. The existing debug fingerprints are read-only evidence of the inspected
configuration, not proof of a future build. Root must verify the actual final
feature/configuration closure with its ordinary checks. No builds or tests were
executed by this source implementation agent.

Historical validation at06b2210e, before the pin correction: root executed the
development feature/cfg checks after adding the direct edge:
`cargo tree --offline -p rawr-import --features import-baseline -e features
--format '{p} features=[{f}]'` reported memchr 2.8.3 with alloc+std and no logging;
`rustc +nightly-2026-07-11 --print cfg --target aarch64-apple-darwin` reported
neon, aarch64, macos and 64-bit pointers. The lock change was only the importer's
direct memchr edge. These are executed development checks, not final candidate
qualification or a waiver of the renewal requirement above.

The exact Cargo lock was restored from commit59800466c5c41c444d264b1010b4d57e85a7117f
at `/private/tmp/rawr-pinned-cargo-lock-sep26.toml` (136477 bytes, SHA256
`9a3e2c4418edbcaadbb1e5b5998a7c52c9131d2e62d64995b15307de731c8a97`).
Independent comparison of the two complete memchr source trees found only the
generic slice wrapper's added in-bounds optimizer assumption and private x86 raw
functions' added unsafe declarations in2.8.3. All six forward/reverse one/two/three
needle API signatures/results, ARM dispatch/search loops, allocation behavior and
features remain unchanged. The finite-work argument above therefore applies to
2.8.2; identical generated machine code or timing is not claimed. No search-code
change or additional test scaffold is needed for this mechanical correction.
