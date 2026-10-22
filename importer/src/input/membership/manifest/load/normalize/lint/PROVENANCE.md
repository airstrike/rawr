# Lint inheritance

Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/mod.rs:1105–1111` (`InheritableFields::lints`),
`:1152–1169` (`lints_inherit_with`), and `:565–580` (normalization caller).
License texts are at `../../path/LICENSE-MIT` and `../../path/LICENSE-APACHE`.

Absent and local tables do not request a workspace. An inherited table with
local overrides rejects before workspace lookup. A missing workspace lint table
rejects, while a present empty table remains present. The result borrows the
already interpreted table rather than cloning its entire map. It cannot expose
an old workspace=true flag or stand for a completed manifest/load proof.

Each transformation checks and charges the original Session; resume preserves
its first refusal. No host operation, allocation, lookup fallback, numeric
limit change, or new workspace authority is introduced. Caller composition must
place this stage after all target and dependency normalization, as pinned Cargo
does. Existing load/lints.rs verification remains separate and unchanged.
