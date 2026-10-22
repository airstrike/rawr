# Embedded conversion restrictions

Derived from Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/mod.rs` 1399–1497. License texts are retained in
`load/path/LICENSE-*`.

The helper checks original typed fields, not normalized synthetic targets or
package defaults. Its caller selects embedded mode. The complete invalid-field
list preserves pinned ordering: workspace/lib/bin/example/test/bench, both
build-dependency spellings separately, then the ten restricted package fields.
Original package takes precedence over project; project fields retain package
diagnostic labels. Empty and false values still count as present.

The helper admits each fixed field lookup and retained name slot through the
original Session. The existing prospective formatter renders all names with
comma-space separation and exact singular/plural text. Semantic rejection is an
owned inner error; resource refusal remains terminal. No new tag, normalized
field projection, or conversion authority is introduced.

Tests cover all eighteen restricted fields, empty/false presence, the complete
reverse-declared priority list, both dependency aliases, package/project
precedence, permitted fields, and original-account refusal.
