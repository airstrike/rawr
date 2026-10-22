# Dependency and lint caller tail

Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/mod.rs:453–585`: top normal dependencies; deprecated dev
alias then selected dev map; deprecated build alias then selected build map;
BTree-ordered platform normal/dev/build maps with the same alias order; lint
inheritance; borrowed hints then badges. The existing helpers pin the inner
dependency and lint semantics. License texts are at `../../path/LICENSE-MIT`
and `../../path/LICENSE-APACHE`.

The caller retains its original cumulative Session, configuration Owner and
warnings across exact map/lint suspension. A successful dependency map returns
the memoized declared root to this outer cursor. That same root serves later
maps and lint inheritance. A resumed map advances only after completion, so
earlier alias diagnostics, warnings and config work are not repeated.

The kebab map wins even when present and empty. Underscore slots remain separate
until their source-ordered alias check. Platforms are ordered by their original
BTree key; absent and present-empty target maps both normalize to None, while a
present named platform with no dependencies remains a platform. Only three
normalized dependency maps are exposed for each platform, never underscore
aliases. Hints and badges retain their admitted original identity.

`all_targets::Owner` now connects the completed library and four target groups
to this tail. It retains the normalized package name/edition, original features,
manifest root and memoized workspace declaration. Warnings move to the enclosing
caller without cloning so a suspended cursor can borrow stable inputs while its
report channel remains mutable. Virtual manifests instead perform the pinned
ordered `requires_package` check and skip the dependency/lint tail.

Connected Reader tests cover workspace suspension/resume through dependencies
and lints, retained target access, alias warning order and the virtual branch.
The 23 connected package/library tests passed with pinned nightly and importer
features `rustc-driver,test-fixtures`, recorded in
`/private/tmp/rawr-connected-tail-final-tests.log`; four virtual-field tests passed in
`/private/tmp/rawr-virtual-fields-tests.log`. Stable rustfmt and diff checks passed.

Independent read-only review accepted the ownership and virtual field seams and
identified an adjacent embedded-script gap: absent original package/project was
incorrectly rejected after the earlier stage synthesized a package. The target
caller now permits this absence only for embedded input, preserving default
false discovery flags. Connected bare-script and empty-frontmatter tests reach
tail completion with the synthesized binary. Review confirmed this correction;
the final 23-test run includes it. The fixture records the existing README
discovery predicates instead of bypassing the original Host refusal behavior.

Completion still does not prove real/virtual conversion, the recursive loader,
successful loading or membership authority. Membership remains test-gated;
these checks do not establish production activation or Gallery performance.
