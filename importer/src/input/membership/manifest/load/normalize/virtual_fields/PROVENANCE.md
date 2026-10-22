# Virtual normalization field gate

Derived from Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f`:
`src/cargo/util/toml/mod.rs` 589–592 and
`crates/cargo-util-schemas/src/manifest/mod.rs` 73–97 (`requires_package`).
License texts are retained in the sibling `path` directory.

The fixed order is badges, features, lib, bin, example, test, bench, dependencies,
dev-dependencies, build-dependencies, target, lints, hints. Both dependency alias
spellings map to the preferred hyphenated diagnostic name. This checks presence,
including empty collections, rather than content. Source document order cannot
change the first rejected field. The helper performs no package-selection test:
its caller must invoke it only on the already-selected virtual branch.

The fixed field lookups and iteration are admitted on the original Session;
exact diagnostic text uses the shared admitted formatter. Native resource
refusal remains a terminal outer Step failure; virtual field rejection is an
owned semantic diagnostic in the inner Result. No new diagnostic tag or loaded
virtual-manifest authority is created.

Tests cover every field and alias, reverse document order for every suffix of
the source priority list, empty presence, allowed non-package sections, and
terminal admission refusal.
