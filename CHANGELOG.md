# Changelog

Each merge to main bumps the patch version. A minor version marks a milestone.

## Unreleased

- The next milestone is every iced example, and every example in `examples/`, running under `rawr run` through importer-generated bindings only.
- rawr-iced is being retired step by step, so that removing it ends up a no-op.
- A crate method whose output is its type parameter's associated type works. iced's `cache.draw(renderer, size, |frame| ..)` on a `canvas::Cache` gives a `canvas::Geometry`, and its closure borrows the `&mut Frame` iced lends it. (#318)
- A crate function or method named as a value can go where `Option::map` or `Result::map` takes a closure, as in `message.map(canvas::Action::publish)`. A crate alias reached through its module, as `canvas::Cache::with_group(group)`, has its type's associated functions. (#327)

## 0.3.27 (2026-10-31)

- A project that depends on serde, or on any other crate rawr's runner also needs, builds its runner again. (#405)
- A save that names a new item of an unchanged path dependency runs no nightly build. The crate's whole table is kept with the project and replayed after its files are checked. (#398)

## 0.3.26 (2026-10-31)

- The run after a program's first proved imported call starts warm instead of compiling again. (#400)
- A crate extracted whole also answers macros such as `column!` that a later program calls for the first time. (#401; part of #281)
- Crates whose dependencies run build scripts keep their trait facts per project instead of sharing them across the machine unverified. (#399)
- A borrow of an element (`let x = &mut a[i]`) reads its index once, where the borrow is made, as Rust does. Changing `i` afterwards, or an index with a side effect, no longer moves the borrow to another element. (#392)
- `T::method(&t)` calls a method of a trait bounding the generic parameter `T`, as `<T as Trait>::method(&t)` does, and `T::method` is a function value. (#376)
- A closure passed to a generic function takes its parameter types from the function's `Fn` bound: `each(4usize, |x| *x * 4)`. (#377)
- A `for` loop and iterator adaptors work over an associated type bounded by `Iterator` with no `Item =`. (#379)
- A struct that implements `Iterator` can be passed where a generic parameter takes `I: Iterator<Item = X>` or `impl Iterator<Item = X>`. (#406; part of #380)

## 0.3.25 (2026-10-31)

- `rawr run` applies a body edit in a plain or marked program by compiling it in the running runner, with no full preparation. On the reload benchmark a save shows at the next callback in about 49 ms, down from about 1 s, in the same process with its state kept. New native needs, manifest or declaration changes, and edits a running `main` can't take still go through full preparation. (#373)
- A new method call on a type a program already uses is proved from the crates' own facts, so a save that adds one needs no nightly extraction. (#387)
- A program with proved imported calls starts warm in about half a second, down from about two: a warm run no longer runs a Cargo probe. (#390)
- Trait facts of crates from registries and git commits are shared by every project on a machine. (#386)

## 0.3.24 (2026-10-31)

- A native `&mut Vec<String>` or `&mut String` lent to a script callback, slot or closure is now the native storage itself. Element edits, pushes, whole assignment, moves, and writes made before an error or caught panic reach native code, with the vector's allocation and capacity kept. Element references such as `v[i]`, `last_mut()` or a helper's `&mut v[0]` reach the native element without copying it. Native functions that take `&mut Vec<String>` can now be called. (#391; part of #365)

## 0.3.23 (2026-10-31)

- An imported trait method whose meaning depends on which traits a block imports now runs. Rawr proves the call from the crates' own facts and calls it natively with exactly that block's imports, as rustc would. (#384; part of #271, #281)
- Each dependency crate's trait facts are kept under their own key, so a later run reuses them and only a changed crate is read again. (#385; part of #271, #281)

## 0.3.22 (2026-10-30)

- Raw identifiers such as `fn r#for` and `let r#for` work as in rustc. (#320)
- An associated type's bound can fix its own associated type, as in `type Fore: Iterator<Item = Self::For>;`, and generic code relies on it. (#320)
- Higher-ranked bounds and types work: `F: for<'a> Fn(&'a T)`, `for<'a> fn(&'a usize)`, `dyn for<'a> Fn` and `where for<'a> &'a T: Sized`. (#320)
- `<Self as Trait>::Name` works in an impl's signatures. (#320)
- `impl<T: Trait> Trait for &T` whose items forward to `T`'s impl gives references the trait. Other impls for references are refused with an error that says why. (#320)
- A closure bound by `let f: fn(&usize) -> usize = |x| ..` takes its parameter types from the annotation. (#320)

## 0.3.21 (2026-10-29)

- The playground bot keeps reporting its running image after becoming ready for requests; readiness checks no longer reset its boot evidence. Real restarts still require a fresh report. (#375)

## 0.3.20 (2026-10-29)

- Script implementations of native traits preserve Result types in callback parameters and return values, so callbacks returning Ok or Err compile with their declared Rust signatures. (#371)
- Deployment waits read the current container application directly, avoiding false rollout timeouts when Cloudflare’s account-wide list still shows the previous image. (#374)

## 0.3.19 (2026-10-29)

- Provided trait methods use the concrete types from their implementation, so generic trait arguments no longer leak into generated callable signatures. (#369; part of #271, #281)
- Valid imported calls keep working after an ambiguous call and on repeated cached runs. Unrelated methods with the same name are deferred until used instead of rejecting the whole imported surface. (#370; part of #271, #372)

## 0.3.18 (2026-10-29)

- Reuse verified crate extraction tables across projects, with atomic publication, active-reader protection and removal by last use. Small whole-crate entries can serve later item demands without another nightly build; larger entries currently reuse their recorded subset. (#367, #272; part of #271, #281)
- Validate declared files, environment and compiler inputs before reusing bindings. Differences between nightly and project build-script outputs keep bindings local with diagnostics, and changes to external declared inputs reach a running app. (#275)
- Track hot reload at every release with a repeatable installed-tool benchmark and versioned raw samples. The 0.3.17 baseline measured 1.000 s for the first edit and 1.108 s median over five later edits, from save to changed native callback output; startup is recorded separately. (#368)

## 0.3.17 (2026-10-29)

- Call a crate trait's provided value-self method on a script type, including blanket impls and macro paths. The unchanged iced custom_widget example now renders through generated bindings. (#359)
- Call methods through a public alias of a concrete generic crate type, including iced's Renderer with private graphics backends. Native compilation checks the concrete implementation's bounds. (#360)
- Infer generic imported struct literals from fields and surrounding uses, and assign fields through borrowed native values. This includes nested Copy field updates. (#364)

## 0.3.16 (2026-10-29)

- A generic method, and a generic whose parameter reaches a crate item only in its body, is compiled once per instantiation its callers use, and a view that passes a later method's opaque output to a crate function builds from a fresh cache. (#341)
- A script impl of a crate trait generic over a crate-bounded parameter, as custom_quad's `impl<Message, Theme, Renderer> Widget<..> for Quad where Renderer: renderer::Renderer`, is compiled for each use a crate's bound makes of it, so its body calls the renderer at its type. (#322)
- A script impl whose signatures name its own parameter, as component's `Option<Message>`, and whose view returns `impl Widget<..> + 'a`, crosses, and a trait with lifetime arguments, as `Component<'a, Message>`, is selected and written in the runner. (#323)
- An associated type Rust code would hold as a script value, as the_matrix's `type State = RefCell<Vec<Cache>>`, crosses as a proxy whose `Default` is the type's. (#324)

## 0.3.15 (2026-10-28)

- A view's column of text made from an iterator, as iced's text and events examples make it, now builds. rawr reads each type's lifetime variance from rustc, and a value made outside the view joins the view's region only when its type is covariant. (#335)
- A function iced runs on its executor thread with crate values, such as `Task::map(Result::ok)` or `listen_with`'s filter, runs with an arena of its own for the call. Upstream text and events run with no rawr-iced. (#347)

## 0.3.14 (2026-10-28)

- A float range such as `1.0..=30.0` passes where a crate takes `RangeInclusive<T>`. `T` becomes the float the call's other arguments give, as in iced's `slider`. (#355; part of #314)
- An array literal such as `[10, 20]` passes where a crate takes `impl Into<T>` and `T` converts from an array of that length, as in iced's `padding([10, 20])`. iced's exit runs with no rawr-iced. (#336)
- `.into()` on a crate value converts into the crate type its use expects, even when the program never names that type, as in iced's modal `Some(Color { .. }.into())`. (#340)
- A borrowed `Vec` passes where a crate takes `impl IntoIterator<Item = T>`, and its items are borrows, as in iced's `table(columns, &self.events)`. A script type named `Event` no longer clashes with the generated runner's own code. (#356; part of #345)

## 0.3.13 (2026-10-28)

- A boot function that returns the state and a task, as iced's `new() -> (Self, Task<Message>)`, runs as `application(Example::new, ..)` takes it. (#300)
- A message made from a crate value on another thread, as `Task::map(Message::InformationReceived)` on iced's executor, is made where it is delivered. iced's system_information runs with no rawr-iced. (#300)
- A `Vec` inside a crate type's arguments, a function-pointer parameter such as `event::listen_with`'s, a bounded generic associated function such as `Task::done` passed as a callable, an `Option` or `Result` of crate values, and a slice of crate values such as `pick_list` takes all cross. iced's text passes `rawr check`. (#300)

## 0.3.12 (2026-10-28)

- A crate's re-export of a std item is that item. `use iced::time::{Duration, Instant};` names std's `Duration` and `Instant`, and so do `time::Duration` after `use iced::time;` and `iced::time::Instant::now()`. Before, rawr refused the import as "a standard library definition". (#312)
- A std type a crate re-exports under another name, as `iced::Never` is std's `Infallible`, is that type under the new name in types. So is `use std::time::Duration as Span;`. Calls through the new name, such as `Span::from_secs(1)`, aren't supported yet. (#312; part of #344)
- std's `Instant` crosses into and out of a crate by value, as `Duration` does. `time::every(milliseconds(10))`, which gives a `Subscription<Instant>`, has a binding, and `.map(Message::Tick)` hands each tick's `Instant` to the script. (#338)
- `{:?}` of an `Instant` prints as Rust prints it, `Instant { tv_sec: .., tv_nsec: .. }`. Before, rawr printed `<timestamp>`. (#338)
- Upstream iced's stopwatch, ferris, game_of_life, table and markdown get past their `iced::time` imports, and clock and the_matrix get past `time::every`. geometry gets past `use iced::Never`. Each still stops at a later gap. (#312, #338)

## 0.3.11 (2026-10-28)

- A method that a crate's extension trait gives every type through a blanket impl, such as iced's `.boxed()` on a `Container`, `Text` or `Scrollable`, is found. Before, rawr said "no method named `boxed`". The blanket impl applies through the type's own impl of the trait its where-clause names, which fixes the trait's arguments. Upstream iced's multi_window, scrollable and system_information get past it.
- A generic function whose signature names a crate's type or trait runs under `rawr run`. This covers a parameter written `impl Widget<Message>`, and a helper generic over its message that builds a crate widget. rawr compiles it once per instantiation its callers use, as Rust does, and each call runs its own instance. Before, `impl Widget<Message>` in a parameter was "unknown type `Widget`", and a crate call inside a generic function failed with "not one exact concrete surface type". A type the argument leaves open, such as a widget's renderer, is fixed by the trait's defaults and the crate's impls. Upstream iced's modal and toast get past it. Other generic functions are still compiled once, as before.

## 0.3.10 (2026-10-28)

- Upstream iced's `url_handler` example runs under `rawr run` without rawr-iced. Its window matches `cargo run`'s.
- `iced::application(..).subscription(App::subscription)` works. The subscription takes `&P::State` and gives a `Subscription<P::Message>`, where `P` is the opaque program `application(..)` gives. Before, rawr said "it takes a callable rawr can't make". A crate method's callable now reads an associated type of a type parameter as a type the call infers, wherever the callable's inputs or output name it.
- A method of a generic impl takes a script function as a slot, as a crate function does. A script state it lends by reference is shared, not copied.
- A call on one opaque program that gives another, as `.subscription(..)` does, is compiled where it is made, and so are the calls on the program it gives, such as `.run()`.
- A crate method whose impl puts its own parameter inside the type, as iced's `impl<T> Task<Option<T>>` does for `and_then`, now has a binding. When the crate overloads the method by the receiver's type argument, as `Task<Option<T>>` and `Task<Result<T, E>>` do, the receiver picks the form.
- A generic crate function can be passed as a value, as in `window::latest().and_then(window::close)`. Its type arguments are inferred from where it goes. Before, rawr said it "must be monomorphic".
- A `&str` that a widget keeps through its lifetime, as `text(url)` keeps the `&String` it is given, is pinned in the view's region. Before, the runner's build failed with E0597.
- iced's `Task` is a crate type like any other when no host profile runs, so `update` can give back `Task::none()`. Before, the runner's build failed in `into_task`.
- The events and exit examples get past `.subscription(..)` and `and_then(window::close)`. events stops where its runner builds `column(..)` of `text!` widgets, and exit at `padding([10, 20])`. arc and delineate stop at `.theme(Theme::Dark)`, and clock and the_matrix at `time::every`.

## 0.3.9 (2026-10-28)

- A crate imports when one of its methods takes a callable parameter that no script closure fits. Examples are tokio's `MappedMutexGuard::map` and cosmic text's `with_buffer_mut`, and warp's `Ws::on_upgrade`, whose closure gives a future. Before, the whole import failed with "unknown type parameter". Such a method has no binding.
- A trait method on a function's opaque `impl Trait` may name a type parameter the function also names, as warp's `form<T>()` and `Filter::untuple_one<T>` both name `T`. Before, the import failed with "duplicate type parameter `T`".
- A method that gives an opaque type, such as image's `Reader::into_decoder`, now imports. So does a member of a function's opaque type that names that type again, such as warp's `Filter::and`. Before, the import failed with "requires missing module".
- Upstream iced's bezier_tool, vectorial_text, layout, sierpinski_triangle, solar_system, tour, component, changelog, websocket, geometry and loupe now get past these imports, and so do markdown and screenshot. Each still stops at a later gap.

## 0.3.8 (2026-10-28)

- A script type can implement a crate's trait, such as `impl canvas::Program<Message> for Arc`. Before, rawr stopped at the impl's header with "an `impl` of a trait path that names no crate or std trait". The runner writes the impl for the type's proxy, and native code calls the script's functions back, as iced calls `draw` while it lays out. Each call runs the current generation's function, so an edit to its body takes effect at the next call.
- A crate's values that native code lends such a function, such as `&Renderer`, `&Theme` and `&Event`, are lent for the call alone and reached by their methods and `match`es. Values it gives by value, such as `Rectangle`, read as their types. A script state it lends `&mut` is changed in place.
- A script's `impl From<Shade> for iced::Color` now reaches the runner, so a crate function that takes `impl Into<Color>` takes a script value. Before, `rawr check` passed and the runner's build failed in rustc.
- `Canvas::new(self)` and `canvas(self)` take a shared borrow of a script value where a crate's generic parameter goes. The proxy shares the value, so nothing is copied.
- A view whose widget leaves a type parameter open, such as `canvas(self)`'s `Message`, no longer stops with "more than one impl of the crate's fits" when the crate reaches one impl by several paths.
- Upstream iced's arc, bezier_tool, clock, sandpiles, vectorial_text, layout, multitouch, sierpinski_triangle, solar_system and the_matrix now get past the headers of their `canvas::Program` impls, and lazy past its `From` impl. Each still stops at a later gap. custom_quad and custom_widget get past `impl widget::Meta`, and stop at their `Widget` impls, which are generic over a renderer bounded by a crate's trait.

## 0.3.7 (2026-10-28)

- Upstream iced's `slider` example runs under `rawr run` without rawr-iced again. Its `text(self.value)` was read as the `text!` macro, because the `text` function had no binding.
- A crate type whose only public name is an alias that passes its parameters on is named through that alias, with the alias's defaults. iced's `widget::Text` is such an alias while `iced::advanced` is off. Functions that give such a type, such as `text(..)`, now have bindings.
- A crate constant whose value isn't a scalar, such as iced's `Font::MONOSPACE` or `Theme::ALL`, is read by its path where the runner runs. Before, rawr said "the importer couldn't extract it".
- A call of a name that a `use` imports as a crate module, such as iced's `combo_box(..)`, says why rawr has no binding for the function of that name. Before, it said "unknown function".
- A crate's type alias whose target type has no other public name now crosses, such as iced's `Element`, `widget::Text`, `canvas::Cache` and `canvas::Frame`. Before, `use iced::Element` stopped with "unsupported Rust declaration (UnnameableType)". Generated code writes the type through the alias.
- A type alias of an associated type, such as iced's `canvas::Geometry`, is the type it names at its defaults. Before, it stopped with "(Projection)". It is written without arguments.
- `#[derive(Default)]` works on a struct with a field of a generic crate type whose `Default` impl is generic, such as `canvas::Cache`.
- Eleven upstream iced examples now get past these aliases: arc, bezier_tool, clock, sandpiles, vectorial_text, layout, tour, todos, multitouch, sierpinski_triangle and solar_system. Each still stops at a later gap.

## 0.3.6 (2026-10-28)

- A project that shares a crate with rawr at fewer features, as iced shares ahash, runs again with the shared copy. Before, the runner's build stopped with "cannot update the lock file because --locked was passed". The runner's lockfile is now what cargo resolves without rawr, with the project's versions kept.

## 0.3.5 (2026-10-28)

- A project whose watched folders include two names where one is the other followed by `-`, such as `target/rawr` and `target/rawr-home`, no longer fails with "invalid stored baseline" on its next run.
- rawr's own crates compile once per machine, not once per project. The first `rawr run` on a toolchain compiles them into `runtime/` in rawr's home, and every project's runner links that copy. A second new project compiled no rawr crate and ran in about 13 s. Before, each project spent about 85 s compiling rawr. A script with dependencies reuses the same copy.
- The copy is keyed by rawr's version, sources and lockfile, the compiler's `rustc -vV`, and the runtime's features and profile. Nothing about a project is in the key, so projects with different dependencies share it. Copies unused for a day go, apart from the three used last.
- A runner that can't link the shared copy, such as one whose crates link a native library that rawr's do, compiles rawr in the project as before, and rawr says so once. iced apps on rawr-iced still compile rawr in the project.
- `RAWR_RUNTIME_OPT_LEVEL` compiles the `rawr` crate at a lower opt-level, such as `0`, for a faster first compile and slower scripts. The default stays 3.
- A project that shares a crate with rawr at fewer features, as iced shares ahash, runs again with the shared copy. Before, the runner's build stopped with "cannot update the lock file because --locked was passed". The runner's lockfile is now what cargo resolves without rawr, with the project's versions kept.
- A native stream is bound through rawr's own `host::stream::Source` trait, not `futures_core::Stream`, so a project's copy of `futures-core` never has to be rawr's.

## 0.3.4 (2026-10-28)

- `std::process::id()` in the browser playground stops with "no pids on this platform", as Rust's std does there, instead of crashing the page.
- A script with its own `[profile.*]` or features no longer makes the next script rebuild rawr's crates. Every script builds in one directory, and a script's run used to remove rawr's crates another script had built under another profile. The same holds for projects that share a `CARGO_TARGET_DIR`.
- `rawr check` and `rawr run` run `cargo check` on a script, as they do on a crate, so a script gets rustc's errors, such as an overflowing literal rawr accepts. rustc refuses a frontmatter block outside `cargo -Zscript`, so rawr checks a copy with the block blanked, and the errors name the script's own lines. It works on a stable toolchain.
- A script with dependencies, run without rawr-import installed, is told it uses "Rust dependencies in its frontmatter". Before, the message named a `Cargo.toml` the script doesn't have.

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
