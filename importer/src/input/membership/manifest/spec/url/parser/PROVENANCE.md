# Complete pinned absolute URL parser

This module specializes `url` 2.5.8's `Url::parse` with no base URL,
`Context::UrlParser`, no query encoding override and no violation callback.
It also contains only the reached `set_query(None)` and `set_fragment(None)`
mutations. This produces typed scalar data, not package, host-lookup, filesystem,
workspace or current-input authority. This controls complete interpretation;
the later compound-source reparse handling belongs to the caller.

## Source and licenses

Upstream: <https://github.com/servo/rust-url/tree/v2.5.8>.
Registry archive `url-2.5.8.crate` SHA-256:
`ff67a8a4397373c3ef660812acab3268222035010ab8680ec4215f38ba3d0eed`.
The exact cached archive checksum and byte equality of the following cached
source members against that archive were verified during extraction:

| Member | SHA-256 |
|---|---|
| `src/parser.rs` | `2dd4210e1773fc5d3fdc15ce506ecff30dbf8eb358a79bbbc2835cc30d513c3a` |
| `src/lib.rs` | `51f47a93654528f3b8cb92c94fc26ad25775860d84197fff0c3124a687f8c5e3` |
| `src/host.rs` | `24bba45b14cb843ea0fe5c857284d458097a13a84847f5c6a15a14c9e1334335` |
| `tests/urltestdata.json` | `58d67bea710d5f46324fe6841df5fd82090fe4ec2d882bc0fc7c1784d4771884` |

The source is Copyright 2013–2016 The rust-url developers, dual MIT/Apache-2.0.
The exact unchanged license texts are retained in `input/LICENSE-MIT` and
`input/LICENSE-APACHE`. Original `tests/urltestdata.json` attributes its vectors
to web-platform-tests commit `befe66343e5f21dc464c8c772c6d20695936714f`.
`tests/inputs.json` retains all 819 object-row input strings, in source order,
without the base or expected-output fields; JSON escaping changes only the
file spelling. Its SHA-256 is
`4c4e9ae7987308ff91214e29d67a61ff3f4d7b1c747d8da689f1ead7d46ffd07`.

## Reached correspondence

| Contained code | Exact source |
|---|---|
| `parser.rs` absolute scheme dispatch | `parser.rs:379–510` |
| `file.rs` every no-base file branch | `parser.rs:512–722,1057–1112` |
| `authority.rs` credentials, host framing and ports | `parser.rs:844–1044,1082–1148,1817–1836` |
| `path.rs` bulk segments, dot/drive handling and opaque paths | `parser.rs:1150–1444,1770–1797` |
| `parser.rs::finish` leading empty-segment disambiguation | `parser.rs:1445–1512` |
| `tail.rs` query and fragment | `parser.rs:1515–1623,1645–1695` |
| `value.rs` getters, removal, opaque trailing spaces and order | `lib.rs:388–411,1028–1030,1442–1445,1582–1597,1637–1667,2840+` |

All special schemes (`file`, `ftp`, `http`, `https`, `ws`, `wss`) and the complete
other-scheme branch remain. Other schemes retain both authority/hierarchical
and opaque paths. File parsing preserves `localhost`, drive letters on all
platforms, host removal, slash/backslash behavior and dot reductions. Username,
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
819 WPT input spellings **interpreted without a base** (not the original WPT base
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
