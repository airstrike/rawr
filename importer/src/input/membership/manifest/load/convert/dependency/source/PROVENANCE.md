# Sequential normalized dependency source selection

Pinned Cargo 59800466c5c41c444d264b1010b4d57e85a7117f:
`util/toml/mod.rs::dep_to_dependency` preceding checks and
`to_dependency_source_id` (2368–2450), `core/source_id.rs::for_git`,
`for_registry`, `for_alt_registry`, `alt_registry`, `for_path`, and
`util/context/mod.rs::crates_io_source_id` (2061–2068).

The actual normalized Dependency caller extends the existing synchronous fields
scope: simple version promotion remains alive across source selection and the
continuation receives the exact promoted version borrow for subsequent stages.
The earlier prefix-only check remains available for focused tests. Source
selection follows the checks for each individual dependency.

Selection preserves Git+path before registry+registry-index ambiguity, then Git
reference ambiguity, branch/tag/rev/default choice, literal reference ownership,
URL parsing, fragment warning, URL clone and SourceId construction. The owned
Selector represents the exact pre-URL clone; the source factory moves it instead
of cloning twice. URL copy copies accepted serialization and offsets without
reparsing. Named-registry key ownership precedes its URL clone; canonicalization
still precedes interner lookup, including hits. The original Git fragment remains
in the URL, matching the pinned implementation despite the warning's wording.

Path resolution copies the normalized text before inspecting the parent. A
non-Path parent is returned as the exact owner-associated handle, including
precise state. A Path parent joins to the declaring manifest parent, normalizes
with the existing native path helper, constructs a native URL, and interns it.
The manifest path's existing complete-loader file context must establish its
parent; this primitive does not classify a new source panic for arbitrary input.

Named crates-io and default requirements share a successful memo field directly
on the modeled config Owner. First initialization checks obsolete registry.index,
parses the fixed Git index and performs named registry construction. A hit checks
Session/config owner and source-history association, then returns the incumbent
handle without config/URL work. Semantic initialization errors leave the memo
empty; terminal errors latch the owner. Other named registries reuse the existing
recorded registry_index operation, including native-base parsing and password
rejection. Direct registry-index uses the original string IntoUrl adapter.

Tests extend the existing connected Reader→package→targets→tail fixture through
actual normalized dependency selection, covering Git reference/warning, native
Path parent and precise non-Path parent, explicit registry-index, earlier feature
failure, both ambiguity priorities and refusal noncontinuation. Config-owner
fixtures cover first representation retention, unsuccessful memo initialization,
foreign source-history refusal, and a successful memo hit using an empty Host
image with a zero-capacity account to rule out repeated observations/ownership.

This is source selection only. Dependency::parse, its exact contextual diagnostic
wrapper and subsequent setters/conversion are not implemented by this stage.
Empty History remains storage rather than proof of absent producer incumbents;
full process prelude, concurrency/history association, current membership proof,
child-free product behavior and Gallery measurements remain downstream.
No policy, cap, trust or support scope changes.

Validation: all 72 config-owner tests passed, including connected package/library
normalization fixtures; all six dependency-prefix and six source-interner tests
also passed. Runs used nightly-2026-07-11 offline with rustc-driver,test-fixtures.
The initial library-module filter selected zero tests; listing identified the
fixtures under config::owner::tests::package_normalization::library_normalization,
and the passing config-owner suite includes them. Independent review found no
blocker; its additional registry-ambiguity precedence case was added and the
72-test suite rerun successfully. Format and diff checks passed. No release or
Gallery qualification was run.
