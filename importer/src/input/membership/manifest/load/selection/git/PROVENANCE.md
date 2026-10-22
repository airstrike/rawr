# Pinned Git discovery paths

This contains native paths, path-file consumers, repository recognition, trust
and default upward search from Cargo's reached Git discovery. They use
the existing closed observations and caller-retained whitespace helper;
they perform no ambient filesystem, environment, identity or configuration operation.
Current-directory occurrences require an explicit caller; native acquisition and
command association remain unconnected. These functions confer no membership
authority and do not replace repository opening or its later current-dir read.

Cargo59800466c5c41c444d264b1010b4d57e85a7117f pins gix-path0.12.1 and
gix-discover0.53.0. Their restored archives were verified against that Cargo.lock:

| Archive | SHA-256 |
|---|---|
| gix-path0.12.1 | afa6ac14cd14939ea94a496ce7460daa6511c09f5b84757e9cfc6f9c8d0f93a6 |
| gix-discover0.53.0 | d624d5b23b10c1d85337645227abe353ac95ab8ff66a7bdd5ce689b2db33a722 |

Inspected source files and exact member hashes:

| Source | SHA-256 |
|---|---|
| gix-path/src/convert.rs | a6c4c6d83c39b82be5463ade1fdb061b00fa90f4c843d57d436bbe953a141012 |
| gix-discover/src/repository.rs | dac79ba89d3c514029dec3eaaac0cfbb0de16fc549ce7b46f995ed43b983e19c |
| gix-discover/src/path.rs | ab0d869d93ce4dbe90dff3001c595f9bb528fdbab9f2a7f159b8b1c561f06b28 |
| gix-discover/src/is.rs | fc32ee101c9a5cc8317db8f6bd4a26351d2240eba7e44ab17adbbeaedcd89f1e |
| gix-discover/src/upwards/mod.rs | e2fd668eb5b348498e75c85317f6f03e9c13e9feb3f579cf19662c493c5b3109 |
| gix-discover/src/upwards/types.rs | 2c4fc74deb088bdf72b9442feb680c6b7f92c40dfac88f9ad2b8a0a08634ee8b |
| gix-sec0.14.1/src/identity.rs | 5eca836f251f3b37275004d0d753ce1f07faadaf91e6f15c2c4fc237866aae16 |
| gix-sec0.14.1/src/trust.rs | d67aa969b29e6e904575eb5751f39896e3bba06fd4196bbba809950521907dcd |

`path::normalize` retains convert.rs:262–297 control flow, including the original
Cow when no parent occurs, the one-use current-directory fallback, root underflow,
and borrowed-dot result. It cannot use Cargo's different lexical normalization.
Native Unix Path components/parent retain non-UTF8, NUL and Windows-looking bytes
as data. The admitted Git cursor implements Unix PathBuf push with minimum-eight
byte growth and separate separator/suffix pushes; truncation retains requested
capacity, full replacements are prospectively charged, and final Vec→OsString→
PathBuf transfers do not allocate. The original input owner remains alive while
its components are consumed. No allocator bonus capacity authorizes growth.

`repository::Path` retains repository.rs:53–113 field evaluation order, conditional
trailing-parent normalization, path moves, borrowed-result ownership, kind cloning
and the ordinary-worktree join. `repository_kind` and `without_dot_git_dir` retain
path.rs:75–100,151–157, including the deliberately unused result of the reverse
`.git` search. They are lexical classifications, not native observations.

All work uses the caller's original Session. Semantic `None` is separate from
terminal refusal; partial owned results drop without further allocation or a new
account. No ambient gix API, native helper, new dependency or production activation
is introduced. Native current-directory acquisition, repository opening,
configuration and actual modeled-process/history association remain separate
connections.

## Default upward search and ownership trust

`upwards::discover` follows gix-discover `upwards/mod.rs:42–199` with the default
options in `types.rs:44–47,89–99`: Required(Reduced), empty ceilings, cross_fs=false,
dot_git_only=false and no configured current_dir. Cargo invokes plain
ThreadSafeRepository::discover; the environment-options and general ceiling
realpath branches are not reached. The `Root::absolute` smart constructor checks
only the input spelling; followed metadata still establishes directory type in
the source order, after the initial owned cwd occurrence.

The absolute domain is a positive actual-caller proof, not a restriction on all
gix callers. Rawr PackagePath and PackageGit both run Cargo's package subcommand.
At Cargo59800466c5c41c444d264b1010b4d57e85a7117f, `commands/package.rs:60–80` reaches
`command_prelude.rs:632–639` and `Workspace::new`. `core/workspace.rs:228–235`
rejects relative initial manifests and `:249–256` starts an empty package map.
Each insertion at `:1878–1888` calls `SourceId::for_manifest_path` before
`Package::new`; `core/source_id.rs:201–214` → `util/into_url.rs:30–34` → pinned
url2.5.8 `lib.rs:2958–2960` rejects a non-absolute parent. `core/package.rs:103–108`
stores the unchanged manifest path and `:137–138` returns that parent as root.
`ops/cargo_package/mod.rs:211–217,272,295–303` selects, clones and sorts those map
entries; `:475–488` passes the original package to list_files. Loading the path
source does not replace it. Exact Cargo source files are retained under
`/private/tmp/rawr-gix-pinned-sep26/cargo-root-invariant`.

Absolute normalization and cursor push/pop preserve an absolute root. Thus the
source dir_made_absolute flag stays false and its relative fallback and
shorten_path_with_cwd branch are unreachable in this domain. The generic gix
counterexample (`directory="..", cwd="/a"`, bare root success) can reach that
helper's parent expect; this implementation neither reproduces nor translates
that out-of-domain panic. No new source-failure exception is introduced.

The loop retains the initial cwd and normalized original directory, makes the
source len-sized cursor clone, preserves requested capacity across .git pushes
and pops, and maintains the original metadata cache/backup and device checks.
It tries .git then bare, including the source's started-as-.git pop behavior.
The uncached `recognize::git` wrapper acquires followed metadata first; only
success requests a fresh owned cwd. Cached recognition receives the original
cwd. Semantic candidate failures retire before cursor restoration and ascent;
missing observations or original-account refusal never become another candidate.

The private caller-owned callback receives Initial or Candidate(path) at each
reached occurrence and the same Session, and must return an admitted owned
PathBuf or source I/O error. It has no default/native implementation, cached cwd,
new observation schema or reset account. It is a composition seam, not the
native/current-command proof. The additional cwd in gix's repository-opening
wrapper (`discover.rs:44`) still belongs to the next connection.

`trust::from_path` follows gix-sec0.14.1 `identity.rs:34–59` and `trust.rs:3–13`:
symlink_metadata first, then sealed effective uid, then SUDO_UID only for a
mismatch. The existing Identity codec gains typed Host/Session accessors; no
ambient uid is read. Present var_os bytes get their own admitted Vec/OsString
before borrowed UTF-8 validation and u32 parsing, and retire before comparison.
Pinned std `sys/env/unix.rs:109–126`, `sys/os_str/bytes.rs:295–296` and
`core/num/mod.rs:1627–1628,1757–1844` allow a leading plus and arbitrary leading
zeros, reject whitespace/invalid UTF-8/overflow, and allocate no parser error.
No root-user-only special case or trimming is added. Metadata CString ownership
retires before identity. Native errors remain exact inner errors; scalar Absent,
wrong types and missing data stay terminal. Both trust levels satisfy Reduced,
but all reached trust operations and owned CheckTrust errors remain present.

Two trust checks and five upward checks compose stored codec→Host→the actual
recognition/read/trust consumers. They cover uid short circuits and byte parsing,
distinct cwd occurrences, metadata-before-cwd errors, cached bare recognition,
ascent capacity, started-as-.git/root termination, device boundaries, owned
errors, missing evidence and original-account refusal at a candidate occurrence.
The existing normalization comparison now also compares source-owned capacities.
No native or release qualification is claimed.

## Supplied-fact repository recognition

`recognize::with_metadata` contains the complete `is.rs:40–163` body over the
caller's original followed metadata, cwd and retained whitespace owner. A file
candidate expands through the gitdir consumer before HEAD; the common-directory
branch still tests the original metadata. File-candidate common errors move the
joined path and existing I/O error into MissingCommonDir. Directory-candidate
common/gitdir semantic errors are discarded at the source Result::ok sites;
original Session refusal always escapes instead. No error wrapper allocates.

Owned Cow cloning copies the current path extent, borrowed cloning preserves the
borrow, and into_owned moves already-owned paths. Common-file, parsed common,
gitdir-file and private-directory owners retain the source expression scopes.
Objects is_dir is reached before refs is_dir, and failure moves that exact joined
path. The final classifier probes index first, even for a `.git` name; the index
temporary remains live through bare()'s filename test and retires before the
extension test. These are followed predicates over sealed Host observations,
with the pinned CString conversion and source-local NUL behavior before lookup.

The final MaybeRepo branch conforms the original candidate. Path equality admits
the whole dot component class (`.`, `./`, `.//././`), whose pinned
gix-path `realpath.rs:49–99` closure only pushes the supplied cwd and visits CurDir.
It reaches no symlink query or semantic realpath error. Fresh push preserves the
source amortized allocation (at least eight bytes for nonempty short cwd), while
the cwd spelling itself remains unchanged. Non-dot candidates use the existing
pinned lexical normalization. Path scans and copied byte extents are admitted
prospectively under the original Session.

The shared source-shaped join also replaces the HEAD predicate join and both
loose-read joins, preserving their existing owner scopes. The generic manifest
join is unchanged. No initial metadata/current-directory acquisition, relative
query association, upward search, repository opening or command-history proof
is implied by this private function.

Four finite checks compose stored codec→Host→path reads/HEAD→recognition. They
cover linked/submodule/worktree branches, owned errors, semantic fallback,
missing observation, general terminal-budget refusal and ordered predicates.
A fifth isolates dot spellings and Cow/capacity transfer. The refusal fixture exhausts the
original path budget before the first HEAD query; it is not a targeted
common-directory-stage refusal claim. These are component checks only.

## Path-file consumers and owned I/O errors

`path/file.rs` follows gix-discover `path.rs:29–70,105–148` and `parse.rs:21–36`.
The size gate is descriptor metadata, not a cap on content growth. The shared
`membership/git_read.rs::run` now formats and retains the complete oversized I/O
error while its `Opened` descriptor guard is still live. Replay executes the
same error construction under the original account; the capability-free read
transcript remains native events/bytes and needs no new schema.

Successful reads close the descriptor before Unicode trimming or parsing. Plain
files map NotFound from open, descriptor metadata or the read to None; other
source I/O errors propagate. Truncating trailing Unicode whitespace preserves
the original read Vec capacity, then Unix Vec→OsString→PathBuf transfers it
without allocation. Empty-path formatting and both error records coexist with
the truncated source buffer. The relative variant retains the plain PathBuf
through the parent join or its formatted no-parent error. Leading whitespace,
arbitrary Unix bytes, embedded NUL content and `..` spelling are preserved.

The gitdir variant checks the exact `gitdir: ` prefix before reaching the helper.
Both bad-prefix and empty-trimmed-path failures copy the entire original input,
not just the suffix. On Unix the successful borrowed native path has no UTF-8
failure branch; `Cow::into_owned` copies the trimmed extent. The original read
buffer remains live through the subsequent parent join. Pinned std
`path.rs:3081–3085,1341–1428` copies the parent first, then pushes separator and
suffix separately. The contained join preserves these requested capacities,
including parent ownership before an absolute suffix replaces it.
Each reached `Path::parent` scan prospectively admits the file path's byte extent
under the original Session; the plain-absolute fast return does not reach or
charge that scan. A refused scan admission stays terminal before join/error work.

Source-local NUL file names return the pinned static InvalidInput error before
Host lookup, with no invented errno. Pinned `small_c_string.rs:24–26,33–63` and
`alloc/ffi/c_str.rs:274–290` require stack conversion below 384 bytes, and a
len+1 owner copied before NUL validation otherwise. These are distinct from NUL
bytes returned as native path content, which remain valid lexical data.

`manifest/io.rs` contains the source custom error lifetime. Pinned
nightly-2026-07-11 `alloc/boxed/convert.rs:577–595` moves the formatted String into
StringError, then `alloc/io/error.rs:42–49` constructs Custom. The source Custom
(`core/io/error.rs:584–589`) includes kind, a fat error pointer and **two** drop
function pointers. Its outer owner invokes outer_drop (:671–678); Custom invokes
error_drop (:606–613). Two private one-record Vec owners safely replace these
inner/outer boxes, following existing contained storage practice. The
replacement Custom owns the inner Vec and both real teardown callbacks. Its
actual full `Layout`, not a guessed source byte size, is admitted. The callbacks
retire each owner once in the original outer→Custom→StringError/String order;
partial outer refusal drops the already-owned inner record. No unsafe, hidden
infallible error allocation, dummy layout padding or removed allocation is used.

Pure Path::display is heap-free on this Unix pin (`std/path.rs:3747` →
`ffi/os_str.rs:1707` → `sys/os_str/bytes.rs:65` → `core/bstr/mod.rs:175`).
Bounded traversal work is admitted before the existing two-pass formatter;
the exact output String and both actual record allocations are admitted in
order. The acquisition bridge's local Session immediately forwards the first
failure to its caller and spends the same Budget; it resets no account.

Finite checks connect live read→stored codec→replay→each consumer, assert read
capacity transfer and source join growth, test all three NotFound stages,
Unicode/full-input errors/native byte paths, source-local NUL conversion, both
real cleanup callbacks and refusal at the formatted/inner/outer ownership
boundaries. These remain component checks, not product qualification.

`reference.rs` contains the unchanged contiguous pinned normalize function with
imports supplied outside it. SHA-256:
5008282f9926e5df43cc6cb0f8aa7dd6101b81924ec484eff841f4adfc0c4321.
Keep it out of rustfmt. The two attached licenses are from the verified gix-path
archive. The small post-implementation checks compare native bytes and Cow
ownership against that function, exercise repository conversions and refusal.

The connected opening-cwd, commondir reread, and local/worktree configuration
StageOne slice has its source and containment record in
[`config/PROVENANCE.md`](config/PROVENANCE.md).

The following conditional NFC path step is connected and documented in
[`precompose/PROVENANCE.md`](precompose/PROVENANCE.md); packed-reference opening
remains a separate boundary.
