# Default Aho-Corasick matcher

This private matcher constructs the mandatory noncontiguous NFA, then applies
the pinned default DFA/contiguous/noncontiguous selection. It retains the
selected engine's actual shared allocation and supplies Standard unanchored
overlapping search. Complete package selection and product admission remain
outside this component.

## Exact source

Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f` pins:

| Crate | Version | Archive SHA-256 |
|---|---|---|
| globset | 0.4.18 | `52dfc19153a48bde0cbd630453615c8151bce3a5adfac7a0aebfbf0a1e1f57e3` |
| aho-corasick | 1.1.4 | `ddd31a130427c27518df266943a5308ed92d4b226cc639f5a8f1002816174301` |
| memchr | 2.8.2 | `88904434abc2901f197fe8cc55f0445e7ded921dba5911dad2e2b39b48e663c4` |

The restored authority lock is `/private/tmp/rawr-pinned-cargo-lock-sep26.toml`,
SHA-256 `9a3e2c4418edbcaadbb1e5b5998a7c52c9131d2e62d64995b15307de731c8a97`.
Globset's default-feature Aho dependency enables `std` and `perf-literal`.
The pinned Aho package has only memchr in its resolved dependencies, without its
optional logging dependency. The importer uses the exact memchr version with
`std` (which includes alloc), without logging. No Aho dependency is added.
Source was read from the matching local archives/registry sources.
Upstream MIT and Unlicense notices are retained at `../../LICENSE-MIT` and
`../../UNLICENSE` (their texts also cover aho-corasick's Andrew Gallant notice).

Globset `src/lib.rs:940–974` borrows its owned literal Strings into
`AhoCorasick::new`, retaining them through construction. Aho
`src/ahocorasick.rs:2171–2267` builds the noncontiguous NFA before attempting a
DFA for at most 100 patterns, then contiguous NFA, then noncontiguous fallback.
All three choices are retained. This specialization retains default Standard
matching, case sensitivity, prefilters and dense depth 3. Standard's
`util/search.rs:1103` returns no packed kind, so no packed/Teddy builder exists.

## Completed correspondence

| Local operation | aho-corasick 1.1.4 source |
|---|---|
| Construction order and retained compiler | `nfa/noncontiguous.rs:935–1051`, Compiler::new/compile |
| Trie, pattern lengths, duplicate IDs | `:1057–1148`, build_trie |
| Sparse links, matches and allocation limits | `:319–610`, next_link/follow_transition/add_transition/init_full_state/add_match/copy_matches/alloc_* |
| BFS failure edges and ordered suffix matches | `:1273–1383`, fill_failure_transitions; queued_set is inert |
| Match/start partition and all references | `:1407–1503`, shuffle; `util/remapper.rs:101–177`; NFA::remap `:260` |
| Dense rows and both start groups | `:1521–1612`, densify/init_unanchored_start_state/set_anchored_start_state/add_unanchored_start_state_loop |
| Prefilter constructor/add/selection | `util/prefilter.rs:135–325,348–377,518–663,759–855` |
| Rare-byte ranking | exact `util/byte_frequencies.rs` table |

Semantic index/pattern-length failures retain SmallIndex's `i32::MAX-1` ceiling
on the supported 64-bit target (`util/primitives.rs:101`). They stay separate
from terminal original Session refusal. No new semantic size cap is introduced.
The packed nine-byte transition and four-byte IDs retain their source layouts.
An admitted initial StartBytes Vec is followed by sentinel transition/match/dense
entries, four states, both full start rows, and the dead row in source order.

The compiler retains its prefilter builder until after final NFA compaction;
failure drops that builder before the NFA, matching source field order. Empty
patterns disable further prefilter collection without prematurely dropping the
retained single-pattern candidate. Both start and rare candidates are built
before choosing between them; refusal building the discarded candidate cannot
be converted to successful selection. Shared prefilters retain actual Arc
ownership; conversion clones admit the operation before the actual Arc clone.

## Original admission and finite closure

Every actual Vec/VecDeque growth reserves the complete new layout before
allocation; source Vec minimum/doubling and source compaction points are retained.
Replacement storage stays alongside its predecessor through admitted moves.
Original text limits are checked while ingesting the borrowed Strings. Trie,
linked-list, queue, byte-class, failure and remap visits are admitted before work.
The queue is a real FIFO VecDeque, not an ever-growing history. Case-sensitive
trie edges are unique, so the source inactive queued set needs no allocation.
All ownership is flat and teardown needs neither recursion nor a new account.

The one-pattern prefilter reaches exact memchr 2.8.2
`memmem/mod.rs:392,470,681–690` and `cow.rs:65`: first build a borrowed Finder,
then one owned needle copy, then the admitted Arc allocation. `Finder::new` is
an allocation-free closed operation over the already admitted needle; one
Items request precedes it, not an invented estimated scan debit. On the supported
aarch64 target, `memmem/searcher.rs:44–164` reaches Rabin-Karp initialization,
fixed-ranker packed-pair selection and optionally Two-Way initialization.
`arch/all/rabinkarp.rs:89` scans the needle; packed pair `:163` considers at most
255 bytes; Two-Way `:114,440,500,655` has fixed-size state, bounded slice scans,
and suffix loops whose candidate position increases whenever the offset resets.
A conservative finite bound for each suffix loop is quadratic in admitted
needle length. It is proof of closure, not a charged estimate. NEON availability
is a compile-time cfg at `arch/aarch64/neon/packedpair.rs:90`; this path reaches
no RNG, TLS, environment, I/O or dynamic feature discovery. `into_owned` copies
exactly the admitted byte extent; the subsequent Arc layout uses the pinned
two atomic counters plus the actual payload, following the existing NFA storage
helper's containment. Search has its own closed helper proof below.

The conversion interface exposes only immutable state/byte-class/length/special
views, admitted ordered transition/match cursors, admitted prefilter Arc clones,
and the source unanchored failure-following `next_state`. No generic automaton,
callback, arbitrary allocator or replacement hasher is exposed.

Focused checks cover direct/duplicate/inherited match order, empty patterns,
shuffle+dense correspondence, immutable conversion cursors, semantic overflow
ordering, prefilter selection/retention and representative terminal refusals.
Root owns compilation and execution; this file makes no qualification claim.

## Default execution and query closure

`engine.rs` follows `ahocorasick.rs:2213–2267`: a DFA conversion is attempted
for at most 100 patterns, then contiguous conversion, then noncontiguous
fallback. Only source semantic conversion failure permits fallback. Original
Session refusal and the selected engine's Arc admission remain terminal.
The predecessor stays live through conversion and final shared allocation.
The closed enum replaces only erased dispatch; it retains a separate actual
Arc allocation for the selected concrete engine. Conversion correspondence is
in `dfa/PROVENANCE.md` and `contiguous/PROVENANCE.md`.

`search.rs` preserves `automaton.rs:1443–1550`: initial empty matches, pending
duplicate/inherited matches, byte advancement, special-state handling and
prefilter restart. Noncontiguous match counting and indexed selection retain
the source linked-list traversals; contiguous queries decode the source packed
words, and DFA queries consume premultiplied state IDs. State storage is private
and produced only by the matching constructor. Query failure never publishes
partial IDs as successful completion.

`prefilter/search.rs` follows `util/prefilter.rs:398,675,699,722,864,881,899`,
including `Candidate::into_option`: a confirmed memmem match contributes its
start, and rare offsets are clamped to the searched span's lower bound. Each
exact helper search has one original Items admission before invocation. The
memchr/memchr2/memchr3 closure is the same pinned ARM closure documented in
`../regex/nfa/dfa/search/PROVENANCE.md`; no duplicate estimated byte debit is
invented.

For the retained memmem Finder, memchr 2.8.2 `memmem/mod.rs:421` creates only a
fresh fixed `PrefilterState` (`searcher.rs:876–940`, two u32 counters), borrows
the retained needle, and invokes constructor-selected immutable dispatch.
No allocation or Arc clone occurs, and no retained Finder state changes.
Length-one dispatch reaches memchr; lengths 2–32 reach compile-time NEON packed
search, whose vector offsets progress and candidate masks clear one bit per
confirmation (`arch/generic/packedpair.rs:80–148,229–257`). Each confirmation
compares at most 32 needle bytes. Short haystacks use Rabin–Karp, whose candidate
position advances per loop (`arch/all/rabinkarp.rs:150–174`). Longer needles
reach Two-Way (`arch/all/twoway.rs:181–270`): every unsuccessful iteration
advances the candidate, with bounded needle and remaining-haystack scans.
A conservative finite bound is O(n² + nm), for haystack n and needle m; it is
a closure bound, not a debit or timing claim. Actual upstream guarantees are
tighter. Equality uses progressing four-byte loads and fixed two/one-byte tails
(`arch/all/mod.rs:118–171`), not an external comparator. No reached search path
allocates, recurses, invokes I/O, reads environment or performs runtime CPU
discovery. Logging remains disabled. Independent read-only source review found
no owner/search/prefilter defect; root's focused checks are recorded separately.

A focused pinned-source reference for `["he","she","hers","his","he",""]`
over `ushers` confirms that failure inheritance plus the later root copy emits
repeated empty-pattern IDs at some offsets. The contained engines preserve
that multiplicity. Only the enclosing GlobSet later sorts/deduplicates global
IDs. The original reference input/output is retained at
`/private/tmp/rawr-aho-empty-reference-sep26/{main.rs,result.txt}`; this is a
pure matcher component check, not a Cargo producer or qualification invocation.
