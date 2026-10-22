# Governed CLI file configuration and normal-value composition

This is the private package command's file-argument route, not a generic
GlobalContext or successful inline TOML implementation. The caller must supply
the actual governed CLI arguments after Cargo's Unicode argv parsing. Borrowed
`&str` makes that precondition explicit; it does not lossy-convert OsString or
assert non-UTF8 CLI arguments successfully passed Cargo parsing.

The governed producer uses absolute file candidates from
`cargo/config.rs::append_hierarchy_candidates` and `append_candidate`, retained
as explicit configuration in the authority flow at lines 899–928. The later
Cargo-home pair candidates do not feed this explicit-argument list.
`cargo.rs::config_arguments` emits only `--config` plus each retained PathBuf.
`cargo/resolution/cold.rs::prefix:521–556` appends the reserved Layout config path
after those files. Root's directory/Layout provenance establishes the absolute
roots for this route; final argv-to-replay association remains an integration
obligation, not proof created by the borrowed slice API.

## Source stages and bounded scope

Pinned Cargo 59800466c5c41c444d264b1010b4d57e85a7117f
`util/context/mod.rs:1550–1624` initializes a Cli(None) table, constructs
cwd.join(arg), then probes nonempty arguments with Path::exists. Every metadata
kind is present. A present argument loads the complete file/includes under
WhyLoad::Cli with one shared seen set across all file arguments. The subsequent
load_includes uses a fresh set for each result, and its value force-merges into
the accumulated CLI table. The final merge_cli_args obtains normal values only
after this CLI stage completes, wraps their root as BuiltIn and force-merges CLI.
configure at line 1178 invokes that stage only for nonempty cli_config.

Files::includes_fresh swaps only its seen buffer, performs the complete second
include pass and restores shared file-argument seen even after refusal. It does
not reset or clone the path/document owners. CLI stage entry first creates
the source's fresh shared set; normal traversal subsequently creates its own
per-file sets. Included file origins remain Cli(Some(file)); accumulated CLI
root is Cli(None), and composed normal root remains BuiltIn.

Issue: the generic upstream function also accepts inline strings, while this
governed argv route emits only absolute file paths.

| | A — general successful-inline implementation | **B — contain governed file argv** |
|---|---|---|
| Work | Full dotted-key parser/conversion/security route | **File loading, includes and exact fallback parse-stage failure** |
| Association | Broader than the actual private command | **Must bind the actual absolute Unicode file arguments** |
| Missing shapes | General parser needs additional qualification | **Nonabsolute input remains incomplete replay** |

Recommend and implement B within the verified command route. This does not
narrow Cargo's accepted input language. A nonabsolute argument yields terminal
Host(Unobserved) after its source join, never a Cargo semantic rejection or
fabricated successful inline result. General caller/argv changes require the
corresponding source implementation and association before acceptance.

If a governed absolute argument is absent or its recorded metadata call returns
a native error, source enters toml_dotted_keys. That function starts with
DocumentMut parsing at context/mod.rs:2397–2405. In pinned toml_edit
0.25.12+spec-1.1.0 and toml_parser 1.1.2+spec-1.1.0, initial '/' is an Atom token
(lexer/mod.rs:89–110), a key-expression event (parser/document.rs:146–193), and
an invalid unquoted key (toml_edit parser/key.rs:137,162 and toml_parser
decoder/string.rs:684–744). DocumentMut refuses accumulated parse errors before
successful deserialization or credential-security checks.

The contained adapter records only ConfigCliParse with no invented span or inner
message. It does not claim first-byte failure timing: toml_edit parser/mod.rs:17–35
performs a token/event pass before key decoding, and error.rs:209 preserves the
first error, which may instead be a structural error later in the string. The
absolute leading slash excludes BOM, whitespace, quoted-key and comment starts.
This is a stage-result derivation for this closed argument domain, not a copied
general parser or a claim that upstream parser heap work was executed.

## Local failures, accounting and configured state

Absolute NUL-containing argument spellings make Path::exists locally false, then
reach the same parse-stage rejection. The adapter scans its admitted joined
bytes and never sends NUL through observed::Path or substitutes an encoding
error/native errno for this source behavior. Missing metadata evidence and all
resource refusals remain terminal before inline fallback. A present directory
enters file loading; its read error stays a read error, not a parse-stage result.
File configuration may contain registry credentials because those security
checks belong only to successfully parsed inline arguments.

All actual joins, string scans, table/merge/include storage and retained
diagnostics use the same original Session. The implementation does not invent
charges for parser work it does not execute, refund previous ownership, or add
an account/cap. Existing Files/Tree machinery preserves first refusal and scopes.
The composed loader accepts already-initialized normal values, avoiding repeated
ordinary discovery. Such supplied values must be the existing normal BuiltIn
root; the helper does not model arbitrary prior GlobalContext state. Otherwise
it performs normal traversal only after the full CLI table has succeeded.

Six focused tests are supplied for root execution: ordered CLI/include origins
and overrides followed by normal merge; shared seen across arguments despite
fresh second include passes; missing/native-error/NUL parse-stage failure before
normal queries; any-kind directory read failure and file credential semantics;
initialized normal/empty argv; and original resource/path-work floors plus latch.
No native command, new parser dependency, current authority, installation or
release qualification is supplied by this slice.
