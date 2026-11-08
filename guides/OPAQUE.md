# Opaque Types

A guide to encoding invariants in types instead of in convention. An *opaque type* hides its internals behind a constructor that enforces an invariant: no instance of the type exists without passing that check.

**Transform, don't create.** Every type worth having is minted from a transformation: raw input to validated value, loose parts to a bundle of proofs, request to ready-to-execute entity. When you catch yourself assembling a struct from loose parts, stop and name the transformation that produced the value.

This guide has three parts:

1. **Non-negotiables** — the hard rules every approach must satisfy.
2. **Approaches that violate them** — common smells, each with a stable name.
3. **Named approaches that satisfy them** — techniques bucketed by how often they're the right call. Ends with a decision guide for picking one.

### Citing by name

Every smell and every named approach in this guide has a **kebab-case id in backticks** next to its heading. Use the id verbatim in commit messages, PR descriptions, review comments, and MAP plans:

```
refactor(view): apply `smart-constructor-newtype` to source::Id
review: L42 is `proof-laundering` — see OPAQUE.md
plan: Step 3 promotes the grab-bag call signature via `proof-bundle`
```

The ids are stable across heading edits, greppable across the codebase, and unambiguous vs. loose English paraphrases.

---

## Non-negotiables

**N1. Validation must produce a type, not a boolean or `Result<(), E>`.**
If you check validity at the edge, the checked-ok result must be a different type than the input. `is_valid_email(&str) -> bool` loses the proof after the check. `validate_email(&str) -> Result<(), EmailError>` loses it too — the Ok branch is `()`, no evidence.

**N2. Proof doesn't leak out of its type.**
No `into_inner() -> String`. No `impl Deref<Target = String>` on a newtype whose whole point is that it *isn't* just a String. No public fields on any struct whose invariants matter.

**N3. Errors carry structured context.**
`Result<T, String>` makes the caller pattern-match on bytes. Use `#[derive(thiserror::Error)]` enums, or types that carry what the handler needs (request id, responder, the rejected value).

**N4. Functions with 4+ scalar parameters or >2 underscores in the name are smells.**
Four `String`s in a signature say "bundle me into a validated type." `insert_object_for_undo_with_history_recording(edit, doc, history, ctx, ...)` says "some of these should have been one type; there's a transformation hiding in the name." A `bool` parameter whose meaning the call site doesn't show is the same smell at its smallest: make it an enum.

**N5. A type with no fields is not a type — it's a namespace.**
`struct Foo; impl Foo { pub fn parse(x) -> Y }` is `fn parse_foo(x) -> Y` with extra syntax. If the type has no invariants to hold, move its methods onto the type that does.

Everything below is *how* to satisfy N1–N5 in practice.

---

## Approaches that violate the non-negotiables

Named so you can cite them in reviews.

### The boolean check — `boolean-check`

```rust
fn is_valid_name(s: &str) -> bool;
if !is_valid_name(&name) { return Err(...); }
do_thing(&name);
```

Violates **N1**. After the check, `name` is still `&str`. Anyone in another call site skips the check and nothing complains.

The same smell inside one expression is `if x.is_some() { use(x.unwrap()) }`. Match instead (`if let Some(v) = x`), so the pattern is the proof.

→ `smart-constructor-newtype`.

### The Result validator — `result-validator`

```rust
fn validate_name(s: &str) -> Result<(), NameError>;
validate_name(&name)?;
do_thing(&name);
```

Violates **N1**. `?` forces the caller to handle errors but `name` stays `&str`. The validator returned `()` — no evidence of what it proved.

→ `smart-constructor-newtype` (`Name::new(raw) -> Result<Name, NameError>`).

### Proof-laundering — `proof-laundering`

```rust
impl Name {
    pub fn into_string(self) -> String;
}
impl Deref for Name { type Target = String; /* ... */ }
```

Violates **N2**. Once the caller holds `String` again, they pass it to any fn expecting `&str` and the proof is gone. `Deref` does this silently — no explicit method call.

→ Expose `as_str(&self) -> &str` for reads. Only expose the inner value at genuine boundaries (serialization, FFI) and mark them as exceptions.

### The validate-then-use split — `validate-then-use-split`

```rust
fn process(raw: &str) -> Result<(), Err> {
    validate(raw)?;
    do_thing(raw);
    Ok(())
}
```

Violates **N1**. The check and the use are *adjacent* but not *linked*. Someone moves `do_thing(raw)` to a different caller that skips the check.

→ Promote `raw` to a type; adjacency becomes enforcement.

### The grab-bag function signature — `grab-bag-signature`

```rust
fn handle_register(&mut self, req: &HttpRequest, responder: Responder, ...) -> ... {
    let email = req.body().get("email")...;
    if !is_valid_email(&email) { return Err(...); }
    let password = req.body().get("password")...;
    // ... 40 more lines of extract, check, extract, check
}
```

Violates **N4** and **N1**. The extract-and-check pattern repeating across parameters is a transformation struggling to be a type.

→ `proof-bundle` + `transformation-method`.

### Error as String — `string-error`

```rust
fn do_work() -> Result<Output, String>;
```

Violates **N3**. Handlers can only pattern-match on text; new error cases silently shift semantics when messages get edited.

→ `structured-error`.

### Public fields on an invariant-bearing struct — `public-invariant-fields`

```rust
pub struct Signup {
    pub email: String,
}
impl Signup { pub fn validate(&self) -> Result<(), Err>; }
```

Violates **N2**. Anyone writes `signup.email = "".into()` and bypasses the invariant. `pub` on an invariant-bearing field is a promise the code doesn't keep.

→ Private field, typed inner (`email: Email`), accessor for reads.

### The thin namespace type — `thin-namespace-type`

```rust
pub struct RegisterUser;
impl RegisterUser {
    pub fn parse(req: &HttpRequest) -> Result<Signup, Rejection>;
}
```

Violates **N5**. `RegisterUser` is `()` with methods. No fields, no invariants. Semantically identical to `fn parse_register_user(req) -> ...` with extra characters.

→ The transformation belongs on `Signup`: `Signup::from_request(req) -> Result<Self, Rejection>`.

### The multi-underscore function name — `multi-underscore-name`

```rust
fn insert_object_for_undo_with_history_recording(edit, doc, history, ctx, ...);
fn apply_user_account_update_from_form_data(email, password, age, ...);
```

Violates **N4** and **N5**. The name is describing a multi-step transformation because no type exists to express the state being moved through.

→ Find the noun in the verb. `insert_object_for_undo` → `UndoEntry::apply(self, doc)`. `apply_user_account_update_from_form_data` → `AccountUpdate::from_form(form)` + `update.apply(...)`.

---

## Named approaches that satisfy the non-negotiables

Three buckets by how often each is the right call. **Strong prior: 80% of opaque-type work is the four *core* approaches below.** The rest is occasional sugar or niche machinery.

### Core — reach for these first

#### Smart-constructor newtype — `smart-constructor-newtype`

```rust
pub struct Email(String);

impl Email {
    pub fn new(raw: String) -> Result<Self, EmailError>;
    pub fn as_str(&self) -> &str;
}
```

One field, one validating constructor, read-only accessor. The constructor is `new`, not `try_new`, because the checked constructor is the only one. `TryFrom<String>` is a common equivalent:

```rust
impl TryFrom<String> for Email {
    type Error = EmailError;
    fn try_from(raw: String) -> Result<Self, EmailError> { /* ... */ }
}
```

Satisfies **N1**, **N2**.

**When to use:** any scalar value with validity rules (email, URL, ID format, bounded integer, non-empty collection).

**When not to use:** values with no invariants. Don't wrap `String` in a newtype just to give it a name — `title: String` stays `String` when any string is a valid title.

#### Proof bundle — `proof-bundle`

```rust
pub struct Signup {
    email: Email,
    password: Password,
    age: Age,
    display_name: String,
}
```

A struct whose fields are already-validated opaque types. The outer struct's existence is proof that every inner invariant holds. Private fields, accessors for the subset readers need.

Satisfies **N1**, **N2**, **N4**.

**When to use:** when a function would otherwise take 4+ scalar parameters. When "these N things travel together" is a concept in the domain.

**When not to use:** when the grouped fields don't actually travel together. If callers sometimes hold `email` without `password`, don't bundle them.

#### Transformation method — `transformation-method`

```rust
impl Signup {
    pub fn from_request(req: &HttpRequest, responder: Responder) -> Result<Self, Rejection>;
    pub fn submit(self, users: &mut UserStore) -> Task<()>;
}

impl Rejection {
    pub fn emit(self) -> Task<()>;
}
```

Read aloud: *"An HttpRequest and a Responder can be transformed into a Signup (or a Rejection). A Signup can be transformed into a scheduled submission. A Rejection can be transformed into a scheduled error reply."*

Three properties make this work:

- **Constructor arguments are the inputs the type is minted from.** Signup needs `HttpRequest` + `Responder`; that's what its constructor takes.
- **The type carries forward what it needs to finish.** Signup holds the `Responder` it was minted with, so `submit()` doesn't need it as an argument.
- **`self` is consumed on execution.** Once `.submit()` runs, the Signup is gone. No double-submission.

Satisfies **N1**, **N4**, and contributes to **N3** when the error branch (`Rejection`) is its own type.

**When to use:** lifecycles — something is created from inputs, carries state, and is eventually consumed. HTTP handlers, builders, transactions, any `open → use → close` sequence.

**When not to use:** data-only types with no lifecycle. `Color { r, g, b }` doesn't need `.display_on_screen(self)` — it's passive data.

#### Structured error — `structured-error`

```rust
#[derive(Debug, thiserror::Error)]
pub enum RegistrationError {
    #[error("email has invalid shape: `{0}`")]
    BadEmailShape(String),
    #[error("password must be at least 12 characters")]
    PasswordTooShort,
    #[error("minimum age is 13, got {0}")]
    UnderAge(u8),
}
```

Or a struct carrying everything a handler needs to emit the error:

```rust
pub struct Rejection {
    request_id: RequestId,
    responder: Responder,
    reason: RejectionReason,
}

impl Rejection {
    pub fn emit(self) -> Task<()>;
}
```

Satisfies **N3**.

**When to use:** every fallible library function.

**When not to use:** `anyhow::Result` at the binary level is fine for top-level plumbing; everywhere else, typed errors.

### Occasional — reach when the shape matches

#### Generic trait with associated type — `associated-type-trait`

```rust
pub trait Resolver {
    type Key;
    fn get(&self, key: &Self::Key) -> Option<&Source>;
}
```

Modeled after `iced::Widget<M, T, R>` / `std::iter::Iterator<Item = T>`. The trait commits to the *shape* of the interaction, not to the identities of the types involved. Implementations each pick their own `Key` / `Item` / etc.

**When to use:** trait consumers genuinely vary in what they produce or accept (different renderers, different iterator element types, different lookup key types). The trait expresses the *contract*; impls express the *specifics*.

**When not to use:** there's really only one implementation, or the variability is speculative ("maybe someday we'll have another kind..."). A generic trait with one impl is machinery for nothing — collapse to a concrete type.

#### Default generic parameter — `default-type-parameter`

```rust
pub struct Infolet<K = source::Id> {
    source: K,
    // ...
}
```

Parameterize over a type that today has one value but tomorrow might have two. The default keeps existing call sites compiling unchanged.

**When to use:** you *know* a second parameterization is coming (next wave, next feature). Rust's default type parameters on structs are stable and cost nothing.

**When not to use:** prematurely. If you don't have a concrete second case in mind, don't parameterize. `Infolet<K = source::Id>` with no non-default call site anywhere is just noise.

#### `#[must_use]` on handles — `must-use-handle`

```rust
#[must_use = "a Signup must be submit()'d or explicitly dropped"]
pub struct Signup { /* ... */ }
```

The lint catches the "built it and forgot it" case:

```
warning: unused `Signup` that must be used
  = note: a Signup must be submit()'d or explicitly dropped
```

Combined with `self`-consuming methods, the lifecycle becomes **minted → consumed**. The compiler physically prevents middle-state bugs.

**When to use:** types whose caller must take a terminal action (submit, commit, close, emit).

**When not to use:** plain data types. `#[must_use]` on `Color` would just be annoying.

### Niche — almost never the right tool

These two patterns come up often in guides and rarely in real code. The shapes they fit are narrow. Reach for them only when your problem *obviously* matches; otherwise a simpler approach above is correct.

#### Sealed trait + marker type — `sealed-trait-marker`

```rust
mod sealed { pub trait Sealed {} }
pub trait Kind: sealed::Sealed {}

pub struct Http;  impl sealed::Sealed for Http {}  impl Kind for Http {}
pub struct Sheet; impl sealed::Sealed for Sheet {} impl Kind for Sheet {}
// Downstream crates cannot add impls.
```

**When to use:** you *absolutely* need downstream code unable to add new kinds, because your own exhaustive matches depend on it. Genuinely rare; mostly shows up in library-design edge cases (`std::error::Error`-adjacent bounds, serde internals, sqlx's typestate machinery).

**When not to use:** an enum would work. `#[non_exhaustive] enum Kind { Http, Sheet }` is simpler, pattern-matchable, and extensible with fewer moving parts. "Closed set" is almost always satisfied by "an enum whose module I own."

#### Phantom typestate — `phantom-typestate`

```rust
pub struct NeedsUrl;
pub struct Ready;

pub struct RequestBuilder<S> {
    url: Option<String>,
    _state: std::marker::PhantomData<S>,
}

impl RequestBuilder<NeedsUrl> {
    pub fn url(self, u: String) -> RequestBuilder<Ready> { /* ... */ }
}

impl RequestBuilder<Ready> {
    pub fn send(self) -> Response { /* ... */ }
}
```

The caller advances the state; `send()` only exists on `Ready`. Calling `send()` on `NeedsUrl` is a compile error, not a runtime panic.

**When to use:** caller-driven *multi-step protocols* where each state has a distinct method surface and getting the order wrong is a real bug worth catching at compile time. Builder protocols with complex required-field combinations, handshakes, transactions with explicit commit/rollback.

**When not to use — i.e., most of the time:**

- **Static kind tagging.** If you want to distinguish "app pool" from "doc pool" at the type level and neither ever *transitions* into the other, typestate is ceremony. Use `associated-type-trait`, or two concrete structs, or an enum — whichever fits the domain.
- **External state changes.** If the state can change because of something outside the caller's control (peer closes a connection, file gets deleted, timer fires), the caller doesn't know what state the type is in. Pattern-match an enum instead.
- **Builders where "forgot a required field" is a soft lint concern.** `must-use-handle` + a fallible `build()` is usually enough. Typestate builders earn their keep when the required-field matrix is complex enough that compile-time enforcement pays back the ceremony.

---

## Decision guide

Pick a technique from a symptom:

| Symptom | Approach | Id |
|---------|----------|-----|
| Scalar value has validity rules | Smart-constructor newtype | `smart-constructor-newtype` |
| Fn takes 4+ primitives that arrived together | Proof bundle | `proof-bundle` |
| Multi-step "create → do stuff → finish" lifecycle | Transformation method | `transformation-method` |
| Fallible operation | Structured error | `structured-error` |
| Caller must not forget to finish the lifecycle | `#[must_use]` on the type | `must-use-handle` |
| Fn name has >2 underscores | Extract the hidden type | (smell: `multi-underscore-name`) |
| Trait consumer varies in output/key/element type | Generic trait with associated type | `associated-type-trait` |
| Same type used with different parameterizations over time | Default generic parameter | `default-type-parameter` |
| Two pools / stores / sinks that differ only in insert rules | Two concrete structs, each with its own methods — **not** `phantom-typestate` or `sealed-trait-marker` | — |
| Need a closed trait-implementer set downstream can't extend | Sealed trait *(niche)* | `sealed-trait-marker` |
| Multi-step protocol with ordering-as-correctness | Phantom typestate *(niche)* | `phantom-typestate` |

**Strong prior:** 80% of real opaque-type work is the four core approaches above. The occasional bucket is sugar. The niche bucket is specialist machinery for specific shapes — if your problem doesn't obviously match, don't reach for it.

---

## Examples in the wild

- **`std::fs::File`** — only obtainable via `File::open` / `create`. Drop closes the OS handle. You cannot hold a File that points at nothing.
- **`Pin<P>`** — encodes "this value cannot be moved" in the type. Zero runtime cost.
- **`Result<T, E>`** — the simplest opaque type. You cannot use `T` without pattern-matching.
- **`iced::Widget<M, T, R>`** — generic over renderer; the widget knows nothing about what rendering is. (`associated-type-trait`.)
- **`std::iter::Iterator<Item = T>`** — trait with associated type. Same pattern, different surface. (`associated-type-trait`.)
- **`mpsc::Sender` / `mpsc::Receiver`** — fan-in-to-one semantics in the types; Sender is `Clone`, Receiver isn't.
- **`sqlx::query!`** — macro checks the query against the DB at compile time. Invalid SQL doesn't compile.

---

## Where to start

Grep for the smells:

```
rg 'fn (is_valid_|validate_|check_|ensure_|assert_)'    # boolean-check, result-validator
rg 'fn [a-z_]*_{3,}'                                    # multi-underscore-name
rg 'pub [a-z_]*: String'                                # public-invariant-fields candidates
rg 'Result<.*, String>'                                 # string-error
rg 'impl Deref.*for [A-Z]'                              # proof-laundering
rg 'fn into_[a-z_]+\(self\) -> String'                  # proof-laundering
```

Every hit is a candidate. Pick the one you touch most often, promote its primitives into newtypes one at a time, commit, repeat.

---

## Further reading

- "Parse, Don't Validate" — https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/
- "Use Opaque Types in Elm" — https://dev.to/hecrj/use-opaque-types-in-elm-3oal
