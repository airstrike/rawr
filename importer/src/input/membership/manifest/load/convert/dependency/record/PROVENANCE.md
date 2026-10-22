# Retained dependency prefix before artifact conversion

Pinned Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`,
`util/toml/mod.rs:2288–2324` and `core/dependency.rs:118–166,255–275,308–363`.
License texts are retained under `load/path`.

The actual normalized dependency enters the existing ordered checks, then source
selection, before package-name ownership and Requirement parsing. Requirement
failure retains both Cargo's context (using the selected package name) and the
pinned semver cause. Unspecified requirements remain distinct from explicitly
specified wildcard requirements. No separate string interner is introduced:
admitted owned strings are transformed storage, not a claim to reproduce
the process-global string interner's allocation layout.

Feature strings retain order and duplicates. Default-feature aliases, optional
state and a copied platform follow. Registry and registry-index assignment is
performed again after those fields, as in Cargo, even when source selection
already consulted the same registry. Kind, explicit rename ownership and public
state follow in order. Normalization removes or rejects non-normal public fields;
the corresponding setter assertion is an internal caller invariant, not a new
source-panic classification for arbitrary inconsistent inputs.

The simple-version promotion is owned exactly once. The synchronous continuation
borrows it throughout checks, source selection and record construction, then
moves it into the private BeforeArtifact record to survive later artifact/native
suspension. Prefix-only check/select APIs discard it at their explicit partial
boundary; complete callers must continue from the retained record.

The prefix is intentionally incomplete. The composed `dependency::convert`
entry point now consumes it into an ordinary/artifact converted declaration or
retains it beside a pending JSON target request. Fields and simple promotion move
without a second parse, source lookup, or allocation. Relative JSON acquisition,
dependency collection, producer-history association and Loaded/current-input
authority remain outstanding. It provides no activation or performance evidence.

Connected tests extend the real Reader/package/target/tail pipeline and recorded
Host through retained fields and all source alternatives. They check rename,
feature ordering/duplicates, defaults/optional/platform, requirement presence,
simple promotion retention and repeated registry assignment. Negative tests
place renamed requirement failure before an unobserved registry, source failure
before requirement parsing, and direct registry reassignment failure after the
earlier version warning. An actual Host pipeline runs at exact and one-short item
and byte limits with terminal refusal. Requirement, storage and platform retain
their independent exhaustive component refusal coverage.

Composition validation: 73 configuration/connected tests pass, including ordinary,
artifact-tuple, and relative-JSON-pending declarations at exact and one-short Host
budgets. A renamed invalid requirement still fails before an unobserved registry
and a disabled artifact gate. Independent read-only review accepted ownership,
source order, and the deliberately pending native boundary.

The shared borrowed Input facade now supplies ordinary normalized declarations,
normalized patch declarations, and direct declarations to this same ordered
converter. Patch fields delegate to normalization's replacement-aware get:
consumed base is absent and the resolved path replaces original text. No second
field parser or path transformation is added. Direct declaration names must come
from their caller's already interpreted package/spec name; this facade does not
perform replacement-key parsing or assert complete patch/replacement handling.
The connected workspace-patch fixture converts the actual resumed normalized
patch and verifies file:///root/dep, not the original path relative to /p. The
74 connected tests passed after introducing the facade; the extended patch test
also passes. Full patch/replacement ordering, URL selection and collection still
belong to their subsequent callers.
