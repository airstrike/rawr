# Cargo string IntoUrl

Source: Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/into_url.rs`, the string and native path implementations.
Cargo license texts are retained in the adjacent `load/path` directory.

This adapter calls the existing complete pinned no-base/default URL parser at
`manifest/spec/url`. It adds no parsing, normalization, IDNA or host algorithm.
Mechanical manifest-visible access is restricted to Url/Invalid, parse and the
Session-checked serialization getter. Existing mutators stay private to spec.

Successful parsing returns the same owned Url. Only semantic parse failure
reaches the error wrapper. For an original input starting literally with `git@`,
the first colon alone is replaced by a slash in one bounded owned suggestion;
absence of a colon still copies the input. The replacement is not parsed. The
message prefixes that replacement with `ssh://`. Other inputs receive the plain
`invalid url` message, retaining original text and exact parser error Display.

All parser work, failed-attempt storage, replacement allocation and final-message
allocation use the original Session cumulatively. Existing bounded formatting
admits each actual string and allocation before ownership. Refusal propagates
directly through `?`, never through the semantic-error suggestion path, and
retains the original cause. Tests cover accepted serialization, exact errors,
literal prefix matching, first-colon behavior, no-colon copying, every admission
failure across parser/suggestion/message, and a parser-terminal refusal.

The native path adapter calls the separately implemented
`Url::from_file_path`, preserving its owned output without reparsing. Semantic
failure alone formats `invalid path url` with the original native path's lossy
Display; a PathBuf caller borrows its Path as Cargo does. Native tests cover
constructor serialization, exact non-UTF8 relative-path errors and refusal at
every constructor/error-formatting admission.

Seven adapter tests and 28 parser regression tests passed with nightly-2026-07-11,
offline rawr-import library features rustc-driver,test-fixtures. After extending
the pinned percent-encoding reference to SPECIAL_PATH_SEGMENT, the focused
output regression also passed. Independent read-only review found no blocker
in the adapter/native constructor boundary. Diff checks passed. This does not implement
canonicalization, SourceId, full
dependency conversion, Loaded authority, or membership/release qualification.
