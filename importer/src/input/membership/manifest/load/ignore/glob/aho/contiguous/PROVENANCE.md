# Default contiguous NFA conversion

This is the conversion stage of aho-corasick 1.1.4's default builder, borrowing
the completed noncontiguous NFA. It does not select the final default engine or
execute overlapping search. Exact Cargo, crate, license and feature provenance
is retained in `../PROVENANCE.md`; no additional dependency is introduced.

The authoritative source is aho-corasick 1.1.4
`src/nfa/contiguous.rs`, from archive SHA-256
`ddd31a130427c27518df266943a5308ed92d4b226cc639f5a8f1002816174301`.

| Local operation | Pinned source |
|---|---|
| Borrowed conversion, construction/drop order, mapping and compaction | `Builder::build_from_noncontiguous`, lines 937–1007 |
| Default dense depth 2 and byte classes enabled | `Builder::default`, lines 900–908 |
| State-ID check, kind selection and ordered match encoding | `State::write`, lines 686–739 |
| Native-endian sparse class chunks and repeated final padding | `State::write_sparse_trans`, lines 749–782 |
| Dense FAIL initialization followed by explicit transitions | `State::write_dense_trans`, lines 793–819 |
| Failure and transition remapping | `State::remap`, lines 486–511 |
| Packed single match and multiple-match interpretation used by tests | `State::match_len` / `match_pattern`, lines 581–635 |

The temporary old-ID-to-offset map is allocated before the output NFA. Pattern
lengths are copied next, followed by the actual admitted prefilter Arc clone.
The predecessor remains borrowed throughout encoding, remapping and compaction;
conversion never consumes it on a semantic failure or resource refusal. The
output owns its complete word/length buffers and shares the actual prefilter
payload. No memmem needle or prefilter is reconstructed during conversion.

Every non-FAIL predecessor state is encoded in original state order. FAIL stays
the constant offset 1 inside the DEAD encoding and receives no state of its own.
The output's state count still includes that sentinel, as upstream does. Depth
below 2 or more than 127 transitions chooses dense; a non-match state with one
transition chooses the packed ONE format; all other states use sparse format.
Sparse chunk padding repeats its last real class, preserving the later decoder's
ordered comparison premise. Match IDs retain predecessor order, including
duplicates and inherited empty/suffix matches. One match occupies the high-bit
tagged pattern word; multiple matches retain their length followed by IDs.

The source semantic StateID limit applies when starting each state. It is not
a 24-bit policy, despite an obsolete upstream format comment, and is not an
extra limit on each word appended or on the final representation length.
Semantic StateId failure remains `Abort::Semantic`; original Session refusal,
arithmetic/allocation failure and violated private invariants stay terminal.

Existing Buffer operations admit actual full replacement layouts before
allocation, retain predecessor storage while moving, and preserve the source
growth and compaction points. The dense exact-length append reserves its known
extent before filling; linked transition and match iterations preserve their
separate count/write passes. Original Session admissions precede traversal,
copying, packing, remapping and shared ownership. No new account, guessed
precharge, fallback or source restriction is introduced.

The immutable query seam exposes the representation, byte classes/alphabet,
pattern lengths, special IDs, length extrema, state count and borrowed prefilter.
It supplies the exact future decoder without a generic Automaton abstraction.
Production query interpretation remains separately owned and unimplemented in
this conversion batch.

Four focused source-authored tests cover exact empty/single-literal word layouts,
sparse padding/ONE/ordered match remapping, the reached 128-edge dense threshold,
and every conversion admission refusal with prefilter lifetime restoration.
No build, test or native execution was performed by this implementation agent.
