# Dependency map normalization

Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/mod.rs:884–955` (`normalize_dependencies`),
`:2883–2910` (`deprecated_underscore`), and `src/cargo/core/dependency.rs:93–99`
(`kind_table`). License texts: `../../path/LICENSE-MIT` and
`../../path/LICENSE-APACHE`.

One invocation retains absent versus empty maps, BTree iteration order, exact
current dependency and lazy workspace continuation. Each dependency passes
inheritance, deprecated default-feature spelling, public gating and path-base
normalization in that order. Workspace continuations retain earlier outputs and
warnings; a base continuation does not repeat warning or config work.

The public feature probe catches only semantic rejection, preserving the pinned
`require(...).is_ok()` branch without swallowing a resource failure in the
admitted representation. Explicit CLI public enablement is supplied by the
eventual closed caller, not read from the environment. False public values still
trigger the presence-based check. Suppressed public and consumed base fields
are inaccessible through the completed view; untouched fields/extras borrow
the admitted originals.

Configured base lookup and lexical join share the patch path's existing helper;
configured workspace bases override built-in workspace roots. This stage owns
no successful Loaded authority and remains unconnected ahead of unfinished
target normalization. Ordinary/dev/build and platform invocation order remains
the responsibility of that later composed caller.
