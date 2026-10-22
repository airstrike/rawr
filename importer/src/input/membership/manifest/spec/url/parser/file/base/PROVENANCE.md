# Registry native-file-base second parse

Source: URL 2.5.8 `parser.rs::parse_with_scheme` (435–477),
`parse_file` (512–713), `starts_with_windows_drive_letter_segment` (1799),
`parse_path` and `shorten_path`; Unix `Url::from_file_path` uses the existing
contained native path constructor. Caller: pinned Cargo
`59800466c5c41c444d264b1010b4d57e85a7117f`
`util/context/mod.rs::resolve_registry_index` and `util/into_url.rs`.

`Accepted` binds the unchanged original input to its successful no-base URL
parse. The borrowing second operation first constructs a native file base, then
parses that exact original again while the first parsed URL remains alive through
the caller's subsequent password check.
There is no API taking unrelated original text plus an arbitrary successful URL,
no API taking a reparsed base serialization, and no generic relative-URL API.
The first no-base failure remains the existing `Invalid` for the enclosing
IntoUrl diagnostic (including the git@ contextual suggestion). Native path
failure and second-parse URL failure remain distinct for enclosing Cargo context.

Successful first parsing proves an explicit scheme. With a native file base,
non-file explicit schemes cannot take the same-special-scheme relative branch.
They therefore use unchanged ordinary parsing. File double-slash authority
processing likewise reuses the unchanged no-base branch. Only the reached file
single-slash/no-slash branches inherit native base data. Native Unix file bases
always have empty host and no query or fragment; their first slash is skipped
before the first path segment is tested for a normalized drive. `/C:/...` bases
retain pinned drive inheritance even on Unix. Filtered drive lookahead, native
parent components, empty/query/fragment copies, path shortening, and explicit
drive replacement preserve source behavior without reparsing the native path.

Parser no-base callers retain their existing entry and default behavior. The
only shared path change makes the existing shorten helper visible to the new
sibling caller. All parsing, copying and construction uses the original Session;
there is no new allocation account, reset, cap, or ambient filesystem query.

Differential tests compare the unchanged pinned URL dependency's native base
constructor plus options/base_url parser against this closure, including drive
inheritance, tabs/newlines, empty and query/fragment input, dot components,
non-file schemes, first failures, native non-UTF-8 paths, and construction failure
before non-file parsing. Every reached admission is refused in turn with prior
account history and terminal refusal checks. These are primitive tests, not
completed registry source selection or product qualification.

All 36 parser tests (including four base cases and password getter differential
coverage), seven URL adapter tests, and the connected registry-resolution fixture
passed on nightly-2026-07-11 with offline rawr-import library features
rustc-driver,test-fixtures. Independent reviews found no parser-result blocker;
the first-URL lifetime correction identified by review was applied and retested.
Caller review separately covered lookup order, original definition/root,
error context and password rejection. No production/Gallery claim follows.
