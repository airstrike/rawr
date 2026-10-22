# Pinned matcher compilation boundary

Source audit at the builder integration following `26449211`. This documents
remaining implementation, not completed matcher or selection authority.

The 10 MiB NFA limit and hybrid-cache capacity in globset 0.4.18
`src/lib.rs:271–305` do not bound the complete regex builder. In particular,
regex-syntax 0.8.11 checks nesting after allocating the parsed AST
(`src/ast/parse.rs:165–185`). Earlier and simultaneous ownership must be admitted.

| | A — call the existing builder after precharging | **B — instrument the reached AST pass** |
|---|---|---|
| Required evidence | Complete bounds for every reached allocation and operation | Preserve the identified pinned parser slice |
| Current evidence | NFA/cache caps do not supply the required bound | Parser control flow and emitted grammar are available |
| Next usable result | No demonstrated admitted call boundary | **Bounded parsed syntax trees; still no matching authority** |

Use B under the existing instruction to instrument uncontained pinned slices.
This does not narrow accepted Cargo globs or change the selected architecture.

Pinned downstream order, regex-automata 0.4.14:

1. `src/meta/regex.rs:3455`: parse all patterns into retained ASTs, then translate
   all ASTs into retained HIRs. Do not translate each pattern immediately.
2. `src/meta/regex.rs:3600`: collect HIR references and construct RegexInfo
   properties and their union (`RegexInfo::new`, around line 1934).
3. `src/meta/strategy.rs:78`: choose the strategy. Start-anchored emitted patterns
   skip prefix extraction on the anchored branch.
4. `Core::new`, around line 456: forward Thompson NFA, PikeVM, bounded backtracker,
   attempted one-pass DFA, reverse NFA, attempted full DFA, hybrid fallback.
   Preserve source-dependent failure/fallback behavior.
5. Construct retained strategy/metadata and cache pool. Later pool retrieval
   (`src/util/pool.rs:542–597`) may allocate additional engine caches.

The emitted-pattern AST slice is now connected in `glob/regex/parser.rs`:
flags/noncapturing groups, alternatives, concatenation, assertions, byte escapes,
bracket ranges and uncounted repetition. `nesting.rs` applies the pinned depth
limit after construction, while parser scratch and continuations remain owned.
This returns a syntax-checked tree, not a matcher. `glob/regex/batch.rs` now
retains the accepted trees and their borrowed source patterns in source order,
with first-failure pattern indexing and the pinned PatternID length boundary.
The consuming batch translator now retains all ASTs alongside the HIRs built
in source order, finishing each translation before growing its output vector.
There is still no executable matcher; engine/cache construction remains required.

`glob/regex/translate.rs` implements pinned flag merge/restoration and literal
scalar conversion for globset's `utf8=false` configuration, connected through
`hir/tree.rs` to the complete emitted-tree translator. Base properties, byte interval canonicalization, literal
aggregation, and consuming concat/repetition/alternation normalization are implemented.
Engine construction remains required. In particular, disabling Unicode
syntax does not exclude Unicode classes produced by alternation normalization
(`regex-syntax 0.8.11 hir/mod.rs:572`, singleton-character folding). Preserve
those reached constructors and their intermediate ownership.

`translate/class.rs` now connects parsed byte classes to canonical interval
sets, retaining translator and class-visitor capacity across classes. Endpoint
conversion and UnicodeNotAllowed errors precede interval insertion; negation
follows all items. `Classes::expression` connects normalization/properties to
the base consuming constructors in
`translate/hir.rs`, called by the complete emitted-tree translator.

`translate/properties.rs` implements empty, literal and Start/End properties,
including fresh UTF-8 classification after literal combination. The private
single-record allocation is admitted before ownership. Repetition, concat and
union properties are implemented. Base empty/literal/anchor/byte-class
constructors now normalize singleton classes to literals and empty classes to
failure expressions, preserving the distinction from empty-string matches.

Repetition properties now preserve saturating minimum length, checked maximum
length, required versus possible look sets, and capture-count propagation.
The repetition constructor allocates/retains the child before applying
the pinned simplifications, then call these properties only for an unsimplified
repetition (`hir/mod.rs:378`, `hir/translate.rs:998`).

Concatenation properties now perform the pinned all-child, prefix and reverse
suffix scans, admitting each visit before inspection and allocating properties
after all three passes. `hir/arena/concat.rs` now performs literal copying,
direct-child flattening and empty/singleton collapse, with admitted intermediate
ownership. Nontrivial results retain child indices in the flat arena.

`Properties::union` now combines alternative/pattern-set properties with pinned
unknown-length propagation, required look-set intersections, possible look-set
unions and capture-count variation. The union reads borrowed properties through
an iterator without materializing an extra reference vector.
Consuming alternation normalization now calls this union after the ordered
simplification attempts and common-prefix factoring fail.

`translate/hir/arena.rs` adds consuming repetition construction with flat child
indices. It retains the child before zero-width/exact-count simplifications,
returns identity children by taking their slot, and releases every descendant
when replacing a repetition, concatenation or alternation subtree with Empty. Structural
disposal preserves pinned shallow fast paths, traversal-stack reservation before
the outer Empty replacement, and child replacement construction. Exact-size
concat drains reserve their incoming extent before transfer. New structural
variants must extend disposal before publication. The emitted-tree traversal now uses these constructors.

`hir/arena/singleton.rs` implements the ordered character and byte singleton
probes used by alternation, preserving duplicates and work performed before a
failed probe. Character decoding inspects only the first scalar, then checks
the literal's full byte length. Ordered Unicode/byte class-union probes are also
implemented, including mixed ASCII conversions, first-incompatible-child stops,
and refusal preventing fallback. The alternation dispatcher connects these
attempts and common-prefix factoring in pinned order.

`translate/unicode.rs` collects Unicode singleton ranges and canonicalizes them
with pinned duplicate/adjacency merging and append-before-drain ownership.
The existing admitted heapsort replaces opaque sorting: ranges contain only
ordered scalar endpoints, so stability among equal values has no effect on
the result. Every comparison is charged. Unicode class properties and consuming
normalization are implemented: empty classes become byte-failure expressions,
singletons become admitted UTF-8 literals, and other classes retain first/last
scalar byte-length bounds. Byte and Unicode class unions now retain the pinned
empty/equal shortcuts and append-before-canonicalization ownership. ASCII-only
cross-conversions collect admitted ranges and reject non-ASCII classes before
allocation. Refusal coverage includes both conversion directions. These helpers
are connected to alternation but are not yet a production selector.

`hir/arena/alternation.rs` implements consuming flattening, empty/identity
normalization, ordered singleton/class probes and common-prefix factoring.
Factoring uses explicit admitted prefix continuations instead of recursion;
duplicate prefix subtrees are disposed through the arena. Structural equality
walks kinds before properties with admitted continuations and ignores the
interval set's folded optimization flag, as pinned equality does. `into_kind`
drops existing properties without the replacement allocation used by
`into_parts`. Tests cover class domains, nested branch ordering, an empty suffix
after a full common prefix, deep equality, subtree disposal and terminal refusal.

`hir/tree.rs` retains separate source-shaped visitor and translator stacks.
Pre-visits precede descent storage; leaves have no outer continuation; initial
and subsequent alternation branch markers precede their visitor continuations.
Literal aggregation stops at source markers, groups restore flags, concat pops
reverse then filters Empty before construction, and classes preserve failures.
Tests exercise emitted Unicode bytes, alternatives, ranges, wildcards, terminal
refusal and all-AST/all-HIR batch retention. This is still test-gated membership
implementation, not production selection authority or Gallery speed evidence.

`translate/info.rs` connects the retained batch to the property-collection
portion of `RegexInfo::new`: HIR references first, each property clone before
vector growth, then the union. The references and cloned properties remain
owned through the next-stage callback. Refusal prevents entering that callback.
Configuration/Arc ownership, engine construction and cache pools remain required;
the property collection alone is not complete RegexInfo or a matcher.

The next engine closure is pinned `meta/strategy.rs:Core::new` (around 456):
forward Thompson NFA, PikeVM, bounded backtracker, optional one-pass, reverse
NFA, full DFA and hybrid fallback. Forward/shrink=false still constructs
`RangeTrie::new` (`thompson/compiler.rs:740`, `range_trie.rs:222–241`), which
allocates its final/root states. UTF-8 maps record capacities 10,000/1,000 at
construction but defer table allocation until clear; do not eagerly allocate
those capacities based on their names alone.

Before the first supplied HIR, `compiler.rs:961` checks pattern/reverse-capture
constraints, resets/configures the builder, scans anchoring, constructs an
empty or synthetic unanchored prefix, and enters pattern/group-zero capture
ownership. `Builder::add` (`builder.rs:1114`) inserts before checking its
semantic NFA size limit; Session admission must still precede actual growth.
Source semantic state size is separately asserted as 32 bytes on 64-bit and
16 bytes on 32-bit (`builder.rs:state_has_small_size`), not the size of an
instrumented Rawr state containing additional admission metadata.

`regex/nfa/builder.rs` now implements state insertion and patching, including
retained mutation before semantic size-limit failure. Actual state and edge
storage is admitted independently before growth. Non-growing patches preserve
the source's behavior of not rechecking a prior semantic size failure; sparse
patching is an internal-state error. State-ID bounds and source edge widths are
retained. `nfa/builder/pattern.rs` now implements start/finish registration,
implicit unnamed group-zero start/end captures, match states and builder clear.
Capture ownership precedes state insertion and survives semantic size failure;
duplicate group-zero states reuse the existing registration. The emitter has
no explicit captures, so named/nonzero capture APIs are not entered. Clear
admits state and capture destruction while retaining outer capacities and the
configured semantic limit. Compiler startup, HIR-to-state compilation,
finalization and executable matching remain required.

`nfa/builder/fragment.rs` adds consuming builder operations for empty/literal
and byte-class fragments. Literals allocate the next state before patching the
previous one, following forward or reverse byte order. Byte classes create
their empty end state before reserving/collecting sparse transitions, including
the zero-transition failure case. The canonical range view is borrowed from the
validated byte class. Every iterator advance, including termination, is admitted
before inspection. These fragments still need the complete compiler driver and
finalization; they are not an executable matcher.

`nfa/builder/utf8/cache.rs` preserves the forward UTF-8 compiler's bounded
transition cache: capacity 10,000 with deferred allocation, version invalidation
that retains old keys until wrap/replacement, FNV hashing and exact collision
comparison. Actual table/key ownership and comparisons use the original Session.
This cache alone does not compile Unicode fragments or enable matching.

`nfa/builder/utf8/ranges.rs` implements the reached `Utf8Sequences::new/next`
scalar splitter from regex-syntax 0.8.11: surrogate exclusion, UTF-8 width
boundaries, alignment splitting and inline byte ranges in source order. Its
private stack uses admitted fallible storage instead of hidden infallible Vec
growth. The caller constructs one traversal per canonical scalar range; the
unreached reset API is omitted. Tests cover every valid scalar exactly once for
the full Unicode range, boundary intervals, invalid encodings and every admission
refusal. Connecting these sequences to forward/reverse NFA construction remains
required before they provide any matcher authority.

`nfa/builder/utf8/compiler.rs` now connects sorted byte sequences to the forward
compiler's shared sparse states. It preserves target-before-cache initialization,
prefix freezing, continuation ownership, transition cloning before builder
insertion, and cache publication only after semantic-size acceptance. The pending
state can be reused across classes without sharing stale state IDs. Full-Unicode
tests traverse the resulting intermediate graph for every scalar, reject invalid
sequences, and verify shared continuations. Admission refusal and semantic-size
failure have distinct regression coverage. The HIR class dispatcher, reverse
construction, finalization and executable engines remain outside this slice.

Forward class dispatch is now connected through `Builder::unicode_forward`.
Canonical Unicode ranges are borrowed from the validated HIR class; ASCII
classes take the direct sparse path (empty end before transition reservation),
while non-ASCII classes initialize the UTF-8 compiler before traversing ranges.
Each scalar-range traversal feeds sorted byte sequences into shared states.
Tests cover empty/ASCII and mixed scalar classes, reused pending state, and
refusal across both dispatch paths. The enclosing HIR driver and reverse class
path are still required; this is not production matcher integration.

`Builder::unicode_reverse` now supplies the pinned shrink=false reverse path
selected by Core, with the same ASCII helper as forward dispatch. Its deferred
1,000-entry suffix cache is cleared before union/end creation; forward sequence
bytes create reverse-linked states, and a key is published only after its new
state is successfully patched. Canonical range order and union patch order are
retained. Tests cover reversed UTF-8 acceptance, cache reuse, collisions, version
wrap, and each admission refusal. The full HIR driver and NFA finalization still
remain; both directional class fragments alone are not an executable matcher.

`nfa/builder/control.rs` implements emitted start/end anchors, failure states,
ordered alternation assembly and normalized `?`, `*`, `+` repetition. A move-only
continuation splits repetition before/after child compilation, preserving source
union placement without imposing recursive driver calls. Nullable star keeps
the source's plus-then-question construction. Tests check accepted languages,
union order/kind, reversed anchors and refusal at each repetition admission.
Literal-only alternations still require the pinned literal-trie optimization;
the general alternation helper does not replace that source branch. The full
HIR traversal and finalization remain required.

`nfa/builder/literal.rs` implements the pinned literal-only alternation trie:
active chunks freeze at match boundaries, new states precede edge insertion,
and leaf duplicates do not allocate another match chunk. Its explicit depth-first
compiler preserves sparse/range selection, chunk priority and parent-link patches.
Search and edge movement are prospectively admitted, with fallible state/chunk/
frame buffers. Tests cover prefixes, duplicates, both directions, match-boundary
freezing and every admission refusal. The HIR dispatcher must select this branch
when all alternatives are literals; it is not yet wired to the full traversal.

`nfa/compile.rs` now traverses the retained HIR through the borrowed
`translate/hir/view.rs` interface. Concatenation follows compilation direction,
general alternation compiles its first two children before union creation, and
an all-literal alternation selects the trie after a shallow kind-only scan.
Repetition reads minimum length only for unbounded zero-minimum cases and uses
the before/after-child continuation. Class/anchor/literal paths are connected.
One admitted continuation stack replaces recursive traversal; no HIR ownership
is cloned. Emitted glob integration tests check forward/reverse acceptance and
terminal refusal during nested construction. Full Thompson startup (including
RangeTrie ownership), captures/pattern assembly, finalization and executable
engine construction remain required. The test-only match-state seal is explicitly
not a substitute for those stages or for production package membership replay.

`compile/startup.rs` now retains the constructor-reached RangeTrie ownership:
two empty state transition buffers, initialized between forward UTF-8 state and
suffix-cache construction. The other source scratch/free vectors are empty and
never populated on Core's shrink=false path. `Compiler::pattern` registers a
pattern, wraps forward expressions with implicit group-zero captures, appends
the match state and finishes registration in source order. Reverse patterns skip
captures. The connected refusal sweep includes constructor and pattern wrapping;
multi-pattern tests verify distinct match IDs and directional capture counts.
Batch configuration, anchoring, top-level alternation and finalization remain.

`compile/batch.rs` now assembles intermediate batches under Core's fixed
UTF-8-empty=false/LF configuration. It checks pattern count before clearing,
sets the semantic size limit, scans directional anchoring with short-circuit
behavior, and constructs either an empty prefix or the retained synthetic AnyByte
lazy-star prefix. The first two complete patterns precede top-level union creation;
later patterns are added in source order before the prefix is patched. Empty
batches produce failure. Tests cover both directions, unanchored scanning,
distinct pattern IDs, size-limit failure followed by reuse, and each admission
refusal. Returned starts refer to intermediate states, not a finalized NFA;
finalization must retain configuration and remap starts/captures/transitions.

`nfa/builder/finalize.rs` now implements pinned `Builder::build` empty-state
removal, singleton-union elimination, sparse/binary-union reductions, reversed
union priority, capture slot conversion and the two-pass empty-chain remap.
`nfa/finalize.rs` retains fixed UTF-8=false/reverse/LF settings, transition byte
classes, capture/look flags and per-pattern epsilon-prefix/empty properties.
Original builder storage remains borrowed and reusable throughout finalization.
Scratch maps, copied transitions, exact final vectors and both GroupInfo/NFA
shared payloads are admitted through the original Session before allocation.
The source's initial empty GroupInfo stays owned until its replacement succeeds.
Empty name maps retain their exact HashMap storage layout as unread opaque
storage: the emitter has only unnamed group zero, so no key, hash or randomized
seed is consumed. Named and nonzero capture APIs remain outside the closed
producer slice. Semantic GroupInfo failures remain separate from terminal
Session refusal. Exact fallible replacement vectors perform shrinking moves
without an uncharged realloc or refund of prior cumulative ownership.

Provenance: regex-automata 0.4.14 archive SHA-256
`6e1dd4122fc1595e8162618945476892eefca7b88c52820e74af6262213cae8f`,
`nfa/thompson/builder.rs:410–617`, `nfa/thompson/nfa.rs:1195–1459,1696–1730`,
`util/captures.rs:1569–1616,2179–2310`, `util/alphabet.rs:695–737`, and
`util/sparse_set.rs:91–195`. `Compiler::build` connects batch construction to
this finalizer. Focused regressions cover final graph languages in both
directions, pattern priority, capture slots, empty chains, sparse/union shapes,
byte classes, semantic failure/reuse and terminal refusal at every finalizer
admission. They have not yet been executed for this source checkpoint. This
produces immutable NFA data only; executable engines/cache pools, ignore matcher
integration, physical selection and membership admission remain unimplemented.

`nfa/builder/utf8/ranges.rs` implements the reached `Utf8Sequences::new/next`
scalar splitter from regex-syntax 0.8.11: surrogate exclusion, UTF-8 width
boundaries, alignment splitting and inline byte ranges in source order. Its
private stack uses admitted fallible storage instead of hidden infallible Vec
growth. The caller constructs one traversal per canonical scalar range; the
unreached reset API is omitted. Tests cover every valid scalar exactly once for
the full Unicode range, boundary intervals, invalid encodings and every admission
refusal. Connecting these sequences to forward/reverse NFA construction remains
required before they provide any matcher authority.
