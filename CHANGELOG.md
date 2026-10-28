# Changelog

Each merge to main bumps the patch version. A minor version marks a milestone.

## Unreleased

- The next milestone is every iced example, and every example in `examples/`, running under `rawr run` through importer-generated bindings only.
- rawr-iced is being retired step by step, so that removing it ends up a no-op.
- rawr's own crates compile once per machine, not once per project. The first `rawr run` on a toolchain compiles them into `runtime/` in rawr's home, and every project's runner links that copy. A second new project compiled no rawr crate and ran in about 13 s. Before, each project spent about 85 s compiling rawr. A script with dependencies reuses the same copy.
- The copy is keyed by rawr's version, sources and lockfile, the compiler's `rustc -vV`, and the runtime's features and profile. Nothing about a project is in the key, so projects with different dependencies share it. Copies unused for a day go, apart from the three used last.
- A runner that can't link the shared copy, such as one whose crates link a native library that rawr's do, compiles rawr in the project as before, and rawr says so once. iced apps on rawr-iced still compile rawr in the project.
- `RAWR_RUNTIME_OPT_LEVEL` compiles the `rawr` crate at a lower opt-level, such as `0`, for a faster first compile and slower scripts. The default stays 3.
- A native stream is bound through rawr's own `host::stream::Source` trait, not `futures_core::Stream`, so a project's copy of `futures-core` never has to be rawr's.

## 0.3.3 (2026-10-28)

- A `u64` or `usize` above `i64::MAX`, such as `u64::MAX`, crosses into and out of a crate's functions. Before, it stopped with "-1 is out of range for `u64`".
- `std::process::id()` gives the process's id.
- Upstream iced's `pick_list` example runs under `rawr run` without rawr-iced, as `cargo run` draws it.
- A type alias that passes its parameters in order, such as iced's `Text`, gives the aliased type's associated functions, so `text!(..)` works.
- A crate trait's method implemented for `&str` can be called on a rawr string.
- A lazy iterator can go where a crate function takes an iterable, and a slice of script values where it takes a slice of a type parameter.

## 0.3.2 (2026-10-28)

- A cargo script that declares dependencies in its frontmatter runs under `rawr run`, and prints what it prints under `cargo -Zscript`. Its manifest and lockfile live in `scripts/` in rawr's home, keyed by the script's path, and nothing is written beside the script.
- Every script builds in one build directory with your own `cargo`, so a crate one script compiled is reused by the next. A second script that shares a dependency started in about 10 seconds where the first took 100.
- What cargo refuses in a script's manifest, such as `[workspace]` or `[lib]`, is refused with cargo's message at its line.
- A crate or script with two dependencies that share a version requirement, such as `itoa = "1"` and `semver = "1"`, runs. Before, rawr dropped the second and stopped with "selected package name changed".

## 0.3.1 (2026-10-28)

- A path dependency inside the project's workspace compiles once for `cargo build` and `rawr run` together. Before, the runner compiled its own copy.
- rawr's nightly build, which reads a project's dependencies, is a check build. It writes only the metadata rawr reads, so it does no code generation for the project's crates.

## 0.3.0 (2026-10-28)

This milestone starts shared bindings: rawr builds bindings, and the project's own cargo builds everything.

- A crate with Cargo dependencies runs under `rawr run` with no `rust-toolchain.toml` and no `.cargo/config.toml`. The project's own `cargo` builds rawr's runner with the project's toolchain, config and profiles, in the project's `target/`. Rust 1.95 or newer is enough.
- `cargo build` after `rawr run` recompiles nothing. `rawr run` after `cargo build` compiles only rawr's own crates and the runner.
- An older toolchain stops `rawr run` with a message that says to update it.
- rawr's nightly build, which reads a project's dependencies, runs in `target/rawr` and no longer compiles rawr's own crates. A project's first `rawr run` through rawr-import takes about half as long.
- `rawr new --template iced` writes only `Cargo.toml` and `src/main.rs`.
- rawr asks smallvec for no features, so a project that uses smallvec without features keeps its one copy.
- `rawr run` no longer deletes the project's own binary when the runner takes the project's crate name.
- rawr-import's runner no longer builds a second `unicode-ident`, which made a project's proc macros build twice.

## 0.2.9 (2026-10-27)

- Upstream iced's `checkbox`, `slider` and `tooltip` examples run under `rawr run` without rawr-iced, with iced's own titles and theme.
- An `Option` of a closure, and a crate's function passed where a closure or function type goes, such as `.style(checkbox::primary)`, work.
- A crate function that returns `impl Trait`, such as `iced::application(..)`, runs, and its value takes the trait's methods.
- A number literal passed to an `impl Into<T>` parameter takes the type `T`, as in Rust.
- A struct that derives `Default` may hold a field of a crate's type.

## 0.2.8 (2026-10-27)

- The `// rawr` marker may follow a shebang line and a cargo-script frontmatter block. rawr parses such a file with its lines and columns kept.
- A `.rs` file with frontmatter is a script wherever it sits, as cargo reads it. A script that declares dependencies stops with an error saying rawr doesn't resolve them yet.

## 0.2.7 (2026-10-27)

- Upstream iced's `counter` example runs under `rawr run` without rawr-iced, through generated bindings only. It has iced's own title and theme, and a save applies at the next event with the count kept.
- A type that only a trait's default parameters and a crate's impls decide, such as the renderer behind `impl Widget<Message>`, is worked out from the crate's own impls.
- A crate's type alias that is the only name of its type, such as `iced::Renderer`, works.
- Generated code names crate types through the crate's own aliases, so iced's `text(..)` works.
- A trait that a dependency's macro reaches through `$crate` is in scope.
- A string literal passed to a crate's generic parameter is a `&'static str`, as in Rust.

## 0.2.6 (2026-10-27)

- Reading or writing a struct's field no longer copies the struct.
- Passing `&s` or `&mut s` to a function lends the value instead of copying it.
- A method called on a field or an element reaches it in place.
- A script that calls `view(&state)` 2,000 times on a 20,000-element state runs about 6 times faster.

## 0.2.5 (2026-10-27)

- A script struct or enum can stand where a crate expects a generic type, such as an app's state and message passed to iced's `application`.
- A script type crossing into a crate keeps its own name, so a crate that reads `type_name` sees the app's type.
- A script function or a closure that captures nothing can be passed where a crate takes `impl Fn`, including views that borrow their state. After a save, it calls the edited code.
- A string a view borrows from its state isn't copied.
- A crate trait whose only impl is for functions, such as iced's `ViewFn`, takes a script function.

## 0.2.4 (2026-10-27)

- A crate method that takes an `impl Fn(..) + 'a` closure, such as iced's `Button::style`, takes a script closure.
- An enum variant a crate re-exports with `pub use`, such as `iced::Center`, works in expressions and patterns.
- A crate's type aliases work, with default type parameters filled in, as in `iced::Element` and `iced::Result`.
- A default method of a crate's generic trait, such as `Widget::boxed`, is found through a type's own impl.
- A crate module and a function with the same name both resolve.
- The release's smoke test retries a fresh bot that drops a connection or runs out of render time.

## 0.2.3 (2026-10-27)

- A dependency's `macro_rules!` macros expand from the crate's own source.
- `$crate` paths inside such a macro reach the crate that defines it, including hidden items.
- A path to a method or function is a function value wherever Rust accepts one, as in `iced::run(Counter::update, Counter::view)`.
- A call written as `Type::method(&mut x, ..)` changes `x` as `x.method(..)` does.

## 0.2.2 (2026-10-27)

- The importer binds futures and `Result`s whose output holds tuples and `Vec`s.
- `&(A, B)`, `&[T]` and `&[u8]` arguments to native calls work, and the script's value stays usable after the call.
- A struct field of type `PhantomData<T>` works.
- A crate's struct whose type parameters all have defaults can be built and read.
- When rawr has no binding for an imported item, the error says why.

## 0.2.1 (2026-10-27)

- Functions from a crate are `Copy`, as they are in Rust.
- A closure inside a tuple or array is `Copy` unless it captures something that isn't.
- iced's `styling` example passes `rawr check` again.
- `scripts/bump-version.sh` sets every rawr crate to the next version.

## 0.2.0 (2026-10-26)

rawr checks borrows and moves with its own checker. It follows stable rustc's non-lexical lifetimes over each function's control flow.

- A borrow ends at its last use, through branches, loops, `break`, `continue`, `return` and `?`.
- Conflicts report rustc's own error codes: E0499, E0502, E0503, E0505, E0506, E0597, E0500, E0501, E0524, E0373, E0382, E0507, E0508, E0509, E0515, E0716, E0106, E0621 and E0590.
- A call's result keeps the borrows its signature says it keeps, including elided and written lifetimes.
- Structs, collections, `Option`s, iterators and closures hold the borrows they're made of.
- Patterns borrow, read and move only the parts they bind or test.
- Moves follow control flow, and a partly moved value's other parts stay usable.
- A value with its own `Drop` impl counts as using its borrows when it is dropped.
- Temporaries live as long as rustc keeps them.
- A function body is checked against its own signature.
- A borrow the checker doesn't cover yet runs, and the background `cargo check` reports any error.
- A `&mut` variable can be given a new borrow, and a closure can use a `&mut` value, with no copy.
- Method calls on native values, `format!` arguments and pattern bindings reach the value without copying it.
- The checker matches rustc 1.96 on 11,409 test programs, with no programs wrongly refused.
- The checker adds 2–5% CPU time to a run.

## 0.1.0 (2026-10-26)

- First release under rawr's own version numbers. Every rawr crate is 0.1.0.
