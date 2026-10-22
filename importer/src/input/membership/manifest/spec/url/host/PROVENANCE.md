# Complete pinned Host composition

This boundary composes the reviewed fixed IP scalars and complete contained
IDNA entry for a borrowed `&str`. It returns a Domain owner or a fixed typed IP
address. It does not select a scheme, preprocess whole URLs, resolve addresses,
or establish package/source/owner authority.

The governing source is url 2.5.8 `src/host.rs:92–172`, with public reference
entries at `host.rs:77–88`. Archive SHA-256:
`ff67a8a4397373c3ef660812acab3268222035010ab8680ec4215f38ba3d0eed`.
Host source SHA-256:
`24bba45b14cb843ea0fe5c857284d458097a13a84847f5c6a15a14c9e1334335`.
The parent `spec/URL_CLOSURE.md` records the verified archive/member provenance.

Percent operations follow percent-encoding 2.3.2, archive SHA-256
`9b4f627cb1b25917193a259e49bdad08f671f8d9708acfd5fe0a8c1455d87220`:
`src/lib.rs:136–172,192–208,240–309` and
`src/ascii_set.rs:35–55,102–114`. The lib.rs SHA-256 is
`641903d385581e76f31ef8c991d83c2a5d7fbbc2879d93909c539037e4e1e3e2`.
The source archive and this member were verified in the parent closure record.
Both source crates use the same retained MIT/Apache-2.0 licenses and original
rust-url copyright notices.

## Selected program and ordering

Both entry methods first recognize a leading `[`. A missing final `]` rejects
as IPv6; a matching bracket pair passes its exact interior directly to the IP
parser. Percent decoding and IDNA cannot reinterpret this branch. Percent-
encoded brackets in a special host reach the different IDNA branch instead.

Special hosts decode every valid `%HH` into raw bytes, preserving invalid and
incomplete escape spelling. Plus has no form-decoding meaning. Complete IDNA
processing follows, using its exact URL deny list, allowed hyphens and ignored
DNS-length setting. Only successful complete output is checked for EmptyHost;
this includes text that becomes empty through ignored-character mapping.
Ends-in-number routing then selects the complete URL IPv4 grammar or retains
the Domain. No decoded UTF-8 prefix enters domain construction or IPv4 dispatch
before the full IDNA result. Invalid UTF-8 is represented by valid percent-
encoded input in tests, since the raw Host entry accepts a Rust `&str`.

Opaque hosts perform the entire pinned forbidden-character check before any
encoding storage. The forbidden set is NUL, tab/newline/CR, space, `# / : < > ?
@ [ \\ ] ^ |`. All are ASCII, so testing UTF-8 bytes is equivalent to the
source character predicate. The separate CONTROLS operation percent-encodes
C0, DEL and every non-ASCII UTF-8 byte, using uppercase hex. Existing percent
escapes and plus remain literal; case is preserved; no IDNA or IPv4 routing
occurs. Empty opaque hosts are accepted.

## Ownership and account composition

Each entry admits its raw string extent under the caller's original Session.
The percent iterator borrows bytes and has fixed local state. One bounded pass
determines the complete decoded or encoded length. Unchanged input returns its
exact original borrow; changed input is written through prospective Buffer
item/full geometric replacement-layout admissions after its output string
extent is admitted. No standard Cow collection, owned string formatter, clone,
shrink, or allocator-bonus growth is used.

IDNA's consuming `Ascii::into_bytes` seam transfers its existing
`Bytes::{Borrowed,Owned}` output. For borrowed raw input, that output can be
retained directly. When percent decoding owns the input, an owned IDNA result
moves directly into the host, dropping the prior decoded storage without
refunding charges. A borrowed IDNA result is checked to be the same complete
slice, including length and pointer, as the decoded buffer. That borrow then
ends and the exact decoded owner moves into the host. This path has no second
copy, reallocation, self-reference or manufactured Ascii proof. All current
borrowed IDNA output exits retain the complete original input slice.

An opaque single encoded control must also own storage: the pinned percent
encoder may borrow its static `%HH` table for one output chunk, but Host compares
it with the input and makes that transformed result owned. The contained path
likewise owns all changed encodings and borrows all unchanged inputs.

A retained Domain host or newly selected host error admits one logical item.
IP results and IDNA errors transfer their already admitted scalar/error item;
wrapping their discriminant does not fabricate another allocation or duplicate
error charge. Failure during any stage remains terminal in the same Session;
semantic errors remain distinct. No graph observation, hash or candidate-
comparison account is used by these string operations.

Host's own percent/forbidden scans and copies take a constant number of passes
over admitted input/output. Decode never expands; opaque encoding expands by at
most three bytes per UTF-8 byte, with checked length arithmetic. IP and IDNA
retain their separate reviewed work arguments. Owners and decoded temporaries
have flat teardown; dropping a failed trial never resets the original account.
The output exposes only borrowed read access to domain bytes or parsed IP data.

## Focused comparisons and refusal tests

Thirteen tests are prepared for root execution. They compare complete Domain,
IP payload, canonical spelling and semantic error category against exact
dev-only url 2.5.8 `Host::parse` and `Host::parse_opaque`, including each raw ASCII
byte and all 256 percent-decoded byte values. The corpus includes bracket
precedence, empty/ignored input, numeric routing, alternate IPv4 radices,
embedded IPv4, malformed escapes, invalid UTF-8, Unicode/ACE names, bidi/context
cases, opaque encoding and whole-URL preprocessing that is absent at Host.

The public upstream reference returns owned String domains. Separate tests
therefore check borrowing at the contained raw-borrowed entry, including the
exact original pointer. The actual decoded-owner/IDNA seam is also tested for
pointer-preserving transfer without new allocation and for owned changed IDNA
output that survives destruction of its decoded temporary.

Resource cases refuse every recorded admission after nonzero prior usage and
compare actual Buffer allocations with the admitted prefix. Additional tests
exercise aggregate item/byte and raw/expanded string floors, repeated stages
under one original Allocation, terminal refusal across later trials, and
exhausted unrelated graph accounts. Expected tests are not themselves execution
evidence; root owns execution, source review, formatting and commits.
