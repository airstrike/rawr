# Pinned optional configuration path reference

Reference pin: Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`,
nightly-2026-07-11 aarch64-apple-darwin, rustc-hash 2.1.2, serde 1.0.228,
toml 1.1.2+spec-1.1.0 and serde_json 1.0.150. Use explicit extern paths to the
existing manifest-reference rlibs and pinned rustc-hash rlib; compile edition 2021.
No Cargo process or live configuration/credential file is needed by this harness.

`value.rs` is the complete unchanged pinned context/value.rs, including the
actual serde Value<T> and Definition programs. `protocol.rs` retains unchanged
ValueSource, ValueDeserializer and Tuple2Deserializer sections from context/de.rs.
`context.rs` retains the get_value_typed macro, get_cv/get_cv_helper,
get_config_env and has_key from context/mod.rs. Only the tracing::trace statement
is omitted to avoid a logging dependency. CV conversion uses the unchanged
config-merge reference; ConfigKey uses the existing unchanged key reference.
Copyright the Cargo contributors, MIT OR Apache-2.0; license texts are in the
contained loader's path directory.

The driver supplies prepared in-memory ordinary/credential tables and raw native
environment values. It does not simulate discovery, includes, lazy values()
loading or credential loading. A small serde Deserializer supplies the exact
reached option/string/magic-Value-struct method bodies and rejects other methods.
The ConfigValue array protocol is unreachable for Option<Value<String>> and its
test stub rejects; wrong configuration arrays reject in get_string_priv before
that protocol. ConfigError and key-context rendering use test-only string errors.
The case-mismatch warning hook has no result or failure and is a no-op here;
warning rendering/IO is outside the functional lookup evidence.

The harness deserializes Option<Value<String>>, exactly the transparent contents
of Option<ConfigRelativePath>, through the actual pinned Value<T> serde protocol.
It then calls the unchanged Definition::root and native Path::join. The test-only
catch_unwind surrounds only that final root/join observation; production uses
the already governed root-failure cause. It does not make arbitrary panics acceptable.

The 552 rows retain full input JSON, making observations directly replayable as
stdin. They cover absent/prefix-only/empty/valid/non-UTF-8 environment, missing or
wrong intermediate/final types, all definition classes, already loaded credential
precedence, Unicode/hyphen keys, empty CLI origin, non-UTF-8 origin paths and the
selected root-file failure. Outcomes are 229 paths, 9 genuine None results,
308 ordinary errors and 6 root failures.

Important source results: ValueSource chooses origin using raw environment
presence and chooses config on equal priority, while get_string_priv uses UTF-8
environment text and chooses environment on equal priority. Thus the returned
path's origin need not equal the string getter's internal origin. Definition
paths pass through to_string_lossy and deserialize; an empty CLI path becomes
None. BuiltIn's emitted integer 0 fails Definition's tuple deserializer before
root resolution; this is an ordinary type error, not a source panic.

The tests compare returned strings, serialized origins, final native path bytes,
ordinary error class and root failure. They do not establish config loading,
current observation validity, a complete membership witness or Gallery speed.
