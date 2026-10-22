# Pinned Cargo include-list extraction

`program.rs` contains unchanged `ConfigInclude`, its constructor, and
`GlobalContext::include_paths` excerpts from Cargo commit
`59800466c5c41c444d264b1010b4d57e85a7117f`,
src/cargo/util/context/mod.rs. Cargo contributors, MIT OR Apache-2.0; license
texts are retained in the contained manifest loader's path directory.

The test driver supplies a unit GlobalContext, String errors and formatting macro,
the exact four-character is_glob_pattern expression from restricted_names.rs:48,
and the existing unchanged config-merge ConfigValue/Definition/ConfigKey program.
Input parsing uses pinned toml's full Table parser. The reference runs actual
Cow cloning/removal and preserves resulting root values even after rejection.
It does not exercise file resolution, discovery or include loading.

The 1,410 observations cover five definition forms and both removal modes.
There are 110 successful extractions and 1,300 first rejections. Paths cover
relative/absolute names, trailing slash, Unicode, case, dotfile, backslashes,
every glob character and brace, and competing syntax failures. Later invalid
rows test full conversion before syntax validation, path-before-optional errors,
missing path, empty lists, mixed row shapes and ignored unknown fields.

Compile reference.rs using pinned nightly-2026-07-11, edition 2021, exact
rustc-hash 2.1.2 rlib, and retained manifest-reference toml/serde_json rlibs.
Feed the observations back as JSONL input to regenerate byte-identical output;
the harness overwrites only its result fields. The contained tests compare every
returned path, exact origin and optional flag, first error class, and root entry
presence after both success and failure. Separate tests exercise distinct child
and table definitions, failure spans, and every original-account refusal.

`resolve.rs` retains the lexical prefix of ConfigInclude::resolve_path with
definition/path/cwd supplied directly instead of inert struct/accessor fields.
Its normalization calls the separately retained unchanged Cargo normalize_path
reference. The optional existence branch is deliberately outside this lexical
reference; recorded predicate and NUL behavior have separate contained tests.
