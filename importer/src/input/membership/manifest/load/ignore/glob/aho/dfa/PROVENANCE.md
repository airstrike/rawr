# Default DFA conversion

This component completes aho-corasick 1.1.4's default unanchored Standard DFA
conversion from an immutable completed noncontiguous NFA. It does not select
the default engine, allocate its outer shared owner, or implement the overlapping
iterator. Its caller must distinguish `Abort::Semantic` from terminal
`Abort::Stop`; only an actual semantic conversion failure permits the source
fallback. The at-most-100-pattern gate belongs to that caller, not this converter.

The pinned version, archive checksums and feature closure are recorded in
`../PROVENANCE.md`. Source citations below refer to that exact 1.1.4 source tree.
No dependency or configuration is added.

| Local operation | Pinned source |
|---|---|
| Unanchored/default classes branch | `src/dfa.rs:386–401`, `Builder::new` |
| Class copy, checked geometry, field allocation and final compaction order | `src/dfa.rs:431–540`, `Builder::build_from_noncontiguous` |
| Premultiplied row IDs, ordered NFA states, failure resolution and special-ID remapping | `src/dfa.rs:545–608`, `Builder::finish_build_one_start` |
| Sparse edge/gap visits and first byte of each class | `src/dfa.rs:801–840`, `sparse_iter` |
| Owned ordered match rows and logical match bytes | `src/dfa.rs:184–201`, `DFA::set_matches` |
| Query predicates, match accessors, one table transition, memory usage and prefilter borrow | `src/dfa.rs:204–319`, `Automaton for DFA` |
| Alphabet length and power-of-two stride | `src/util/alphabet.rs:45–69` |
| State ID ceiling | `src/util/primitives.rs:101–111,326–331`, `SmallIndex` |
| Outer attempt/semantic-fallback order (outside this component) | `src/ahocorasick.rs:2220–2267`, `AhoCorasickBuilder::build_auto` |

The conversion borrows NFA states, classes, lengths, ordered transitions,
ordered matches and prefilter. Output fields retain source drop order: transition
storage, outer match rows and their inner vectors, pattern lengths, then the
shared prefilter. The prefilter clone is the actual admitted Arc clone; its
payload and owned needle are neither reconstructed nor copied. The NFA remains
unchanged and alive through the conversion result, so the caller controls its
source destruction point after installing the selected engine.

The semantic overflow check tests the last premultiplied state ID, before
allocating the transition table. It deliberately does not reject a table length
that exceeds the ID ceiling when its last row still fits. The table contains all
NFA rows, including DEAD, unused FAIL and the unused anchored start row. DEAD's
ID remains zero, the unsupported anchored start marker is zero, and unreachable
FAIL can still satisfy the source's numerical match predicate; only valid
search-produced IDs may be passed to match-row accessors. Gaps use the explicit
DEAD-failure shortcut before considering the NFA's unanchored failure walk.
Byte-class padding remains initialized to DEAD. Direct, duplicate and inherited
pattern IDs retain NFA order without sorting or deduplication.

Every physical allocation is admitted using its actual layout before the
fallible reserve. Table and pattern-length storage use the existing closed
Buffer. The exact outer rows allocation stores `Buffer<u32>` metadata and admits
that actual layout, rather than claiming the smaller upstream `Vec<u32>` header.
Its length and capacity never change afterward, so the source final outer
`shrink_to_fit` is a no-op. Each inner match Buffer uses the existing source Vec
minimum/doubling growth and stays unshrunk. Reported `memory_usage` separately
preserves upstream logical lengths and `Vec<u32>` header sizes; it is not used
as a physical allocation charge or a new semantic cap.

The class copy, table initialization, retained length copies, outer-row
initialization, state visits, ordered cursor advances, byte visits, emitted
transitions, match pushes and special-ID writes admit their actual work through
the original Session. Failure walks use the NFA's admitted cursor/transition
operations; no estimated aggregate cost is substituted. A class is emitted
once, in ascending byte order. Each NFA row visits at most 256 byte positions;
each failure walk is bounded by the completed NFA's decreasing trie depths and
start loop. Conversion and destruction are iterative and finite over admitted
storage. No native discovery, allocator callbacks, new account, refund or reset
is introduced. Scalar query accessors borrow existing storage; `next_state`
admits its one actual table lookup before reading.

Five focused checks combine transition/failure and padding correspondence,
duplicate/suffix/empty output order, exact premultiplied-ID geometry, actual
shared prefilter lifetime, and representative table/row/late-work refusals.
The query accessor refusal check verifies the same original terminal latch.
Root owns compilation and execution; this component makes no full-engine or
GlobSet acceptance claim.
