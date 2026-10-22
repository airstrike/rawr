# Private dense constructor finishing

Source root: `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/regex-automata-0.4.14/src/`.
The applicable compiler/target and interner containment remain those in
`../intern/PROVENANCE.md`. This continuation constructs a private pair;
it adds no search, Core selection, hybrid engine or membership connection.

| Local operation | Pinned source |
| --- | --- |
| `finish::shuffle` | `dfa/dense.rs:2823`, `DFA::shuffle` |
| `finish::remap::Remap` | `dfa/remapper.rs:91,100,115`, `Remapper::{new,swap,remap}`; `dfa/dense.rs:3538`, `TransitionTable::swap`, swaps only alphabet slots; `:2547`, `DFA::remap`, visits all transition and start entries |
| Match arrays and offset failure | `dfa/dense.rs:4455`, `MatchStates::new`; `util/primitives.rs:144`, `SmallIndex::MAX = i32::MAX - 1` |
| Accelerator state discovery and three relocation phases | `dfa/dense.rs:2604`, `DFA::accelerate`; `:4846`, `State::accelerate` |
| Accelerator byte visitation and representation | `util/alphabet.rs:600`, `ByteClassElements::next`; `dfa/accel.rs:157,405,449`, `Accels::add`, `Accel::{new,add}` |
| Special ranges | `dfa/special.rs:260`, `Special::validate`, and `validate_state_len`; source `set_max` and `set_no_special_start_states` |
| Universal starts and four shrinking conversions | `dfa/dense.rs:2928`, `DFA::set_universal_starts`; `Builder::build_from_nfa:1209–1293` |
| Forward followed by anchored, All reverse with identical limits | `meta/wrappers.rs:845–916`, `DFA::new` |
| Final unused eager dense Builder/Compiler | `dfa/regex.rs:690–705`, `Builder::new`; `:818`, `Builder::build_from_dfas` |

The first shuffle traverses every start-table entry, including the dead
unanchored group of an anchored-only DFA (`dense.rs:4317`, `StartStateIter`).
Ordered match IDs, start IDs and accelerator IDs remain ascending. The local
flat maps preserve each source contains/insert/remove/consume operation and
replacement semantics. Their admitted storage and binary search follow the
existing adapter in
`manifest/load/convert/dependencies/sources.rs:check,insert`: actual fixed-key
comparisons and shifts are admitted before mutation; Buffer reserves a full
replacement extent prospectively. The new removal operation prospectively
admits the suffix rotation and then releases the final inert entry. There are
no callbacks, implicit tree nodes, recursive values or new hasher choices.

Each remapper owns an exact collected state-ID vector and later an exact clone,
both alive while inverse permutation cycles are followed. Each cycle terminates
because the map starts as the identity and changes only by pairwise swaps.
Every cycle step, transition cell (including padding) and start remap is admitted.
The same source algorithm deliberately revisits a normal state when a swap
places another accelerator there. Progress either advances the normal boundary
or decreases the current ID. Class-element discovery scans bytes 0–255 for each
non-self class, then visits its EOI sentinel; EOI never becomes a needle.

MatchStates replacement builds both new arrays while the previous arrays and
ordered map remain owned. The pattern offset is checked before the first slice
push. The later acceleration replacement cannot introduce that semantic error,
since counts are unchanged; failure there is an internal invariant violation.
No new DFA-size or auxiliary-size checks are added after determinization.
Accelerators retain their growing vector after the four source shrinking calls.
Shrinking uses an admitted exact replacement/copy; old extents stay charged.

Determinizer scratch survives the entire first shuffle, then drops before
acceleration. Each eager dense Builder compiler survives all finishing calls
and drops before the next builder. On successful forward completion the reverse
builder uses anchored-only starts, All match kind and the same per-builder
quarter limits. A reverse semantic failure discards dense ownership but retains
the reverse NFA in Prepared. A Session refusal remains terminal and never turns
into semantic fallback. Successful pair assembly also admits the eager compiler
inside the source's final regex Builder temporary, before moving both DFAs.

Tests compare pre/post-shuffle transitions and ordered match lists across all
start groups, EOI, every one-byte input and a two-byte corpus. A synthetic graph
forces all three accelerator relocation categories and normal-state revisiting;
additional tests cover space/four-needle rejection, high/zero bytes, universal
start short-circuit, exact compaction, semantic offset and reverse-size failure,
final eager-builder ownership and terminal refusals. These are deterministic
source-correspondence tests, not a claim of production resource fit or a
completed matcher search implementation. Root owns execution of the tests.

Root validation on 2026-09-26: all 114 NFA tests pass, including the ten new
finishing/pair cases; default importer and feature-enabled CLI checks pass.
A separate read-only focused comparison found no actionable issue in shuffle,
remapping, acceleration or admitted scratch operations. This is not the required
full independent candidate review or release qualification.
