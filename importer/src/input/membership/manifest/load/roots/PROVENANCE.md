# Workspace declaration cache

Pinned Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`:

- `src/cargo/util/toml/mod.rs:86–101`: features and workspace configuration
  precede root publication under the manifest parent; normalization follows.
- `src/cargo/util/toml/mod.rs:1003–1025`: inheritance looks up the manifest's
  parent. On a miss the complete nested read must finish before post-load
  publication under the full manifest filename. The cache accepts distinct
  caller keys to preserve that difference; the full miss driver is outstanding.
- `src/cargo/core/workspace.rs:2245–2258`: the entire nearest-first cached
  ancestor pass precedes candidate probing and full nested loading.
- `src/cargo/core/workspace.rs:1990–2003`: excluded lexical prefixes are tested
  first, followed independently by member prefixes. Members override exclusion;
  neither list is glob-expanded here.

The sorted admitted table retains the existing native-equal key on replacement,
matching the pinned HashMap insertion behavior. Values borrow immutable typed
declarations and their declaring directory. They are not loaded-manifest proof.
Every allocated table/path layout uses the original Session. No filesystem
operation, native identity normalization, or new cache-miss shortcut is added.

`Preparing` represents only the successful pre-normalization entry. Complete
normalization, nested-load continuation and successful manifest conversion must
still run before a loaded result exists. Earlier publication is not rolled back
when later work fails; terminal failure must still stop the containing loader.
