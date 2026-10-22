# Patch registry selection

Pinned Cargo 59800466c5c41c444d264b1010b4d57e85a7117f,
util/toml/mod.rs patch() URL selection and util/context/mod.rs get_registry_index.
License texts under load/path.

crates-io parses the fixed registry index without consulting registry configuration.
Other spellings first pass RegistryName validation, then registry_index; semantic
errors fall through to string IntoUrl and its contextual patch diagnostic.
Invalid registry-name URL spellings do not consult configuration. The crates
misspelling hint remains part of the final contextual error. Refused observation,
allocation and work operations never select URL fallback.

Config Owner retains terminal semantic state for consumed partial configuration.
This helper does not clear that state: a RegistryName has no colon and cannot be
an accepted absolute URL, so a configuration rejection followed by literal URL
fallback cannot produce a successful patch registry. Session trial rejection
still permits the source-reached final URL error construction; resource refusal
remains latched. No additional configuration operation follows that branch.

Connected tests cover fixed/default and literal URL paths with no config records,
definition-relative named index, invalid index/type fallback, the crates hint,
and an unobserved named lookup whose refusal also prevents a later valid URL.
All 75 configuration/connected tests pass. This is URL selection only; patch
warnings, dependency collection, duplicate URL replacement, and full caller
authority are subsequent work. No production activation or timing claim follows.

Per-entry conversion now forms the patch table label, collects all unused keys,
emits warnings, runs the shared normalized dependency conversion with the caller's
retained platform, and only then copies the manifest path into its owned location.
Pending JSON retains the dependency and borrowed manifest path without claiming
location ownership or exposing a completion method. Source grouping, insertion
and duplicate URL replacement remain outside this entry stage.

The actual Host fixtures verify workspace-normalized path preservation, supplied
platform, owned location, and warning order on success, invalid version, and
JSON suspension. All 76 configuration/connected tests pass. These checks do not
qualify native suspension resumption or full patch/manifest authority.

Group conversion now visits normalized source groups in original key order,
resolves the registry before any entries, finishes all entry conversion, and
then inserts by complete serialized URL equality. Equal keys preserve the first
URL object and replace its entry collection; warnings/work from the overwritten
group remain. The sorted transformed buffer uses admitted binary comparisons,
movement and original storage. It is not Cargo HashMap iteration-order authority.
Pending JSON retains completed groups, active URL/entries and exact source/entry
indices without advancing to another group or exposing native completion.

Connected alias tests retain both warning sets while replacing entries for two
spellings of one URL, and stop at JSON before a later unobserved registry. All
77 configuration/connected tests pass; independent read-only review accepted the
bounded collector. Replacement handling, the full manifest caller and native
JSON resumption are still required before Loaded/current-input authority.
