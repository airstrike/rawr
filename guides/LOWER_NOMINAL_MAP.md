# Where code in the nominal lowering goes

`src/resolve/lower_nominal.rs` is one file of about 64,000 lines, most of it one
`impl Parser` block. It is being split into modules under
`src/resolve/lower_nominal/` by concern, one region at a time. This map says
where each concern lives, so new code goes to its module from the start.

## Rules

- New parser methods and types go into the module their concern names below,
  never into `lower_nominal.rs`. A module that only extends the parser holds
  `impl Parser<'_> { … }` blocks and may `use super::*;`. A module with types
  of its own imports explicitly.
- A feature branch does not move regions. It changes code where it is, and new
  or changed code follows `RUST_STYLE.md` and `OPAQUE.md`.
- The root moves a region on main between merges, as a pure move (no edits),
  and only when no open branch touches it. Branches then rebase over the move.
- A long function is split only when a fix touches it and the split is shown by
  the tests the fix adds. A signature with many parameters becomes a bundle the
  same way.

## Modules

Regions are named by their symbols; line numbers drift.

| Module | Concern | Symbols |
|---|---|---|
| `format` | format strings and specs | `FormatArgument`, `FormatCount`, `FormatCountRef`, `FormatKind`, `RawFormatSpec`, `FormatSpec`, `DebugLeaf`, `DebugSpec`, `panic_format`, `parse_assert_compare` |
| `wide` | 128-bit and pending numeric literals | `Wide`, `PendingWide`, `PendingFloat` |
| `inference` | type variables and rollback | `Inference`, `Checkpoint`, `Undo`, `Patch` |
| `ty` | the lowering's types | `Ty`, `Int`, `Float`, `Pointer`, `Counter`, `HostTyProof` |
| `op` | lowered operations | `Op`, `PreludeOp`, `ExecutableOp`, `WitnessedOp`, `Keys` |
| `models` | program and crate models | `Models`, `Tables`, `ScriptTraitImpl`, `Nominal` |
| `coverage` | match exhaustiveness | `CoverageKey`, `CoveragePattern`, `CoverageConstructor`, `CoverageWitness`, `Scalar` |
| `pattern` | patterns and bindings | `PatternSyntax`, `EnumPatternSyntax`, `ResolvedEnumPattern`, `ResolvedRootPattern`, `HostPattern`, the `parse_pattern*`, `resolve_*_pattern*` and `bind_let_pattern` methods |
| `lvalue` | assignment places | `Lvalue`, `PlaceStep`, `OpenedPlace` |
| `imported` | crate instantiation | `imported`, `reveal` and their helpers |
| `dispatch` | function and method writers | the `write_*` dispatchers, `dispatch_host_method` |
| `trait_call` | trait method calls | `parse_std_trait_call`, `parse_typed_std_trait_call` and the `parse_*trait*_call` family |
| `postfix` | method chains and indexing | `parse_postfix_steps`, `parse_array_method`, `parse_sort_method` |
| `path` | qualified paths | `parse_qualified_path`, `parse_owner_function`, `parse_imported_variant` |
| `closure` | closures and async blocks | `ClosureScope`, `LiftedClosure`, `parse_closure`, the `async_*` methods |

Existing modules (`places`, `borrows`, `drops`, `std_method`, `std_impl`,
`iterator`, `sequence`, `literal`, `method_path`, `cow`, …) keep their concerns.

## Order of moves

Type-only regions first, since they carry no parser methods: `format`, `wide`,
`inference`, `ty`, `op`, `models`, `coverage`. Parser method clusters follow,
each only when cold.
