# Build-script stem validation

Derived from Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/targets.rs` 907–929. License texts are retained in
`load/path/LICENSE-*`. Missing-stem containment is governed only by this containment rule.

The helper computes each actual native file stem in script order, groups every
successful stem before reporting duplicates, and retains path order inside each
group. It reuses the existing admitted Fx/std Table implementation: occupied
lookup precedes vacant reserve, and consuming iteration determines conflict
group order. Inert indices replace owned vector values without changing key
hashing, insertion history, bucket count, or control-group traversal. Actual map
allocation remains the existing specialized `(&str, usize)` layout; grouped
path storage is separately admitted and contains borrowed original scripts.

When actual file_stem returns None, the original Session admits the exact
source cause and stops. Earlier grouping work remains spent. No later script or
duplicate diagnostic is computed, including when earlier scripts already
duplicate a stem. A present component borrows UTF-8 input; the subsequent UTF-8
conversion is not classified as another source failure.

Complete diagnostic text uses the existing admitted formatter. No generic
hashing/layout implementation, native query, panic catch, input blacklist or
loaded-manifest authority is added. Full caller ordering and qualification
remain downstream; disabled build and metabuild-conflict handling belong to the
caller before this helper.

Tests cover valid/empty input, hidden and multi-extension stems, grouped output
order through the independently qualified table, original path order, every
isolated missing-stem fixture, later failure before duplicate reporting, prior
resource refusal and refusal to admit the cause, with no subsequent work.
