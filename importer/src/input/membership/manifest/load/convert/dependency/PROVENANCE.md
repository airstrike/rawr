# Dependency checks before source selection

Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/mod.rs:2212–2287`, supplies this ordered prefix of
`dep_to_dependency`. License texts are adjacent in `load/path`.

The production entrypoint accepts the already normalized
`normalize::dependencies::Dependency`, reading its name, simple value, and
selected fields. It therefore sees workspace inheritance, composed feature
order, and normalized path values. A simple version is copied through bounded
formatting before checks, corresponding to Cargo's temporary detailed value.

Order is missing version/path/git rejection; optional metadata warning; branch,
tag, rev rejection when git is absent; then features in retained encounter order,
with slash rejection before the `dep:` prefix check for each feature. Presence
is not nonemptiness. Version syntax, URL syntax, mutually exclusive git selectors,
source identities, registries, package names, and later dependency conversion are
deliberately not checked at this prefix.

The original Session admits string scans and actual iteration events, formatted
message allocation, and retained warning storage. Existing bounded formatting
and warning ownership are reused. Refusal remains terminal and preserves the
original cause; no later field check or warning can proceed. Semantic rejection
is returned separately as complete source text, retaining earlier warnings.

Focused tests exercise actual schema and inheritance Views through a private
projection seam: missing-source precedence, warning before fixed git-key order,
feature encounter order, slash before prefix, present empty values, simple
promotion, workspace-before-member features, and exhaustive helper refusal.
Production normalized-Dependency connection requires the separate caller fixture;
the projection tests alone do not establish that integration. The connected
Reader/normalization fixture now calls this entrypoint on the actual normalized
tail after target/link and original-dependency validation. Six prefix tests and
32 connected package-normalization tests passed on nightly-2026-07-11, offline,
rawr-import library features rustc-driver,test-fixtures. Independent read-only
review found no blocker within this prefix. Success means this prefix passed, never successful full
dependency conversion, Loaded authority, membership admission, or release proof.

Continuation constraint: Cargo retains its promoted simple version through the
remaining conversion. When extending this prefix, keep downstream conversion in
that scope; do not call this unit-returning helper and promote the version again.
An owned continuation is only needed if that later operation must suspend.
