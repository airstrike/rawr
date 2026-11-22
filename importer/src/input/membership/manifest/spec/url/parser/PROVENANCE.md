# Complete pinned absolute URL parser

This module specializes `url` 2.5.9's `Url::parse` with no base URL,
`Context::UrlParser`, no query encoding override and no violation callback.
It also contains only the reached `set_query(None)` and `set_fragment(None)`
mutations. This produces typed scalar data, not package, host-lookup, filesystem,
workspace or current-input authority. This controls complete interpretation;
the later compound-source reparse handling belongs to the caller.

## Source and licenses

Upstream: <https://github.com/servo/rust-url/tree/v2.5.9>.
Registry archive `url-2.5.9.crate` SHA-256:
`f00cd2407bb42c0ef17be88d4cf020f5cceffd121665bac9b8b9493a369b3b1a`.
The exact cached archive checksum and byte equality of the following cached
source members against that archive were verified during extraction:

| Member | SHA-256 |
|---|---|
| `src/parser.rs` | `9f868941a6291c323bf8020db4a733b0b460397b693164cb7da863e69ecfc69e` |
| `src/lib.rs` | `208c0d522e74e04bafe1107b58acd2c932095b013a86de60e9126f15cb73f0c8` |
| `src/host.rs` | `24bba45b14cb843ea0fe5c857284d458097a13a84847f5c6a15a14c9e1334335` |
| `tests/urltestdata.json` | `d2eef2efe1d230f86456b2910ddf268d755dcec4ee491efc1ecd7cf742d1499e` |

The source is Copyright 2013–2016 The rust-url developers, dual MIT/Apache-2.0.
The exact unchanged license texts are retained in `input/LICENSE-MIT` and
`input/LICENSE-APACHE`. Original `tests/urltestdata.json` attributes its vectors
to web-platform-tests commit `876fe001f2279b96e7c6b9e8970dc92c484835ff`.
`tests/inputs.json` retains all 869 object-row input strings, in source order,
without the base or expected-output fields; JSON escaping changes only the
file spelling. Its SHA-256 is
`f5d7411d1bc612e137d26482c978fa9ceed65a5209f270750e263d9e947f7f2e`.

## Reached correspondence

| Contained code | Exact source |
|---|---|
| `parser.rs` absolute scheme dispatch | `parser.rs:374–499` |
| `file.rs` every no-base file branch | `parser.rs:501–702,1037–1092` |
| `authority.rs` credentials, host framing and ports | `parser.rs:824–1024,1062–1131,1807–1826` |
| `path.rs` bulk segments, dot/drive handling and opaque paths | `parser.rs:1133–1434,1760–1787` |
| `parser.rs::finish` leading empty-segment disambiguation | `parser.rs:1435–1502` |
| `tail.rs` query and fragment | `parser.rs:1505–1613,1635–1685` |
| `value.rs` getters, removal, opaque trailing spaces and order | `lib.rs:387–410,1027–1029,1441–1444,1581–1596,1636–1666,2839+` |

All special schemes (`file`, `ftp`, `http`, `https`, `ws`, `wss`) and the complete
other-scheme branch remain. A backslash ends a port only in a special
scheme with a default port, as url 2.5.9 parses it. Other schemes retain both authority/hierarchical
and opaque paths. File parsing preserves `localhost`, drive letters on all
platforms, a host kept before a drive letter, slash/backslash behavior and dot reductions. Username,
host and path offset checks remain even when the contained caller does not
retain those fields. Port conversion checks the same `u16` boundary and renders
through a fixed five-byte local array with checked UTF-8 instead of upstream's
unsafe conversion. Query/fragment scanning keeps raw bulk slices, ignored
tab/LF/CR positions, percent-encode sets and absent/empty components. NUL in a
fragment is encoded: the upstream violation name does not mean omission.

Base-relative parsing, setter contexts, violation callback bodies, encoding
overrides, and the already-prefixed base-path `:/.` reversal branch are
unreachable with this entry configuration. Their omission does not filter
accepted absolute input. The scheme-less entry returns the pinned relative-URL
error. Impossible cursor/UTF-8 states latch the existing internal failure.

## Ownership and failure

`Input::new` admits the entire original source before trimming. All serialization
and filtered-host temporary ownership uses the existing admitted `Output`/
`storage::Buffer` under the same `Session`; full replacement layouts and scalar
expansion are admitted prospectively. Host interpretation uses the existing
complete contained special/opaque, IDNA, normalizer and IP components; no stock
allocating URL/IDNA parser runs in production. Its fixed semantic errors remain
distinct from terminal admission/allocation refusal. Successful URLs and new
parser semantic errors retain an admitted logical item. Existing host/offset
errors are propagated without constructing another payload.

File leading-slash normalization uses in-place moves instead of split-off and
copy. Query removal also moves the already-owned fragment in place, adjusts its
offset, and strips opaque trailing spaces only when both query and fragment are
absent. These transformations retain exactly the pinned serialization with no
temporary string ownership. Removals and all borrowed getters check the original
failure latch first. Eq/Ord compare retained normalized bytes without allocating
or making new admission calls. No implicit clone, unsafe code, filesystem,
process, environment, graph-comparison, hash or graph-decode event is introduced.

## Focused checks

Nine new tests compare actual contained parsing, all exposed components and both
removal orders against the actual pinned dev-only `url::Url`. They include all
869 WPT input spellings **interpreted without a base** (not the original WPT base
expectations), 1,215 ASCII/Unicode state cases, every scheme family, credentials,
ports, IPv4/IPv6, percent escapes, filtered hosts, file drives and dot segments.
Separate tests cover normalized Eq/Ord, semantic retry, every recorded refusal,
nonzero original-account exact/one-under item/layout/string floors, and blocked
getters/mutations after refusal. This is a focused extraction suite, not a claim
that all upstream tests, complete manifest differential parity, loading or
current-source validation passed. Root owns compilation, execution and scoped
formatting; formatting changes only Rust whitespace/import layout.

This interpreter remains behind the existing test-only membership module.
Neither production routing nor the full T06 package-membership witness is
activated by this parser boundary.
