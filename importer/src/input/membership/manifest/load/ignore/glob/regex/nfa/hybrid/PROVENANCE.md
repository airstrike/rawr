# Private hybrid construction, cache lifecycle and closed forward queries

Pinned source root:
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/regex-automata-0.4.14/src/`.
This component constructs both hybrid descriptors and their private cache
lifecycle, and implements the two closed forward query forms.
It provides no production membership authority. See
`intern/PROVENANCE.md` for the separately governed closed table proof.

## Constructor correspondence

`meta/strategy.rs:520–534` attempts this wrapper only after the full DFA was
declined. That Core gate is owned separately. `meta/wrappers.rs:547–625`,
`HybridEngine::new`, configures both directions with the original 10 MiB cache
capacity; unlike the full DFA, it neither quarters the limit nor configures the
reverse DFA to support anchored starts only. Reverse match kind is All. Both
directions retain starts for each pattern, byte classes, heuristic Unicode word
support, no prefilter, no specialized starts, capacity checking enabled, minimum
clear count three and minimum bytes per state ten.

`hybrid/dfa.rs:3999`, `Builder::new`, creates its eager Thompson compiler before
the NFA argument is cloned. `Builder::build_from_nfa:4072` computes quit bytes,
rebuilds classes, computes the minimum cache capacity, checks the configured
capacity, checks minimum state-ID capacity, then builds the start-byte map.
The closed emitted grammar has only Start/End assertions, so the quit set is
empty. Existing dense start-map construction is shared without a conversion.
Each builder/compiler drops before the next builder. The final
`hybrid/regex.rs:773,851`, `Builder::{new,build_from_dfas}`, creates one additional
eager builder before transferring both descriptors. Each descriptor retains its
own NFA Arc, whose clone is prospectively admitted and released on failure.

`hybrid/dfa.rs:4301,4335`, `minimum_lazy_state_id` and
`minimum_cache_capacity`, retain the pinned five-state/three-sentinel projection.
The minimum formula deliberately uses one base start group and two sparse
state-ID arrays, even though actual cache construction allocates additional
storage. Its maximum state estimate is `9 + patterns * 4 + nfa_states * 5`.
Its dead-state size is obtained by actually invoking the existing admitted
`dfa::key::State::dead` encoding and Arc construction, then dropping both
temporary allocations. That operation precedes the capacity comparison even
when the comparison will fail. Semantic lengths and `size_of<State>()` remain
separate from actual physical allocation requests. Checked overflow is terminal;
the original bounded NFA producer keeps the source arithmetic in range.

Eight focused tests cover exact floor boundaries and temporary ownership,
forward/reverse retained configuration and NFA lifetime, final eager builder,
forward and reverse semantic failure order, every admission refusal, resource
refusal before semantic decline, and exact cumulative account floors including
prior ownership. Root executed all122 focused NFA tests with import-baseline on
nightly-2026-07-11/aarch64-apple-darwin; they passed. Default importer and
feature-enabled CLI checks also pass. Cache/search or product qualification does
not follow from these constructor checks.

## Cache call graph implemented

All following locations are in `hybrid/dfa.rs` unless otherwise named:

1. `meta/strategy.rs:673–680`, `Core::create_cache`, constructs Pike, backtrack,
   onepass and then hybrid caches in field order. `meta/wrappers.rs:770`,
   `HybridCache::new`, maps an existing engine to its cache; it does not turn a
   missing engine into a cache.
2. `hybrid/regex.rs:612`, `Cache::new`, creates forward then reverse DFA caches.
   `Cache::new:1875` initializes empty transition/start/state vectors, then
   `StateMap::new:1880`, then the two sparse sets, remaining empty scratch and
   counters, and calls `Lazy::init_cache:2517`.
3. `StateMap:2074` is `std::collections::HashMap<State, LazyStateID>` on the pinned
   std build. `hybrid/id.rs:169` stores LazyStateID as **u32**, including tags.
   `State` is the same immutable encoded Arc byte key used by dense construction.
   Upstream default map creation reaches RandomState before sparse-set allocation;
   This closed hybrid table alone replaces that initialization.
4. `init_cache:2517` extends both general start groups and per-pattern groups
   with the unknown ID, constructs one dead State, then calls `add_state:2285`
   three times for unknown/dead/quit. Each call checks semantic fit before ID
   creation, extends transitions, increments state heap usage, clones the state
   into the canonical vector, and inserts into the map at 2321. These are equal
   keys: later sentinel inserts **replace** the prior tagged value. After fixing
   sentinel loops, `init_cache:2570` inserts the same dead key a fourth time to
   restore the canonical dead ID. An adapter that rejects replacement is wrong.
5. Normal `add_builder_state:2257` gets a borrowed encoded key at 2263. A hit
   returns the canonical ID and restores builder scratch. A miss calls
   `add_state`, then restores scratch. IDs come from transition-table length
   (`next_state_id:2331`), never hash bucket placement.
6. `state_fits_in_cache:2772` and `state_builder_fits_in_cache:2785` add
   `memory_usage_for_one_more_state:2799` to `Cache::memory_usage:2021`. The latter
   uses map **length**, vector lengths, sparse memory, stack/builder capacities
   and accumulated state heap bytes; it does not use map bucket capacity/seed.
   The predicted increment includes one row, one canonical State, one map entry
   and the state bytes even for the duplicate sentinel replacements.
7. `try_clear_cache:2360` compares clear count to three, then searched bytes
   against `10.saturating_mul(states.len())`; failure is a search retry/fallback
   condition. `clear_cache:2457` clears transition/start/canonical state vectors,
   clears the map at 2461, zeroes state memory, increments clear count, resets
   byte progress, and calls `init_cache` on the **retained allocations**. It may
   subsequently reinsert one saved state with its source tags. `reset_cache:2436`
   first disables state saving, invokes clear/init, resizes sparse sets, then
   restores clear count and progress. These are repeated epochs, not final clear.
8. Direct map usage is new, borrowed get, insert/replacement, len and clear.
   There is no explicit iteration, remove, drain, entry callback or map escape
   in these methods. `Cache:1776` derives Clone/Debug; the reached Core wrapper
   does not call clone in this constructor path, but that exposed source
   capability must be excluded or qualified before broadening the proof. No
   allocation/performance/native probe was run for any map alternative.

The dense adapter remains unchanged. The hybrid adapter is governed separately;
only the exact encoded State and pure bucket-layout arithmetic are
shared. Source layout assertions are still required after every growth.

`cache.rs` preserves tagged u32 IDs and source publication order: semantic fit
and its possible clear precede the separate ID-capacity check and possible clear.
Repeated clear retains starts/transitions/canonical/map/scratch extents and
admits a newly encoded dead State each time. The minimum-capacity invariant
permits the three sentinels and one saved state after clear; the default
efficiency check also makes malformed undersized reconstruction terminate in
an internal invariant failure instead of creating an unbounded recursive path.
Reset drops the saver before clear, resizes sparse sets after reinitialization,
then resets clear count and progress, exactly as the source does. It never
resets the original Session. The closed grammar's quit set is empty.

`cache/sparse.rs` retains two length/dense/sparse sets. Each resize clears length,
resizes dense then sparse, then repeats for the second set. Logical length,
rather than retained capacity, contributes to semantic sparse memory. Canonical
states have fallible full-replacement Vec growth, with admitted moves and no
implicit Arc clones. The original empty builder/stack capacities and retained
scratch remain part of the source memory projection.

Fourteen new focused tests cover actual fixed-hasher borrowed equality and
collisions, full-table replacement/refusal, repeated retained buckets, initial
sentinel tags and Arc counts, padding versus alphabet transitions, cache memory
projection, scratch restoration on hit, saved-state ownership and restored tags,
progress and efficiency boundaries, actual fit-triggered clears, reset across
NFA sizes, paired-cache construction order, every initialization/clear/reset
refusal, flat teardown and original cumulative floors. Test-only Weak observers
do not expose Arc ownership in the production map/cache surface. Root owns
execution; the earlier constructor test result does not cover these new tests.

## Lazy transitions and forward query correspondence

`Hybrid::{is_match,overlapping}` borrows only the retained forward cache from
the already constructed pair. The reverse descriptor/cache remain owned and
unchanged. Boolean search corresponds to
`meta/wrappers.rs:645`, `HybridEngine::try_search_half_fwd`, for whole borrowed
input with earliest enabled. The set query corresponds to
`meta/wrappers.rs:730`, `HybridEngine::try_which_overlapping_matches`, with
earliest disabled. Both carry original Session refusal in the outer `Step`;
only source cache inefficiency produces `Failure::GaveUp { offset }` in the
inner result. An absent engine/cache is an internal contract error. The future
Core dispatcher owns the prescribed fallback and never receives a synthetic
successful result from an incomplete engine.

`cache/transition.rs` follows these `hybrid/dfa.rs` functions:

| Local operation | Pinned source | Preserved order and ownership |
|---|---|---|
| `start` | `start_state:1554`, `start_state_forward:1593`, `cache_start_group:2174`, `cache_start_one:2211`, `get_cached_start_id:2693` | Resolve text/byte lookbehind, select the 0/6/12+6pid group, reuse a known ID or take builder, seed lookbehind, ordered closure, encode, intern, then publish the start entry. Invalid pattern IDs return the canonical dead sentinel. |
| `cached`, `next` | `next_state:1215`, `next_state_untagged:1343`, `next_eoi_state:1506`, `cache_next_state:2119` | Read byte-class/EOI cell first. On UNKNOWN take scratch, determinize from borrowed canonical state, test predicted fit then ID capacity, optionally save, insert, recover saved ID, and finally write the outgoing transition. |
| match access | `match_len:1716`, `match_pattern:1741`; `util/determinize/state.rs:154–159,462–483` | Read implicit pattern zero or explicit native-endian IDs directly from the immutable shared key. No temporary pattern-ID Vec is created. |

The saved-state predicate deliberately uses `trans.len() >= MAX` before the
borrowed interner lookup, whereas actual `add_state` ID handling uses `> MAX`
after semantic fit. A predicted miss may still hit the interner; in that case
the saver returns the original ID without clearing. If an insertion clears,
the surviving state is reinserted with original tags and the outgoing edge is
stored under its remapped ID. Semantic insertion failure leaves the source
save obligation retained; terminal admission stops without another operation.
Existing map insertion, equal-key replacement, retained bucket epochs and
separate physical admission/semantic memory projections are unchanged.

`cache/sparse.rs` implements `util/determinize/mod.rs::{next:92,
epsilon_closure:369,add_nfa_states:448,set_lookbehind_from_start:583}` using the
existing two retained sparse sets and stack. It decodes ordered signed-delta
IDs, recomputes closure only when EOI makes a needed End assertion newly true,
delays matches by one transition, stops at the first match only for
LeftmostFirst, and preserves union alternative priority. Union/BinaryUnion and
Fail remain in encoded keys; Capture is traversed but omitted. Byte ranges and
sparse ranges use ordered source traversal. Start/End are the only assertions
admitted by the producer, so line/word/CRLF state flags and quit bytes are absent.
The shared key changes expose only existing encoding/decoding and direct match
reads to NFA scope; neither dense determinization nor either interner changed.

`cache/search.rs` follows `hybrid/search.rs::find_fwd:13` and
`find_fwd_imp:50` with no prefilter and earliest enabled. It preserves the safe
equivalent of the source four-cell untagged grouping, including the prior state
needed after an UNKNOWN transition. Tagged input updates progress before the
transition; untagged cached runs do not, and UNKNOWN exits update before lazy
construction. A match reads pattern zero and finishes at its delayed offset;
dead finishes at the same current offset. Whole-input EOI follows `eoi_fwd:693`.
An EOI semantic error returns before the Boolean search's final finish.

The overlapping continuation follows `find_overlapping_fwd:443` and
`find_overlapping_fwd_imp:466`: remaining IDs at one position are returned in
encoded order without restarting progress; otherwise advance the position,
start progress, and update after each completed nonmatching byte. Matches/dead
finish at the current offset. It stores the EOI result, updates continuation,
and finishes progress even when EOI returned a semantic cache error. The outer
loop follows `DFA::try_which_overlapping_matches:1128`: attempt `try_insert`,
discard Full, then test fullness. It does not clear caller IDs or pre-check a
full/zero-capacity set. Boolean search never constructs a PatternSet.

Both entrypoints accept exactly the whole-input byte slice; therefore an invalid
span, lookbehind quit, prefilter, reverse-search loop or UTF-8 empty-match retry
cannot occur. `closed` checks forward/non-UTF8 producer invariants. Start-group
support remains source-shaped internally, and the public-in-NFA query boundary
always selects unanchored/Text. No engine clone, cache clone, reentrant call,
native pool or account reset is added. Establishing complete enclosing owners
and clone relationships remains the separate integration obligation.

Twelve focused query tests cover emitted byte/Unicode/empty/EOI language,
start groups and lookbehind, delayed match IDs, source preference and ordered
overlap continuation, ignored Full, retained caches across different queries,
warm untagged progress without Boolean set allocation, actual automatic clear
and remapped edges, predicted-clear interner hits, exact GaveUp byte/EOI offsets,
the different EOI progress finishes, every cold-search/automatic-clear admission
refusal, partial caller IDs, and flat Arc cleanup. Test-only floor construction
asks the actual forward/reverse builders for their minimum capacities and takes
their maximum; it does not inject a semantic error into production. Root owns
formatting, compilation, execution and qualification of this source batch.
