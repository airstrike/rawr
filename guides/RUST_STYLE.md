# Rust Style Guide

## Module Structure

### No `mod.rs`

We use the modern module style introduced in Rust 2018. A module `foo` is either:

- `foo.rs` (if it has no children)
- `foo.rs` + `foo/` directory (if it has children)

Never `foo/mod.rs`. The old style makes every tab in your editor say `mod.rs` and that's reason enough to avoid it.

```
# Good
src/
├── engine.rs              # the engine module
├── engine/
│   ├── registry.rs        # engine::registry
│   ├── policy.rs          # engine::policy
│   └── scheduler.rs       # engine::scheduler
├── resource.rs            # the resource module
├── resource/
│   ├── schema.rs          # resource::schema
│   ├── action.rs          # resource::action
│   └── dispatch.rs        # resource::dispatch

# Bad
src/
├── engine/
│   ├── mod.rs             # no.
│   ├── registry.rs
```

### One Semantic Type Per Module

Each module should contain one primary type and its closely related types. "Closely related" means types that exist only to serve the primary type — enum variants, configuration structs, small helpers.

```rust
// action.rs — good. Scope only exists as part of Action.
pub enum Action {
    Read,
    Write { scope: Scope, reversible: bool },
}

pub enum Scope {
    Local,
    External,
}
```

```rust
// Don't put Action, Interface, Identity, and Requirements all in schema.rs.
// Each gets its own module.
```

When in doubt: if a type could conceivably be imported independently by code that doesn't care about the parent type, it deserves its own module.

---

## Naming

### No Composite Names — Use the Module Path

The module system is a namespace. Use it. Don't flatten hierarchies into names.

```rust
// Good
mod resource {
    pub struct Schema { ... }
    pub struct Registry { ... }
}

// Usage at call site:
let schema: resource::Schema = ...;
let registry = resource::Registry::new();

// Bad
pub struct ResourceSchema { ... }
pub struct ResourceRegistry { ... }
```

This applies everywhere. If you're writing `FooBar` and `Foo` is a module, it should be `foo::Bar`.

### No Aliased Imports

Never rename imports to avoid conflicts. If two types have the same name, use the module path to disambiguate at the call site.

```rust
// Good
use crate::resource;
use crate::policy;

fn check(schema: &resource::Schema, rule: &policy::Rule) { ... }

// Bad
use crate::resource::Schema as ResourceSchema;
use crate::policy::Rule as PolicyRule;
```

The one exception: external crate types that have genuinely terrible names. But even then, prefer the module path.

### Import Style

Prefer importing the parent module, not individual types, when you're using more than one item from it:

```rust
// Good — clear where things come from
use crate::resource;

fn register(schema: resource::Schema) -> resource::Id { ... }

// Also fine — when you use only one thing, often
use crate::resource::Schema;

fn validate(schema: &Schema) -> Result<()> { ... }

// Bad — star imports
use crate::resource::*;
```

For standard library and well-known crate types (`HashMap`, `Vec`, `Result`, `anyhow::Context`), direct imports are fine. Everyone knows where `HashMap` comes from.

---

## Error Handling

### Use `thiserror` for Library Errors

Each subsystem defines its own error type:

```rust
// registry/error.rs
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("resource not found: {0}")]
    NotFound(String),

    #[error("dependency cycle detected: {0:?}")]
    CyclicDependency(Vec<String>),

    #[error("duplicate entry: {0}")]
    Duplicate(String),
}
```

### Use `anyhow` at the Binary Level

Binary crates and CLI tools can use `anyhow::Result` for top-level error handling. Library crates within the workspace should use typed errors.

### No `unwrap()` in Production Code

Use `expect("reason")` if a panic is genuinely the right response (invariant violation). Otherwise, propagate errors. Tests can use `unwrap()` freely.

---

## Struct Design

### Builder Pattern for Complex Construction

If a struct has more than 3-4 fields, especially optional ones, use a builder:

```rust
// Good
let config = pipeline::Config::builder()
    .name("daily-sync")
    .schedule(Schedule::interval(Duration::from_secs(300)))
    .resources(vec!["email::inbox", "calendar::events"])
    .mode(pipeline::Mode::Incremental)
    .build()?;
```

### `#[non_exhaustive]` — Use Sparingly

Don't reflexively annotate public enums or structs with `#[non_exhaustive]`. The friction it adds for downstream code (no struct literals, must use a constructor; no exhaustive matches, must include a wildcard arm) usually outweighs the speculative forward-compat benefit. A clear major-version bump is fine and usually clearer than a permanently-open type.

Reach for it only when both of these hold:

1. Code outside the crate will pattern-match on the type or construct it via struct literal, AND
2. We genuinely intend to grow variants/fields in patch or minor releases.

This should be rare.

For builder-pattern structs (`Config::new().theme(...)`), the builder is the forward-compat seam — `#[non_exhaustive]` is redundant. For codegen-targeted types (built by macros or build scripts), the codegen updates with the type — also redundant.

---

## Code Organization Within a File

```rust
// 1. Module-level doc comment

// 2. Imports (std, external crates, crate-internal — separated by blank lines)

// 3. Type definitions (structs, enums)

// 4. Trait implementations (Display, From, etc.)

// 5. Inherent implementations (impl Foo { ... })

// 6. Private helper functions

// 7. Tests (mod tests { ... })
```

---

## Concurrency

### Prefer `tokio` for Async

We use `tokio` as the async runtime for all async applications.

### Channels Over Shared State

When components need to communicate, prefer `tokio::sync::mpsc` channels over `Arc<Mutex<...>>`. Event buses are channels. Inter-component messages are channels. Shared mutable state is a last resort.

---

## Edition

All crates use the Rust 2024 edition:

```toml
[package]
name = "my-crate"
edition = "2024"
```

---

## Dependencies

### Be Conservative

Every dependency is a liability. Prefer:

1. Standard library
2. Well-maintained, widely-used crates (`serde`, `tokio`, `toml`, `thiserror`, `anyhow`, `tracing`)
3. Nothing else unless there's a strong reason

### Baseline Allowed Crates

| Crate | Purpose |
|---|---|
| `serde`, `serde_json` | Serialization |
| `toml` | Config parsing |
| `tokio` | Async runtime |
| `thiserror` | Library error types |
| `anyhow` | Binary error handling |
| `tracing`, `tracing-subscriber` | Structured logging |
| `clap` | CLI argument parsing |
| `hyper` | HTTP (prefer over `reqwest` — less magic, more control) |
| `jiff` | Date/time handling |
| `uuid` | Unique IDs |

Add to this list deliberately and with justification. If you need a crate not on the list, note it in the plan or PR.

---

## Testing

### Test Placement

Unit tests go in the same file as the code they test, in a `#[cfg(test)] mod tests` block. Integration tests go in `tests/`.

### Test Naming

```rust
#[test]
fn read_action_has_no_scope() { ... }

#[test]
fn policy_denies_irreversible_write_by_default() { ... }
```

Descriptive, reads like a sentence. No `test_` prefix (the `#[test]` attribute is sufficient).

---

## Documentation

Write every comment to the standard of the most senior engineer in the building —
the staff/principal who has earned the right to say less. That person never
narrates the code back to you, never leaves a diary of how the file got this way,
and earns trust precisely because every comment they *do* leave carries signal.
Comments explain **why**, in the present tense, describing the code as it stands
now. Everything below follows from that bar.

### Doc Comments on Public Items

Every public type, function, and module gets a doc comment. Keep it concise — one line if possible, a short paragraph if needed.

```rust
/// A registered resource's full schema, comprising identity,
/// interface, action classification, and requirements.
pub struct Schema { ... }
```

### No Redundant Comments

Don't comment what the code obviously does. Comment *why* when the reason isn't obvious.

```rust
// Bad
// Increment the counter
counter += 1;

// Good
// We skip the first entry because the coordinator's own
// invocation is always at index 0 in the dependency chain.
let deps = &chain[1..];
```

### No Process, Phase, or Archaeology Comments

A comment describes the code as it is *now* — never the journey that produced it.
The reader doesn't care which step of the plan added this, what it used to be, or
how you reasoned your way to it. That history lives in git and the PR; the comment
lives in the present. Strip three kinds before you commit:

**Chain-of-thought** — the algorithm narrated back to you. The code already says
*what* it does; restating the steps in prose is noise.

```rust
// Bad — narrating the steps the code already shows
// First load the user, then validate the token, then issue the session.
let user = load_user(id)?;
let token = validate(token)?;
let session = Session::issue(user, token);
```

**Phase / plan bookkeeping** — `// Phase 2:`, `// Step 3`, `// wire this up
later`, `// TODO from the migration`, `// placeholder for now`. The phases were
yours during implementation; they mean nothing to the next reader.

**Archaeology / changelog** — comments that describe a *change* rather than the
code:

```rust
// Bad — all archaeology; git already knows
// Changed from a Vec to a HashMap for O(1) lookup.
// Was previously inlined in handle_auth.
// Per review feedback, moved the guard up.
// NEW: keeping the old path around for now.
let index: HashMap<Id, Entry> = ...;
```

The test: **would the comment still make sense to someone who has never seen any
earlier version of this code and doesn't know it was ever different?** If it
leans on a before-state, a build phase, or "we used to…", it's archaeology — cut
it. A good comment reads as though the code was always this way, and quietly
explains the one non-obvious *why* that remains.
