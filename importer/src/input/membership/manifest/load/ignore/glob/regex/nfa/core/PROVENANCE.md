# Private Core construction, cache lifecycle and query dispatch

Pinned upstream root:
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/regex-automata-0.4.14/src/`.
This component contains `meta::strategy::Core::new:456–546` for the closed
globset-emitted grammar and retained `info::Info`, together with explicit Core
cache creation/reset and whole-input Boolean/overlap dispatch. The outer Regex
execution owner, top-level strategy selection, physical membership and production
connection remain outside this component.

## Source order and ownership

1. The forward `Compiler::new` and complete `build_many_from_hir` run first, with
   the existing `utf8=false`, `shrink=false`, source NFA limit, implicit captures
   and newline LookMatcher configuration. The compiler temporary drops at the
   end of this construction, before PikeVM. A semantic NFA failure returns from
   Core; an original Session refusal remains terminal.
2. `meta::wrappers::PikeVMEngine::new:74` builds its eager compiler before sharing
   the forward NFA. `BoundedBacktrackerEngine::new:194` follows. Its actual
   LeftmostFirst predicate determines whether its compiler and NFA owner exist.
   The Core retains its own forward NFA in addition to those engine owners.
3. `OnePassEngine::new:361–380` first checks the configured enable flag, then the
   retained union's explicit capture count and Unicode word look set. The exact
   six Unicode word bits come from `util::look::LookSet::contains_word_unicode:354`.
   Every expression from the closed emitter has no explicit captures and only
   Start/End assertions, so its actual configured outcome is NotUseful before any
   one-pass builder or NFA clone. The predicate reads the retained properties;
   this is not a guessed unsupported-engine marker. A changed producer that
   reaches the build branch fails the internal contract terminally until that
   builder is contained; it cannot silently decline and be accepted.
   Locally, `translate/hir.rs:21` has no Capture variant,
   `translate/properties.rs:27` admits only Start/End looks, and its base/union/
   concat/repetition property operations preserve the zero explicit-capture
   count. Unicode literals/classes do not introduce word assertions.
4. If both hybrid and dense engines are disabled, the source skips the reverse
   NFA. Otherwise `dfa::Prepared::begin` builds the complete reverse NFA with
   captures disabled before considering the dense enable/state-count gate.
   `Prepared::complete` performs determinization and finishing before yielding
   owned reverse-NFA and complete-only dense wrapper values. Pending and
   Unshuffled states cannot inhabit Core or stand for semantic decline.
5. Only after this completed dense outcome does Core inspect the hybrid branch:
   disabled hybrid or a built dense pair produces the source empty wrapper;
   otherwise the actual forward/reverse hybrid descriptors are constructed.
   Semantic descriptor rejection leaves the other engines intact. An allocation,
   arithmetic or admission refusal never enters a later fallback constructor.
6. Core owns fields in the upstream order: Info, absent prefilter, forward NFA,
   optional reverse NFA, PikeVM, backtracker, one-pass outcome, hybrid and dense.
   The closed emitter is anchored, so the source strategy supplies no prefilter.
   Forward/reverse graph and group Arc ownership stays with the actual engines;
   completion transfers ownership without cloning. Property collection's HIR
   reference vector remains live while its `Info::with` continuation constructs
   Core, then drops as in the existing property-collection correspondence.

The construction composes the existing admitted operations through the same
Session. It introduces no separate account, resource policy, map implementation,
numeric cap or runtime configuration API. Test-only limit/enable overrides reach
source branches; shipping configuration remains the closed globset configuration.
The owned constructor result is a `proof-bundle` of completed engine outcomes.
Its private query methods are described below; neither construction nor those
queries establish an enclosing membership or qualification witness.

## Focused evidence boundary

Source tests compare the complete prospective request sequence against existing
components assembled in pinned Core order. They cover both match kinds, completed
dense suppression of hybrid, heuristic and semantic dense decline, hybrid semantic
decline, actual one-pass property predicates, enabled-engine guards and mandatory
reverse ownership. Arc counts and weak references check retained owners and flat
release. Refusals at each component boundary and its first allocation retain the
first cause and prohibit later construction. Exact cumulative item/allocation
floors include prior retained storage, property collection and the complete Core.

These tests were written without executing them in this source-only task. Root
owns compilation and checks. Dense/hybrid internals retain their component
coverage, while the joined cache and dispatch coverage is described below. The
enclosing strategy, physical membership, review, freeze, consolidated
qualification and Gallery measurements do not follow from this private Core.

## Explicit cache creation and reset

`meta/strategy.rs:673–690` creates every Core cache field in this exact order:
`Captures::all`, lazy Pike, lazy backtracker, one-pass, hybrid and absent reverse
hybrid. Local `core/cache.rs` follows that order and returns the cache only after
all fields complete. Selecting a completed dense pair does not bypass any field.
`meta/regex.rs:2353–2359` supplies the retained field ownership/drop order.

The first field clones the actual forward `GroupInfo` Arc before reading its
slot count. `util/captures.rs:215–217` allocates a vector of every slot, initialized
to absence, and retains both the group owner and absent pattern ID. The local
`core/cache/captures.rs` uses that same owner, `Option<u32>` pattern storage and
the same optional NonZeroUsize layout used by the contained NFA engines for
NonMaxUsize slots. It admits the actual clone operation, scalar slot-count read,
exact vector layout and initialization through the caller's original Session.
It does not replace full captures with an empty or implicit-match-only shortcut.
Zero slots own no allocation, while still retaining the group owner.

`meta/wrappers.rs:60,153` creates empty lazy Pike/backtracker wrappers. Their
first initialization remains in their reached get/reset operations. The closed
Core constructor's actual `Disabled` or `NotUseful` one-pass outcome corresponds
to the absent cache from `OnePassCache::new:478–486`; a local explicit absent
cache type represents only those already-completed outcomes. It cannot represent
a pending or rejected construction as a completed engine. `HybridCache::new:770`
uses the actual wrapper outcome and, when present, the existing contained
forward-then-reverse cache constructor. `ReverseHybridCache::none:1164–1173`
remains an explicit absent field. No reverse-only strategy is installed here.
Absent wrapper construction/inspection retains its actual logical operation;
there are no fictional pool/TLS/dispatch allocations or requests.

Reset follows `meta/strategy.rs:685–690`: Pike, backtracker, one-pass, hybrid.
It does not clear or replace capmatches, its group owner, slot values/capacity,
pattern ID or the reverse-only field, including across a different Core target.
Pike reset initializes an absent lazy value before resetting it
(`meta/wrappers.rs:128–137`); backtracker does so only when its target wrapper is
present (`:301–304`). One-pass remains absent under the same actual predicate.
Hybrid reset uses the existing forward/reverse implementation and retains its
source capacity/clear behavior; it does not reconstruct the entire Core cache.
The existing hybrid wrapper maps the reached absent-cache/present-engine source
unwrap (`:781–785`) to terminal internal State. This composition neither fabricates
a missing cache nor treats that state as a semantic engine decline. No new public
cross-engine reset guarantee is introduced.

Construction refusal drops partial fields without publication, new admissions
or allocation in teardown. Reset can have completed earlier field operations
before refusal, as the source order requires, but the original Session latch
prevents further work or a successful retry. There is no replacement account,
pool, Clone policy, cache sharing graph or migration policy in the cache slice.
The query composition is described in the next section. The enclosing lazy
owner and captured strategy Arc proof remains outside this explicit per-Core
cache boundary.

Focused tests compare independently assembled source creation/reset request
traces; exercise zero/one/multiple patterns, both match kinds, dense and hybrid
outcomes and absent wrappers; check the exact capture allocation and Arc lifetime;
refuse every cold creation and cold/reused reset admission; retain capture state
across cross-Core reset and graph teardown; and check full Core-plus-cache-plus-two
resets against exact cumulative item/allocation floors with prior retained work.
Same-Core reset reuse still pays the actual hybrid sentinel allocation while
retaining its arrays/maps. These source-only additions have not been executed by
their author; root owns compilation and checks.

## Private Boolean and overlapping dispatch

`core/search.rs` adds the two reached whole-input query shapes to this private
Core boundary. It does not provide the outer Regex impossibility gate, lazy
execution-owner factory, GlobSet strategies or physical membership connection.
Those enclosing obligations remain separate from the engine dispatch here.

Pinned `meta/strategy.rs:765–798` selects the completed dense engine first for
Boolean matching. A completed true or false returns immediately. Only when the
dense wrapper is absent is hybrid considered. A selected engine's inner source
semantic error enters `is_match_nofail:638–656`; original Session refusal stays
in the outer Step and propagates without any fallback. In particular, dense
semantic failure does not select hybrid next. The contained one-pass outcomes
are actually absent, then `meta/wrappers.rs:158–190` tests the backtracker wrapper:
earliest haystacks longer than 128 bytes decline before its span-capacity check.
Completed `Some(false)` is a result, not a decline. Only `None` reaches Pike's
zero-slot earliest Boolean search. No match is returned after a later refusal
in an engine's required current batch.

Pinned `meta/strategy.rs:861–900` similarly selects dense else hybrid for
overlapping queries, returning on completion. Source semantic failure goes
directly to Pike, without one-pass or backtracker and without clearing the
caller's PatternSet. Any preexisting IDs and IDs emitted before the fast engine's
failure remain. The engines retain their distinct insertion/fullness behavior;
dispatch introduces no eager capacity check or post-failure set reconstruction.
LeftmostFirst Pike may stop on an already nonempty set, unlike the fast engines;
the common-language tests therefore start empty, while partial-ID fallback tests
use the actual All match kind. Dispatch neither resets caches between queries
nor replaces hybrid's retained clear/efficiency history.

Focused tests select real dense, real hybrid and both non-DFA kinds using the
existing test-only configuration controls. They cover bytes, empty input, EOI,
Boolean/overlap results, completed backtracker false, the 128/129-byte earliest
boundary and repeat queries without new storage allocation. Independent direct
engine request traces establish the actual selected/fallback operations.

The dense semantic-error fixtures mutate only test tables, as the existing
dense search tests do: a real start transition to Quit, the existing unsupported
start flag, or an existing match row followed by Quit. A real hybrid is retained
beside the faulted dense engine solely in these fixtures, making accidental
hybrid dispatch observable. Production Core construction continues to suppress
hybrid after dense success. The partial-overlap fixture records one ID before
Quit; Pike then adds its real matching ID while retaining that ID and a prior
caller ID. No production engine return value or matching configuration is
stubbed to force these branches.

The hybrid fallback fixture uses the actual maximum forward/reverse minimum
cache capacity obtained from `Machine::build`'s source capacity rejection. Its
62-byte literal reaches an actual inefficient clear epoch and GaveUp, then the
prescribed Boolean or overlap fallback. The test-only minimum helper does not
change production limits or fabricate a search error.

Cold and warm dispatch tests refuse every reached query admission for dense,
hybrid, non-DFA, dense-error fallback, partial-ID fallback, completed backtracker
false and long-input backtracker decline. Each warm prefix uses the same original
Session; the expected request prefix includes prior spent work. Separate joined
hybrid-GaveUp cases refuse both sides of the semantic-return boundary and every
reached allocation in cold and retained epochs; subset-transition refusal is
exhausted by the hybrid component tests. Retry through either query or cache
reset stays latched, preserves prior IDs and emits no additional request. These
tests add no pool, native operation, Clone API, account reset or qualification
claim. Root owns their compilation and execution.
