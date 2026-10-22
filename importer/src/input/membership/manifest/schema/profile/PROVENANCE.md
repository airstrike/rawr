# Pinned profile leaf interpretation

The `debug` and `opt` leaf modules implement complete `TomlOptLevel`, `TomlDebugInfo`, and
`codegen_units: Option<u32>`'s present-value conversion. These leaves do not construct
a partial `TomlProfile`, apply profile defaults, interpret PackageIdSpec keys,
normalize a profile, or establish original-manifest or membership authority.
The surrounding Option/missing-field visitor remains the whole-schema caller's
responsibility. These are converted data, not raw-value success markers.

## Exact sources

The Cargo source is commit `59800466c5c41c444d264b1010b4d57e85a7117f`,
`crates/cargo-util-schemas/src/manifest/mod.rs`, SHA-256
`fc503d6532663f2b0f3217b53f235b6c24690e9c85116f1364ec134ca78cd92c`.
The local primary source is under
`/private/tmp/rawr-p9-pinned-cargo-full/cargo-59800466c5c41c444d264b1010b4d57e85a7117f`.

- `mod.rs:1085–1121`: `TomlOptLevel` installs only checked-i64 and string
  callbacks. Every accepted i64 becomes its decimal `to_string` spelling;
  only exactly `s` and `z` succeed in the string callback. The displayed
  diagnostic's 0–3 enumeration is not the integer acceptance domain.
- `mod.rs:1124–1210`: `TomlDebugInfo`'s declared variant order is None,
  LineDirectivesOnly, LineTablesOnly, Limited, Full. False/0/none and true/2/full
  collapse; 1/limited collapse. The two line variants remain distinct. Invalid
  integer or string callback values reject immediately.
- `mod.rs`, `TomlProfile::codegen_units`: the declared value is `Option<u32>`.
  Serde core 1.0.228 `src/de/impls.rs:428–434` installs signed and unsigned
  visitor methods through 64 bits; `int_to_uint` and `uint_to_self` reject
  negative/out-of-range values. It does not install an i128/u128 visitor.
- serde-untagged 0.1.9 `src/lib.rs:734–788` and `src/int.rs:49–159`:
  integer dispatch tries checked conversions into installed callbacks. The
  two custom leaves install only i64, so a representable u64/i128/u128 can
  reach that callback; an unrepresentable number rejects as a type mismatch.
  Rejection from a selected callback does not resume the integer search.
- TOML 1.1.2+spec-1.1.0 `src/de/deserializer/value.rs:87–130` and
  `src/de/parser/devalue.rs:24–36`: raw numbers first dispatch i64, then u64,
  i128, u128. These leaf functions call the existing contained `Input::view`
  once, preserving deferred integer/float overflow and the input span before
  applying their concrete visitor. No nonnumeric visitor is tried for an
  integer callback failure.

Registry primary sources are under
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f`.
Dependency/version and executed-source ownership remain documented in the
parent manifest provenance and committed reference inventory.

## Representation and resource ownership

`Opt` is a `smart-constructor-newtype` with private inline storage and a read-only
`as_str` accessor. Its decimal formatter uses `unsigned_abs`, so i64::MIN is
handled without signed negation overflow. A magnitude needs at most 19 digits;
the negative sign makes the maximum 20 bytes. Both counting and writing have
at most 19 iterations. Zero emits one digit. Radix spelling, separators and
leading plus are already interpreted by the pinned numeric dispatcher; the
result is normalized decimal. The exact s/z branches store one ASCII byte.

Numeric Opt conversion checks the resulting per-string length through the
original Session before writing the inline bytes. String input uses the
existing Input::view per-string check. Debug and u32 are fixed scalar values.
No production path allocates, clones owned text, formats an owned diagnostic,
or substitutes a guessed heap precharge. A caller retaining these scalar
records must admit its actual slot/container layout through the same Session;
inline bytes are part of that layout. Fixed temporary stack storage is bounded.

All inputs first pass Input::view's permanent refusal check. Semantic failures
retain the existing bounded diagnostic tag, source span and static expectation
through Session::reject, which prospectively admits the diagnostic item.
Admission failure remains terminal, including if a caller discards an
intermediate result. There are no graph path/hash/decode/comparison events,
recursion, owned descendants, ambient observations, or recursive teardown.

## Focused evidence

Tests invoke the actual three leaf functions. They cover signed extrema and
decimal digit boundaries, exact string matching, numeric/base normalization,
debug aliases/order, zero and u32 boundaries, distinct u64/i128/u128 dispatcher
behavior, deferred overflow, source spans, every string/diagnostic admission
refusal, and an original account with prior usage and exact retained-slot,
byte and normalized-string floors. Heap growth in tests is fixture/observation
transport, outside the production functions.

`tests/reference.rs` compares 22 leaf results across 20 explicitly selected
cases from the committed, executed pinned Cargo observation corpus (SHA-256
`4bdb6ccb690b4d8eb75a45895d04a659994198666e7c09cfdf24e123cc171b7b`). It compares
typed string/enum/u32 payloads on success, and typed rejection category plus
exact TOML spans on failure. It reuses the shared read/transport helpers and
does not call any whole-profile implementation. This is a scoped comparison,
not complete Original parity. The root owns execution results, formatting,
independent review and commits; test source alone does not establish a pass.
