# Path-base configuration entry

Pinned Cargo commit: `59800466c5c41c444d264b1010b4d57e85a7117f`.
Source correspondence: `src/cargo/util/toml/mod.rs::lookup_path_base`,
`src/cargo/util/context/key.rs::ConfigKey::{from_str,push}`, and
`src/cargo/util/context/mod.rs::GlobalContext::has_key`.

`Key::path_base` requires the existing `path-bases` feature before allocating a
lookup key. It accepts only the existing PathBase name grammar, which excludes
periods; another scalar grammar cannot introduce dotted lookup components.
TOML components borrow the exact original spelling. Environment spelling follows
Cargo's hyphen replacement then Unicode uppercase conversion. One prospectively
admitted owned string holds the key and its trailing-underscore prefix; the exact
key is a borrowed slice. The original Session admits the input, output, item and
allocation, retaining its failure latch through every operation.

`has_environment` implements only the first two branches of `has_key`: an exact
present value, including an empty string, returns true without querying a prefix;
an absent exact value requires the separately recorded prefix predicate. Missing
evidence remains terminal. False still requires the full effective configuration
lookup. True still requires the typed string getter, including configuration
type/error precedence; it cannot authorize a path or skip configuration loading.

Tests compare name conversion with the exact pinned replacement/uppercase
expression, including Unicode expansion and case-preserved TOML names. Every
admission refusal is checked before allocation, feature rejection precedes key
work, another name grammar is rejected, and the environment arm is exercised on
the actual recorded Host under one original account. These are component checks;
there is no complete config merge/definition lookup, normalized package, current
membership proof, production reuse, or Gallery performance claim here.

`Definition` retains borrowed origin data and Cargo's class-only equality/order:
CLI > environment > file > builtin, with equal priority inside each class even
when paths/keys differ. Exact path/key provenance is not discarded. Relative
path resolution follows `context/value.rs::Definition::root` and
`context/path.rs::ConfigRelativePath::resolve_path`, then calls the existing
admitted native join. Files and file-backed CLI use two parents; other origins
use Cargo cwd. Neither canonicalization nor template replacement occurs.

Only the reached absent second parent is classified as a terminal source cause,
admitted before retention on the original Session with no synthetic manifest
span. No join occurs even for an absolute raw value. Earlier failure or refusal
to admit the cause wins, and later work cannot continue. Missing first-parent
geometry is not included; it remains an internal-state failure for data
that cannot represent a successfully loaded config file. Root-level files with
a first parent remain valid historical definitions until resolution is reached.
Complete loading/selection must still establish these input invariants and
preserve unused/overridden definitions without eager resolution.

Tests include the unchanged upstream Definition excerpt from
`importer/reference/manifest/config-definition/` and compare all class pairs and
ordinary root/join results. Test-only catch_unwind confirms both file-backed
failure variants for relative and absolute raw values; original-account and
post-failure checks cover retained-cause refusal and no continuation. This is
component evidence, not the still-missing effective config getter/merge route.

`Values::from_document` first runs complete generic TOML conversion with a Table
root, matching Cargo `parse_document` followed by `ConfigValue::from_toml`.
`walk::toml_table` differs from generic Value only at the root: a literal private
datetime key there is an ordinary table member. Nested values retain the existing
special datetime behavior. Thus later TOML conversion errors precede the config
type pass, even if an earlier value is an unsupported float or datetime.

The fresh flat projection is in source preorder. Initial config conversion checks
every node, admitting its work on the original Session, and rejects the first
unsupported Float/Datetime category with its retained source location. Allowed
strings, signed integers, booleans, lists and tables remain in the same owned
projection; the wrapper transfers it without a second recursive tree or copies.
All initial nodes share the supplied historical file definition. This is not a
merged tree or an effective selected-value proof. The complete getter and merged
per-value origins remain downstream.

The test-only reference in `importer/reference/manifest/config-values` executes
the unchanged pinned inner converter against its pinned TOML dependency. It
retains reverse diagnostic key/index paths; the contained diagnostic currently
uses category and original source location, not Cargo's rendered anyhow context.
Tree data and first failure category/order are compared across all 14 cases.

`merge::key` preserves the exact nonmergeable-list classification reached by
`ConfigValue::merge_helper`. All six pinned patterns are retained. The key
context appends on occupied entries and does not pop between sibling merges,
matching the source even when that produces an unconventional path. Its matcher
also preserves ConfigKey's wildcard behavior when a part is missing. This is
unused environment-path bookkeeping: admitted borrowed components are permitted,
so no unused uppercase environment String is retained. The component cannot
perform an environment lookup. Storage growth and each bounded pattern step
use the original Session; allocator slack does not authorize added entries.

The unchanged ConfigKey implementation is retained as a direct test reference
under `importer/reference/manifest/config-merge-key/`. Complete merge still needs
the actual occupied-entry iteration and mutable value operations. Cargo uses
rustc-hash 2.1.2 FxHashMap (per its lock), so table iteration must not silently
be replaced with source order or sorted order while this shared path affects
list behavior. No complete merge or current membership authority is claimed.
