# Pinned configuration merge-key reference

`reference.rs` contains the unchanged ConfigKey declaration and complete inherent
implementation from Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/context/key.rs`, ending before its Display implementation.
Copyright the Cargo contributors, MIT OR Apache-2.0; license texts are in the
contained loader's `path/` directory. No Cargo dependency or custom matcher is
used by this test reference.

Tests instantiate this source directly and compare the six nonmergeable-list
patterns, wildcard edge cases, and successive pushes. The caller correspondence
is `config_value.rs::ConfigValue::merge_helper`: occupied entries append a key;
there is no pop after the recursive return. Consequently a subsequent sibling
uses the accumulated path. This observation does not establish the full table
iteration order or complete merge execution.
