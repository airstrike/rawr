# Where code in the nominal lowering goes

`src/resolve/lower_nominal.rs` is the parent of the nominal lowering's modules
under `src/resolve/lower_nominal/`. It holds their `mod` declarations, the
imports and re-exports that tie them together, the `Parser` struct and its
constructors (`new_with_offset`, `for_method`, `for_function`, `bind_scope`).
Everything else lives in the module its concern names below. This map says
where each concern lives, so new code goes to its module from the start.

## Rules

- New parser methods and types go into the module their concern names below,
  never into `lower_nominal.rs`. A module that only extends the parser holds
  `impl Parser<'_> { … }` blocks and may `use super::*;`. A module with types
  of its own imports explicitly.
- Outside its module a type is named by its module path (`coverage::Key`,
  `format::Spec`, `pattern::Syntax`), never by a composite name.
- A feature branch does not move code between modules. It changes code where it
  is, and new or changed code follows `RUST_STYLE.md` and `OPAQUE.md`.
- The root moves code on main between merges, as a pure move (no edits), and
  only when no open branch touches it. Branches then rebase over the move.
- A long function is split only when a fix touches it and the split is shown by
  the tests the fix adds. A signature with many parameters becomes a bundle the
  same way.

## Modules

Modules are named by their symbols; line numbers drift.

### Types and data

| Module | Concern | Symbols |
|---|---|---|
| `ty` | the lowering's types | `Ty`, `Int`, `Float`, `Pointer`, `Counter`, `Keyed`, `HostTyProof`, the type predicates (`contains_inference`, `same_inferred`, …) |
| `inference` | type variables and rollback | `Inference`, `Checkpoint`, `Undo`, `Patch` |
| `unify` | unification and projections | `unify_type`, `selected_projection`, `unifies` |
| `op` | lowered operations | `Op`, `PreludeOp`, `ExecutableOp`, `WitnessedOp`, `Keys`, `register`, runtime validation |
| `models` | program and crate models | `Models`, `Tables`, `ScriptTraitImpl`, `Nominal` |
| `scope` | the parser's scopes | `Binding`, `BindingKind`, `Frame`, `LoopFrame`, `Branch`, `BoundIterator` |
| `expr` | expressions | `ParsedExpr`, `parse_expr*`, `parse_operand`, `parse_primary`, `parse_name_primary`, `parse_name_value` |
| `wide` | 128-bit and pending numeric literals | `Wide`, `PendingWide`, `PendingFloat`, `wide_literal`, `wide_binary` |
| `naming` | generated names and placeholders | `SENTINEL`, `DEBUG_DISPATCHER`, `method_name`, `function_display_name`, the `*_dispatcher_name` and `*_placeholder` functions |
| `tokens` | the token stream | `next`, `peek*`, `expect*`, `lex_rust`, `split_arguments`, `span`, `text_end` |

### Bodies, statements and control flow

| Module | Concern | Symbols |
|---|---|---|
| `entry` | the lowering's entry | `lower`, `with_uses`, `lower_with_context`, `Output`, `Lowered` |
| `body` | function and method bodies | `parse`, `method_body`, `function_body`, `fix_opaque*`, `finish_uninitialized` |
| `statement` | statements | `parse_statement`, `parse_expression_statement`, `parse_assignment`, `terminate`, scope frames |
| `binding` | `let` and its bindings | `parse_binding_statement`, `parse_let_else`, `destructure*` |
| `branch` | `match`, `if`, let chains | `parse_match`, `parse_if`, `parse_chain`, `parse_match_action`, `parse_host_match`, `matches!` |
| `loops` | loops and jumps | `parse_loop`, `parse_for`, `parse_jump`, `parse_return` |
| `attributes` | attributes and items in bodies | `cfg_predicate`, `outer_attributes`, `skip_nested_item` |
| `moves` | moves | `move_local`, `move_part`, `move_capture`, `moved_value` |
| `mutability` | mutability checks | `immutable_assignment`, `require_mutable_receiver` |
| `lvalue` | assignment places | `Lvalue`, `PlaceStep`, `OpenedPlace`, `nested_parts`, `compound_assignment`, `nested_step` |
| `pattern` | patterns and bindings | `pattern::Syntax`, `pattern::Host`, `parse_pattern*`, `resolve_*_pattern*`, `host_pattern*`, `bind_let_pattern` |
| `coverage` | match exhaustiveness and refutability | `coverage::Key`, `coverage::Pattern`, `coverage::Witness`, `Scalar`, `coverage_witnesses`, `require_irrefutable` |
| `closure` | closures and async blocks | `closure::Scope`, `closure::Lifted`, `parse_closure`, `parse_async*`, `parse_tokio_macro` |

### Calls, operators and values

| Module | Concern | Symbols |
|---|---|---|
| `call` | calls and arguments | `parse_script_call`, `parse_call_arguments`, `call_function_value`, `mutable_arguments` |
| `postfix` | method chains, indexing and `?` | `parse_postfix_steps`, `parse_array_method`, `parse_sort_method`, `parse_try` |
| `path` | qualified paths | `parse_qualified_path`, `parse_owner_function`, `parse_imported_variant`, `parse_builtin_path` |
| `trait_call` | trait method calls | `parse_std_trait_call`, `parse_typed_std_trait_call`, `parse_*trait*_call` |
| `traits` | trait resolution | `select_impl`, `receiver_method`, `type_implements`, `check_associated_bounds` |
| `generics` | generic instances and witnesses | `fresh_generics`, `finish_witnesses`, `bind_instance`, `instantiate` |
| `operator` | operators and casts | `parse_binary`, `apply_binary`, `cast`, `operator_impl`, `index_impl` |
| `compare` | comparisons | `equality`, `partial_ord_operator`, `parse_ord_method`, `compare_value`, `equal_value` |
| `conversion` | conversions | `parse_conversion_into`, `finish_conversion`, `parse_try_into`, `PendingConversion` |
| `coerce` | coercions at a use site | `coerce_expr`, `coerce_branches`, `coerce_implemented_trait` |
| `construct` | values built by name | `parse_struct_fields`, `parse_array`, `parse_prelude_constructor`, `parse_imported_struct` |
| `constant` | literals, constants, compile-time macros | `literal`, `literal_kind`, `constant_use`, `compile_time_text` |
| `local_type` | types written in bodies | `parse_local_type`, `dyn_type`, `resolve_type`, `parse_alias_type` |
| `format` | format strings and specs | `format::Spec`, `format::RawSpec`, `DebugSpec`, `parse_format`, `parse_print`, `parse_panic_macro`, `parse_dbg` |
| `builtin_method` | builtin values' methods | `parse_string_method`, `parse_option_method`, `parse_result_method`, `vec_method`, `str_method`, `int_method` |
| `time` | `Duration` and `Instant` | `parse_time_method`, `time_binary`, `time_method` |
| `from_str` | `FromStr` and std's parse errors | `parse_str_parse`, `ParseError`, `parse_text` |
| `std_item` | std paths in bodies | `check_std_path`, `parse_mem_call`, `parse_size_of`, `std_alias` |
| `names` | names in scope and their errors | `lexical_scope`, `unknown_variable`, `unknown_function` |
| `emit` | emitting operations | `op_call`, `materialize`, `private_call`, `write_over` |
| `proof` | proofs of executable operations | `prove_executable_op`, `type_provides`, `proven_type` |

### Crates

| Module | Concern | Symbols |
|---|---|---|
| `imported` | crate instantiation | `imported`, `reveal` and their helpers |
| `surface` | crate surface types | `imported_surface_type`, `instance_surface_type`, `ImportedUse` |
| `host_signature` | crate signatures at a call | `select_host_signature_form`, `instantiate_host_signature`, `instantiate_host_type` |
| `host_call` | crate calls, values and fields | `parse_host_method`, `call_host_function`, `parse_host_macro`, `read_imported_field`, `ImportedContext` |
| `collect` | crate uses a body collects | `collect_imported_call`, `crossing_slots` |
| `dispatch` | function and method writers | the `write_*` dispatchers, `dispatch_host_method`, `copied_parameters` |
| `relocate` | syntax tree rewriting | `relocate`, `rewrite_statements` |

Existing modules (`places`, `borrows`, `drops`, `std_method`, `std_impl`,
`iterator`, `sequence`, `literal`, `method_path`, `cow`, `keyed`, `cell`,
`location`, `overflow`, …) keep their concerns. The parent's unit tests are in
`tests.rs`.
