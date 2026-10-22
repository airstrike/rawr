# Embedded package name

Source: Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/mod.rs:676–689`, `src/cargo/util/toml/embedded.rs:50–61`,
and `crates/cargo-util-schemas/src/restricted_names.rs:91–121`.
The final package-name check reuses the contained complete scalar grammar in
`manifest/scalar/name.rs`, corresponding to `restricted_names.rs:44–87`.
Unicode classification uses the existing exact `unicode-ident = 1.0.24` pin.
Cargo license texts are retained in `../../path/LICENSE-MIT` and
`../../path/LICENSE-APACHE` relative to this document.

The caller invokes `embedded` only at the pinned missing-name branch of an
embedded package. Native `Path::file_stem` precedes lossy UTF-8 conversion.
Missing file stems violate the source loader's prior filename requirement;
they are not a newly recognized source-panic category or supported fallback.

The transformation streams `slice::utf8_chunks`: each valid chunk contributes
its characters, and each nonempty invalid chunk contributes one replacement
character. This is the invalid-sequence grouping of `String::from_utf8_lossy`,
without allocating a temporary string. ASCII underscore remains unchanged by
replacement, so scanning native bytes selects the same global placeholder as
the source's lossy string search.

The visitor recognizes nonoverlapping `::` separators, including empty parts.
For each part it discards leading characters until XID-start/underscore and
non-digit; subsequent XID-continue/hyphen characters survive and all others
become the chosen placeholder. An empty result becomes `package`. Every part
is nonempty after that substitution, so unconditional separators between parts
are equivalent to the source's nonempty accumulated-string check. Namespace
separators are processed before the part sanitizer, preserving single-colon
and three-colon behavior.

One pass measures the lossy input spelling for its string bound. Two visits
measure and write the final text, eliminating source temporary per-part strings
and joining reallocations. Actual ownership is one admitted result item and
one exact prospective UTF-8 output layout, under the caller's original Session.
The full final string bound is checked before reservation. No input-sized
allocation guess, hidden lossy conversion, infallible clone, refund, or new
account is used. The final complete package-name validator allocates nothing;
its borrow ends before the owned String is returned. The String is the actual
converted result, not a marker requiring later sanitization.

All scans are monotone over bounded native input or output, with a constant
number of passes. Empty namespace parts can expand to seven ASCII bytes, and
that expansion is measured with checked arithmetic before allocation. Traversal
and partial-failure teardown are flat and allocation-free. A refused Session
prevents all later naming work; an allocation failure is latched before output
is written. There are no filesystem, hash, or candidate events in this helper.

Tests compare the sanitizer directly against the root-provided 707 pinned
`embedded::sanitize_name` observations in
`importer/reference/manifest/name-observations.jsonl`. They separately cover
native stems, namespaces, Unicode and invalid UTF-8 grouping, global placeholder
choice, exact/prior item-byte-string boundaries, and each admission refusal.
The parent owns execution of these tests and the reference provenance; writing
this module does not itself establish that the differential tests have passed.
