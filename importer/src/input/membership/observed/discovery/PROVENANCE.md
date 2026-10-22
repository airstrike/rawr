# Closed config filename transcript

This is a capability-free boundary. The pinned source is Cargo
59800466c5c41c444d264b1010b4d57e85a7117f, util/context/mod.rs:1644–1689,
with same-file 1.0.6 src/unix.rs. The closed filename families are `config`
and `credentials`; each pairs its legacy spelling with its `.toml` spelling.

same-file opens read-only and then fstats the live descriptor. A successful
attempt retains device and inode; equality compares precisely both. Open
failure and fstat failure remain distinct facts. The transcript stores no
descriptor and cannot prove that live ownership or comparison was correct.

Pinned nightly-2026-07-11 std `sys/fs/unix.rs:1390` uses `cvt_r(open64)`;
`sys/pal/unix/mod.rs:245–255` retries Interrupted. Completed open failures
therefore cannot carry EINTR (the named target `nix::libc::EINTR`). The fstat
at `sys/fs/unix.rs:1409` uses one-shot `cvt`, so its EINTR remains a valid
completed result, as does fallback metadata's native EINTR. Every retried open
attempt still requires original-account work admission by the future producer;
excluding a completed EINTR is not permission to skip retry costs.

The checked constructor (`proof-bundle`) accepts exactly the source branches:
legacy success with warning disabled has no second operation; success with
warning enabled reaches a second acquisition; legacy failure reaches only the
fallback `.toml` exists operation, regardless of warning mode. The latter does
not demand successful open or a file-body read. Native OS errors use positive
Unix errno values, while std's interior-NUL InvalidInput is retained without an
invented errno. An fstat error cannot be InvalidInput from pathname conversion.

Both names share the same native directory and have NUL-free fixed suffixes.
Consequently legacy InvalidInput requires fallback InvalidInput; a successful
legacy open forbids second-open InvalidInput. The enclosing observation must
also bind these facts to the actual directory bytes. This module alone does not
validate that external association or relax observed Path's accepted bytes.

The observation codec now appends Discovery query tag 9 and value tag 12 without
reordering existing variants or tags. Older tag dispatchers reject these new
tags; old images gain no discovery evidence. The record checker binds filename
family and warn mode and rejects InvalidInput for its existing NUL-free absolute
Path. Decoding checks finite enum/boolean tags and invokes the same constructor,
including positive errno and reachable-branch constraints. Retention charges
the complete record layout plus native directory ownership; the transcript is
fixed inert data. Immutable Host lookup charges its native path copy and one
original graph path check on each reached query, including repeated/missing
queries. Missing discovery is terminal, not evidence that no filename exists.

Selection and diagnostic intent are derived rather than independently asserted.
Equal opened identities suppress the dual-name diagnostic in the pinned source;
unequal identities select its both-exist warning. Second acquisition failure
selects deprecation reporting. These are diagnostic determinants only: selection
is conditional on completion of reached rendering/output in full Cargo. The boundary
now classifies exactly those terminal closures outside the current physical
projection: `Selection` denotes that projection's spelling, never hypothetical
full-command success. No warning is emitted or claimed successful by this type.
Actual cold Cargo still runs with its real warnings and must succeed. Resource refusal, missing
witnesses, and warning-output failures cannot be encoded as native failures.

All enums and the checked transcript have fixed-size inert data. Construction
performs finite scalar comparisons without allocation or iteration. Original
account admission, acquisition, handle scope/cleanup, and current-source
association remain separate connection obligations. This module
does not authorize reuse, instantiate an observed record, or grant membership
authority. Tests cover every branch category for both filename/warning modes,
both components of identity equality, and invalid/unreached combinations.
