# Changelog

Each merge to main bumps the patch version. A minor version marks a milestone.

## Unreleased

- The next milestone is every iced example, and every example in `examples/`, running under `rawr run` through importer-generated bindings only.

## 0.3.88 (2026-11-01)

- std's `PathBuf`, `Path` and `io::Error` are the same types in a program and in its crates: a crate struct's `PathBuf` field, a crate function taking `&Path` or returning `io::Error`, and `?` from an `io::Result` into a crate error through its `From<io::Error>` (thiserror's `#[from]`) work as with rustc. (#653)
- `std::io::Result<T>` works, as `Result<T, std::io::Error>`, and `Path::new("x")` borrows text as a path. (#644)

## 0.3.87 (2026-11-01)

- A dependency that the workspace's `[patch]` replaces with a Git or path crate resolves to that crate, as `cargo build` does, instead of failing on every run. (#668)
- A program that is a member of a Cargo workspace resolves with the workspace root's `[patch]`, `[replace]`, profiles and `Cargo.lock`, as Cargo resolves every member, so fount's own examples check in place. (#664)
- `Type::from(value)` of a crate type's own value gives the value back, as core's `impl<T> From<T> for T` does, so `column![..]` of `Element`s checks; `Option<String>::as_deref()` is an `Option<&str>`. (#676)

## 0.3.86 (2026-11-01)

- A string literal passed where a crate takes `&'static str`, directly or through a trait implemented for `&'static str` (iced's `.title("..")`), lives for the program as in Rust: kept once per distinct text per call site, with no copy. A borrowed local `String` there gets rustc's E0597 at the borrow. (#667)

## 0.3.85 (2026-11-01)

- `rawr check` publishes the bindings it extracts to rawr's home once the `cargo check` beside it has finished, so a later project with the same crates checks with no second nightly build and no extraction process. (#661)
- A first check asks for everything its program misses in each round instead of one type per round: download_progress drops from five extraction processes and nightly builds to three. (#662)
- The project's compiler is asked for its version and sysroot once per run instead of about six times per demand round. (#663)

## 0.3.84 (2026-11-01)

- ?play and the playground build again. A widget whose `Widget` impl depends on its content, as iced's `Button` with a string label, takes the theme its view gives, as rustc infers it: an impl is chosen by its self type and trait arguments, and a where-clause bound rawr can't prove no longer undoes the choice. A value whose type can't be named fully is stored natively instead of in a region. (#669)
- A view of text alone, `fn view(&self) -> Element<'_, Message>` returning `Element::from(center(text(..)))` or `.into()`, takes its message, theme and renderer from its return type through the crate's `From` impls, as rustc infers them. (#666)

## 0.3.83 (2026-11-01)

- A closure gets the output of the async block given to `Task::future`. In `Task::future(async move { fount::system::load(&font).await }).then(|bytes| match bytes { .. })`, `bytes` is the `Result` that `load` gives, as rustc reads it, and a `Result` holding a crate's own error crosses as itself. (#652)

## 0.3.82 (2026-11-01)

- A first `rawr check` or `rawr run` of a program with Rust dependencies reuses the facts it already extracted for each later round of demand, with no second nightly build or extraction process. Cold counter goes from 2 extraction processes to 1, and download_progress from 6 to 5. (#660)

## 0.3.81 (2026-11-01)

- A program whose dependencies patch a crate, such as fount with `iced_core` patched to git, no longer fails every run with "changed during the build". rawr no longer asks Cargo for its metadata and unit graph again after a build or before publishing, which saves two to four Cargo processes per fresh resolution. Edits to manifests, the lock or the Cargo configuration during a build are still caught by their digests. (#655)

## 0.3.80 (2026-11-01)

- A crate struct's field that holds no native value, such as a `String`, is read without moving the struct: `format!`, method calls, `&` and `==` on `x.f` leave `x` whole, and a by-value use moves only that field, as rustc's partial move does. (#648)
- A crate's `async fn` that borrows its arguments, such as fount's `async fn load(font: &Font)`, runs: each argument is leased to the future for its life, shared or mutably, with no copy, and is whole again after the await. (#649)

## 0.3.79 (2026-11-01)

- rawr is secure by default. An `Engine` starts with no file, environment, process or network access, and `Engine::set_access(Access)` grants each, with file grants optionally limited to directories. Without a grant, `std::fs` returns `PermissionDenied`, `env!` is undefined, `env::args` is empty and `process::id` panics, as a sandboxed program sees them. `rawr run` and `rawr check` grant everything, as `cargo run` does. ?play and the playground grant nothing. A native crate's own code is not gated. (#642)
- `std::fs` gains `read`, `write`, `exists`, `create_dir`, `create_dir_all`, `remove_file`, `remove_dir`, `remove_dir_all`, `rename` and `copy` beside `read_to_string`. Each returns `io::Result` with the operating system's errors. (#643)

## 0.3.78 (2026-11-01)

- A trait's provided method called on an opaque alias, as `iced::Renderer`'s `renderer.with_translation(v, |renderer| renderer.draw_mesh(mesh))`, lends the alias's renderer to the closure as its `&mut Self`. iced's geometry example passes `rawr check`. (#634; part of #624)

## 0.3.77 (2026-11-01)

- A call can take an `Option` of a crate's opaque `impl Trait` value even while it is `None`, as gallery's `stack![content, viewer]` takes `viewer: Option<impl Widget>`. (#626)
- A crate impl whose input names an associated type of an associated type of its parameter works, as palette's `Rgb::from_color(lab)` takes `Lab<<S::Space as RgbSpace>::WhitePoint, T>`. (#628)

## 0.3.76 (2026-11-01)

- The store also evicts API, generic, overlay and bindings entries an older rawr wrote, when it opens and when it makes room. (#630)
- rawr check no longer reports that the Cargo configuration changed during the build when CARGO_INCREMENTAL is exported. (#625)
- A closure that captures a value can take a crate value as `&mut`, as a trait's provided `with_translation(v, |renderer| ..)` lends the renderer, when the receiver's type is concrete. (#633; part of #624)

## 0.3.75 (2026-11-01)

- rawr's store evicts executions an older rawr wrote when it opens or makes room, with the pins that name them, and keeps each project's four newest unpinned runners and extraction builds. Running and pinned ones are never evicted. (#577)

## 0.3.74 (2026-11-01)

- A function returning `Option<impl Trait>` (or a Result, Vec, box or tuple of one) takes the open type arguments of its value from the written bound's defaults, and a crate function's opaque output implements its bounds' traits. (#623)
- A struct update `..base` of a generic crate struct, as `Rgb { red, ..Rgb::default() }`, copies the fields it takes when their type is Copy, instead of panicking. (#617)

## 0.3.73 (2026-11-01)

- A closure input that is a std trait's associated type, as `Task::sip`'s `impl FnOnce(<S as Future>::Output) -> T`, now works through the opaque type's supertraits, and a generic type's function whose callables are all filled by program functions is no longer missing at run time. download_progress passes `rawr check`. (#621)

## 0.3.72 (2026-11-01)

- A crate struct or variant whose public field is an array of known length, like iced's `SolidVertex2D { position: [f32; 2], color }`, can be built with a literal. When a public field's type still can't be held, the error names that field and its type instead of blaming private fields. (#608)
- `iced::run((), view)` works: the crate's update for `()` is accepted, and a view of `&()` can return widgets that borrow string literals, held once each with no copy. (#616)

## 0.3.71 (2026-11-01)

- A program's own native function that takes `impl AsRef<str>` and returns an `impl Trait`, as download_progress's `download` does, no longer stops the check with an internal error. A program with native modules and a path dependency inside its directory builds instead of failing with a lockfile collision. (#568)

## 0.3.70 (2026-11-01)

- A generic method whose signature names its impl's parameter in a crate type is compiled once per type it is used at, and `C::COMPONENT_RANGES` and `C::new(..)` read that type's impl of the bound's trait, as rustc does. (#607)
- A view that sends no message, `fn view(..) -> impl Widget<Never>`, works: iced's `Never` (std's `Infallible`) is named in the type the body reveals and passed to the crate's generic items. (#599)

## 0.3.69 (2026-11-01)

- A cache missing one deleted execution forgets it and the pins that held it, and rebuilds it, instead of failing with "lost a registered allocation". (#612)
- A fresh build whose cache key still holds an older entry that no longer checks replaces that entry instead of failing to publish. (#614)
- A cache entry that goes stale after `rawr check`'s first pass, or during `rawr run`'s runner build, prepares the program again instead of failing. (#613)

## 0.3.68 (2026-11-01)

- A dependency resolution whose inputs change while `rawr check` uses it is rebuilt and the check continues, instead of failing with "invalid stored baseline". (#609)
- A rawr store missing one of its directories is cleared and rebuilt the next time it opens, instead of failing every check. (#584)

## 0.3.67 (2026-11-01)

- An imported struct whose public field borrows a slice of numbers or a `&str` is built with a struct literal, as iced's `canvas::LineDash { segments: &[3.0, 6.0], offset: 0 }` is. (#595)
- A blanket impl's associated functions run on each type that meets its bounds, and the argument picks the conversion, as palette's `Hsl::from_color(rgb)` does. (#594)
- A number's `.into()` reaches the target crate type's `From` as rustc infers it, so `size: 14.0.into()` becomes iced's `Pixels`. clock and the_matrix pass `rawr check`. (#426)

## 0.3.66 (2026-11-01)

- A `#[derive(..)]` path resolves as rustc resolves it: std's derives by name or path, and a crate's derive macro through the file's `use` or its full path. A name that resolves to nothing gets rustc's "cannot find derive macro `X` in this scope" at its span, and deriving a crate's macro on a program type is reported at its span as not supported yet. Item, field and variant attributes keep their source text, so `#[serde(rename_all = "camelCase")]` now parses. (#602; part of #593)

## 0.3.65 (2026-11-01)

- rawr no longer keys any behaviour on iced's paths. Bounds, type paths, coercions and calls of iced items follow the same rules as any crate's. (#587)
- A method that lends its receiver to a closure, as `frame.with_save(|frame| ..)` does, runs, nested too. (#586)

## 0.3.64 (2026-11-01)

- A crate whose trait impls name a type with a const argument, as palette's `ArrayTable<256>`, imports instead of panicking. (#592)
- A reference to a crate value fills a crate's type argument, as `pick_list(Some(&self.backend), Backend::ALL, Backend::to_string)` does. The view borrows the value in place, and `update` then changes it in place. (#591)
- A crate function whose output borrows its `&'a T` inputs, as iced's `qr_code(&data)` does, has a binding. qr_code passes `rawr check`. (#590)

## 0.3.63 (2026-11-01)

- A crate item with impls that differ only in how they take a string, such as iced's `widget::Id::from`, no longer fails the whole program. The argument picks the impl as rustc does, and impls rawr still can't tell apart are reported only at a call that uses them. (#580)
- Members of an inherent impl whose self type fixes an argument, such as palette's `impl<T> Hsl<Srgb, T>`, belong to their own type instead of the argument's type. (#579)

## 0.3.62 (2026-11-01)

- A generic crate function named as a value, such as `.map(text)` over an iterator or an `Option`, works even where a macro and a module share its name, as rustc reads it. (#569)

## 0.3.61 (2026-11-01)

- Closures passed to iced's `canvas::Path::new` take the `&mut Builder` iced lends, once, so bezier_tool and arc pass `rawr check`. (#583)
- A crate's trait impl for std's `Option<T>`, `Result<T, E>` or `Vec<T>` is used as an impl for the crate's own type is, so `column![.., maybe.map(|x| slider(..))]` boxes an `Option` of a widget through the crate's `impl<T> Widget for Option<T>`. (#589; part of #564)

## 0.3.60 (2026-11-01)

- Type errors name a crate's types by a path the program can write, such as `iced::widget::Slider` or `iced_core::text::Span<u64>`, never by rawr's internal crate paths. A crate function item's parameters print as rustc writes them. (#563)
- A crate call rawr can't build yet is reported at the call, naming the item, instead of with no location in rawr's terms. A generic function whose type parameter rustc would infer from a later use names that parameter and the turbofish to write. (#565)
- A widget's `update` can name iced's `Shell<'_, Message>`: its type argument counts as rustc counts it. (#566)
- An associated function that takes a closure lent a `&mut` value, such as iced's `Path::new(|p| ..)`, is found and runs. (#585; part of #477)

## 0.3.59 (2026-11-01)

- A program's own function returning `impl Sipper` or `impl Straw`, as download_progress, gallery and websocket have, no longer crashes `rawr check`. Its return type is an ordinary `impl Trait` type. (#561)
- A crate call whose argument ends a line, as table's `text::success` does, no longer crashes `rawr check`, and source positions sent to the native build are no longer one character off. table now passes. (#562)
- A crate call whose one-shot closure takes a reference, such as canvas `Cache::draw(renderer, size, |frame| ..)`, no longer fails with "surface instance arguments do not match the selected declaration". (#567)
- Fixed undefined behaviour that Miri found in rawr's call path: a call that swaps its first argument froze the copy its callee wrote through. The debugger also reads a caller's bindings only through what the call site lends it, and a binding a running call holds reads as borrowed. (#540)
- While a debugger is attached, native calls no longer copy their arguments into the call stack. (#513)

## 0.3.58 (2026-11-01)

- The importer no longer counts request items and bytes that nothing read. About 1,600 lines of accounting are gone, with no change in behaviour. (#547)
- A closure passed to a method of iced's opaque `application(..)` program without annotations, as in `.subscription(|state| ..)` or `.scale_factor(|_| 2.0)`, takes its input from the program's `State` as rustc reads it. It used to stop with E0609 "no field on type `_`". (#550)

## 0.3.57 (2026-11-01)

- Rawr no longer counts graph work on every charge, and no longer records or warns when a build passes an accounting figure. Allocation failures, checked overflow and physical limits still refuse. (#479)
- Generating a runner for a program whose closures cross as slots, such as a mapped iced widget, no longer stops with "rawr's generated Rust agrees with the crates on a slot's input". This regression shipped in 0.3.56. (#549)
- A closure whose input borrows the program's state, as `.theme(|state: &State| ..)`, `.subscription(|state: &State| ..)` or an update closure taking `&mut State`, runs as the state's methods do. (#542)
- A call with the wrong number of arguments reads as rustc's E0061, or E0057 for a closure: "this function takes 2 arguments but 1 argument was supplied", with rustc's 'defined here' note and its help. (#544)
- A crate function whose closure returns a future, as sipper's `sipper(|sender| async move { .. })`, is refused at the call, "it takes a closure that returns a future", instead of stopping the build with an internal panic. (#545)

## 0.3.56 (2026-11-01)

- A condition only a rawr bug can cause in the importer's profile, checked-program and runner-generation steps panics and names what broke, instead of printing an "internal error" as if it were about the program. (#538; part of #451)
- Errors name free functions, closures and generic parameters as the program wrote them. A generic parameter named twice gets rustc's E0403, and a generic call that leaves a parameter's type unknown gets E0282. A crate signature's type the build doesn't reach is reported at the call that uses it. (#539)

## 0.3.55 (2026-11-01)

- ?play sends what an iced program printed together with what it drew. The bot attaches the image to the reply, and the playground shows the image above the text. (#528)
- ?play draws an iced program at its own `scale_factor`, so a program at scale 2 comes back twice as large, as iced opens its window. (#529)
- ?play answers an iced program that animates with a GIF, recorded at 20 frames a second within the snippet's time. It plays once and holds its last frame. A program that never changes still comes back as a PNG. (#527)

## 0.3.54 (2026-11-01)

- Errors about calls into crates speak about the program's code. A type the program passes is named as it was written, and an unknown one gets rustc's E0282. Moved values, mutable borrows, methods named without a call and associated functions read as rustc's E0382, E0596, E0615 and E0599. Rawr's placeholders such as `<host item>` are gone, and a condition only a rawr bug can cause panics and names its invariant. (#523)
- A rawr bug during a fresh build panics and names the invariant it broke, instead of printing an internal error. Stale cache entries still rebuild silently, and damaged ones rebuild with a note. (#522)
- `inspect` reads a `&mut` parameter and `self` under the names the program gave them. (#534)
- `inspect` lists the calls the program is in and reads a caller's bindings with `frame`. (#511; part of #513)
- `wait` and `status` show a program paused at a breakpoint or step as `paused`, with its file, line and function. (#535)

## 0.3.53 (2026-11-01)

- `rawr mcp` can pause a running program, with or without Rust dependencies: `break` on a line or a function, `step` over, into or out, `continue` and `clear`. `inspect` reads values where it paused. A breakpoint stays across saves while its line's text is in the file. A plain `rawr run` pays nothing. (#489)
- ?play prepares its runner once. The runner decodes its bindings, installs them and binds their runtime when the bot starts, and each snippet runs in a fork of it under its own time and memory limits, so a snippet pays only for its own compile and run. (#515; part of #516)

## 0.3.52 (2026-11-01)

- `rawr mcp`'s `inspect` reads the values of a program with Rust dependencies, such as an iced app's `state` in `update` and `view`. A crate value prints through its own `Debug`, and a crate value without one names its type, as rustc does. (#510)

## 0.3.51 (2026-11-01)

- A program that doesn't format with `Debug` no longer carries the generic `Debug` formatter, unless `rawr mcp inspect` is attached. (#526)
- A runner executable over 256 MiB builds. rawr hashes executables and other build outputs as streams, with no size limit. (#518)
- A symlinked `.cargo` directory or Cargo config file is followed, as Cargo follows it. Editing the target or pointing the link elsewhere triggers a rebuild. (#520)
- A Git dependency with a `version` is accepted, and Cargo checks the version against the Git package. (#521)

## 0.3.50 (2026-11-01)

- `?play` and the playground's Run on server answer an iced program with a PNG of its first frame, drawn once the program has booted and the tasks it started have finished. The snippet's `iced::run` is unchanged Rust: ?play's iced runs it in iced_test's emulator, with no window. (#434)

## 0.3.49 (2026-11-01)

- rawr accepts Cargo setups it used to refuse: a `CARGO_PROFILE_*` variable for another profile, `config` beside `config.toml` (rawr reads `config`, as Cargo does), a Git dependency at a branch, a tag, a short rev or its default branch, pinned by `Cargo.lock`, and a symlinked project or Cargo home. (#485)
- A type error names a crate type by a real path, preferring the one the program imports. iced's `Vector` used to print as `iced::<semantic imported type><f32>`. Inferred types print as `_`, the never type as `!`, and function types leave out `-> ()`, as rustc does. (#508)
- Build errors name your files, Cargo.toml, Cargo.lock, the Cargo configuration and environment variables in Cargo's words, instead of rawr's internal stages. A failure only a rawr bug can cause says so. (#483)

## 0.3.48 (2026-11-01)

- The ?play image builds its universe runner, and a tokio snippet runs in the deployed image within the bot's time and memory limits. (#465)
- A runner starts much faster. Rawr's runtime dependencies compile optimized, a runner embeds only the bindings it installs, and installing them no longer scans every type for each type. ?play snippets now run within their time budget. (#507)

## 0.3.47 (2026-11-01)

- The work plan at rawr.airstrike.fyi/todo, its changelog and the docs update on each push to main, not only at a release. (#496)
- The /todo chart puts milestones at the top level, each with the epics that have work in it. (#501)
- CI fails only when the code is broken. Each push to main gets its own Linux run, tests compare with `cargo run` instead of hand-copied output, and a release tests the site before it builds the image. (#492)
- The core no longer carries rawr-iced's operation endpoints or signature supports. About 1,600 lines went, with no change in behaviour. (#450)
- The importer no longer carries the request path for rawr-iced's host profile. (#487; part of #451)
- `rawr mcp` has an `inspect` tool that reads a running program's values: the bindings in scope where the program is, or one binding by name or file. Each value prints as the program's `{:?}` would, and a crate type without `Debug` says so as rustc does. Programs with Rust dependencies don't answer it yet. (#488)

## 0.3.46 (2026-10-31)

- An operator on a crate type picks the impl its right-hand side matches and takes that impl's `Output`, as rustc does. iced's `Point - Point` is a `Vector`, so game_of_life's `translation + (cursor_position - start) * (1.0 / self.scaling)` checks. Every operator on a crate type now also compiles in the generated runner. (#502)

## 0.3.45 (2026-10-31)

- A crate alias that is the only name of its type, as `iced::Renderer` is, stays its own type when another dependency re-exports what it names. So `fn view(&self) -> impl Widget<Message>` checks in a project that also depends on iced_test. (#504; part of #465, #434)
- Generated runners build when a dependency's trait with a `from` method, such as num-traits' `NumCast`, is in scope. Before, the runner's own `u8::from` calls failed with E0034. (#505; part of #465)

## 0.3.44 (2026-10-31)

- Programs whose dependencies use zerocopy's derives, like pick_list and custom_widget, no longer stop at "owned trait implementation is absent". The importer reads signed integer constants in impl headers, and an implementation it still can't read is set aside instead of breaking its crate. (#478)
- rawr no longer reports its own inventory invariants as the program's error. A broken invariant is a rawr bug that names itself, and a damaged stored entry is rebuilt. (#481)

## 0.3.43 (2026-10-31)

- A value of a type that implements a crate's trait over `Fn` fills a parameter of that trait, so iced's `.theme(Theme::Dark)` checks. A function of the state still fills it as before. (#337)

## 0.3.42 (2026-10-31)

- `rawr run` calls `.map` on a widget returned as `impl Widget<M>`, such as a `button(..)` behind `fn part() -> impl Widget<Part>`, at the renderer the `impl Widget` names, as `cargo run` does. (#428)
- `rawr run` names iced types that the `iced` crate doesn't re-export, such as the `Map` that `.map` returns, through the crate that defines them. (#476)

## 0.3.41 (2026-10-31)

- Checking a program that uses iced's whole dependency set, like pick_list or custom_widget, no longer stops at a byte limit. The dependency inventory keeps each crate's facts and export targets, and builds no API of the whole set. (#472)
- Errors about your Cargo setup or environment now read the way Cargo writes them. They name the dependency, the config file or the environment variable involved, never rawr's internals. Examples are a short git `rev`, a `rustc` wrapper in `.cargo/config.toml`, a dependency named like the package's own library, and an unset `PATH`. A condition that only a rawr bug could cause panics and names the broken invariant instead. (#480)

## 0.3.40 (2026-10-31)

- A function returning `impl Widget<..>` that takes no reference, or holds a `&'static str`, no longer gets E0106. rawr reads the written `impl Trait` under edition 2024 capture rules, as cargo does. (#462; part of #428)
- `column![part().map(..)]` checks as it does under cargo. iced gives `Map` no `_boxed` of its own, so the call goes through the trait's method. (#424; part of #428)

## 0.3.39 (2026-10-31)

- A method of a crate's generic impl whose type names the impl's parameters in another order now runs, as `Widget::_boxed` of iced's `widget::Map<W, F, A>` does, so `column![grid.view().map(..), controls]` checks. (#425)

## 0.3.38 (2026-10-31)

- Each library in a program's dependency set gets its own name, including one crate version that comes from two sources, so the import request no longer stops at "dependency aliases must be unique". (#438)

## 0.3.37 (2026-10-31)

- A warm `rawr check` of a new iced program no longer reruns Cargo's resolution on each demand round, and keeps Cargo's answers for the project's next run. Upstream counter's warm check fell from about 21 s to 12 s. (#466; part of #446)
- Replaying a crate's shared table reads only the parts a program reaches, instead of parsing iced's whole 89 MB table every time. (#420)
- `rawr run` of a crate method whose output names its closure type, as iced's `Widget::map` does, runs. Before, it failed with `Unknown member Canvas::map` although `rawr check` passed. (#423)
- Generated bindings read iced's whole dependency set: crates sharing a name (rustc-hash 1 and 2) go in by name and version, a crate built for host and target is told apart by its path, and each crate gets its own byte budget. (#467; part of #438)
- rawr drops more of what rawr-iced drove: the parse-time application rewrites, the tokio macro wrapping nothing used, unused reload stages, and the site's iced render leftovers. (#452, #453, #454, #455)

## 0.3.36 (2026-10-31)

- `?play` runs snippets on a runner built once with the bot's image, through generated bindings of iced, iced_test and tokio, with no cargo per run. A tokio program runs there. A snippet outside the runner's calls fails to compile, as any program with a missing native call does. (#436)

## 0.3.35 (2026-10-31)

- A shared playground link shows the site's card with the program's first line of output. A share no longer keeps a theme, size or messages. (#442)
- rawr no longer carries the application, preview and widget protocols, host operations and streams, or the importer's view recipes that rawr-iced drove: about 23,800 lines removed. Async code in any program uses rawr's own future, so `join!` and `async fn` work the same with or without iced. (#448; part of #439, #440)

## 0.3.34 (2026-10-31)

- A warm `rawr check` of an iced program no longer spends minutes classifying: definitions and implementations are looked up by hash instead of by scanning the whole graph. Upstream clock's warm check went from 176 s to 79 s. (#437)

## 0.3.33 (2026-10-31)

- `?play` and the playground run every snippet through one compile path. A snippet that names a crate they don't have gets the ordinary compile error, not a special message. (#444)
- An `async fn main` with no runtime attribute is rustc's E0752 ("`main` function is not allowed to be `async`"), under `rawr run`, `rawr check`, `?play` and the playground. (#445)

## 0.3.32 (2026-10-31)

- Every iced program runs and checks on generated bindings. rawr-iced and its CLI are gone, with `rawr preview`, `rawr snapshot`, `rawr cache`, `rawr new --template iced` and rawr-iced's agent tools. (#430)
- No message says "facade". The codes `RAWR-FRAMEWORK-FACADE` and `RAWR-FRAMEWORK-FACADE-DEPENDENCY` are now `RAWR-FRAMEWORK-BINDINGS` and `RAWR-FRAMEWORK-BINDINGS-DEPENDENCY`, and rawr's routing errors use `RAWR-FRAMEWORK-EXTENSION`. A failure registering generated bindings is one internal error that names the Rust item. (#430)
- `?play` and the browser playground run plain Rust programs. An iced or async snippet gets a message saying it doesn't run there yet. (#432)

## 0.3.31 (2026-10-31)

- On Linux, `rawr run` compiles rawr's runtime again. The runtime build gets the toolchain's library path, so a build script that runs rustc (libc's) no longer fails to load libLLVM. (#431)

## 0.3.30 (2026-10-31)

- A crate type compares with a number through the crate's `PartialOrd<f32>` or `PartialEq<f32>`, as clock's `rotation < 180.0` does with iced's `Degrees`. When the type also compares with itself, the right-hand side picks the impl, as in rustc. (#412)
- `rawr check` accepts a crate method whose output names the closure it takes, as iced's `Widget::map` does: the runner holds the closure as a `Box<dyn Fn(A) -> B>`. game_of_life gets past its `.map(..)`. (#422; part of #417)

## 0.3.29 (2026-10-31)

- A save that names any item of a dependency rawr has already read needs no nightly build, for large crates such as iced too. rawr reads each crate's whole API the first time a program reaches it. (#281)
- A macro defined in a dependency replays its `$crate` paths from that dependency's shared bindings. (#404)
- Trait facts of registry and git crates whose dependencies run build scripts are shared by every project on a machine, once a project's own build agrees with rawr's nightly. (#403)

## 0.3.28 (2026-10-31)

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

- `rawr run` applies a body edit in a plain or marked program by compiling it in the running runner, with no full preparation. On the reload benchmark a save shows at the next callback in about 80 ms, down from about 1.3 s, in the same process with its state kept. New native needs, manifest or declaration changes, and edits a running `main` can't take still go through full preparation. (#373)
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
