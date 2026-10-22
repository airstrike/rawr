# Ordinary target conversion

Derived from Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/targets.rs:220–273,353–385,437–465,495–516,563–585,891–905`,
with constructors in `core/manifest.rs:832–968` and string crate conversion in
`core/compiler/crate_type.rs:119–130`. License texts are in `load/path/LICENSE-*`.

Library conversion joins the normalized path first, retains crate-type error
and warning priority, then applies configuration and original-name inference.
The pin maps `procmacro` to the known ProcMacro type in its generic conversion;
the library's special `proc-macro` branch is separate. Unknown names remain
Other values. Crate types alone do not set the proc-macro or host boolean flags.

Binary filename gates run for the whole group before unique names. Duplicate
names reject at their first repeated occurrence; an admitted sorted name buffer
uses binary lookup and explicitly charged insertion movement without examining
later targets after rejection. The group then joins paths, retains filename and
required-feature presence, applies example kind selection and configuration.
Empty/all-bin example types become ExampleBin rather than a retained library
type vector. Name and original field lifetimes borrow the normalized owner.

`Owner::ordinary_targets` composes library, binaries, examples, tests and benches
in the pin's order. This boundary is intentionally incomplete: custom-build
scripts and metabuild follow, then remaining manifest conversion. It does not
mint Loaded authority or enforce the entire closed-driver protocol.

29 connected package/library tests passed in
`/private/tmp/rawr-ordinary-target-final-tests.log`; five target settings tests
passed in `/private/tmp/rawr-target-settings-tests.log`, using pinned nightly
and importer features `rustc-driver,test-fixtures`. Tests cover converted order,
types, flags, paths, empty required-feature presence, filename gating before
duplicate/edition errors, library conflict-before-warning, proc-macro warning
before mixed-type rejection, and explicit-empty crate types. Initial compilation
found helper visibility and a Buffer::push return-arm mismatch; both were fixed
before successful runs. Stable rustfmt and diff checks passed.

Independent read-only review accepted ordinary conversion, configuration,
source order and ownership with no actionable blocker (confidence 0.94).
Membership remains test-gated. No complete target conversion, production
activation, installed candidate, release qualification or Gallery timing claim.
