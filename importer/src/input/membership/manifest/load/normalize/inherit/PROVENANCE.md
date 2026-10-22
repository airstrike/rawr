# Dependency inheritance

Source: Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/mod.rs:1075–1098` (`get_dependency`),
`:1170–1252` (`dependency_inherit_with` and inner merge), and
`:1255–1276` (`deprecated_ws_default_features`). License texts are retained in
`../../path/LICENSE-MIT` and `../../path/LICENSE-APACHE` relative to this file.

The direct branch borrows the original dependency without looking up a workspace.
The inherited branch suspends with the exact member, label, edition and package
root. Its caller must provide the declaration selected by the existing Cargo
workspace lookup; this stage does not manufacture a loaded-root proof.

The transformed view borrows all untouched workspace fields and extras, rebases
only paths lacking a base, applies Cargo's two-spelling default-features merge,
and exposes workspace then member features without deduplication or string
copies. Optional and public come exclusively from the member, including absence.
Inherited unused keys do not become workspace dependency extras. A simple
workspace version becomes a detailed version field. Empty feature lists retain
their presence. New owned paths and warnings use the existing admitted helpers;
lookup steps and stage results share the caller's sticky Session account.

This is the inheritance substage of `normalize_dependencies`: deprecated alias
handling, public gating, and path-base normalization still follow it in Cargo's
order. It is deliberately not connected ahead of unfinished target stages and
does not grant complete normalization, conversion, loading or membership.
