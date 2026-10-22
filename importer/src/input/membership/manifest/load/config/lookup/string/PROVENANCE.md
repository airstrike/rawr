# Shared optional configuration string protocol

Pinned Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f`:
`context/mod.rs:1056` implements get_string as get::<OptValue<String>>;
`context/value.rs` defines OptValue<T> as Option<Value<T>>;
`context/path.rs:9–11` defines ConfigRelativePath as a serde-transparent
Value<String> wrapper. Therefore Option<ConfigRelativePath> and get_string
reach the identical lookup and origin-deserialization closure. This extraction
shares that existing implementation rather than creating a second protocol.

See ../PROVENANCE.md for the complete pinned has_key → ValueSource →
get_string_priv → Definition codec order, priority rules, non-UTF-8 environment
behavior, and admitted immutable borrowing correspondence. The existing closure
was moved without adding reads, allocations or changing semantic/refusal order.
The optional_path entrypoint wraps the returned text and decoded origin without
any account event; only later resolve adds path work. Its existing tests remain
unchanged. The passed-Key string helper grants no key construction, eager table
loading, registry lookup, URL conversion or current membership authority.

The returned private Value retains borrowed text and decoded Origin ownership.
Getters expose immutable text and the exact post-codec Definition. A native
non-UTF-8 environment can therefore supply the selected origin while the text
comes from configuration. This intentionally follows ValueSource instead of
substituting the inner typed getter's origin. The original Session remains the
sole account through all reads, decoding and errors; no source-required clone
is newly introduced beyond the existing qualified borrowed-view transformation.

New tests exercise text/provenance selection, comparison with transparent path
delegation and callback count, wrong-type/definition-codec precedence, and sticky
missing-record refusal. Existing pinned lookup fixtures remain the broader
protocol correspondence evidence. Eight config lookup tests and six key tests
passed on nightly-2026-07-11, offline rawr-import library with
rustc-driver,test-fixtures. Independent read-only review found no blocker in
the shared protocol or new registry keys. This does not establish registry URL
resolution, full dependency conversion or production reuse.

The configuration owner now delegates through this same getter, preserving
lazy cv loading and retained lookup failures. Connected fixtures verify registry
text with its defining file and the obsolete registry.index check, where even
an empty value rejects. All 67 config-owner tests passed on the same pinned
configuration. Independent review found no blocker in this owner connection.
