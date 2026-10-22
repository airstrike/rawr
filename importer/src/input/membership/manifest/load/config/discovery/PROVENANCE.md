# Recorded configuration filename selection and file loading

Pinned Cargo 59800466c5c41c444d264b1010b4d57e85a7117f
`src/cargo/util/context/mod.rs:1644–1689` constructs both joined spellings before
get_file_path attempts acquisition. The contained selector does the same through
the existing admitted native `load::path::join`. It returns ownership of exactly
the selected spelling without normalization or a second output allocation.

Session::discovery transports the already checked closed observation through the
existing original Host account and first-failure latch. Directory bytes, filename
family and warn mode are part of the exact query. No metadata/file observation
can substitute for a missing discovery transcript. Acquisition outcomes and
device/inode comparison have already established Selection in the checked
transcript; the selector does not duplicate their branch algorithm. The
physical warning projection applies: no renderer, warning-success receipt or
hypothetical full Cargo success is supplied.

## Local native-conversion failure and query domain

The fixed suffixes contain no NUL and native join does not normalize away any
directory byte. A directory NUL therefore makes source legacy open conversion
fail InvalidInput and the subsequently reached fallback exists conversion fail
InvalidInput, selecting None without a filesystem operation. The selector
derives that result locally after both admitted source joins; it never sends NUL
through observed::Path and relabels its encoding error as Cargo behavior. This
also holds for relative NUL-containing directories because no native CWD lookup
is reached. Original refusal during either join still precedes this result.

The derivation scans only admitted directory bytes after Session's String check.
Its pure work is bounded by those bytes, with no invented native-path debit.
It does not execute or claim the source std small_c_string conversion allocation
strategy (stack copying below 384 bytes, CString construction above it). The
actual adapter owns and charges both joined byte buffers, then performs its
bounded NUL scan; it neither allocates a native conversion buffer nor claims that
one was allocated. Original retained/replayed native acquisition is separate.

NUL-free relative directories expose an existing observation-domain gap. They
produce a latched Host(Unobserved), not None, a Cargo semantic rejection, invented
errno or unsupported-source policy. No implicit CWD anchoring is introduced.

| | **A — keep incomplete replay explicit** | B — add relative/CWD observation semantics |
|---|---|---|
| Scope | **Uses existing checked absolute observation domain** | Requires additional native context binding |
| Caller connection | **Must establish governed absolute directory inputs** | Must prove relative resolution and returned spelling |
| Source support | **No successful proof for unresolved inputs** | Could support broader callers after separate implementation |

Recommend A for this bounded connection. Root's governed caller-domain tracing
remains an integration condition; this helper alone does not prove every reached
directory absolute or authorize narrowing supported Cargo input shapes.

## Selected file boundary

Files::load_discovered composes one physical selection with existing complete
file/include loading under the same Session. None does not read a body. Some
loads only the selected spelling and its reached includes, preserving file
origins, include order and first read/parse/merge failure. The caller supplies
the include mode and still scopes a fresh visited set wherever pinned Cargo
does; repeated calls do not silently reset Files. No walk_tree, ancestor stop,
home suppression, canonicalization, credential merging or current authority is
implemented by this composition.

Six focused tests are supplied for root execution: exact selected native
spelling/family/mode; local NUL including second-join refusal; missing/relative
query gaps and first-failure latching; original account/path-work floors;
selected body plus include loading with no unselected-body observation; and
absence versus first selected-file read failure. The integration fixture checks
both legacy and .toml selection, child-before-parent override and exact origins.
Producer, codec and native matrices remain covered by their existing tests.
