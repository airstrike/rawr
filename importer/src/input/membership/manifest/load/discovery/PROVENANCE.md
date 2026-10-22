# Lazy workspace candidates

Pinned Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`:

- `src/cargo/core/workspace.rs:2240–2278`: the complete cached ancestor pass
  precedes uncached candidate iteration. Only reached candidates are loaded.
- `workspace.rs:2290–2314`: `paths::ancestors(...).skip(2)`, then LookBehind,
  stop at `target/package`, stop when the preceding yielded ancestor is Cargo
  home, append `Cargo.toml`, and filter using followed `Path::exists`.
- `workspace.rs:2316–2348`: LookBehind starts with no previous path, after skip.
- `crates/cargo-util/src/paths.rs:433–475`: ancestor creation reads
  `__CARGO_TEST_ROOT` via `env::var(...).ok()`. A matching stop path is yielded
  inclusively. This applies even to paths discarded by skip. Absent or
  non-Unicode environment values mean no stop path.

The contained implementation borrows the stop path from the sealed Cargo
environment observation and home from the containing Cargo context. It never
reads Rawr's ambient process environment. Unrecorded environment/metadata and
resource failures remain terminal through the original Session. Candidate path
allocation and host metadata consume the original allocation/graph accounts.

Candidate state advances lazily and retains constant inline traversal storage.
A present directory or other metadata kind is yielded, matching `exists`; the
subsequent full loader must handle read errors. Yielding a path is not evidence
of successful manifest loading or workspace membership. Complete nested loading
and post-load workspace configuration inspection remain downstream.
