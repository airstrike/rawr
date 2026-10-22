# Pinned manifest-document reference

This isolated reference package calls unmodified upstream
`toml::de::DeTable::parse_recoverable` from **toml 1.1.2+spec-1.1.0**.
It has its own `[workspace]`, is not a root workspace member, and has no
`rawr-toml` dependency. It does not use `toml::Value`, deserialize numbers into
machine types, copy private upstream implementation, or modify registry files.

The author prepared source only; the root execution owner then generated
`Cargo.lock` and `output.jsonl` in one serial offline run. All 35 cases matched
their source-derived acceptance expectations: 14 accepted and 21 rejected.
The output has 159,938 bytes and SHA-256
`a97c1ee915dd7ecca125d0077dbe5a2276f2f9df2564a57515077a638953d6b1`.
The separate local comparator passed every valid document field/order, first-error
span and bounded category, and available raw first-error context. A subsequent
locked run after scoped formatting reproduced every output byte. These are
executed observations, rather than hand-authored golden results. The execution evidence is recorded in
`provenance.json`; the implementation author ran no Cargo or native fixture.

## Execution owner

Use the already selected pinned nightly. Run these commands serially with the
existing Cargo slot, offline and with one job. The first command is needed only
when intentionally recreating the isolated lock; it does not compile or execute
a fixture:

```sh
cargo +nightly-2026-07-11 generate-lockfile --manifest-path toml/reference/Cargo.toml --offline
```

Before accepting results, compare each package's full version and checksum in
`provenance.json` with the generated lock. Cargo ignores SemVer build metadata
in version requirements; the lock/checksum check establishes the exact
`+spec-1.1.0` package. This check also detects a second, differently resolved
copy of a relevant parser dependency. Check the relevant feature closure for
the manifest's stated defaults/explicit features; no `unbounded`, `debug`,
`fast_hash`, `simd`, or `unsafe` feature is requested by this harness.

Root verified that every active dependency in the captured feature tree and
compile log matches the relevant Cargo pin. The entire lock is **not** an exact
subset of the pinned Cargo lock: it also records inactive `memchr 2.8.3`,
`proc-macro2 1.0.107`, `quote 1.0.47`, and `syn 2.0.119`. Those packages do not
appear in the effective feature tree or compile log. The artifact supports the
active parser-path comparison; it does not claim whole-lock identity.

Then generate the artifact used by the separate local comparator:

```sh
cargo +nightly-2026-07-11 run --manifest-path toml/reference/Cargo.toml --offline --locked --jobs 1 --quiet > toml/reference/output.jsonl
```

Require success and valid JSON on every line before using `output.jsonl`.
An acceptance-expectation mismatch returns failure **after emitting the actual
observations**. Inspect that evidence and the governing pinned source before
correcting fixture intent or local semantics. Do not treat a partial file from
a failed execution or I/O error as a generated reference artifact.

Optional focused invocations use the same manifest and execution settings:

```sh
cargo +nightly-2026-07-11 run --manifest-path toml/reference/Cargo.toml --offline --locked --jobs 1 --quiet -- --list
cargo +nightly-2026-07-11 run --manifest-path toml/reference/Cargo.toml --offline --locked --jobs 1 --quiet -- --case escaped-duplicate-key
```

`--list` emits fixture metadata as JSONL without parsing TOML. Normal execution
accepts only compiled-in cases, not arbitrary external input. Do not substitute
the local 10,000-array malformed-depth case or the 6,400-table compound stress
case: upstream retains recursive ownership, including during recovered failure.
Here, the largest array chain has 82 levels and the compound sample uses four
inline levels with eight dotted keys per level. These are compatibility
fixtures, not stress, safety, allocation, or live manifest-source qualification.

## Observable APIs

The recovered document uses public `DeTable` iteration, `DeValue` variants,
`DeArray` slice iteration, and `Spanned::{get_ref,span}`. String and key text is
decoded. Integers use `DeInteger::{as_str,radix}` and floats use
`DeFloat::as_str`; underscore normalization and overflow remain visible without
eager coercion. Date/time text uses the pinned `Datetime` implementation's
`Display`. `toml::de::Error::{message,span}` preserves report order.

The separate raw observation uses `Source::new(source).lex().into_vec()` then
`Vec<Event>` wrapped by `ValidateWhitespace`, wrapped by `RecursionGuard(80)`,
passed to `toml_parser::parser::parse_document`. This is the exact wrapper order
in pinned `toml/src/de/parser/mod.rs:28–46`. Event kind/encoding are their pinned
public Debug names. Raw error description, context, unexpected span, and
expected items are public `ParseError` accessors. This raw pass does not decode
semantic scalar values, so it is not interchangeable with document diagnostics.

Upstream's inline, dotted, implicit, and array-of-tables flags are private.
The harness does **not** claim to observe their bits. Paired extension/conflict
fixtures and latest-AoT descent expose their behavior through the public API.
Ordinary arrays of tables still emit `kind="array"`; the fixture's extension
result carries the independent behavioral evidence.

## JSONL comparison contract

`schema.json` describes one line. Each case includes its exact `source`, stable
`id`, matching `local_test`, purpose, source-derived `expected_acceptance`,
observed `accepted`, flattened `document`, ordered `errors`, and separate `raw`
events/errors. `accepted` means `parse_recoverable` returned zero diagnostics.

Document nodes are in preorder with contiguous `id` values. `parent` is the
parent ID, `index` is the table insertion ordinal or array index, and the root
has `parent=null,index=0,key=null,key_span=null,kind="table"`. Table children
carry the decoded `key` and its span; array children carry null key fields.
No map sorting occurs. The root span is the pinned default `[0,0]` from
`toml/src/de/parser/document.rs:99`; it is not rewritten to the input length.
All spans are half-open **UTF-8 byte offsets**, encoded as `[start,end]`; missing
diagnostic/key spans are `null`. There are no borrowed/owned memory identity
claims in this transport.

| Kind | Additional fields |
|---|---|
| `table`, `array` | None; subsequent nodes identify their parent |
| `string` | `text`: decoded text |
| `integer` | `text`: normalized spelling; `radix`: 2, 8, 10, or 16 |
| `float` | `text`: normalized spelling, including overflow and special forms |
| `boolean` | `value`: JSON boolean |
| `datetime` | `text`: pinned parsed date/time Display |

For accepted sources, the local comparator should compare all document fields,
node order, decoded keys, and scalar payloads. The flat transport avoids JSON
parser recursion limits being confused with TOML depth limits. For rejected
sources, compare rejection and the governed bounded diagnostic category,
span, and precedence. `errors[0]` is the first upstream diagnostic. Full wording,
all post-failure diagnostics, and the recovered document are additional upstream
evidence; the harness does not require local retention/equivalence of those artifacts.
In particular, do not let matching recovered values upgrade a rejected source
to accepted. This harness confers no whole-file EOF, freshness, path, physical
source, or resource-budget authority.

## Fixture matrix

The 35 cases are declared in `src/fixtures.rs`; `--list` is their machine-readable
matrix. The `local_test` field associates each fixture with the semantic test
family, including where this reference deliberately uses a smaller sample.

| Cases | Focus |
|---|---|
| `full-document`, `insertion-order`, `implicit-reinsert-order` | Complete document, ordered keys, implicit remove/reinsert |
| `escaped-duplicate-key` | Decoded-key collision and precise error span |
| `inline-dotted-siblings`, `inline-explicit-then-dotted-conflict`, `inline-dotted-then-explicit-conflict` | Inline/dotted extension state |
| `dotted-parent-header-conflict`, `extend-inline-header-conflict`, `extend-ordinary-array-conflict`, `duplicate-explicit-header` | Header extension and redeclaration |
| `latest-array-table-descent`, `ordinary-table-array-is-not-aot` | Latest AoT instance and ordinary-array conflict |
| `deferred-numeric-overflow`, `signed-radix-float-spelling` | Deferred integer/float representation and normalization |
| `datetime-forms`, `invalid-datetime`, `datetime-before-duplicate`, `duplicate-before-later-datetime`, `raw-syntax-before-semantic-duplicate` | Date validation and error precedence |
| `unused-joined-scalar`, `unused-invalid-escape`, `unused-inline-duplicate` | Unselected fields still decode and reject |
| `unicode-multiline-escapes`, `unicode-key-and-json-controls` | Unicode keys, multiline forms, expansion and JSON escaping |
| `raw-malformed-recovery`, `bounded-negative-depth-recovery` | Bounded raw and semantic recovery evidence |
| `array-depth-80`, `array-depth-81`, `dotted-depth-80`, `dotted-depth-81`, `header-depth-80`, `header-depth-81` | Raw container and semantic key-count limits |
| `compound-dotted-inline`, `compound-dotted-inline-then-error` | Modest compounded retained depth and later failure |

The package/checksum and selected source-member evidence is in
`provenance.json`. It records the prior verified audit and its immutable Cargo
commit, rather than claiming source preparation executed a new audit.

Run the local comparison without executing the reference producer again:

```sh
cargo +nightly-2026-07-11 test --offline --locked --jobs 1 -p rawr-toml --test reference
```
