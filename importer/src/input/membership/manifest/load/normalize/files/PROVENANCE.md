# Build-script and readme normalization

These private helpers contain Cargo
`59800466c5c41c444d264b1010b4d57e85a7117f`'s complete
`src/cargo/util/toml/targets.rs:1094–1123` (`normalize_build`) and
`src/cargo/util/toml/mod.rs:844–870` (`normalize_package_readme` and its default
selection). The pinned source tree is the previously verified Cargo archive;
this slice adds source inspection, not a new archive/binary identity claim.
MIT/Apache license texts are retained at `../../path/LICENSE-*`.

Absent build checks exactly `package_root.join("build.rs").is_file()` and
produces single `build.rs` or Auto(false). Explicit false stays false, true
becomes single `build.rs`, a single string uses the shared lexical
`path::normalize_text`, and every multiple-script list is retained unchanged,
including an empty list and unnormalized paths. Successful helper output is
always present in the pin, so the local return type omits an impossible outer
None. The caller retains the earlier multiple-build feature check and embedded
Auto(false) override; these helpers do not move either policy into their body.

Explicit readme strings remain byte-for-byte unchanged; true selects README.md
without probing and false produces None. Only absence tries README.md,
README.txt and README in that order, stopping at the first file. Each probe
uses the shared native lexical join and the same Session's followed metadata
predicate. Recorded absence, ordinary OS errors and non-file metadata produce
false. Unrecorded observations, malformed image errors and account refusal
remain terminal rather than falling through to the next candidate. This does
not add canonicalization, no-follow or regular-source restrictions.

Build Single owns the actually allocated normalized String. Unchanged multiple
lists borrow their original typed slice; readme text borrows the original
`value::Text`, preserving inline datetime text if that typed value is supplied.
Default filenames borrow static text. No String/Vec clone is needed to preserve
these values. Borrowed selected text is checked against the original string
bound and each normalized result item is admitted. Native paths, component
buffers and full replacement layouts remain charged by the existing path
helpers before allocation; Host separately admits its exact query ownership
and actual observation work. There is no new account, hash, decode or candidate
comparison charge for this pure normalization.

The source tests cover every build variant, native/followed probes, every
metadata kind, explicit readme ownership, all default probe prefixes and the
complete Document → Original → helper route. Resource tests use prior account
spend, exact/item/byte/string/graph floors, refusal before path allocation and
unconsumed later probes after failure. No test execution is claimed here.
These outputs are field data; complete package normalization, lazy inheritance,
loaded-manifest validation and source/current authority remain with the driver.
