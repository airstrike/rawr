# Rust Style Guide

Sources: the Rust API Guidelines, Google's Rust style guide and this project's own practice. Where this guide departs from a common convention on purpose, the rule says so.

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

### A Module With Children Holds Only Declarations

A module with child modules is a table of contents. It holds `mod` declarations, `pub use` re-exports and at most a little code that ties the children together. Everything else lives in a leaf module.

Tying code is code every child shares: the imports the children use, and, when the children all extend one type, that type's definition and its constructors. A method that serves one concern is not tying code, however short it is.

```rust
// engine.rs: good
mod policy;
mod registry;
mod scheduler;

pub use registry::Registry;
pub use scheduler::Scheduler;
```

```rust
// engine.rs: bad. The scheduler's logic sits beside the declarations.
mod policy;
mod registry;

pub struct Scheduler { ... }

impl Scheduler {
    fn tick(&mut self) { ... }
}
```

A parent declares its children and never defines them inline (`mod scheduler { ... }`). A test module doesn't make a module a parent, whether it is inline or in its own file. A parent may `pub use` items from its children so that callers never name the children, which then stay an implementation detail.

Why: a reader finds a concern by reading the parent's list of modules, never by scrolling the parent. Structure and content stay apart.

### One Abstraction Per Leaf Module

A leaf module holds one abstraction. Usually that is one primary type and its closely related types. "Closely related" means types that exist only to serve the primary type: enum variants, configuration structs, small helpers.

```rust
// action.rs: good. Scope only exists as part of Action.
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

A leaf may instead hold one concern with no primary type: a group of functions, or a group of one type's methods, that do one job. `pattern`, `coverage` and `dispatch` are concerns.

Name a leaf module after its primary item, so `Action` lives in `action.rs`. A leaf with no primary item is named after its concern. Either way the file name tells you what the file holds.

Never write a grab-bag module. `util`, `helpers`, `common`, `misc` and `types` name no concern, so anything fits in them and eventually everything goes there. Put each item in the module of the concern it serves, or give it a module named for its own concern.

When in doubt: if a type could conceivably be imported independently by code that doesn't care about the parent type, it deserves its own module.

### Split One Type's Methods With Plain `impl` Blocks

When one type's methods span several concerns, the type's definition stays in one module and its methods spread over leaf modules, one concern each, as plain `impl Type { ... }` blocks. A type may have any number of inherent `impl` blocks anywhere in its crate.

```rust
// parser/pattern.rs: good
use super::*;

impl Parser<'_> {
    fn parse_pattern(&mut self) -> Result<pattern::Syntax, Error> { ... }
}
```

```rust
// parser/pattern.rs: bad. A trait that exists only to carve up the parser.
trait PatternLowering {
    fn parse_pattern(&mut self) -> Result<pattern::Syntax, Error>;
}

impl PatternLowering for Parser<'_> { ... }
```

Never introduce a trait to organize code (see "Define a Trait Only for Generic Code"). Inherent impls can't leave their type's crate, so this kind of split makes modules, not crates.

### When a Module Needs Splitting

A module needs splitting when it holds more than one abstraction. The test is whether a reader can name its concern in a few words. "Patterns and their bindings" passes. "Patterns, coercions and format strings" fails, and so does a module that only a list can describe.

Line count is not the test. A long module with one concern stays whole. A short module with two concerns splits. Length is still a reason to look, because a file too long to skim usually holds several concerns.

A new concern gets a new leaf module from the start. It never grows a parent, and it never moves into a neighbour's module because that module was close by.

### How to Split a Module

Split by concern. Each concern becomes a leaf module named after its primary item or its concern. The parent shrinks to declarations, re-exports and its tying code.

Move code as a pure move. Cut and paste it, changing only the visibility and imports the move needs. Commit the move on its own, with no change to behaviour or names in the same commit. A reviewer then checks a move by diffing it, and no change hides inside a move.

When a split spans many commits, write a map before the first move. The map names each target module, its concern and the symbols it holds. It names symbols, never line numbers, because line numbers drift. It also states the parent's end state: declarations, re-exports and the tying code, named item by item. A map that only names regions to move leaves the bulk behind in the parent, because nothing says the parent isn't finished. `guides/LOWER_NOMINAL_MAP.md` is such a map.

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

This departs on purpose from guides that import types and write them bare: the module path carries the meaning a composite name would, and a bare `Schema` in a file that touches three kinds of schema says nothing.

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

This departs on purpose from guides that allow `use std::io::Error as IoError`: an alias gives one type a second name, so a search for the type misses the uses written under the alias.

### Std Naming Conventions

Follow the standard library's names, so a reader can guess a method's cost and ownership from its name.

- `as_foo` is a cheap borrow, `to_foo` is an expensive or owned conversion, and `into_foo` consumes `self`.
- `foo()` returns `&T` (or `T` if it is `Copy`), and `foo_mut()` returns `&mut T`.
- `set_foo(value)` sets, `replace_foo(value)` sets and returns the old value, and `take_foo()` clears and returns the old value.
- A type with `len()` also has `is_empty()`.
- A constructor is `new`. With several, the others are `with_foo` or `from_foo`.

A constructor that validates is `new` returning `Result`, and the same holds for `TryFrom`. This departs on purpose from `try_new`: when the checked constructor is the only way to make the type, there is no unchecked `new` to tell it apart from.

```rust
// Good
impl Email {
    pub fn new(raw: String) -> Result<Self, email::Error> { ... }
}

// Bad: there is no infallible new for try_new to differ from.
impl Email {
    pub fn try_new(raw: String) -> Result<Self, email::Error> { ... }
}
```

### Lifetimes and Generic Parameters

Give a lifetime a name that says what it borrows from (`'source`, `'tcx`), especially on a type. Use `'_` where elision allows it. A one-letter lifetime is fine for a simple tie from an input to an output.

A generic parameter is short and hints at its main bound (`I: Iterator`, `F: Fn()`). Use `T` only when nearly any type fits.

### No Unfamiliar Abbreviations

Spell words out. Never drop letters inside a word (`cnt`, `mgr`, `ctx_mgr`). An abbreviation is fine when a newcomer to the field already knows it (`Ty` in a compiler, `IpAddr`).

---

## Imports

### Import Style

Prefer importing the parent module, not individual types, when you're using more than one item from it:

```rust
// Good: clear where things come from
use crate::resource;

fn register(schema: resource::Schema) -> resource::Id { ... }

// Also fine: when you use only one thing, often
use crate::resource::Schema;

fn validate(schema: &Schema) -> Result<()> { ... }

// Bad: star imports
use crate::resource::*;
```

For standard library and well-known crate types (`HashMap`, `Vec`, `Result`, `anyhow::Context`), direct imports are fine. Everyone knows where `HashMap` comes from.

### `use super::*` Only in a Parent's Extension

A glob import is allowed in two places. A module that only extends its parent's type with `impl` blocks, and defines no types of its own, may `use super::*;`. A test module may `use super::*;`. Every other module imports what it uses by name.

```rust
// parser/loops.rs: good. It only adds methods to Parser.
use super::*;

impl Parser<'_> { ... }
```

```rust
// parser/coverage.rs: it defines coverage::Key, so it imports explicitly.
use super::Parser;
use crate::resolve::ty::Ty;

pub(super) struct Key { ... }
```

Why: an extension is a piece of its parent cut along a concern, so it sees what the parent sees. A module with types of its own is an abstraction, and its imports say what it depends on.

### One Module Per `use`

Each `use` names one module path. Never nest a path inside braces.

```rust
// Good
use crate::resolve::pattern;
use crate::resolve::ty::{Int, Ty};

// Bad
use crate::resolve::{pattern, ty::{Int, Ty}};
```

Why: each line adds or removes one dependency, so diffs stay one line per change and a search for a module path finds every import of it.

### Import a Trait Where Its Methods Are Called

Import a trait inside the function that calls its methods, unless the whole file uses it.

```rust
fn write_header(out: &mut String, name: &str) {
    use std::fmt::Write;
    writeln!(out, "// {name}").expect("writing to a String can't fail");
}
```

Why: the import sits next to the call it enables. In a long module a top-of-file trait import is far from its one use, and it survives after the use is gone.

### Call Free Functions Through Their Module

Import a free function's module and call the function through it. This holds for the standard library too.

```rust
// Good
use std::mem;
let old = mem::take(&mut self.pending);

// Bad
use std::mem::take;
let old = take(&mut self.pending);
```

Why: an unqualified lowercase call then always means a function of the current module.

---

## Ownership and Borrowing

### Borrow, Don't Clone

Call `.clone()` only when the code needs a second owned value. A clone that stands in for a borrow is a defect: it allocates and copies, and it hides who owns the value. When the borrows get hard, restructure first. Narrow a scope, borrow two fields apart, or return a reference.

```rust
// Good
fn label(item: &Item) -> &str { &item.name }

// Bad: copies the string so the caller doesn't have to hold a borrow.
fn label(item: &Item) -> String { item.name.clone() }
```

Cloning an `Rc`, an `Arc` or a `Copy` value is not a deep copy and is fine.

### Unique Ownership Over `Rc` and `Arc`

Use `Rc` or `Arc` only when unique ownership and borrows would be unreasonably complex. Reference counting costs at runtime and hides which part of the program owns the value.

### Accept the Most General Borrow

| Take | Instead of |
|---|---|
| `&[T]` | `&Vec<T>` |
| `&str` | `&String` |
| `&T` | `&Box<T>` |
| `Option<&T>` | `&Option<T>` |

Why: callers pass what they hold without converting or copying.

---

## Functions

### Enums Over Unexplained Bools

When a `bool` parameter's meaning isn't obvious at the call site, make it an enum.

```rust
// Good
lower(expr, Place::Moved, Position::Statement);

// Bad: what do true and false mean here?
lower(expr, true, false);
```

A `bool` whose meaning the call already shows (`set_visible(true)`) stays a `bool`.

---

## Types and Traits

### Define a Trait Only for Generic Code

Define a trait only to bound a generic parameter, to accept or return `impl Trait` or `dyn Trait`, or to add methods to a type from another crate. Everything else is an inherent `impl`.

Why: a trait announces an abstraction with several implementors. A trait with one implementor sends every reader looking for the others. OPAQUE's `associated-type-trait` says the same: one impl means collapse it.

### Newtype or Alias

Write `type Foo = Bar;` only when `Foo` is a shorter name for exactly `Bar`. Write a newtype when the meaning differs, when values are easy to mix up (a line and a column), when the type needs methods of its own, or when its representation may change. OPAQUE's `smart-constructor-newtype` covers newtypes that hold an invariant.

### Named Fields Over Tuples

Prefer named fields to tuples and to tuple structs with more than one field. `.0` and `.1` say nothing where they are read. A one-field newtype is fine, and so is a tuple that lives for a few lines.

### Std Traits Over Look-Alike Methods

If a method does what a standard trait does, implement the trait: `impl FromStr`, not `fn from_str`, and `impl Default` beside a `new()` that takes no arguments. Derive what can be derived. Add other trait impls when a use needs them, not in advance.

### `#[must_use]` on Values Worth Reading

Mark a function `#[must_use]` when ignoring its result is always a mistake: a pure function, or one whose result reports an error. Mark a type `#[must_use]` when it is a lazy computation or an error. A function that returns `Result` or another `#[must_use]` type needs no attribute. OPAQUE's `must-use-handle` covers lifecycle handles.

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

### Option, Result or Panic

Return `Option` when a failure has one obvious cause (`checked_add`). Return `Result` when the caller needs to know what went wrong. Panic only for a bug in this program: a broken invariant that has to be fixed in the code. Bad input, from a user, a file or a program being compiled, is never a panic.

### No `unwrap()` in Production Code

Use `expect("reason")` if a panic is genuinely the right response (invariant violation). Otherwise, propagate errors. Tests can use `unwrap()` freely.

The `expect` message says why the value is there, not that it is missing.

```rust
// Good
let frame = self.frames.last().expect("parse_body pushes a frame before any statement");

// Bad: restates the failure.
let frame = self.frames.last().expect("no frame");
```

### Match, Don't Check Then Unwrap

Use `if let`, `let ... else` or `match` instead of checking a value and then unwrapping it.

```rust
// Good: the pattern proves the value is there.
let Some(field) = fields.get(name) else {
    return Err(Error::UnknownField(name.to_owned()));
};

// Bad: correctness rests on a check the compiler doesn't link to the unwrap.
if fields.get(name).is_none() {
    return Err(Error::UnknownField(name.to_owned()));
}
let field = fields.get(name).unwrap();
```

### Assertion Messages Say Why It Holds

An assertion's message says why the condition holds, and so whether a failure is the caller's misuse or a bug here. Put the reason in the message, not in a comment beside it, because the message reaches the log.

```rust
// Good
assert!(spans.is_sorted(), "spans come from the token stream, which is in source order");
assert!(self.stream.is_some(), "call connect() before running queries");

// Bad: restates the condition.
assert!(spans.is_sorted(), "spans should be sorted");
```

---

## Expressions

### Annotate Types the Reader Can't See

Annotate a `let` whose type the right-hand side doesn't show: after `.into()`, `.parse()`, `.collect()` and long iterator chains. Put the type on the `let` rather than in a turbofish. If a turbofish is needed anyway, `_` inside it is fine.

```rust
// Good
let ids: Vec<source::Id> = entries.iter().map(Entry::id).collect();
let port: u16 = text.parse()?;

// Bad
let ids = entries.iter().map(Entry::id).collect::<Vec<_>>();
let port = text.parse()?;
```

Why: code is often read in a diff, a search result or a terminal, with no editor to show the inferred type.

### Loops When Clearer Than Combinators

Use combinators for short chains, and where an adaptor carries the logic (`.any(|v| v > limit)`). Use a loop when the chain would branch, mutate state outside it or run long. Never build a chain in a `for` header. Bind it to a named variable and loop over that. Don't use `for_each` without a strong reason.

### `format!` and `write!` for Strings

Join more than two pieces with `format!` or `write!`, not `+`. Fill a template with `format!`, not `str::replace`.

```rust
// Good
let line = format!("{indent}{name}: {ty}");

// Bad
let line = indent.to_owned() + name + ": " + &ty;
```

Why: one allocation, and no home-made template syntax.

---

## Formatting

Format with `rustfmt` and the repo's `rustfmt.toml`. Use `#[rustfmt::skip]` only for deliberate alignment (rows of similar lines, a matrix) or generated code, never for taste.

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

This departs on purpose from the struct of public fields filled with `..Default::default()`: public fields can't hold an invariant (OPAQUE N2), and a builder's `build()` can check how the fields combine.

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

## Macros and Generated Code

Reach for the simplest tool that works, in this order:

1. functions, types, traits and modules;
2. `macro_rules!`;
3. a code generator (a build script or a separate tool);
4. a procedural macro.

Why: each step down is a dialect that readers, editors and search understand less well. A code generator is the right tool when the generated code is itself the product.

---

## Unsafe, Unstable and Startup

- Use `unsafe` only when nothing safe can do the job. Wrap it in a safe abstraction, kept apart from the logic that uses it. Every `unsafe` block gets a `// SAFETY:` comment that says why it is sound.
- Use stable Rust. A crate that can't exist without an unstable feature (a compiler driver needs `rustc_private`, a bench needs `test`) enables it at its crate root and nowhere else.
- No code runs before `main`. No `ctor`-style initializers.
- Make a public function `const fn` only if it will stay `const`, because removing `const` breaks callers.
- An item another crate marks `#[doc(hidden)]` is private to that crate. Don't use it.

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

Unit tests go in the same file as the code they test, in a `#[cfg(test)] mod tests` block. When a module's tests outgrow its file, they move to a sibling file declared as `#[cfg(test)] mod foo_tests;`. Integration tests go in `tests/`.

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

A doc comment says something the name doesn't. `/// Removes an element.` on `remove_element` is worse than none. Always document what makes a function panic, and the invariants an `unsafe` function or trait asks its caller to keep.

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

This covers a TODO that links an issue, such as `// TODO(#123)`. It departs on purpose from guides that allow one: open work lives in the issue tracker, where it gets closed, and a TODO in the code goes stale without anyone noticing.

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
