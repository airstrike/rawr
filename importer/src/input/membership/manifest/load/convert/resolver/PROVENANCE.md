# Explicit resolver selection and conversion ordering

Derived from Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/mod.rs:1500–1512` and
`src/cargo/core/resolver/types.rs:119–130`. License texts are retained in
`load/path/LICENSE-*`.

Both explicit declarations reject before either string is parsed. One explicit
value accepts exactly 1, 2 or 3. Neither declaration retains None rather than
inventing an edition-dependent default at this stage. Normalized package fields
and the preserved original workspace supply these inputs. An embedded synthetic
workspace has no explicit resolver.

`Owner::conversion_header` connects the real preamble, embedded-only original
field restrictions and explicit resolver selection in source order. It returns
only edition and optional resolver behavior, not target conversion or Loaded
authority. Connected tests complete the earlier normalization tail and verify
that embedded restrictions beat an invalid resolver, that a permitted script
reaches this boundary, and that package/workspace conflicts precede parsing.

26 connected package/library tests passed in
`/private/tmp/rawr-conversion-header-connected-tests.log`; 11 conversion tests
passed in `/private/tmp/rawr-conversion-header-helper-tests.log`, using pinned
nightly and importer features `rustc-driver,test-fixtures`. Stable rustfmt and
diff checks passed. Independent read-only review found no P0/P1 in the scoped
ordering, presence, formatting and resolver behavior. An initial helper
visibility mismatch was fixed before the successful compile and test runs.

Membership remains test-gated. No complete target conversion, production
activation, installed candidate, release qualification or Gallery timing claim.
