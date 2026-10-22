# Private Pike Boolean search

Pinned source: regex-automata 0.4.14, `src/nfa/thompson/pikevm.rs` in
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/regex-automata-0.4.14/`.
This extends the existing private Pike/cache/epsilon implementation. It does not
activate Core dispatch, pool replacement, a membership matcher or qualification.

## Source correspondence

- `is_match:767–774` sets earliest=true and calls `search_slots` with no slots.
  The private `Pike::is_match` likewise projects the actual earliest search's
  pattern result to Boolean. It never allocates, inserts into or consults a
  PatternSet, and does not reuse the different overlap stopping rule.
- `search_slots:1091–1102` and `search_slots_imp:1139–1151` directly reach
  `search_imp` for the actual forward utf8=false NFA. Changed UTF-8 or reverse
  producer state is a terminal internal contract error; it cannot silently omit
  the upstream UTF-8 capture-scratch/empty-split retry branch and accept input.
  The meta Pike wrapper gets its lazy cache before invoking this search.
- `search_imp:1224–1385` first performs `setup_search(0)`. Existing setup clears
  retained epsilon frames, then current and next active lengths and active slot
  widths. Full sparse/dense/slot allocations remain retained; zero active slots
  do not authorize removing source cache construction or storage.
- Whole borrowed input means start=0, end=len, Anchored::No and not already done.
  `start_config:1771–1785` still uses the NFA's anchored start and its actual
  always-anchored property. Empty input visits position zero. The loop includes
  the final position so delayed Match states and End assertions are visited.
  An anchored search inserts its starting closure only at position zero; an
  unanchored search simulates the source prefix through later closure insertions.
  There is no configured prefilter in this closed producer.
- `search_imp:1349–1379` requests the zero-width all-absent slot row for an
  initial closure. `nexts:1467–1491` visits active states in their existing sparse
  insertion order, copies each reached match's zero-width row, and retains the
  last matching pattern in Kind::All. LeftmostFirst stops at its first match.
  Earliest stops only after that batch, before swapping active sets or clearing
  the next set. Subsequent queries clear the reached state at normal setup.
- The existing `next:1541–1598` and epsilon-closure operations remain shared
  with overlap: ordered byte/sparse transitions, zero capture rows, explicit
  admitted epsilon stack, source union priority and Start/End looks. Boolean
  matching does not use overlap's UTF-8 per-pattern filter or PatternSet bounds.
  `SlotTable::all_absent/for_state:2149–2166` touch no owned payload for zero
  active slots, while their original retained backing tables remain alive.

## Original account and checks

The same Session reaches lazy cache initialization, setup, every source search
position/active-state/row/transition/epsilon event, and existing actual stack or
cache growth. No new account, numeric limit, String request, input-sized guessed
precharge or synthetic match-set ownership is introduced. Sparse-set membership
deduplicates epsilon visits; the input loop is bounded by len+1 and the existing
cache state count bounds each active batch. These finite bounds do not introduce
additional byte debits. First admission, allocation, arithmetic or internal
contract failure remains terminal, including refusal after a match is reached.
Lazy initialization still publishes only after both complete ActiveStates exist.

Only the Lazy type and reset operation become visible within the NFA module for
Core cache composition; cache fields, initialization accessor and internals stay
private to Pike. Cache reset keeps its existing source order and ownership.

Six focused deterministic tests cover both match kinds, all 256 single-byte
values under the actual literal-separator glob configuration, Unicode byte
sequences, empty pattern sets, empty input and EOI, whole-path anchoring, low-level
source-builder earliest and unanchored fixtures, complete All batches versus
first-match priority, retained slot payload/capacity and stale-frame cleanup,
every cold/reused refusal point, and changed producer contract failure. The
low-level fixtures expose search timing hidden by the emitted whole-path End
assertions; they add no production grammar or query configuration surface.

These tests were authored source-only. Root owns formatting, compilation and
execution. Complete enclosing cache/query behavior and qualification remain
separate from this private query boundary.
