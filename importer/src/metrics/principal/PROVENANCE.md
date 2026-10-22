# Principal publication observation join

Implementation continuation from `49aeee06`, scoped to the existing paired cold
cell. The source inventory is
`.Codex/map/zero-touch-rust-importer/evidence/runner-boundary/DURABLE-WRITE-SOURCES.md`.
This is observational evidence, not storage authority or release qualification.

## Reached producers and consumer

- `Store::publish_reserved` supplies actual kind/schema/action/content,
  publisher and reservation identities. Production action/content path
  constructors are shared with the inert target decoder. Commitments to the
  actual root and relative target must match those reconstructed paths.
- `accounting::Record::seal` already computes the raw member SHA-256 and length.
  The observer reuses that result; it hashes only the already produced allocation
  encoding. It does not copy or hash a second payload image.
- `index::publish_row` observes the already produced 112-byte row. The existing
  row fast path observes the bytes already read, without claiming publication.
- Both entry publication functions report rename immediately after successful
  rename and before parent synchronization. Terminal outcome follows cleanup.
  Thus a later sync failure retains both the rename and the failed result.
  Sealed candidate commitments alone never assert that the candidate was written.
- `Store::with_pin_roles_at` observes the supplied proof only after the
  Live/LastGood transaction and consumer callback both succeed. Prepared,
  in-flight and rolled-back callbacks do not become consumer admissions.

Each authenticated child carries its own ordered observations. Tokens join
interleaved attempts within that process; they impose no cross-process order.
The parent joins actual target/member commitments to its existing raw after
manifest and each publisher's own admitted read set. A losing content candidate's
allocation encoding is not mistaken for the existing winner's allocation record.
The cold parent prestate is empty, so every principal regular file after the
pair must have an observed rename owner. No additional filesystem acquisition is
performed by the join.

## Bounded ownership and handoff

There is no guessed per-kind publication count. Event encoding is counted without
a buffer; representation growth and serialized evidence obey the existing 8 MiB
cell bound. Event-array allocations and snapshot copying debit the same original
graph. Allocation/index/path hashes also use its original hash axis. Failure
latches incomplete evidence and never changes a publication result.
The paired cell's single-original-account domain is checked by shared usage
identity. A second graph latches incomplete evidence before its event is stored;
the last writer cannot take ownership of earlier observation costs.

The mutable history has a separate recorder mutex. Original graph observations
can therefore reenter the scalar recorder without holding its State lock.
Snapshot rows are immutable and shared by Arc, so Snapshot clones do not copy
the event array. Late charges retain only the original usage state, not the graph
scratch registry. Snapshot charges are merged into recorder accounting explicitly.

The actual cell `Session::finish` consumes the snapshot and prepares the two
reached encodings: `Envelope::seal` encodes the payload for its digest, then
`frame::send` encodes the enclosing envelope. The cell canonical encoder first
counts JSON into a bounded allocation-free writer, requests exactly that many
bytes, and writes into fixed storage. No implicit Vec growth or shrink is used.
The new event array therefore adds exactly two requested wire-byte copies and
one payload-digest hash pass. Those incremental Decoded and Hashed charges are
merged into the snapshot's graph evidence before sealing. Existing outer capsule
fields and framing remain separate from this new dynamic observation component.
Serialization itself is pure; decoded parent evidence has no producer account
or live capability.
This does not expand the existing whole-frame limit. The separate LaunchSink
transport is explicitly marked UnsupportedTransport for principal closure.
With no enabled principal observer, the added work does not hash, allocate or
spend the graph account.

## Focused checks and scope

The connected fixture constructs two distinct actions with shared content using
the actual Store, admits their real read sets, transports the observations through
the codec and joins them to raw endpoint members. It also rejects altered raw
bytes and a substituted own-process consumed action. This deterministic sequential
fixture is not a concurrency or Gallery measurement. Existing entry fault
injection checks rename before parent-sync failure. Additional checks cover
sticky frame/account refusal, immutable snapshot sharing, late graph observations,
inactive observation, rejection of a second original graph, exact wire-copy/hash
charges and hash refusal, fixed-output length drift, and retirement of unrelated
graph scratch.

Pins, ledgers, repair/deletion operations, control metadata, directories and the
remaining release cases are outside this slice. The paired report continues to
declare `durable_write_closure` Unavailable. No new native permission, source
selection, numerical cap, scenario, probe, installation or performance claim is
introduced.


## Ledger replacement connection

The next connected boundary extends this same bounded history, account and cell
transport. `Store::open_cargo_home` now supplies its original graph before store
bootstrap; the test convenience `Store::open` creates its one graph before entry.
`Publication` borrows that graph, and all recovery, migration, keyed-lease and
bundle callers pass it explicitly. No new account is installed after bootstrap,
and retained telemetry still holds only the existing thin usage handle.

Every production `replace_admitted_file` caller supplies a typed installation or
project target and its actual operation cause. Reservation, cancellation,
planning, commitment and keyed recovery carry the reached action/execution lease.
Migration, bootstrap, eviction, repair and project/pin purposes remain distinct.
Each event binds the actual canonical root/relative target, ledger schema,
publication-lease occurrence, existing scalar purpose and commitment of the exact
already-produced buffer. No payload copy or filesystem rescan is introduced.
Pin binding includes the prior raw ledger bytes or the observed absence;
migration includes its actual old-schema bytes; bootstrap includes its observed
absence. Other predecessors are explicitly `Unknown` where the current caller
has only a decoded ledger. This is not an invented prestate or a complete
ledger-transition proof.

The actual replacement primitive records rename immediately after it succeeds,
cleanup only when reached, and terminal outcome after the existing cleanup error
precedence. A later parent-sync failure cannot erase the rename. A replacement
performed by a retry or restoration is a new actual occurrence. Rollback's
original control flow and Drop retry remain unchanged.

The existing paired parent join now retains replacement summaries and compares
final ledger members with actual renamed commitments or identical raw initial
members. Intermediate writes need not equal the final endpoint. Matching process
writers are reported as candidates; no inter-process last-writer order is
invented. This remains a replacement join only: pin-record ownership/liveness,
transaction rollback-stage history, ledger retirement/deletion, controls,
directories and remaining cases are not asserted complete. The CLI durable-write
closure remains unavailable.

Focused checks added after implementation cover original-account bootstrap,
inactive and refused observation, real rename followed by sync failure, exact
pin-bind predecessor/restoration, cell serialization and the actual two-process
store/consumer/endpoint fixture. Validation completed on September30:99 metrics tests pass. The selected store
and connected endpoint run passed121 tests; its single failed assertion still
counted the pre-ownership bootstrap layout. The corrected assertion explicitly
accounts for18 directories and7 files and passes on focused rerun. One existing
expensive store operator remains ignored. The CLI baseline suite passes84 tests
with11 governed operators ignored, and the ordinary importer check passes.
Scoped formatting and diff checks pass. Independent account/lifetime review has
no actionable finding; write-history review's missing project-recovery key is
corrected and re-reviewed clear, with a distinct removed/current-key regression.
Logs: `/private/tmp/rawr-p9-ledger-{metrics-fixed,store-fixed,bootstrap-fixed,default,cli}-sep30.log`.
These are development checks, not release qualification or Gallery measurements.


## Pin owners, transactions and retirement connection

The next boundary continues the same history and thin original Usage handle.
`pins::lock` binds the encoded source record, role/project/owner/proof and ordered
actual allocation references before acquisition. Existing reuse follows its real
record validation; publication records the actual rename. A validated shared or
exclusive guard emits its opening, retains no graph scratch or recorder, and
emits owner retirement only after its native File field drops. This is owner
destruction evidence, not a claim that a native close succeeded. A different
active recorder cannot complete the original lifecycle merely by sharing Usage:
the parent requires each opening and retirement in the same process history.
Original-account charges from retained Usage are forwarded to graph observations;
the separate History/State lock boundary is unchanged.

Transactions retain the requested role order, previous and acquired guard tokens,
reached ledger/validation/callback outcomes, and every actual restoration attempt.
Explicit restoration failure and the later Drop retry have distinct tokens and
results; the original restore ordering and cleanup error precedence are unchanged.
Released and stale pin retirement and empty project-ledger retirement record the
actual source target and temporary-destination commitment immediately after rename,
before either sync. Removal start and successful/failed removal are distinct.
A failed recursive removal can have changed part of its tree and is never called
unchanged or completely deleted. Absent project ledgers record absence without
inventing deletion. No observation hook acquires fresh filesystem evidence.

The existing paired parent now checks these histories, canonical pin bytes and
owner derivation using the already produced typed reference rows, and joins
successful Live/LastGood roles to that process's own admitted read-set identities.
It checks final pin regular files against actual rename writers or identical raw
initial members, and missing initial pin/project files against observed retirement
renames. Temporary destinations retain any remaining regular-member count.
Intermediate process-local writes are not required to equal the final endpoint;
no inter-process chronology or last-writer ownership is inferred.

Focused assertions extend the real-store/codec/parent fixture to reject omitted
owner openings and retirements. The existing rollback-failure fixture checks two
separate restore occurrences, terminal guard retirement, and rename-before-removal.
Root owns execution of these checks. This is still not complete durable-write
closure: non-pin deletions, controls, directories/modes, and the remaining release
cases remain outside this boundary. No production activation, native permission,
new scenario or performance claim is made.

Allocation/accounting sites: transaction `retained`/`acquired` guard reservations,
migration's guard reservation, snapshot `stale`/`active` reservations, regrouped
stale-vector growth, retained-stale reservation, and reached nonempty boxing
shrink requests admit their full actual augmented array layout. These arrays had
no earlier GraphBudget Decoded admission; recovery's metadata-byte counter is a
distinct source account and does not fund them. The native Vec and boxing calls
are unchanged. Current capacity decides whether growth/shrink is reached. The
requested amortized capacity for the one existing regrouping `Vec::push` is the
pinned Rust `alloc/raw_vec/mod.rs:158,503–532` formula; nonempty shrink uses
`:831–862` and `Vec::into_boxed_slice`. These exact sources are in the installed
`nightly-2026-07-11-aarch64-apple-darwin/lib/rustlib/src/rust/library/alloc/src`.
No copying or fresh allocation of the underlying Usage Arc is charged. Observer
refusal latches incomplete evidence and leaves the original product operation and
error precedence unchanged.


The scoped parent-review correction records `LedgerAttempted` at the actual
pre-call flag assignment, so even a PinBind failure before publication (or after
rename but before sync) keeps its restoration obligation. Each retry has its own
bounded event interval. `RestoreApplied` is emitted after its body succeeds and
before captured previous guards drop; success then requires that interval's
actual PinRestore write to the captured bytes, or its reached project retirement
result restoring absence. A later transaction cannot satisfy the obligation.
The parent also retains the complete previous-guard cohort and checks acquired
and previous guards across binding, validation, callback, commit and restoration
mutations. It permits bind-failure acquired guards to clear after the Restore
marker, callback-failure guards to drop before it, and previous guards to drop
before Committed/Restored after their protected operations finish. Focused
mutations of the existing real-store fixture remove actual restoration events,
move acquired retirement before validation or after restoration, and move a
previous retirement before its protected work; the source rollback fixture
checks the legitimate before/after-drop marker ordering.


The final scoped lifetime correction retains each reached PinBind write token
from its Started event through its actual Finished event, including failure.
Acquired owners must remain alive through rename, cleanup and terminal outcome;
retirement in that interval is rejected even if no completed Ledger step follows.
A failure before the write starts leaves no active write token, while a started
write that fails admission closes its token with its actual failed Finished.
The connected fixture derives a rename-then-sync-failure transcript from its real
rejected transaction, first verifies that intact branch is accepted, and then
moves a guard retirement before rename or before failed completion. This is a
bounded transcript-mutation regression, not a new native failure experiment; the
existing entry fixture separately exercises actual rename-before-sync-failure.


Validation completed September30 after implementation: the initial scoped run
passed221 importer metrics/store/connected-endpoint tests with1 governed operator
ignored. After the parent review corrections, all109 affected metrics, transaction
and connected-endpoint tests pass. The ordinary importer check passes; all84 CLI
baseline tests pass on the final correction with11 governed operators ignored.
Scoped formatting and diff checks pass. Independent account/lifetime review is
clear. Parent review's restoration-operation and guard-lifetime findings are
corrected and bounded rereview is clear, including PinBind rename followed by
failed completion. Logs: `/private/tmp/rawr-p9-pin-importer-sep30.log`,
`rawr-p9-pin-default-sep30.log`, `rawr-p9-pin-final-sep30.log` and
`rawr-p9-pin-cli-final-sep30.log`. These are development checks. No release
operator, installation or Gallery performance measurement was executed.

## Directory and permission endpoints

The parent `cell/project.rs` manifest now retains regular files with Unix
permission bits (`mode & 07777`) and a separate sorted directory list. An empty
directory path is the present root; an absent root has no directory rows. Only
`symlink_metadata` returning NotFound establishes absence. Directory metadata is
retained before recursion in the existing walk; regular-file metadata validation
also checks the captured permission bits. This remains visited endpoint evidence,
not an atomic snapshot, writer attribution, ownership/mode authorization or a
native permission claim.

The cache aggregate uses the `rawr.phase9.cell.cache/v2` domain and binds typed
directory/file rows and modes. File content digests and `State.bytes` still count
only regular-file contents. New serde fields are mandatory. The existing 200,000
population ceiling now covers files and directories together, including the root;
this deliberately tightens the admitted tree domain. Directory entries are
retained incrementally only after count and raw relative-path bounds pass, with
fallible Vec reservation. The 16 KiB relative-path, per-file and 8 GiB content
bounds are unchanged. This parent walker has no graph allocation account; these
population bounds are not represented as GraphBudget or report-encoding charges.

`attribution.rs` preserves modes and directories in both raw and projected
manifests, uses projected-manifest/v2 and attribution/v5, and rejects structural
changes before nonce-only comparison. `baseline/cache.rs` still requires exact
full raw equality. `baseline/paired.rs` checks explicit root absence as well as
regular-file emptiness. `baseline/corruption.rs` checks structural equality for
the byte-only injection and includes directories/modes in bundle-prefix repair
comparisons. Principal, ledger and pin joins retain their regular-file byte
meaning; directory/mode writer attribution is still unfinished.

`baseline/corruption/seed.rs` recreates every captured directory, including empty
ones, and copies full file permission bits after file contents. It applies final
directory modes deepest-first after all contents, checking retained destination
identities. Ancestors must already be represented by captured directory rows.
The exact captured source reread and exact destination Manifest equality remain
mandatory; unsupported or changed facts fail instead of being filtered out.
Combined explicit seed population counts include both roots and all directories.

Focused checks cover absent/present roots, empty directories, file/root chmod,
file-to-directory replacement with equal content-byte totals, incremental count
refusal, missing legacy serde fields, nonce projection refusal, exact seed copies
of empty/read-only directories and modes, and captured source-mode drift. The
connected node fixture literal now explicitly represents an absent directory
list. Root owns compilation and execution of this batch.

The older independent `baseline.rs::tree_state` descriptive/ordinary-control
path remains unchanged. Its coverage stays under the existing unfinished R3
controls gate, along with non-pin deletions, control mutations, directory/mode
writer attribution and the remaining cases. `durable_write_closure` remains
Unavailable. No diagnostic operator, qualification, installation or performance
measurement is part of this endpoint extension.

Endpoint validation completed September30: all28 focused cell, attribution and
connected store/endpoint checks pass; all86 CLI baseline checks pass with11
governed operators ignored. Scoped formatting and diff checks pass. Independent
filesystem/seed-copy and consumer reviews found no actionable issue. No ordinary
product code changed in this boundary. Logs are
`/private/tmp/rawr-p9-endpoint-importer-sep30.log` and
`/private/tmp/rawr-p9-endpoint-cli-sep30.log`. These checks do not establish full
durable-write closure or Gallery performance.

## Non-pin retirement occurrences

`principal/retirement.rs` extends the same Event/history, original thin Usage and
bounded cell transport. It observes enrollment-pending retirement in `lease.rs`,
garbage/action/content retirement in `eviction.rs`, planned allocation recovery
in `accounting.rs`, exclusive bundle repair/eviction in `bundle.rs`, and direct
index bucket/row removal in `index.rs`. `entry.rs` shares its existing rename
point with the pin observer through a small typed hook. Actual temporary PID and
sequence are captured at construction, and relocation is emitted immediately
after rename, before either directory sync. Removal start, removal result and
the caller's final result remain separate; early errors finish as failures.
Bundle thaw or later index/publication failures retain the reached earlier
mutation. Existing cleanup and product error precedence are unchanged.

Targets preserve the existing closed namespace. Garbage decoding covers all
`control::allocation_position` outputs: known kind, any four-digit lowercase
action storage version or eight-digit content schema, and a canonical digest.
It does not fabricate valid records for malformed contents. Allocation references,
reservation/publisher identities and validated member commitments are reused
from already-held values. Content eviction retains its allocation/record-owner
facts but no payload commitment: that path inspected metadata, not payload bytes.
Bundle callers pass their already-associated allocation instead of enlarging
retained native guard owners. No fresh read, payload copy/hash, scratch registry,
account or captured sink is introduced. Inactive observation performs no new
hash/storage charge; refusal remains evidence incompleteness, not product failure.

The parent checks cause/target compatibility, original and temporary namespace
commitments, occurrence order and terminal results. It records candidate actual
mutations for missing initial regular-file and directory endpoints within these
namespaces. Failed removal remains possibly partial. Retained temporary rows
keep their exact source-relative association and separately state whether their
bytes/mode match an initial row; a successful removal cannot retain descendants.
The principal join can discharge a publication's terminal-presence obligation
only through that same process's later observed rename or successful removal.
It keeps final members tied to a remaining observed publisher, and does not
invent cross-process order to excuse an absent or replaced publication.

Focused extensions use the existing real runner-eviction fixture (bundle, action,
content and index paths), actual staged-but-abandoned entry fixture, codec and
parent manifests. Mutations reject omitted rename and altered temporary identity.
A failed-removal continuation derived from the real renamed history retains its
temporary, while a false successful-removal claim rejects. A small pure check
covers the complete legacy garbage target grammar. Root owns compilation and
execution. Temporary recovery, bundle initialization/Drop cleanup, mode writes,
other control/directory writes and remaining cases are still outside this slice;
the full durable-write closure remains Unavailable.

The scoped review correction binds each temporary's actual process ID to its
reporting child's process ID. The real stage and eviction fixtures use their
actual current process ID. A mutation retaining the exact temporary and digest
but changing the reporting process is rejected, so another child's temporary
cannot discharge this process's publication obligation.

The first focused build exposed an empty-suffix association error for a retained
temporary root: `PathBuf::join("")` appended a separator to its exact original
path. Both file and directory joins now preserve the original path unchanged
for the root and append only nonempty descendant suffixes. The real staged-file
fixture asserts that exact root association as well as initial-byte/mode equality.

Retirement validation completed September30: the initial focused run passed116
checks and exposed the retained-root path error above. After its correction, all3
affected staged-retirement, real-eviction and producer/consumer/endpoint checks
pass. The ordinary importer check and all86 CLI baseline checks pass, with11
governed operators ignored. Scoped formatting/diff checks and both independent
producer and parent reviews are clear after the PID-binding correction. Logs:
`/private/tmp/rawr-p9-retirement-{importer,fixed,default,cli}-sep30.log`. These are
component checks; no qualification, installation or Gallery measurement ran.

## Empty controls and ensured directories

The same principal Event/history and original account now record actual empty
control creation/adoption in `lease.rs`: bootstrap, fixed heavyweight/publication
locks, keyed action/execution locks, and migration/project-enrollment controls.
`entry.rs` records each reached ensured-directory component, including the
Cargo-home `rawr` and `rawr/v1` producer before Store opening. Migration's explicit
ownership-directory creation is connected separately. Store passes its original
graph into bootstrap; no replacement account, captured recorder or graph scratch
owner is added. Short-lived observations borrow that graph, and existing history
snapshot/transport accounting covers their full actual Event storage and encoding.
Inactive observation adds no hash or storage charge. Refusal latches incomplete
evidence without changing the product operation or error precedence.

The closed typed targets reconstruct the existing fixed and keyed namespaces.
Cargo-home events bind their actual Cargo-home anchor and `rawr`/`rawr/v1` paths;
the parent checks that anchor against its actual store-root spelling. Bootstrap
and pre-publication checks have no Publication ID. Published mutations retain
only their actual existing Publication occurrence. Existing directory and file
validation supplies the already-reached metadata; no new filesystem query is
introduced. Adopted file length/mode is not a content commitment, including when
length is zero. Successful `create_new` establishes produced empty content;
actual create outcomes are emitted before file/parent sync, whose reached results
are recorded separately. Concurrent AlreadyExists differs from creation and
from other failure. A create followed by failed sync remains a create. Finished
is explicit caller outcome, never inferred successful native close from Drop.
For `ensure_directory`, each component's outcome ends before the later whole-path
canonicalization; it does not claim success of that subsequent operation.

The paired parent checks source/target/anchor compatibility, process-local order,
required successful syncs and actual adoption/creation outcomes. New covered
regular endpoints require a real empty creation; existing raw initial content can
instead explain retained bytes. New covered directory/root presence requires an
actual create. Metadata adoption cannot fill a missing creation or initial-content
proof. Endpoints retain initial/final mode facts separately: these events do not
establish the mode requested by creation or authorize mode changes. Intermediate
created paths may later disappear, and the occurrence history is retained rather
than claiming final presence or cross-process last-writer identity. The `rawr`
parent is outside the store Manifest and is reported only as an anchored event.

Focused extensions follow implementation: the existing original-account bootstrap
fixture joins actual absent/built endpoints through the codec, observes later
adoption, and checks a derived create-then-sync-failure history. The bootstrap
contender fixture reads nonempty preexisting control bytes and proves its adoption
cannot establish a new empty-content writer. The existing full consumer fixture
now captures actual absent prestate and includes first bootstrap in its recorder;
its first reporting PID is the actual enrollment temporary PID. Its second
participant remains a sequential synthetic identity, not concurrency evidence.
Removing actual bootstrap creation rejects. Existing inactive/refusal checks also
cover the new hooks. Root owns compilation, formatting and execution.

Coverage remains bounded to these producers. Publication temporary directories,
temporary recovery, runner bundle initialization/Drop cleanup, mode mutations,
older ordinary-controls evidence and remaining release cases are unclosed. No
new resource limit, native mechanism, filesystem pass, scenario, qualification,
installation or Gallery metric is introduced. `durable_write_closure` remains
Unavailable; the pending packed-reference proposal is untouched.

The scoped parent correction keeps each successful create's actual event index.
The existing already-joined retirement history excludes a candidate after that
same process's later rename or successful removal of its path; cross-process
order is still not inferred. The connected enrollment fixture adds the exact
retired empty pending-control path to its final Manifest and requires rejection.
Historical creation remains visible in the attempt list but cannot justify a
new final endpoint after its observed retirement.

Creation validation completed October1: all115 focused metrics/lease/publication
and connected consumer checks pass; the ordinary importer check and all86 CLI
baseline checks pass, with11 governed operators ignored. Scoped formatting/diff
checks and independent producer review are clear. The independent parent review's
retired-creation P2 was corrected before execution and its bounded re-review is
clear. Logs: `/private/tmp/rawr-p9-creation-{importer,default,cli}-oct01.log`. No
qualification, installation or Gallery launch/reload measurement ran.

## Temporary recovery, thaw requests and batch synchronization

`store/recovery.rs::recover_temporaries` now borrows the caller's actual existing
Publication in every production path: store reservations/recovery/pinning,
lease enrollment/keyed creation, accounting initialization/migration and eviction.
It starts one observation on that Publication's original graph; no replacement
account or captured recorder is created. Reached measurements reuse the existing
path, directory flag and Charge before the measured vector drops its byte field.
The vector's actual representation is unchanged. Old producer and semantic target
remain explicitly Unknown. Decimal temporary names are validated by the same
source predicate and are not parsed as bounded process IDs or sequence numbers.

Recovery paths and thaw-member paths are represented by length/hash plus ordered
32-byte rows in the existing Copy Event stream. The already-owned source path is
borrowed; hashing is prospectively charged and the only retained copies occupy
centrally admitted Event storage. Existing snapshot/cell encoding and hash
accounting cover those rows. No separate path journal, graph account, payload
read/hash, filesystem pass or source-size cap is introduced. Inactive observation
returns before copying/hashing path rows. Parent reconstruction obeys the existing
8 MiB report bound and rejects absent/reordered/duplicate/trailing byte rows,
length/hash drift, unbound paths and wrong temporary/member associations.
The reconstructed paths are inert evidence and never filesystem authority.

Thaw observations use the existing metadata and exact requested mode word before
`set_permissions`, then record its returned success/failure. Successful requests
are not post-write stats; the parent compares their requested permission bits to
separate visited endpoint modes without claiming an atomic inode snapshot or
last writer. The existing Unix paired domain is retained. Each temporary records
thaw start/result, removal start/result and explicit terminal outcome. A failed
recursive removal may be partial. The one batch parent sync has a separate result,
so its failure does not rewrite successful individual removals. Source cleanup,
error precedence, native calls and their owner lifetimes remain unchanged.

The parent checks measurement order, complete path binding, per-temporary thaw and
mode intervals, removal outcomes and final batch synchronization. Missing initial
temporary endpoints require reached removal attempts. Retained endpoints can be
consistent with partial failure and separately matching mode requests; successful
removal cannot itself explain them. A later relocation requires an actual later
same-process event already accepted by the existing pin/non-pin occurrence join.
No cross-process order or old origin is inferred from a temporary name. Paired
reports include these histories while retaining `durable_write_closure` Unavailable.

Focused checks extend the existing real file/directory recovery fixture after
implementation: an empty temporary with long decimal fields, a frozen tree with
a non-ASCII raw path, actual chmod requests and removals, codec/parent
joining, omitted path-row rejection, and derived failed-batch-sync/partial-removal
histories. The failure transcripts are explicit mutations of the real observed
history, not native fault experiments. Existing exact/one-over recovery fixtures
now obtain a real publication lease from their established store before invoking
the same recovery body. Root owns compilation, formatting and test execution.

Bundle initialization/Drop lifecycle, publication temporary creation, nonrecovery
mode/deletion routes, older ordinary external controls and remaining qualification
cases remain outside this slice. This does not activate membership, approve the
pending packed-reference proposal, run qualification or add a Gallery metric.

The final scoped parent correction excludes a later relocation once that same
replacement has a successful removal. Non-pin outcomes come from the existing
joined removal result; pin outcomes check the same process/attempt's subsequent
Removed step. Unresolved and failed-removal replacements remain eligible. A
narrow derived extension of the actual recovery fixture checks both occurrence
families with unresolved, partial and successful removal, then injects a retained
endpoint at the recovery join. Unknown prior origin and the full-closure gate are
unchanged.

Recovery validation completed October1. The initial focused run passed115 checks;
its remaining fixture failed because the native filesystem rejects a non-UTF-8
filename (confirmed by one exact unsandboxed rerun). The real fixture now uses a
valid non-ASCII filename; that affected check passes, including both relocation
families and all three removal outcomes. Production code was unchanged by this
fixture correction. The ordinary importer check and all86 CLI baseline checks
pass, with11 governed operators ignored. Scoped formatting/diff checks pass;
producer review is clear, and the parent review's resolved-relocation P2 is
corrected and independently re-reviewed clear. Logs:
`/private/tmp/rawr-p9-recovery-{importer,fixture,fixture-fixed,default,cli}-oct01.log`.
No qualification, installation or Gallery latency measurement ran.

### Bundle-owned controls, modes and cleanup

`store/bundle.rs` now joins the actual bundle lifecycle to the same principal
history. `Store::with_bundle_reservation` supplies its original graph before
`Attempt` allocation; no Publication is claimed after reservation's scope ends.
The inline observation owner retains only the original thin Usage and transfers
from Attempt to Pending at seal. `Store::publish_reserved` supplies its actual
borrowed Publication for allocation-control publication and freeze. Pending's
existing product GraphBudget is unchanged. Drop never inherits a previously
passed Publication or infers a native close result.

Allocation records each attempted fresh root (including bounded identifier
collisions), ordered partitions, lease exclusive-open/write/sync, and root/parent
sync. Seal and allocation.pending reuse the already-produced bytes for admitted
commitments. Open success, completed write and file-sync success are distinct:
a failed write grants no supplied-byte commitment; a later sync failure cannot
undo a completed write. Allocation rename is observed before its subsequent
root sync. The actual Record supplies class/kind/schema/content/reservation and
publisher facts, and retained writer identities join their own admitted bundled
read set. Existing planned recovery passes its already-held valid Record;
incomplete/orphan/dead cases do not invent one. Repair/eviction reuse the existing
accepted retirement association and facts.

Freeze and thaw observations use existing metadata, exact requested permissions
and actual syscall results. Files freeze during the original walk; directories
freeze deepest-first with their existing syncs. Thaw preserves the original
root-first recursion. A successful request is not a post-write metadata fact.
Initialization failure removes directly and preserves its original error. Drop
records thaw and removal separately and still attempts removal after failed
thaw, without parent sync. Checked removal stops at failed thaw and otherwise
records removal before the existing parent sync. No observer adds a filesystem
walk, payload read, copied payload, new account or recorder; inactive owners do
not hash or allocate evidence. Operation storage and transport remain the
centrally admitted original-account principal Event array.

The parent reconstructs typed bundle/retirement targets and matches admitted
member-path commitments against existing raw manifests, including exact
original-to-temporary associations. Unmatched occurrences remain unmatched;
commitments do not enumerate absent-both-endpoint members or prove complete
traversal. Native create/write/rename lifetimes, attempted partial cleanup and
same-process later retirement remain distinct. No cross-process last-writer or
atomic filesystem snapshot is claimed. Existing deterministic initialization
failure and retained-bundle fixtures now traverse codec and parent join, plus
one actual Pending Drop fixture and a derived omitted-write-completion check.
They use the normal filesystem domain, without qualification operators.

This is a bounded R3 connection. Publication-temporary construction, ordinary
external controls and remaining mutation/case coverage still keep the complete
durable-write closure Unavailable. Packed-reference policy and acquisition are
unchanged.

The allocation-control join also reuses the source Record encoder to bind the
observed identity, charge, seal commitment and canonical record length to the
actual supplied control digest. The retention fixture rejects derived digest
and charge mutations while keeping control bytes and endpoints unchanged.
Producer publication reads Record fields without re-encoding: its observation
hashes the existing encoded bytes through original Usage. Planned deletion
reuses the Allocation already computed for its existing deletion charge.

Parent corrections retain creation obligations through later same-process
cleanup/retirement and require surviving creation for new fixed bundle
directories. A successful lifecycle stage rejects earlier aborting native
results. Completed control commitments also follow the accepted exact rename
into a surviving temporary, including partial-removal histories. The existing
retention fixture derives failed freeze/sync, relocated-seal corruption and a
stranded original-directory history to exercise those checks. The producer now
reuses one original-account allocation Commitment for its Record and control
write observations.

Bundle validation completed October1. All111 focused importer checks pass in
1.48s after1m10s compilation, including the actual Store/read-set/raw-endpoint
connection and the bounded derived corrections. The ordinary importer check
passes in6.48s. All86 CLI baseline checks pass in0.77s after51.02s compilation;
11 governed qualification operators remain ignored. Scoped formatting/diff
checks and both independent producer/parent re-reviews are clear. Logs:
`/private/tmp/rawr-p9-bundle-importer-oct01.log`,
`/private/tmp/rawr-p9-bundle-default-final-oct01.log`, and
`/private/tmp/rawr-p9-bundle-cli-oct01.log`. These are component/integration checks,
not installed Gallery performance evidence. Full durable-write closure remains
Unavailable; the packed-reference contract referral is unchanged.

### Entry publication temporaries

The three actual entry helpers now attach their existing `RESERVATION` process
and sequence to the existing principal, pin-acquire or ledger-replacement
attempt. The same original GraphBudget or retained Usage admits the fixed Event
rows and their existing snapshot/transport copies. No new recorder, graph,
payload hash, encoding, path copy or filesystem pass is introduced. Inactive
observers do not hash, copy or allocate evidence. These helper events claim no
additional Publication lease; their source attempt retains its existing scope.

Observations distinguish exclusive creation/open, completed writes, file and
temporary-directory sync, native rename, parent sync, existence check and
cleanup. Native results are recorded before propagating the original error.
Directory creation failure still returns before cleanup; replacement-open
failure still reaches its original existence/cleanup path. Cleanup failure
still overrides the operation error. A successful rename is recorded before a
later parent-sync failure. Early Existing has no reservation; a losing sealed
candidate retains its own completed bytes independently of the winner.

The parent projects the already-validated source token, typed target/member,
commitments and lifetime into this narrower native history. Sealed payload
commitments remain in the original metadata/seal callback after payload sync;
earlier failures explicitly lack that commitment. No missing byte identity is
invented from final bytes. Native order and source outcomes are checked,
including the specific unobserved sealed metadata/callback failure boundary.
A successful outer pin acquisition cannot follow a failed helper, while later
pin-lock failure may follow a successful helper.

Exact reporting PID plus reservation identify temporary endpoints. Root and
individual opened-member presence obligations last until actual rename or
removal; failed recursive cleanup may leave partial survivors. Completed-byte
commitments apply to retained members; unavailable/partial writes remain marked
unavailable. Later recovery can discharge an occurrence only with the same
reporting PID and exact raw path, after the relevant create/open occurrence.
These are process-local occurrence associations, without a cross-process last
writer, atomic snapshot or post-operation mode assertion.

The existing entry fixtures exercise actual sealed and ledger rename-before-sync
failure through codec and parent projection. Bounded histories derived from the
real sealed writes cover retained temporary members, byte corruption, omitted
members, wrong reporting PID and misplaced source seal. The existing connected
Store fixture now executes its second history in one exact sequential test
child, retaining the real captured PID and shared store instead of relabeling
a same-process history. Its workload is extracted once; no global environment
mutation, concurrency measurement or qualification operator is added. Derived
pin failure and rollback checks preserve the corresponding native helper rows.
Ordinary external controls and remaining mutation/case coverage still keep full
durable-write closure Unavailable; this change does not alter the pending
packed-reference contract.

Temporary validation completed October1. The initial focused run passed111
checks, including the real two-process Store fixture, and exposed one accidental
field rename in an older retirement mutation fixture. Restoring that field
changed no production code; the affected test passes in0.14s. Compilation took
1m10s initially and1m09s for the corrected fixture. The ordinary importer check
passes in7.36s. All86 CLI baseline checks pass in0.77s after51.48s compilation;
11 governed operators remain ignored. Scoped formatting/diff checks and the
independent producer, parent and test-child re-reviews are clear. Logs:
`/private/tmp/rawr-p9-temporary-{importer,fixture-fixed,default,cli}-oct01.log`.
No qualification, installation or new Gallery performance measurement ran.

### Published leaf and later-retirement endpoint composition

The parent now composes each actual successful entry-helper rename with its
leaf directory and fixed regular members. Rename remains effective after a
later sync failure. Existing/losing candidates do not publish their allocation
bytes. Typed final action/content/index/pin leaves also require a surviving
actual rename, so an empty extra leaf cannot evade the regular-file scan.
Parent directories and permission changes are outside this association.

Already-validated non-pin retirement facts and the narrow pin staged projection
supply original paths, source record commitments, relocation positions and
removal outcomes. Same-process later moves/removals are followed component-wise;
failed recursive removal explains missing descendants without claiming complete
deletion. Surviving original principal members retain their exact writer/byte
checks. Exact matching allocation-record bytes retain their original sealed
member binding, including when no repeated payload observation was made; an
explicit contradicting member fact is rejected. Pin roots require their matching
encoded-record commitment. Garbage/project-ledger moves do not inherit old byte identities
without those facts. Their associations remain explicitly unavailable. Ledger
replacement versions retain the existing matching-writer join.

A pin destination is reconstructed only from an exact digest match to a visited
raw temporary root or a later same-process recorded recovery path. No absent
path, numeric owner or sequence is invented. Successful removal cannot explain
a retained endpoint unless a concrete surviving later occurrence exists.
Matching mutations/publications in another process remain unordered candidates,
reported as unavailable associations instead of fabricated chronology.

The existing real Store fixture adds missing-leaf and empty-phantom-leaf checks.
The entry history derives a later validated action relocation and partial
removal from its real completed writes, checking that retained known bytes
still reject corruption, and the existing Store history checks a successful
direct removal after an earlier partial failure. No producer hook, account,
producer schema, filesystem pass, native mechanism or qualification operator
was added. Full durable-write closure remains Unavailable.

Validation completed October1: all8 focused publication-history/connected Store
checks pass in1.07s after1m11s compilation. All11 CLI paired-report checks pass
in0.72s after51.18s compilation; the governed cold operator remains ignored.
Scoped format/diff checks and both independent re-reviews are clear. The pin
recovery and other-process recreation corrections were checked by source review;
the added regressions cover leaf presence, repeated removal and known moved
bytes. Logs: `/private/tmp/rawr-p9-published-endpoints-{importer,cli}-oct01.log`.
No release qualification, installation or Gallery measurement ran.

## Sequential corruption-history consumer

The corruption observer now joins each authenticated child's existing principal
history to that child's own raw endpoints: absent store to completed setup, then
post-injection state to completed repair. The parent's injected byte change lies
between those intervals. Sequential phases may reuse a numeric PID, but their
local tokens are never combined. The paired cold consumer retains its distinct
two-process admission rule.

The single-process join requires unchanged warm principal bytes and modes unless
a surviving actual publication explains them. Final pin/ledger bytes match the
last recorded replacement, and completed bundle mode effects take precedence
over earlier directory creation. The repair result is a `proof-bundle`: it binds
the setup's actually consumed old allocation, exact repair owner and journaled
retirement, successful owned removal, and consumed retained replacement. Parent
sync after removal remains valid only within that same retirement interval;
operations on the deleted root and premature retirement completion reject.

The CLI retains joined setup and repair evidence on both success and failure.
Missing, incomplete or inconsistent histories reject without another child run.
Corruption observation/failure schemas are v2; the original output bound remains.
No producer hook, new filesystem pass, graph account, native mechanism or release
operator is introduced. Full durable-write closure remains Unavailable.

The connected fixture uses a real Store, consumer admissions, codec transport and
endpoint manifests. It changes one existing seal byte without changing its
extent or final mode. Setup first consumes the bundle, then uses a real unrelated
admission to replace its Live/LastGood roots. This deliberately makes the selected
bundle unrooted before corruption; it proves the unrooted repair consumer, not
that the real CLI corruption scenario succeeds. Negative cases cover unchanged
warm facts, exact allocation and admission identity, modes, control bytes, and
cleanup ordering. Earlier failed fixture directories were removed after their
processes and open references were confirmed absent.

### Protected CLI predecessor remains a separate integration issue

CLI shutdown drops runner/Prepared guards but retains ledger-owned LastGood
records. `recover_replaced` removes obsolete pin records, not bundle allocations;
`recover_rejected` runs before rebuild and excludes protected/stale roots. There
is no later targeted repair call. If rebuilt resolution completion and recipe
inputs remain equal, its action also remains equal: the existing action can block
replay/publication before successor promotion. The fixture's bundle-reference
salt does not establish that production case. The existing CLI corruption oracle
already required whole old-bundle absence before this consumer was added.

| | A — cleanup after promotion | **B — resolve protected same-action replacement** | C — relax repair acceptance |
|---|---|---|---|
| Coverage | Only replacements that can already publish | **Addresses the blocked ordinary replacement path** | Hides the integration gap |
| Governing condition | Existing unrooted cleanup rules | **Preserve predecessor/active-owner safety; consult Oracle for the finite route** | Not authorized |
| Limitation | Cannot make same-action promotion reachable | **Implementation policy remains unresolved** | Does not deliver existing scope |

The subsequent Oracle and unanimous Tribunal resolve the identity premise:
completed path-bound resolution actions bind their actual bundle; NativeInputs
and downstream semantic recipes remain placement-independent. Correct execution
and replay together, then connect bounded successor-associated cleanup. No general
publication-version system, forced unpin or acceptance exception follows. The
existing predecessor protection and exact unrooted-retirement rules remain.

Validation completed October1: all12 principal-focused checks pass in2.09s after
1m09s compilation. The retirement selection passed20 checks and exposed the
sandbox's denial of `ps` in one existing process fixture; its exact permitted
rerun passes in0.43s without source changes. All85 CLI baseline checks pass in
4.67s after55.21s compilation;12 governed operators remain ignored. The ordinary
importer check passes in6.53s. Scoped formatting/diff checks and the independent
CLI, endpoint and final cleanup-order re-reviews are clear. Logs:
`/private/tmp/rawr-p9-corruption-{history-final,retirement-final,retirement-ps,cli-final,default-final}-oct01.log`.
No release qualification, installation or Gallery performance measurement ran.

### October1 completed-resolution implementation evidence

Execution/replay now use the same bounded188-byte bundle/seal/record/completion
wire, schema7, and original graph-account charge. Six unit, five actual scripted
owner and two genuine same-action repair checks pass; default importer and baseline
CLI compilation pass. Scoped formatting/diff and independent re-review are clear.
The protected predecessor survives while a distinct completed successor under the
same execution key publishes and then reuses without entering cold execution.
This proves coexistence/reuse, not the still-unconnected post-admission cleanup.

The initial regression revealed an existing NativeInputs defect: exact
`FinalLock` completion already includes attempt paths, and the runner consumes
that exact input fingerprint directly. The conditional same-action collision in
the earlier analysis was not reproduced. The retained failed fixture has138 input
entries per bundle;11 differ only through harness/artifact placement, generated
package identity and dep-info facts, with equal file digests. Do not equate this
exact completion with semantic NativeInputs or normalize away freshness evidence.
The separately governed correction is a bounded semantic projection feeding the
runner identity, followed by original-account successor-associated cleanup.

Logs: `/private/tmp/rawr-p9-resolution-identity-{unit,owner-fixed,recovery,default,cli}-oct01.log`;
the first failing owner log is retained as discovery evidence. This changes no
corruption acceptance, retirement authority or release gate. No installation or
Gallery performance result follows.
