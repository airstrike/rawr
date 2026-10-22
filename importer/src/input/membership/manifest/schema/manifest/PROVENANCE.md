# Complete original manifest interpretation

The top-level visitor follows Cargo commit
`59800466c5c41c444d264b1010b4d57e85a7117f`,
`crates/cargo-util-schemas/src/manifest/mod.rs:33–69` and the reached component
visitors. That complete member's SHA-256 is
`fc503d6532663f2b0f3217b53f235b6c24690e9c85116f1364ec134ca78cd92c`.
Cargo's successful unused-key assignment and renderer are
`src/cargo/util/toml/mod.rs:193–232`, member SHA-256
`940a2998ebff9915b4a0d050dcadc4d12e05615c7fcc3a5fb447b7d3fea4ea8e`.
Source licenses are retained in `importer/reference/manifest/LICENSE-MIT` and
`LICENSE-APACHE`.

All 22 deserialize fields are visited. Package/project and kebab/underscore
dependency spellings occupy distinct slots; no accessor preference suppresses
the other field's conversion. The skipped `_unused_keys` member is not a field
recognized by deserialization. Maps dispatch in adapter encounter order and
reject duplicate known fields before converting the second value. Unlike the
defaulted Profile structure, the top-level positional visitor requires every
non-skipped field, then invokes the same source adapter's exhaustion operation.

Each field delegates to the complete existing component visitor. The top-level
entry carries no file, workspace, normalization, target inference or package
selection authority. It remains behind the existing cfg(test) membership
boundary until the complete selector and cold/current association are ready.

## Successful unused-key result

Unknown fields retain their callback position before their IgnoredAny walk.
Nested component records preserve original map keys and sequence positions;
Profile's flat locations and global callback log also preserve replaced values.
Top-level events interleave completed fields and unknown callbacks without
sorting by source spans or reconstructing paths from winning map values.

After all typed fields succeed, `unused::collect` renders ordered paths using
Cargo's exact literal map-key/decimal sequence rules. Some/newtype wrappers do
not appear. A dot is added only when the already rendered prefix is nonempty;
distinct component paths can therefore collide. Each string has one owned
allocation. A separately admitted index array supplies sorted, deduplicated
keys without cloning strings. Dependency/lint flattened extras are consumed
data, not unused-key callbacks.

Cargo assigns its local unused set to the returned manifest only after
deserialization succeeds. The contained error result does not expose discarded
partial manifests or the reference harness's external failure-callback trace.
Negative comparisons cover the first rejection and applicable span; ordered
callback comparison applies to successful results.

## Ownership and verification scope

Entries, events, profile nodes/continuations, callback ancestry and index arrays
use the caller's Session and fallible, prospectively admitted full layouts.
No helper creates a substitute account or clears a resource/source refusal.
Recursive profiles use indices into flat storage, including on failure/drop;
the top-level union composes the existing finite field families.

The complete reference test compares all 130 committed pinned observations:
every typed field and successful callback for accepted manifests, all typed
first-error categories/spans, and document-first errors. Dedicated tests cover
original-account floors, refusal at every operation, aliases, callback ordering,
semantic profile-key replacement and deep flat ownership. Actual execution and
scope acceptance are recorded in the MAP tracker; this document is source
correspondence rather than a release qualification result.
