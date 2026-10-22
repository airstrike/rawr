# Pinned internal Punycode reference

`punycode.rs` is the complete, unchanged `idna 1.1.0` source member
`src/punycode.rs`. This version is selected by Cargo commit
`59800466c5c41c444d264b1010b4d57e85a7117f`'s lockfile. The importer compiles it
only for tests, exposing the exact internal caller, reusable decoder and encoder
to contained-implementation comparisons. It is not a production dependency or
a complete IDNA/URL reference.

The source and both licenses were compared byte-for-byte with the members of
the checksum-verified cached crate archive before copying. SHA-256 values:

- Crate archive: `3b0875f23caa03898994f6ddc501886a45c7d3d62d04d2d90788d47be1b1e4de`.
- `punycode.rs`: `0cc6fa84989898ccf6de0a665a287d7488a6eba4b178bea065d311ecabd81604`.
- `LICENSE-APACHE`: `a60eea817514531668d7e00765731449fe14d059d3249e0bc93b36de45f759f2`.
- `LICENSE-MIT`: `b38f11f6096706e6de553dabe2a7ed142d59b6fa8c97e290c67496154745cdd5`.

Its only external dependency is the test-only `smallvec =1.15.2` with
`const_generics`, also matching the pinned Cargo lock. The upstream ignored
`huge_encode` test is preserved and is not part of focused development checks.
Tests of UTS46's caller-side input guards must distinguish those guards from
the unguarded reference methods; this module alone does not apply them.
