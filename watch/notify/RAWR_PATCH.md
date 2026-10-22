# Private watcher maintenance

Base: crates.io notify8.2.0, CC0-1.0 (LICENSE-CC0 retained).
UPSTREAM_SHA256.json records every copied original file before modifications.
The private package name and direct path dependency ensure generated runners
select this source, without an ambient workspace-only registry override.

Changes are limited to FSEvents lifecycle error/cleanup and stop signaling.
Safe rawr-watch owns incumbent/successor coverage separately. No startup canary,
polling timer, privilege, process observer or event-selection change is intended.
Startup result propagation follows notify-rs/notify PR733; the existing8.2.0
bindings remain. A CoreFoundation source signals shutdown, preserving requests
made before loop entry and avoiding the original idle-spin shutdown.

Focused independent review is complete with no remaining P0/P1 in this boundary.
This is not release qualification or installed-artifact acceptance.

Focused review found one P1 before activation: drain incumbent notifications
queued before successor creation before retiring the old stream. SinceNow on
the successor plus overlapping stream lifetimes does not cover undispatched old
events. Apple SDK FSEvents.h1324-1372 defines the FlushSync delivery guarantee.
A drain must stay off-gate, avoid callback-required locks and handle loop loss. The correction now uses FlushAsync plus a scalar delivery
acknowledgement after all batch handlers return; backend exit wakes the waiter
with failure. Deterministic target/loss, queued-write and blocked callback through
successor tests pass. The same independent reviewer confirmed the P1 addressed.

Current focused evidence: four native lifecycle tests and four actual child
observer integration tests pass outside the sandbox; CLI import-baseline check
passes. Sandbox watcher tests correctly now report native startup failures
(13 pass/10 fail); rerun with native access after the drain repair. No install.

Final focused checks: seven native lifecycle/drain tests,23 safe watcher tests
and four actual child-observer tests pass with native access. CLI import-baseline
check and scoped formatting pass. No Gallery timing or release claim follows.
