# Private canonicalization foundation — scoped qualification

2026-09-07 update: the installed arm64 absolute-input, non-NULL caller-buffer
route and original-account inert producer have passed joined independent review.
Retained loop, helper, kernel-field, exact attribute-set and review evidence is
in the MAP's `canonical-live` evidence directory. The earlier
investigation below records the initial obligations. The joined review closes
those memory/ownership/state obligations for that scoped route by covering both
state-selected branches, without assuming completed global initialization.
Relative routes, other images/architectures and arbitrary loader overrides are
not qualified by this result. Complete source association, exact caller-context
binding and candidate selected-image correspondence remain required before
current Path/Git admission. No Gallery performance result follows.

This is a separate private boundary. It is not connected to importer
reuse, and it grants no production activation or release qualification. The only
unsafe code is the foreign declaration and call in `src/sys.rs`. The safe API
borrows a CStr and initialized caller-owned 1024-byte transfer array, returning
a CStr that borrows only that array. The crate is not a general platform service.

## Contract and framing

The declaration uses C pointer arguments and pointer return, with Rust's target
`c_char`, and links exactly `realpath$DARWIN_EXTSN` on 64-bit macOS. Pinned
nightly-2026-07-11 std selects that variant through libc 0.2.185. Active Xcode SDK
`sys/syslimits.h` defines PATH_MAX as 1024. Other targets return Unsupported
without invoking std or another canonicalizer. Other macOS architectures have
not been exercised by this implementation work.

The input is live and terminated by the CStr contract. Rust borrowing ensures
the mutable transfer array cannot overlap the borrowed input. Native code is
required to retain neither pointer and to respect the PATH_MAX writable extent.
The array is initialized before the call, so bounded framing never reads Rust
uninitialized bytes, including partial or stale contents. Native failure is
detected by NULL and errno captured immediately with `last_os_error`; output is
then ignored completely. Errors are not retried. Native success must return the
original array base. No returned pointer is dereferenced or converted to an
unbounded CStr: identity is compared, then the caller's known array is scanned
with `CStr::from_bytes_until_nul`. A different pointer or missing bounded NUL
is a distinct Framing error and must never select native-error fallback. No
native buffer is freed: ownership always remains with the caller.

The successful native contract, rather than framing alone, establishes that the
prefix was written by this call and denotes its canonical result. An initialized
old NUL cannot independently prove that property. Pointer and bounds checks do
not make an incorrect foreign ABI or out-of-bounds native write safe.

## Allocation, work and concurrency gates

This Rust adapter allocates no path, C string, collection or retained output.
It does not call realpath with NULL output, convert to PathBuf, or copy the result.
The caller must prospectively admit input conversion, transfer ownership, native
operation work and any eventual retained-output allocation/copy through its
original cumulative account. This low-level API has no account, reset, allowance
or admission bypass of its own. PATH_MAX specifies native output storage; it
does not impose a new input length limit. The native function receives every
CStr, including relative and long paths, and decides the native outcome.

The retained Apple Libc source at
71bbe350ab79eef58113991d817ccc6165061a64, `stdlib/FreeBSD/realpath.c`, separates
caller-buffer mode from its top-level malloc branch and has fixed scratch.
That is not proof of the complete installed call graph's allocation or work.
Relative paths can reach private_getcwd. Any eventual absolute-input invariant
must be proved for every reached caller without excluding supported shapes.
Fixed stack scratch, actual user-space heap ownership and kernel work remain
distinct, and variable transitive allocations are not exempt from accounting.

The root's preliminary installed disassembly identifies libc image UUID
66EBD321-6899-3863-BA24-5CFC3076A0CB and variant offset 0x128A0, with non-NULL
output skipping the top-level malloc. Full loaded-symbol correspondence and
transitive analysis remain the root's separate qualification work. That audit
also observes initialization of rootdev_inited before stat and rootdev storage,
without visible acquire/release synchronization. This adapter adds no claim of
statelessness, thread safety, serialization or safe process-wide initialization.
Library concurrency/state qualification is mandatory before activation; serial
tests or a crate-local lock would not prove safety against external std callers.

## Focused checks and limits

One serial native
fixture compares native byte results or exact OS error codes with pinned std for
absolute root, relative dot without CWD mutation, directories, dot-dot traversal,
multibyte filename, differing-case lookup, symlinks,
missing paths, dangling symlink, symlink loop, non-directory traversal, invalid
UTF-8 missing-name bytes, long input and empty input. The fixture also attempts
an existing invalid-UTF8 filename. If creation succeeds, successful canonicalization
is required and compared; the previously observed EPERM (1) refusal is explicitly
printed as unexercised success coverage. Every other creation error fails the
test. Differing-case outcomes are compared without assuming filesystem case
sensitivity. These fixtures do not certify general case/mount behavior or prove concurrency. Both
sides observe a test-owned quiescent fixture, not an atomic filesystem snapshot.
Pure framing tests use valid pointers only and never dereference the differing
pointer; they cover pointer mismatch before termination, missing NUL, final-byte
NUL and stale tail exclusion. A compile-fail doctest checks that output prevents
mutating its buffer, and the native fixture checks that output outlives input.

The development fixture `tests::cwd::relative_cwd_lifecycle_matches_pinned_std`
passed on 2026-09-08 with nightly-2026-07-11. An isolated test child owns cwd
mutation and an exclusive temporary tree. It compares exact result bytes/native
errno for relative symlinks, dot-dot traversal, a root-resolving JSON symlink,
missing targets, renamed/replaced cwd, and unlinked cwd. After replacement,
relative lookup resolves inside the moved original directory. Child selection
is checked so an empty test run cannot count as success. This supplies concrete
lifecycle parity evidence only: it does not qualify `__private_getcwd` transitive
allocations/state, concurrent mutation, or bind an importer observation to the
actual Cargo producer. Existing absolute observation guards remain unchanged.

Required before activation: root-run focused tests/format/build checks, complete
installed ABI/semantics/transitive-work/state qualification, original-account
integration proof, and independent safety review. Discovery ordering,
warning classification, exact before/cold-success/after/current association and
old-record clean misses remain separate gates.
