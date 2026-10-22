# Artifact dependency pure conversion

Source: Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/mod.rs` artifact dispatch at 2332–2365,
`src/cargo/core/dependency.rs` `Artifact::parse` (507–528),
`ArtifactTarget::parse` (554–560), and artifact kind parse/validation
(644–688); `src/cargo/core/compiler/compile_kind.rs` `CompileTarget::new`
(194–219). This is the bounded pure-stage implementation.

The caller must reach this stage after dependency source, requirement and other
setters. The helper preserves bindeps gating, complete ordered kind parsing,
all-bin conflict before duplicate validation, target construction before the
normal/dev Assume restriction, and lib-presence before target-presence errors
when artifact is absent. Empty selected-bin suffixes and empty kind lists retain
Cargo's behavior. Duplicate validation sorts a bounded borrowed index instead
of cloning owned kind strings; it retains original kind order and computes the
same duplicate count. Storage is prospectively admitted on the original Session;
work and strings are bounded and a refusal remains terminal. The sorted index
uses the fallible form of the existing nonrecursive heap sort and owns no string
payload. Every ordering and duplicate comparison admits one operation; selected
name comparisons separately check both string extents and prospectively charge
the shorter byte length as cumulative comparison work. This bounds repeated
common-prefix scans without changing O(n log n) comparison complexity. The old
infallible wrapper uses the same loop and comparison order; tests compare traces
and exhaustively abort each comparator call.

ArtifactTarget recognizes exact `target` before CompileTarget trims. The latter
preserves empty, non-JSON tuple, and unstable JSON gate order. Pinned std's Unix
`sys/helpers/small_c_string.rs` uses the 384-byte stack/heap threshold before
local NUL validation. The transformed long-input representation prospectively
admits and retains a byte vector including its terminator; it is not a claim
about CString's allocator capacity. Local NUL produces the source static
InvalidInput description, with no invented errno.
The long-input vector drops before diagnostic formatting on local NUL, matching
CString failure-payload teardown before NUL_ERR. Its admitted bytes remain
charged. A long-NUL test checks input allocation before diagnostic allocation
and terminal refusal at every intervening admission.

JSON success is deliberately unavailable. `Progress::Canonical(Pending)` retains
completed kinds, lib flag, dependency name and exact trimmed spelling, including
relative spelling. It performs no directory join and uses no current absolute
Host query. Actual producer-CWD object association, native operation, bounded
native errno/description acquisition, and replay proof remain required.
Dropping the pending value discards the uncompleted construction. There is no
public arbitrary-byte completion API and no complete artifact/dependency claim.

The private post-native transformation is tested in isolation: whole-path UTF-8
validation precedes owned full-path storage, which precedes native Path stem
extraction. A missing stem is the distinct terminal CompileTargetStem source
cause; its admission can fail first. Whole-path UTF-8 establishes stem UTF-8,
so no second source panic classification is introduced. A successful target need
not retain a JSON suffix or name a regular file. This pure helper must eventually
receive only the canonical result associated with the pending native operation.

Tests use actual schema/inheritance views for dispatch and cover source error
precedence, kind order and duplicates, relative pending spelling, target literal
versus trimming, JSON gate versus local NUL, post-native UTF-8/stem behavior and
prospective refusal. They do not qualify native acquisition or full Loaded.
