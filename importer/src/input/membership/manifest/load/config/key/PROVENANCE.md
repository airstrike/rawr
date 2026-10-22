# Registry lookup keys

Registry constructors follow Cargo context/mod.rs get_registry_index and
check_registry_index_not_set, with context/key.rs environment encoding at commit
59800466c5c41c444d264b1010b4d57e85a7117f. A validated registry name stays one
component between registries and index. The fixed default key is registry.index.
Hyphens map to underscores and characters uppercase as in existing path-base
key construction. Descendant-presence prefixes retain their final underscore.

The shared encoder preserves the existing path-base allocation and admission
sequence. Components live in a fixed three-slot array with a private length;
lookup iterates only its two or three active components. No dynamic collection
or arbitrary key-construction interface is exposed.

Six key tests and eight shared lookup tests passed on nightly-2026-07-11 with
rustc-driver,test-fixtures. Independent source review found no blocker. These
keys only identify lookups; they do not resolve registry configuration or URLs.
