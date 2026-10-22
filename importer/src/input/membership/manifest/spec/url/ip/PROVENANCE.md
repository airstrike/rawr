# Pinned IP scalar correspondence

These private transformations implement the IP portion of url 2.5.8. They do
not parse a whole host or URL, normalize domain names, resolve addresses, or
establish package/source/owner authority. The caller supplies an already
admitted native string and the same original Session.

Source: registry crate `url-2.5.8`, archive SHA-256
`ff67a8a4397373c3ef660812acab3268222035010ab8680ec4215f38ba3d0eed`.
`src/host.rs` SHA-256:
`24bba45b14cb843ea0fe5c857284d458097a13a84847f5c6a15a14c9e1334335`.
The verified archive/member provenance is recorded in the parent
`spec/URL_CLOSURE.md`. The source is available locally at
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/url-2.5.8/src/host.rs`.

| Contained operation | Exact pinned source |
|---|---|
| `v4::candidate` | `host.rs:276–294`, ends-in-a-number dispatch |
| IPv4 radix and syntax/overflow classification | `host.rs:299–329` |
| IPv4 pieces, width and range checks | `host.rs:333–361` |
| IPv6 parser, including embedded decimal IPv4 | `host.rs:365–512` |
| IPv6 output and first-longest compression | `host.rs:214–272` |
| IPv4 decimal output dispatch | `host.rs:187–197`, standard IPv4 display |

The source is dual MIT/Apache-2.0. Both complete licenses are retained beside
the contained code and the test extraction, with original copyright notices.

## Entry and semantic order

`v4::candidate` is only the pinned routing decision after special-host
normalization. A numeric candidate can still be an invalid address. It removes
at most one final empty label and distinguishes syntactic validity from u32
overflow; an all-decimal last label is numeric even if octal interpretation
will fail. The number accumulator continues syntax checks after overflow, so
a long valid hexadecimal number remains numeric while a later invalid digit
makes the same spelling nonnumeric.

IPv4 removes exactly one trailing empty part, checks complete cardinality
before numeric conversion, converts parts in encounter order, then checks the
last part's available width before the earlier octet limits. At most four
parts reach the fixed four-number array. All scalar arithmetic is checked or
bounded by these preceding range checks. The empty string is an explicit
unreachable-source precondition: Host rejects an empty normalized domain at
`host.rs:111–113`, before candidate dispatch. The upstream private IPv4 function
would panic when called directly on that input. The contained scalar returns
`Invalid::V4`; tests do not invoke the unreachable upstream panic.

IPv6 receives only the interior of brackets; host owns bracket recognition.
Its eight-piece array follows the pinned parser's pointer/compression state.
The embedded IPv4 tail has its own decimal grammar: exactly four decimal
pieces, no leading zeros, no radix prefixes, and each piece at most 255. It is
not delegated to the different URL IPv4 grammar. Four hex digits fit u16;
decimal continuation rejects above 255 before another continuation can occur;
each pair of decimal pieces fits one u16. Compression inserts only the pinned
zero gap and preserves all eight final segments. No input length cap replaces
these grammar checks.

IPv6 serialization emits lowercase hex, uses the first longest zero run, and
does not compress one zero. Embedded IPv4 is serialized as hex segments. The
text is deliberately unbracketed; host owns the literal brackets. IPv4 emits
four canonical decimal octets. Neither path calls a standard IP parser,
allocating formatter, String, Vec, or external output sink.

## Ownership and work

Every entry checks the original Session through its input string admission.
Semantic results and semantic errors each retain one admitted logical item.
These errors remain distinct from terminal Session refusal, so the containing
URL trial can discard only semantic invalidity. No failure clears/refunds prior
usage. Candidate dispatch produces no owned result slot and admits no heap.

Parsing uses fixed primitive local arrays. A four-number IPv4 array follows
the completed cardinality check; eight IPv6 pieces follow the address grammar.
IPv4 counting and conversion make a fixed number of linear passes over
admitted input. Candidate inspects the suffix in a fixed number of passes.
IPv6 advances one byte cursor; its decimal branch revisits at most four bytes
of the current hex candidate. Numeric arithmetic has fixed width, including
the latched-overflow IPv4 accumulator. There is no recursion or collection.

The opaque Text stores at most 39 inline bytes: eight four-digit hex segments
and seven colons. IPv4 needs at most 15. Two fixed passes count then emit digits;
the original string limit and result item are admitted before writing the
retained output. A preexisting terminal refusal prevents either pass. No
allocation request is fabricated for stack arrays, copies or inline results.
When a caller retains results in a Buffer, its full prospective container
layouts remain the caller's responsibility under that same Session; allocator
bonus cannot extend the admitted container capacity. There are no GraphBudget
filesystem, hash or candidate-comparison events in this scalar component.

## Direct pinned test reference

`reference.rs` contains the contiguous, unchanged `host.rs:214–512` region,
starting immediately after its `BEGIN UNCHANGED UPSTREAM SOURCE` marker. All
imports, the two-variant ParseError shim, wrappers and Display bridge precede
that marker. The bridge calls the exact private writer. No percent decoding,
IDNA processing or public Host preprocessing substitutes for these functions.

The shim uses std::net::{Ipv4Addr,Ipv6Addr}, matching url's std cfg at
`src/lib.rs:198–203`; `Cargo.toml:54–67` enables std by default. Without std,
the same pinned source selects core::net at `lib.rs:204–207`. The shim retains
the source's address construction, octet/segment access and standard IPv4
Display; it does not run a network operation. The reference's original Vec and
String operations are confined to cfg(test), outside production accounting.
Root independently verifies the extracted bytes against the pinned archive
and owns execution; this note does not claim a new reference run.

Nine focused tests are prepared: direct candidate/IPv4/IPv6 comparisons,
radix/width/empty/trailing/overflow/error boundaries, embedded-tail variations,
all 256 zero-run masks and serialization ties, semantic fallback availability,
each admission refusal with prior usage, exact aggregate/string/container
floors, terminal output refusal and exhausted unrelated GraphBudget accounts.
Expected-output tests are not execution evidence until root runs them.
