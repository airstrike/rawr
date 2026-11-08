# Changelog

Each merge to main bumps the patch version. A minor version marks a milestone.

## Unreleased

- The next milestone is every iced example, and every example in `examples/`, running under `rawr run` through importer-generated bindings only.
- A package with marked modules builds its native side with its `[features]`, as `cargo run` does: its default features are on, code under `#[cfg(feature = "…")]` builds as under Cargo, and an optional dependency a feature enables is there, inherited from a workspace or not. (#1144)
- A build script that declares modules with `mod`, at `build.rs` or wherever `build` puts it, finds them beside it, nested modules included. (#1143)
- A dependency whose build script isn't named `build.rs` no longer loses its dependency information in rawr's store. (#1147)
- A call through a crate the program doesn't depend on, as `tokio::spawn(..)` without tokio, is rustc's E0433 naming that crate, and a panic in a native async call reaches you as cargo shows it. (#1149)
- A crate whose method returns an `impl Trait`, such as polars-core's `downcast_iter_mut`, no longer crashes rawr-import's driver with a rustc internal error about a missing visibility. (#1151)
- The whole site now has the work plan's look: Abitaro Sans, black on white with one red, heavy headings and thick rules. Every page, /todo included, has a dark mode that follows the system, with a switch in the site bar that remembers the choice. (#1155)
- The playground's examples have a Rustlings group: 13 programs from the rustlings solutions (ownership, enums, Option, strings, HashMap, errors with ? and Box<dyn Error>, traits, lifetimes, iterators, Box, Rc and TryFrom), each crediting its source under rustlings' MIT licence. (#1161)
- A module, its file, a function, a field or a binding can be named `switch`, `import`, `print`, `this` or any other word the old engine reserved, in marked code and in a crate's items, as in Rust: `mod switch;` opens `switch.rs`. rawr's grammar is Rust's, so the old engine's `switch`, `do`/`until`, `try`/`catch` and `import`/`export` syntax is gone. (#1153)
- The landing page's reload section now covers any library: tabs show the same idea in iced, ratatui and a plain std loop, and a new docs page, Apps and libraries, explains how libraries are compiled by Cargo, called from marked code and reloaded on save. Section rules, headings, links and inline code read more evenly. (#1162)
- An `Option` or a `Result` is an iterator of the value it holds: `for x in opt`, `for x in &opt`, `opt.iter()` and `opt.into_iter()` walk it, as std's `IntoIterator` impls do. (#1079)
- Assigning through an `Rc` or an `Arc` is rustc's E0594, "cannot assign to data in an `Arc`". (#1078)
- A formatted value whose type nothing settles is rustc's E0282 at its `let`, such as "type annotations needed for `Vec<_>`". (#1077)
- `assert!()`, `assert_eq!()` and `assert_ne!()` with missing arguments give rustc's own macro errors. (#1076)
- An unknown name inside `thread::spawn`'s closure, or in any std path's arguments, is rustc's E0425 rather than a not-yet error. (#1080)
- A bound such as `T: AsRef<str>` or `T: AsMut<u32>` holds for std's types by std's impls, and `as_ref`, `as_mut`, `borrow`, `borrow_mut` and `deref_mut` run through it, a mutable one changing the value where it is. An array of different functions of one signature holds `fn` pointers. (#908)
- A type can implement one generic trait more than once: `impl From<&str> for Bar` beside `impl From<String> for Bar`, two `impl Convert<T>` of your own trait, or one trait for both `f32` and `f64`. `Bar::from(x)`, `x.into()`, methods, operators and functions without `self` pick the impl by type, as rustc does, and a call nothing picks an impl for is rustc's E0283. (#988)
- A build input reached through a symlink, such as a header under the SDK path `xcrun --show-sdk-path` prints, is the file the link resolves to, as for Cargo. Pointing the link elsewhere or editing its target runs the build script again and rebuilds the crate. (#1171)
- The play bot's status page and its invite page now look like the rest of the site, in light and dark themes that follow the system or the site's theme switch. (#1156)
- A program may name its types and modules anything, `Program`, `Error` and `Message` included: the generated runner keeps its own items in a module no program names. (#1163)
- A marked `src/main.rs` names its package's library by the crate name, as a Cargo binary does: the library is built, read and bound like any dependency. (#1164)
- A crate function whose `impl Trait` output captures a parameter a closure fills, such as iced's `markdown::code_block`, no longer panics rawr-import when a member of that output is called. (#1098)
- The site's Run buttons run snippets in your browser, as the playground does, with no server round trip and no browser check after an edit. The landing page runs its hello program and a bouncing-ball loop, and the borrow-checking examples in How it works run and show their errors. A few blocks that need what the browser lacks (threads, the clock, wide integers) still run on rawr's server and say so. The site bar is plain at the top of the page and takes a light shade and a hairline once the page scrolls. (#1179)
- A widget moved into a `row!` in one branch and a `column!` in the other runs: each container's methods are that container's own. iced's progress_bar runs. (#1129)
- A view closure that captures nothing and takes a crate value native code lends it, as `transition`'s `|animation, at| ..`, runs: the value is lent for that call. (#1130)
- iced's lazy runs: the widgets its closure makes are `'static`, and a string literal they take outside the view, as `button("Delete")`, is a `&'static str`. (#1081)
- A program's impl of a crate's generic method passes a value of a parameter's associated type from one call to the next, as `painter.fits(painter.font())`. (#1022)
- Two default methods of one trait that give opaque types, as `explain` and `outlined` of one extension trait, each have the members of their own type. (#943)
- An idle app under `rawr run`, such as a paused simulation or a window waiting for events, is no longer stopped after a few minutes with "the runner did not finish a frame before its deadline". The owner waits for the runner's reply for as long as the runner lives. (#1165)
- A program under `rawr run` can tell it is and hears what each save did. `RAWR_RELOAD_EVENTS` names a socket that carries one JSON event per line: `restarted` first in a process a save restarted, then `queued`, `swapped`, `failed` with its message, file, line and column, and `restarting` with its reason. Under cargo the variable is absent. The new dependency-free `rawr-reload` crate reads it: `rawr_reload::outcomes()` gives each `Outcome` as it comes, and is `None` outside `rawr run`. (#1166)
- The old engine is gone from rawr: try/catch, do/until loops, import/export and the module resolvers, custom syntax and operators, and the old built-ins (type_of, is_shared, is_def_var, is_def_fn, eval, Fn). The lexer reads ?., ?[, ??, !in, ===, !==, := and :; as rustc does, and code writing them gets rustc errors such as "invalid comparison operator `===`" and "keyword `catch` cannot follow a `try` block". The reserved words are exactly Rust own. (#1196)
- A crate's `impl Iterator<Item = T>`, given by a function or a type's method, is an iterator: `next()`, `for` and the iterator's adaptors pull from it. A type's method that gives an `impl Trait` has the trait's methods. (#1187)
- A snippet of top-level statements drops its locals at its end, in reverse order, as `fn main` does, natively and in the browser. (#1180)
- In the browser, `u128` and `i128` values print, compare and do arithmetic as they do natively, instead of printing their type names. (#1181)
- In the browser, `Instant` and `SystemTime` read the page's clock instead of stopping rawr. If rawr itself stops on a program, that program runs on the server and the next run loads a fresh copy of rawr. (#1182)
- A playground snippet that opens with an inner attribute such as `#![allow(dead_code)]` runs as a file, as `rawr run` runs it. (#1183)
- A marked module inside an existing app names the items of its native crate root as rustc lets a child module: `crate::Message::Page(..)`, `use crate::Message;`, `super::helper()`, the root's constants, aliases and `use` re-exports. rawr's copy of the crate makes the root's own items public, and your files stay as they are. (#1185)
- A workspace whose path or Git package has a build script that prints no rerun-if line now checks and runs: rawr watches that package's whole directory (all but its .git, its target and nested packages), so a second run replays and an edit to any file there runs the script again, as under `cargo run`. A staged API table left by a failed run is rechecked against that directory too. (#1186)
- A widget made of what a view borrows can be wrapped by crate functions that give `impl Widget` and by types that hold one, as iced's modal does with `opaque(mouse_area(center(opaque(content))))` and pane_grid with `Content<'a, .., impl Widget>`. Methods and crate functions that take it first work, including ones that take another of the view's widgets. Your own impl of a crate trait can be used at a type of yours that holds a crate's `impl Trait` value, as loupe's `impl Widget for Loupe<W>`: `rawr check` accepts it and the runner builds it. (#1190)
- A generic function of yours that names a crate type takes the type parameter a later use of its value fixes, as rustc infers it: iced tour's `padded_button("Back").on_press(..)` fixes `Message`. (#1074)
- A crate impl's bound that names an associated type fixes a type argument, as rustc's projection does: iced's `image("ferris.png")` takes the renderer's `image::Handle`, so ferris's runner builds. (#1071)
- A function written `-> impl Widget<Message>` gives a crate bound on its output the trait's default renderer, as rustc does, so url_handler runs. (#1072)
- The runner names the type of a native value it takes from a call's arguments, so delineate's runner builds. (#1070)
- Rust code on the site and in the playground is highlighted in nine classes (keywords, types, functions and macros, strings, numbers, escapes, attributes and lifetimes, comments, doc comments), with a palette that meets 4.5:1 in light and dark. Static blocks, Run blocks and the editor share one grammar and one mapping. (#1219)
- `rawr run` of a program that ends while a save is being prepared ends with the program's own exit status, instead of exiting 1 with a framework error that said the save failed. (#1127)
- The site, the playground and the bot share a new favicon: a red disc over a heavy rule, with a red triangle for the playground and the bot, following the browser's light or dark theme. The link previews for the site, /todo and the bot's invite are redrawn in the site's black, white and red with that mark, and each preview's URL carries its content hash so chat apps fetch the new card. The build progress bar draws in the site's reds instead of pink. (#1157)
- A marked function can give native code back a value of a native type: the app's own `Message`, a struct from `main.rs`, or another crate's type. It works bare, inside an `Option`, a `Vec`, a tuple or a `Result`, and for a type with a lifetime such as `iced::Element<'a, Message>`. Native code gets the value itself. (#1203)
- Native code can pass a marked function a native value by value, and the function keeps it as Rust would. A `&mut` native value the function changes, or replaces with `*value = new`, comes back changed. `item.count += 1` on another crate's struct field works, and `item.name.push('!')` on its `String` field changes the struct instead of a copy. (#1139)
- A plain http request to the site, the playground or the bot pages moves to the same URL on https (301 for a GET, 308 for a POST so a playground request stays a POST). Before, http served the page as is. (#1228)
- rawr puts no time limit of its own on anything. The runner protocol waits for the runner as long as it lives. The `rawr mcp` tools take no `timeout_ms` or `wait_ms`: `start` answers once the program runs, `check` once it has checked, `wait` at the next event, `step` where it paused; `break` and `continue` answer at once and `wait` tells the pause. A client that bounds its calls cancels them. (#1205)

## 0.3.171 (2026-11-07)

- A build script that declares `cargo::rerun-if-env-changed` for a variable that `[env]` in `.cargo/config.toml` sets now runs as under `cargo run`: rawr gives it the value Cargo gives the script, and a change to `[env]` runs the script again. Before, every crate whose build script uses `cc` failed on a machine whose Cargo config picks a C compiler in `[env]`. (#1137)
- A dependency whose build script isn't named `build.rs`, such as aws-lc-sys's `builder/main.rs`, no longer panics rawr-import with a lost build input. (#1145)

## 0.3.170 (2026-11-07)

- A native `main`'s calls into marked code run on the thread that makes them, the process's main thread for `main` itself, as cargo's build runs them. A library whose marked `app.rs` starts iced runs from a `main.rs` that only calls it, and a save to `app.rs` shows at iced's next `update` or `view`, keeping the app's state. (#1119)
- A marked function can return a `Result` to native code: a `Result` of crossable types, one whose `Ok` or `Err` is another crate's type, or an alias such as `iced::Result`. The native value itself goes back, so `fn main() -> iced::Result { mycrate::app::run() }` gets iced's own error. (#1118)
- An embedded script can hold inline modules: `Engine::script`, `load`, `compile`, `eval` and `run` resolve `mod m { .. }` with rustc's paths, `pub`, `use`, `super` and E0603, and the host calls the root's functions by name. A file module (`mod m;`) still needs a package. (#1131)
- `call_fn` calls a script function with the parameters it is written with, a `&mut` one included, so `call_fn("update", (model, by))` works. `CallFnOptions::bind_this_ptr` still lends that value apart. (#1132)
- The default call-depth limit is 64 in debug and release builds alike. A script call that finds the host thread's stack nearly full gets more stack, so the limit, not the thread, ends deep recursion. (#1133)
- A package with marked modules builds its native side with the package's own files: `include_bytes!` and `include_str!` through `env!("CARGO_MANIFEST_DIR")` or a path relative to the source file, even outside the package, and its build script, which runs in the package's directory with its build-dependencies. Cargo reruns the script on the files its `rerun-if-changed` lines name, never on a save to a marked module. (#1136)
- A native `src/main.rs` with marked modules no longer prints Cargo's warning that the file is in both the lib and bin targets. (#1135)

## 0.3.169 (2026-11-07)

- Two projects that share a dependency now both check, whichever runs first. A dependency's shared bindings used to include implementations from other crates the first project had loaded, so the second project failed with `external crate was not loaded`. A crate's bindings now hold only what its own dependencies reach, and bindings saved before this are extracted again once. (#1097)
- A widget that keeps your closure to make its content, as iced's `transition(..)` and `lazy(..)` do, now builds in a view beside widgets that borrow your state. progress_bar's runner now builds. (#1083)
- A bound on an iterator's items can fix another type parameter, as rustc infers it: rand's `WeightedIndex::new([10, 4, 2, 1])` makes a `WeightedIndex<i32>`, so iced's the_matrix runs. (#1084)
- In a marked module, a `use` that binds a native module (`use crate::units;`, `use crate::units as u;`) binds it as a module, and its constants, functions, types and nested modules are reached through the name as rustc reaches them. (#1117)
- A project that depends directly on a proc-macro crate, such as `rust_decimal_macros`, now checks and runs instead of failing as if `Cargo.toml` changed during the build. Its macros expand in native modules through Cargo's build. A `cdylib` or `staticlib` dependency builds as Cargo builds it, and a dependency whose only library is a Rust `dylib` stops with an error that names it. (#1124)

## 0.3.168 (2026-11-07)

- `rawr mcp` sends input to the window of the program it started: `move`, `click` (left, right or middle, with a count for a double click), `drag`, `scroll`, `key` (with shift, control, alt or command held) and `type`, at positions in the window's points as `screenshot` shows it. The input goes to the program alone, so your own pointer, keyboard and focus stay as they are and the window isn't brought forward. An orange agent cursor shows where the pointer last went, in screenshots too. Each tool answers once the program has taken the input and drawn again. macOS only for now. (#989)
- A Cargo.lock that names a git dependency in its qualified form (`name version (git+…)`) no longer stops rawr-import with "dependency identity is absent". (#1123)

## 0.3.167 (2026-11-07)

- A package with `src/main.rs` and `src/lib.rs` whose marked modules are all in the library's tree runs from its library. The binary stays native and reaches the marked modules through the library as `cargo build` builds it, and so do the library's native modules. `rawr run` and `rawr check` both work, and the crate still builds unchanged with `cargo build --release`. Marked modules in both trees, or a marked `src/lib.rs` beside a native binary, stop with an error that says what to mark. (#1107)
- A Cargo.lock where a path package and a git package share a name and version, as a `[patch]` of a git dependency can leave it, no longer stops rawr-import with "dependency identity is ambiguous". (#1120)

## 0.3.166 (2026-11-07)

- Error, panic and status messages say what failed, on what, and what to do, in rustc's and Cargo's plain style. A future that isn't `Send` now reads "the future returned by `f` cannot be sent between threads safely", and an unsupported construct followed by a hint no longer has "is not supported" tacked onto the end of the hint. (#1109)

## 0.3.165 (2026-11-07)

- A Git dependency on a branch, tag or rev with a character Cargo encodes in a package id, such as the slash in `feat/reqwest-0.13`, no longer panics rawr-import. A package record in `cargo metadata` output that can't be read names the package and what was wrong. (#1108)

## 0.3.164 (2026-11-07)

- Fixes a regression from #1055: an `a..=b` of `isize`, `u8` or any integer other than `i64` crosses into a crate as a `RangeInclusive` of its own type, so `rows.cartesian_product(columns)` of two `isize` ranges builds again; a range a crate's type parameter gives back is your range of that type, and an integer range is `Clone`, `Debug` and an `Iterator` where a crate's bounds ask. (#1103; part of #1062)

## 0.3.163 (2026-11-06)

- `rawr check` no longer overflows its stack on iced's gallery, markdown and screenshot: a trait method's type parameter whose bound sets an associated type to the method's `Self`, as image's `copy_from<O: GenericImageView<Pixel = Self::Pixel>>` does, reads that associated type once, as rustc does, and a crate member whose closure names a type parameter it doesn't declare, as rayon's `from_par_iter` gives one, is left out instead of panicking. (#1069)

## 0.3.162 (2026-11-06)

- A crate method whose value's type has a lifetime keeps what it borrows shared for as long as the value lives, as rustc reads its signature, for any crate rather than only iced's widgets, and the runner names each crate type's lifetimes as rustc does, so a script function that wraps an `impl Widget` in a container and a mouse area builds. (#598; part of #955)
- A crate's `&mut` value with lifetimes that it lends your trait impl, as iced lends a widget's `update` its `&mut Shell<'_, Message>`, is lent for the call, not cloned, so iced's geometry runs, and a crate value a call borrows, as rand's `ThreadRng`, no longer needs `Send`. (#957)
- A float or integer literal whose only constraint is a crate trait bound takes the one number type that implements it, as rustc infers: iced's `Animation::new(0.)` is an `Animation<f32>`. (#1028)
- The site serves /llms.txt, a short summary of rawr's docs for coding agents: every command and flag, the environment variables, the MCP tools and their arguments, previews, imports and their limits, hot reload, embedding and ?play. Each section links to its full page, and the docs index links to it. (#1089)

## 0.3.161 (2026-11-06)

- ?play snippets no longer copy the universe's bindings for each run: every compile shares the engine's registry, so a snippet pays only for its own compile and run (locally, the site's snippets went from about 0.7 s each to 26 ms, and the live bot took about 2 minutes each). (#1065)
- The site deploy's snippet check waits for the Durable Object to run the version just deployed, so a deploy no longer fails its own check, and a run a limit stopped fails the check instead of passing it. (#1066)

## 0.3.160 (2026-11-06)

- A crate function that takes a crate value whose type has a lifetime the call picks, as serde's `de::Error::invalid_value(Unexpected::Unsigned(n), ..)` in a derived visitor, is called through the opaque error type as rustc calls it. (#1031)
- A method whose value borrows a format it takes, as jiff's `dt.strftime("%B %d")`, runs: a literal, a `&String` or a `&[u8]` fills its `&F`, the value prints through its `Display`, and changing the format while it lives is rustc's E0502. (#1014)

## 0.3.159 (2026-11-06)

- An `a..=b` of any integer type is a `RangeInclusive` of that type, as rustc types it: a function can return `RangeInclusive<isize>`, and the caller can read `start()`, `end()` and `contains` or iterate it. iced's game_of_life gets further. (#1055)
- A crate method whose parameter's bound names the program's own types, such as iced's `Application::presets(impl IntoIterator<Item = Preset<P::State, P::Message>>)`, can be called, and a string literal fills an `impl Into<Cow<'static, str>>`. iced's todos gets further. (#1050)

## 0.3.158 (2026-11-06)

- A method of a crate's trait that only iterators have, or that a blanket impl gives every `Iterator`, works on your iterators when the trait is in scope, as itertools' `rows.cartesian_product(columns)` does. Your iterator crosses as a Rust iterator that runs your chain to its next item on each `next()`, so your closures run as the crate pulls, in Rust's order, and only as far as it pulls. The `Product` it gives back is an iterator you drive with `for`, `next()` and the iterator methods, and one a crate takes as itself, so `a.cartesian_product(b).cartesian_product(c)` works. An array fills `J: IntoIterator`; where a crate takes `I: Iterator` it is rustc's E0277. (#1051, #1043)
- Program structs a crate decodes on one of tokio's worker threads, such as a reqwest response's `json()`, decode again instead of panicking that a native value was kept past the call. 0.3.156's arena change let one thread's arena clear the mark another thread's releases waited on. (#1058)
- A crate's facts are the same whether Cargo rebuilt it or reused it, so caches keyed on them no longer miss once after every cold build. (#1059)

## 0.3.157 (2026-11-06)

- A `use` path can start at a name another `use` brings in, in a function body too, as in `use keyboard::key;` after `use iced::keyboard;`. A derive's expansion and a child module's `use super::*;` reach it as rustc does. (#1044)
- A cached check whose stored demand belongs to another build of a crate's contract is rebuilt instead of stopping with "a stored demand or facade plan is not canonical" (D744). (#1053)

## 0.3.156 (2026-11-06)

- A crate's call into your impl of its trait finds the impl once per generation, and the runner's arena does no release work when nothing was let go: serde's round trip of 1,000 tasks serializes about 20% faster and deserializes about 30% faster. (#1038)
- An edit that changes only a crate derive's expanded bodies, such as a new `#[serde(rename)]`, swaps into the running program in a fraction of a second and keeps its state. Derive expansions run in one rustc run against the run's own build, so adding a field reruns sooner, and a derive used under another name (`use serde::Serialize as Save`) expands. (#1039, #1040)

## 0.3.155 (2026-11-06)

- A program that uses tracing-subscriber, as iced's clock, gradient, changelog, game_of_life and todos do, no longer stops with "symbolic surface identity belongs to a different contract". That error can no longer happen: an item enters a run's demand only through its own contract. A member whose signature names its trait's own parameter, as `Layer<S>::and_then` does, now says it isn't supported yet, and the rest of the crate works. (#1032)

## 0.3.154 (2026-11-06)

- Several derives in one file work: a derive's expansion declares its items in a module of its own that sees its item's module, as rustc's expansion does. A `Vec` of your values fills a crate's type parameter, as serde's `serialize_field` and `next_value::<Vec<T>>()` take it, and a crate may call your `Deserialize` on its own threads, as reqwest's `json()` does on tokio's workers. (#593)
- A member missed on a std type such as `String` no longer makes the next request fail. (#1030; part of #593)

## 0.3.153 (2026-11-06)

- ?play runs iced's canvas programs. The bot's universe now holds iced's arc, counter and pick_list examples and a solar system drawn with plain shapes, each as a program of its own, so a snippet that makes their calls (a canvas `Program` with its `Cache`, `Frame` fills and strokes, a `LineDash`, rotations and translations, `iced::application::timed` and `.theme(..)`) runs and answers with an animated GIF. (#1000)

## 0.3.152 (2026-11-06)

- serde's `#[derive(Serialize, Deserialize)]` on your types runs from its expansion: `serde_json::to_string(&task)` and `serde_json::from_str::<Task>(..)` print what `cargo run` prints. A generic method of a type native code fills takes a turbofish, as `map.next_key::<Field>()` does, and a function that takes `&'static [&'static str]` takes an array of your strings. (#1009; part of #593)
- Your value lent `&mut` to a crate function that takes `&mut T` or `&mut dyn Trait` keeps what the crate changed in it. (#1008)
- The ?play universe builds again, after an associated type whose bound names itself (iced's `type Font: Copy + PartialEq`) overflowed rawr's stack, and a crate trait method that takes a future no longer captures every tokio call. A `let` of a value that never comes, as a snippet ending in an endless `loop`, compiles again: it falls back to `!` as rustc 2024 does. (#1027, #1019, #1020, #1021)
- A dependency whose build script reads an environment variable it doesn't declare gives each project its own output: when the script's output in rawr's shared build differs from the project's own build, rawr cleans that package there once and reads the crate again, so the program sees the value `cargo run` gives. The imports docs explain Cargo's rule and the lasting fix, `cargo:rerun-if-env-changed`. (#1011)

## 0.3.151 (2026-11-06)

- Checking a program that needs proved applications no longer fills the disk. Each crate's intrinsic entry keeps only its own facts. The inventory build reuses the nightly's shared build directory, and each loaded artifact is hashed once. iced's changelog example now finishes `rawr check` in about 400 s. (#958)

## 0.3.150 (2026-11-06)

- A program's impl of a crate trait's generic method runs at each type native code calls it with: the body reads the type parameter as an opaque type bounded by its trait, calls its methods and associated functions, names its associated types, takes it inside another type, and takes a bound at the impl's lifetime. Its values stay native in a frame of the runner's generic impl. (#1003, #1004; part of #593)
- A borrow of your own value fills a crate function's `&T`, as `name_of(&Square)` for `fn name_of(shape: &impl Shape)`; it no longer panics. (#993)
- A crate's re-export of a std item is that item, a std type's method can be called by its path (`Formatter::write_str(f, ..)`), and a crate function that takes `&dyn Trait` takes a crate value, a std value the crate implements the trait for, or a borrow of your value. A `fmt` body can pass its formatter on to a function of yours. (#1005, #1006, #1007; part of #593)
- A `fn` pointer value passes where a crate takes `impl Fn(..)`, `FnMut` or `FnOnce`, wherever it comes from: an array element, as iced's styling passes `.style(style)`, a parameter, or a field. iced's styling and vectorial_text run. (#973)

## 0.3.149 (2026-11-06)

- A conversion's target chooses among a crate's `From` impls only when every candidate `into` is the same trait method, by the importer's identity. Exactly one impl must fit. A target the program leaves open, as in `let y = degrees.into(); y * 2.0`, or one no use fixes, is rustc's E0283. A crate trait's own `into` imported beside `Into::into` is rustc's E0034. (#996)
- Projects that share one target directory no longer fill it: when rawr starts a check or run there, it removes what the other projects left, unless one of them is being checked or run at that moment. (#960)
- rawr removes a compiled runtime that no installed rawr uses and no build used in the last hour, when it compiles a new one. (#963)

## 0.3.148 (2026-11-06)

- A function or closure that native code calls on another thread and that gives a value of a program type, as iced's `Subscription::map(|_| Message::Tick(Local::now()))`, now runs on the program's thread where its value is first used, so it can make imported values. iced's clock runs. (#981)
- An opaque program whose bound binds an associated type to a tuple, as `application::timed(..)`'s `Message = (Message, Instant)`, keeps that binding, so `.theme(..)` and the other builder methods read it. iced's solar_system runs. (#979)

## 0.3.147 (2026-11-05)

- A crate's derive macro on a program type is expanded by rustc and runs as part of the program. A derive whose expansion is a plain impl runs as `cargo run` does, and an error in an expansion is reported at the derive with rustc's note. serde's `Serialize` and `Deserialize` now reach their expansion's generic methods, which aren't supported yet. (#985; part of #593)
- `std::result::Result`, `core::option::Option` and `Box` written by their std paths resolve, and a crate's re-export of a std type, such as serde's `__private::Result`, is that std type. (#995)

## 0.3.146 (2026-11-05)

- A number on an operator's left with a crate's type on its right, as ferris's `(rotation + step) % (2.0 * Radians::PI)`, calls the crate's impl of the operator for that number (`impl Mul<Radians> for f32`), as rustc selects it. A number whose impls don't take the other type is rustc's E0277. (#974)
- A program with no dependencies can use `async fn`, `async` blocks and `.await`, which check and run as rustc's do. `#[tokio::main]` or `tokio::join!` there is rustc's E0433, and an `async fn main` is E0752. (#968)
- A crate value inside an `Option` or `Result` below a variant's fields, a tuple or an array now has its type at any depth, so layout's key press with `text: Option<SmolStr>` no longer fails. A by-value method's `Option` output, such as `Cursor::position_in`, now has its type too. A crate's closure that returns an `Option` now compiles. (#971)

## 0.3.145 (2026-11-05)

- A view's widgets are made at the view's lifetime even when nothing they take borrows, and a call that takes a script closure takes the view's widgets too: iced's table, multi_window and bezier_tool views no longer panic. (#952, #352)
- A crate type named at two paths, as a macro's `$crate` names it, is one type when a value crosses: custom_quad and gradient run. (#953)
- Two crate types sharing a name no longer stop a runner at start, and a borrow of a crate value or a byte slice fills an `impl Into<_>` parameter: sierpinski_triangle, arc, clock and multitouch run, and solar_system builds. (#954)
- The generated runner builds when a program reads a field that borrows for its struct's lifetime, as iced's `Stroke { width, ..Stroke::default() }` reads `line_dash`: the read no longer gives the borrow out of a lend (E0521). (#951)
- rawr-import-driver can expand a crate's derive macro on a program's type with the pinned nightly, and keeps each expansion in the project's target/rawr/expand/. (#984; part of #593)

## 0.3.144 (2026-11-05)

- Programs can write the items a derive expands to: `const _: () = { .. };` blocks of items, `extern crate x as y;`, paths through a renamed module (`use std as s;`), a turbofish on a program method or a trait's path (`v.f::<T>()`, `Trait::f::<T>(&v)`), and a generic trait function called by its path. (#976; part of #593)

## 0.3.143 (2026-11-05)

- A crate's derive macro on a program type, such as serde's `#[derive(Serialize)]`, resolves through the program's `use`, at the top of the file or in a function body, and is reported at its span as not supported yet, where `rawr check` used to say it couldn't find the derive. (#975; part of #593, #205)

## 0.3.142 (2026-11-05)

- A crate's From impls for several std types convert through the one the target names: f32::from(rotation.degrees()) and .into() pick From<Degrees> for f32 beside the f64 one. A crate function casts to a fn pointer type with inferred parts, and an array's later crate functions coerce to it. A closure parameter used as a crate's RangeInclusive<T> takes T from the call's other arguments, so 2.0..=80.0 is a range of f32. (#956)

## 0.3.141 (2026-11-05)

- `rawr run` starts with a relative CARGO_TARGET_DIR (and with an absolute one after a relative run): the reload watcher no longer stops at start with "malformed filesystem observation transport". A relative target directory now names the same paths as the absolute one. (#959)

## 0.3.140 (2026-11-05)

- A crate's `impl Trait` result that borrows, as iced's layout gets from `element.explain(color)`, is held in the scope that lends to it, whether or not a public path names its type, and also through `.explain(a).explain(b)`. Keeping it past the block of a local it borrows is rustc's E0597. (#936)
- A path to a std type's method is a function value and a call, as in rustc: `.flat_map(HashMap::values)`, `.map(Vec::len)`, `str::trim`, `char::to_ascii_uppercase`, `Iterator::count`, `std::cmp::max`, `String::new`, and `Vec::len(&v)` or `String::push_str(&mut s, ..)` called by path. Each takes its receiver as std declares it, so passing a `&self` method where values are given is rustc's E0631, and a path doesn't reach through `Deref` (`String::trim` is E0599). (#926)
- A crate type's methods are function values and path calls: `Look::to_string` where `impl Fn(&Look) -> String` is taken, `Look::name` as a `fn` pointer, `Look::mix(a, b)`, `Look::to_string(&x)`, and a crate trait's method as `Tone::tone` or `<Look as Tone>::tone`. (#319)
- A crate's async function that takes a `&str` through a type parameter, as `tokio::fs::read_to_string(path)` does, runs: rustlings' async1 runs in a Cargo project with tokio. (#970)

## 0.3.139 (2026-11-05)

- A program's own `macro_rules!` macros expand as rustc expands them: all fragment specifiers, repetitions with separators, rules tried in order, hygiene for the macro's locals, nested and recursive calls up to the recursion limit, and Rust's textual scope with `#[macro_use]` and `#[macro_export]`. A call used as a statement splices in its statements. The errors are rustc's: `no rules expected`, `cannot find macro`, which points at a definition that comes later or asks for `#[macro_use]`, and its errors for a malformed definition. All of rustlings' macros exercises now match rustc. (#236)

## 0.3.138 (2026-11-05)

- `std::thread::spawn` runs each closure on a thread of its own, in parallel; `join` gives its value or `Err` for a panic, and a program exits when `main` returns. `Arc<Mutex<T>>` locks with `lock().unwrap()`, and `std::sync::mpsc` channels send, receive and end a `for` loop when every sender is gone. A closure that captures an `Rc` is E0277, one that borrows a local without `move` is E0373. (#901)
- `Chars::as_str` gives the text left in a `chars()` iterator. (#923)
- `Rc<T>`, `Arc<T>`, `Box<T>` and `&T` meet a `T: Display` bound, as std's impls say, and a generic `{}` writes what they point to. (#928)
- `let c: Result<Color, _> = x.try_into();` takes its error type from the `TryFrom` impl its target selects, and `Color::try_from(&v[..])` selects the `&[i16]` impl, the one a literal inside its argument fits. (#927, #944)

## 0.3.137 (2026-11-05)

- A `let` may leave out its value (`let x;`, `let x: i32;`). A later assignment gives the local its value, and its type if the `let` names none. A use before that is rustc's E0381, also after a branch that may not have assigned it. A second assignment to a local not declared `mut` is E0384. A borrow the assignment stores is checked like any other borrow (E0597). (#149)
- An integer literal its type can't hold, such as `let x: u8 = 300;` or `-129i8`, is refused with rustc's deny-by-default `overflowing_literals` error, worded as rustc words it. (#932)
- A path into a module that has no such item, such as `snacks::FRUIT`, is rustc's E0425 "cannot find value `FRUIT` in module `snacks`". It was E0432. (#933)
- Errors come in the order rustc gives them: impl methods are checked in source order with free functions. A number's `.into()` converts to the type its use needs. A type nothing settles is E0282 or E0283, as for `Vec::new()` with only `.into()`s pushed, or a method of `let x = todo!();`. `Result<T, _>` from `try_into()` takes the impl's error type. `Cow<[T]>` indexes and comes from `&Vec<T>` and `Vec<T>`. `todo!()` in a pattern is E0532. (#934)

## 0.3.136 (2026-11-05)

- `collect()` into a `Result` or an `Option` works, including when the target comes only from the return type, and stops at the first `Err` or `None` as Rust does. A set or map collected from the return type is built as one. `let Ok(x) = s.parse() else { .. }` with a target a later use gives no longer panics. (#906)
- A `mut self` or `&mut self` method on a std type can change it (`self.push(..)`), and a changing method runs on a temporary (`vec![..].resize(0, 5)`), as in Rust. (#907)
- A struct or enum that holds itself by value is refused with rustc's E0072, and a `collect()` whose items its target can't be built from gets rustc's E0277, even in a function nothing calls. (#911)
- Starting rawr is quick again: building the std bindings takes about 20 ms instead of 0.35 s with a release build, so `rawr check`, a restart on save and a program started from `rawr mcp` no longer wait on it. (#931)

## 0.3.135 (2026-11-05)

- A function of yours can take a parameter such as `impl Into<Length> + Copy` and pass it on to a crate, as iced's layout example's `square` does. The bound holds of each type the crate's `From` impls convert from, and a number literal takes the type of the one impl of its kind, so `square(50)` passes a `u32`, as rustc infers it. (#912)

## 0.3.134 (2026-11-05)

- Errors use rustc's codes and wording: mismatched types E0308, an `if` without `else` E0317, `return;` E0069, unknown names E0425, E0433 and E0432, E0423, assignments E0384, E0594 and E0596, and `?` or mixed-number arithmetic E0277. Syntax errors and an unknown macro print a plain `error:` as rustc does. (#897, #909, #910)

## 0.3.133 (2026-11-04)

- `rawr test` runs a file's or a crate's `#[test]` functions as `cargo test` does: `cfg(test)` holds under it, `#[should_panic]` (with `expected`) and `#[ignore]` behave as in cargo, it takes cargo's name filters and libtest's options, reports in libtest's format and exits with 101 when a test fails. A syntax error in an item a `#[cfg]` leaves out stops `rawr run` and `rawr check`, as rustc parses every item. (#898)
- Modules see what Rust lets them see: a module's private methods and associated functions are visible to its descendants (`mod tests { use super::*; }`), `use super::*;` brings in the parent's own imports, and a glob-imported name can start another `use`. From any other module a private method is E0624 and a private item E0603. Unknown functions and values are rustc's E0425. `{self:?}` in a method reads the receiver. (#899, #900, #903)
- More of std runs: `SystemTime` (`now`, `UNIX_EPOCH`, `duration_since`, `elapsed`), `ParseIntError::kind` with `IntErrorKind`, `char::to_uppercase`/`to_lowercase` and `Result::as_deref`. A type holding `ParseIntError`, another std error or a `Duration` derives each trait std implements for it. String literals span lines, and a `\` at a line's end skips the next line's leading whitespace. (#902, #904, #905)

## 0.3.132 (2026-11-04)

- A `parse()` whose target a later call in its chain gives, followed by a closure that formats (`.map(Command::Push).map_err(|e| format!(..))`), runs as in Rust. The playground's "Enums, match and ?" example runs again. (#918)
- A use after a move reads as rustc's E0382: `borrow of moved value` when the use borrows (as `println!("{x}")` does), pointing at the name inside the format string, with the moved value's type at its declaration. (#919)
- Every playground example is tested against rustc's output, so an example that stops matching fails the build. (#920)

## 0.3.131 (2026-11-04)

- A crate's opaque value whose type arguments only a later use fixes now runs: `widget.map(..)` inside `column!` takes its message, theme and renderer from the later use, as rustc infers them. An opaque type has its bounds' supertraits too. A column of opaque widgets alone, as `column![opaque(slider(..))]`, is the view's widget. (#886)
- A crate's implementation of a std trait for a std type is callable: with iced's `impl From<Degrees> for f32`, `f32::from(degrees)` and `degrees.into()` convert through it, and a missing one is rustc's E0277. (#887)

## 0.3.130 (2026-11-04)

- A program runs in a project as it would in a fresh one after other programs ran there. A crate's types reached through its macros' `$crate` are named at their public paths, as rustc names them, and they are one type to the checker at every path. (#893)

## 0.3.129 (2026-11-03)

- A trait's method whose `impl Trait` the trait's own default body gives, as iced's `Widget::explain`, now runs: its result goes in `column!` as it is, and its trait's methods and the methods a blanket impl over that trait gives, as `.explain(color).boxed()`, run on it. (#885)
- `rawr check` now refuses your own impl of a crate's trait used at a type that holds the crate's `impl Trait` value, as iced's loupe uses `impl Widget for Loupe<W>` at `Loupe<impl Widget>`, instead of passing a program the runner can't build. (#885)

## 0.3.128 (2026-11-03)

- The never type `!` works as rustc has it: a function, method, closure or fn pointer can return `!`, a call of one is a value of whatever type its place takes in `if`/`match` arms, `let` and `let .. else`, a crate's `-> !` function is imported, and `std::process::exit` and `abort` run. `!` anywhere else gives rustc's E0658. (#896)

## 0.3.127 (2026-11-03)

- ?play snippets can tick with `iced::time::every`, as iced's stopwatch and clock do. (#890)
- A function's return type can hold `impl Trait` inside a generic type of a crate's or the program's own, as iced's loupe gives `Loupe<impl Widget<Message> + 'a>`; the hidden type is the body's, as rustc infers it. (#845)

## 0.3.126 (2026-11-03)

- ?play's GIFs play at the program's pace, not the bot's drawing speed: a change made on a frame shows for 50 ms however long the frame took to draw, and a change made on the program's own clock shows for the time that clock ran. (#889)

## 0.3.125 (2026-11-03)

- ?play has no time limit while no other snippet is waiting. Once one waits, the running snippet gets the usual budget (60 s of script time, 120 s in all) from that moment, then stops with its output so far; a run nothing waits behind stops after 10 minutes. (#880)
- Releases run in stages (binaries, playground, bot, site): the ?play image is rebuilt only when its sources change and with cached dependencies when it is, and a Worker settings change deploys in minutes. (#881)
- An animated iced snippet on ?play answers with a GIF of every frame it drew on the bot's slow CPU, instead of a PNG of its first frame. (#888)

## 0.3.124 (2026-11-03)

- Methods std marks stable on their own, such as `std::io::Error::new`, `other` and `kind`, and methods of `Condvar`, `RwLock`, `PoisonError`, `LocalKey`, `Cursor` and `Take`, are no longer refused as unstable library features. (#838)

## 0.3.123 (2026-11-03)

- `*w` on your type with `Deref` reads through its `deref`, `*w = x` writes through its `deref_mut` (rustc's E0594 without one, E0507 for a move out), and a `DerefMut` impl may write `&mut Self::Target`. (#858, #859)
- A text `.into()` becomes the type its use needs once inference has it, such as a `Cow<str>` from the other branch of an `if`, and `return *p = P::default()` is accepted as rustc accepts it. (#860, #862)
- A `Cow` that ends a ?play snippet prints its text, not a Rust type name. (#861)
- A path through a crate module a `use` binds reaches the modules nested under it at any depth and through re-exports, as `operation::scrollable::snap_to(..)` after `use iced::widget::operation;`. (#851)
- A crate function that takes a borrowing `impl Trait` result by value, as `fn wrap(shown: impl Show) -> impl Show`, takes it with the values it borrows, which stay borrowed as long as its result lives, as rustc has it. (#856)
- ?play refuses a call its universe doesn't bind when it checks the snippet, with an ordinary error at the call, instead of panicking at run time, and its universe now covers spacing, text of a `String`, sliders including `Radians::RANGE`, `pick_list`, `keyboard::Key` matching and a subscription with a scale factor. (#863)

## 0.3.122 (2026-11-03)

- ?play gives a run 15 seconds of script time and 30 seconds in all, so programs that fit comfortably on a laptop don't hit the time limit on the bot's shared container. (#872)

## 0.3.121 (2026-11-03)

- A result that borrows its call's values works whatever its trait: generic methods at several types, methods that give `Self`, associated types the result binds and supertrait methods all run, through a proxy rustc still checks. A `Self` such a method gives keeps the same values borrowed, as rustc has it. (#849)

## 0.3.120 (2026-11-03)

- rawr-import reads crate APIs with the nightly the project names (`cargo +nightly`, `RUSTUP_TOOLCHAIN`, a rustup override or `rust-toolchain.toml`), from nightly-2026-07-11 through nightly-2026-11-02; a project that names none, or a stable or beta release, reads with the pinned nightly and still builds on its own toolchain. A nightly other than the pin gets its driver and std table built once into rawr's home. (#692)
- A crate function's `impl Trait` result that borrows a slice of crate values, as `fn view<'a>(items: &'a [Item]) -> impl Show + 'a`, keeps them borrowed as long as rustc says: changing or moving them while it lives is rustc's E0502 or E0505, they're read in place rather than copied, and they go back to their `Vec` when the result drops. (#846)
- A daily CI job builds rawr-import-driver on the latest nightly and opens a pull request moving the supported nightly range to it, or files one issue with the compiler errors when it no longer builds. (#864)

## 0.3.119 (2026-11-03)

- A crate a macro names that the project reaches only through a `[patch]` no longer joins the runner's workspace, so the runner resolves no more than the project does and settles offline; the ?play image builds again. (#854)

## 0.3.118 (2026-11-03)

- A crate type that implements `Ord` can key a `BTreeMap` or `BTreeSet`, as iced's `window::Id` does in multi_window, and the map keeps the crate's order. (#363)
- An imported generic enum is matched and constructed at the types its use gives, with declared defaults filling what nothing else fixes, as iced's `keyboard::Key` in `KeyPressed { key: Key::Named(Named::Tab), .. }` and `key.as_ref()` matches. (#841)
- A crate constant whose type is a `RangeInclusive` of the crate's own type, such as iced's `Radians::RANGE`, can be read and passed on, so `slider(Radians::RANGE, self.angle, ..)` makes a slider of `Radians`. (#842)
- `.into()` on a crate value read through a field chain converts through the target's `From`, and a crate function generic over a trait it also implements for closures, such as iced's `selector::find(id)`, takes a plain value. (#843, #844)

## 0.3.117 (2026-11-03)

- A crate enum's field bound through `&T` is borrowed where it is, a `Clone` field too, and the borrow lives as long as Rust's: a function can return it, a `match` can give it to a `let`, and a `let else` keeps it. (#783, #782)
- A crate function or method taking `&mut PathBuf` changes the program's own path with no copy, and a crate method giving `&io::Error` gives a borrow of the error its value holds. (#680)
- The std facade's `path`, `ffi`, `io`, `env`, `fs` and `process` items have std's own signatures, read from std's table: a parameter bounded by `AsRef<Path>` takes a `&str`, `String`, `OsString` or `PathBuf`, `read_line` takes `&mut String`, and each type has exactly std's capabilities. (#747)
- `&value.field` of a crate struct lends the field in place instead of moving the struct, so `save(&screenshot.rgba, screenshot.size.width)` runs as in Rust, and a `&Bytes` coerces to the `&[u8]` its `Deref` gives. (#228)
- A crate type that a signature gives but that has no public path in the crate you import, such as iced's `widget::Map` from `Widget::map`, works as in Rust; iced's bezier_tool checks again. (#334)
- `Option`'s `clone` borrows its receiver again, so `self.t.clone()` through `&self` no longer gives a false E0507; iced's qr_code checks again. (#826)
- A crate function that lends an `Option<&T>` at its own lifetime crosses, as iced's `combo_box(&state, .., self.selected.as_ref(), ..)` does, and a late-bound lifetime a function's output borrows at is read as rustc reads it at a call. (#827)

## 0.3.116 (2026-11-03)

- `v.get(a..b)` on a Vec, array or slice gives `Option<&[T]>`, as std does. (#772)
- A `&mut self` method of a program type on a local not declared `mut` reports rustc's E0596. (#774)
- `"a" + "b"` and other operators a type has no impl of report rustc's E0369 in its words. (#733)
- `.into()` on text with nothing to give its target reports rustc's E0283. (#736)
- A std type written with too few or too many type arguments reports rustc's E0107. (#752)
- An unstable std method reports rustc's E0658 with its feature and tracking issue. (#771)
- A `VecDeque`'s own `remove`, `swap_remove_back`, `swap_remove_front`, `rotate_left`, `rotate_right`, `range`, `as_slices` and `make_contiguous` run, with std's panics and borrow checks. Holding a `range` or `drain` iterator across a change of its collection is rustc's E0502 or E0499. (#779)
- A program can implement one trait for both `[T]` and `Vec<T>`: a `Vec` uses its own impl or the slice's through `Deref`, and a slice never uses the `Vec`'s (rustc's E0599). (#781)
- A closure passed to a std method has the parameter types std declares for it, so `v.sort_by_key(|x| -*x)` needs no annotation, and a std method rawr doesn't run yet gives the not-yet error rather than an error inside the closure. (#795, #770)
- A pattern matched through a reference binds one more reference to a member that is already one, as rustc does: in `let (_, x) = &(1, &n);`, `x` is `&&i64`, so closures like `filter(|(_, x)| **x > 1)` work. (#794)
- An `iter_mut()` kept in a local can be stepped with `next()`, as in a `while let Some(x) = it.next()` loop. (#693)
- A std type outside the prelude, such as `HashMap`, `RefCell` or `Duration`, written with no `use` is refused with rustc's error (E0425 in a type, E0433 at the head of a path). (#751)
- A match on a crate enum reads each field it needs on its own: nested patterns such as `A(B(x))` work, and so does binding only a `Copy` field beside a field without `Clone`. A match covering a variant through the patterns below it is accepted as exhaustive. (#785)
- `ref mut` bindings and matches through `&mut T` lend a crate enum's crate-value fields mutably, so a change through the binding stays in the matched value. (#786)
- An `Rc<T>` or `Arc<T>` of a crate's type crosses as the crate's own pointer, keeping its count and identity, with nothing copied. (#775)
- `HashMap`, `HashSet`, `BTreeMap` and `BTreeSet` keyed by strings, numbers, chars or bools cross to and from crates; a `&HashMap` of crate values lends each value without a copy. (#777; part of #708)
- Method calls, fields and `&T` coercions go through a crate type's own `Deref`, and `&mut self` methods through `DerefMut`, for crate and program types alike; without a `DerefMut` it is rustc's E0596. (#762, #764)
- `use std::borrow::Cow` works: `Cow<'_, str>` and `Cow<'_, [T]>` are std's, with `Borrowed`/`Owned`, `into_owned`, `to_mut` and every method of their target. `*` of a `&str` or `String` is rustc's `str`. (#701)
- rawr can be installed with one command, `curl -LsSf https://rawr.airstrike.fyi/install.sh | sh`, from prebuilt macOS and Linux binaries, and rawr-import installs the Rust nightly it reads crate APIs with through rustup the first time it needs it. Windows gets `rawr` through install.ps1, best effort. (#797)

## 0.3.115 (2026-11-03)

- An iterator's methods and what each call borrows come from std's own declarations of Iterator, DoubleEndedIterator, Peekable and IntoIterator, and of std's adapter and FromIterator impls. A method std declares that rawr doesn't run yet, such as `rposition` or `next_back`, says so instead of being an unknown method, and calling `next` on an iterator not declared `mut` is rustc's E0596. (#768)
- A ratatui builder method that keeps a string, such as `Block::title("list")`, works under `rawr run` again, and a program using `ratatui::DefaultTerminal` builds its runner. (#787)
- A marked file may use `#![cfg_attr(..)]` and build-setting crate attributes such as `#![windows_subsystem = "windows"]`, `crate_name`, `recursion_limit` and `doc` after its `// rawr` marker, as rustc allows. Attributes that change the prelude or the language, such as `no_std` and `feature`, are refused with an error that names them. (#791)

## 0.3.114 (2026-11-02)

- `Vec<T>`, `[T]`, `[T; N]` and `VecDeque<T>` are four types, as in Rust. A `&Vec<T>` or `&[T; N]` coerces to `&[T]` through std's `Deref`, a range of a `Vec` and a crate's `&[T]` are slices, and calling another sequence type's method (`push` on a `&mut [T]`, `push_back` on a `Vec`, `sort` on a `VecDeque`) is rustc's E0599. (#769)
- `VecDeque` has std's own methods and borrow summaries. rawr runs `push_front`, `pop_front`, `push_back`, `pop_back`, `front`, `back`, their `_mut` forms and the methods it shares with `Vec`; its other std methods give the not-yet error. (#769)
- A `match`, `if let` or `let`-`else` on a crate enum binds the variant's fields as rustc does: by value it moves fields such as an `io::Error` out instead of cloning them, and through `&T` it lends a field without `Clone` to the arm, where `&self` methods and `&T` parameters reach it while the matched value is read too. Testing a variant no longer reads its fields. (#678)

## 0.3.113 (2026-11-02)

- A crate macro whose `$crate` path points into a path dependency expands under `rawr run`, a struct built through a type alias constructs correctly, and moving a field out of a borrowed variant reports E0507 as rustc does. (#778)

## 0.3.112 (2026-11-02)

- A std type in a crate's signature is rawr's own type in every position: functions take tuples and `Range<usize>`, and take and give `Box<T>`, read from std's own table. A std type rawr can't pass to a crate yet, such as `Rc`, `Arc` or `HashMap`, reports "rawr doesn't run `X` yet" where the function is called. (#746, #707)
- A crate type's `Index`, `FromStr` and `Iterator` impls work as its own trait impls do: `grid[i]`, `"1,2".parse::<Coords>()` and a `for` loop over it call them. (#709)

## 0.3.111 (2026-11-02)

- `Rc` and `Arc` are real shared pointers: `Rc::strong_count`, `weak_count`, `ptr_eq`, `downgrade`, `Weak::upgrade`, `get_mut`, `make_mut` and `try_unwrap` give rustc's answers, `Rc<RefCell<T>>` mutation is visible through every clone, and an `Rc` no longer passes a `T: Send` bound. (#748)

## 0.3.110 (2026-11-02)

- Inherent methods of `Option`, `Result`, `char`, floats, ints, `String`/`str`, `Vec`/`[T]` and maps come from std's declarations: their receiver modes (`into_bytes` moves, `sort` takes `&mut self`, a program trait comes before a method a Deref step away) and what each call keeps of its inputs. A std `&mut self` method on a local not declared `mut` is rustc's E0596, and a std method rawr doesn't run yet is the not-yet error. (#745)
- `x.try_into()` into a program type uses its `TryFrom` impl, and `x.clone_from(&y)` works on any `Clone` type through method lookup. (#757)

## 0.3.109 (2026-11-02)

- `Message::Loaded.with(id)`, and any method a crate trait's blanket impl gives every `Fn`, run the crate's own code on your functions, variant constructors and closures, and the `impl Fn` it returns passes to `Task::map`, `Task::sip` and other callable parameters. (#597)

## 0.3.108 (2026-11-02)

- A method a blanket impl gives through another trait, whose input is the associated type that impl sets, as palette's `hsl.darken(0.075)`, resolves at that type. (#727)
- A crate function with several callable parameters takes a capturing closure in each, as iced's `Task::sip(sipper, Message::Progress.with(id), Message::Done.with(id))` does, and a closure input that holds a crate value, such as `Result<Bytes, String>`, crosses. (#728)

## 0.3.107 (2026-11-02)

- std's trait impls on rawr's own types come from std's own declarations: `==`, `<`, Deref coercion and `From`/`Into` select the impl rustc would, and refuse the rest with rustc's errors. (#744)
- A program's own `impl Deref` works: methods and fields are found through it, `&Wrapper` coerces to `&Target`, and `Option::as_deref` gives the target's reference (`Option<Vec<u8>>` gives `Option<&[u8]>`). (#710)
- `BTreeMap`, `BTreeSet`, `Cell`, `RefCell` and `Result` compare with `<` as std orders them, and `==` follows std's `PartialEq` impls. (#704)

## 0.3.106 (2026-11-01)

- A program implements any stable std trait Rust lets it implement (`FromStr`, `AsRef`, `AsMut`, `Borrow`, `Deref`, `TryFrom`, `IntoIterator`, `Extend`, `Sum`, `fmt::Write`, `io::Write` and the rest), declared as std declares it, from std's own table. A missing item is rustc's E0046 and a trait nothing brings into scope is E0405; where rawr can't run a trait's machinery yet, the use fails with "rawr doesn't run `X` yet". (#743, #705)
- `s.parse()` finds a program's `impl FromStr` by the turbofish or the expected type, with the impl's `Err` as the error, a `for` loop takes a program's `IntoIterator`, and `write!` writes through a program's `fmt::Write`. (#705)
- Trait functions called by path resolve through std's table and method lookup instead of a list: `Iterator::next(&mut it)`, `Clone::clone_from(&mut a, &b)` and `std::cmp::max_by_key(a, b, f)` and its kin. (#711)

## 0.3.105 (2026-11-01)

- A provided trait method that lends `Self` to a closure, as `renderer.with_translation(v, |renderer| ..)`, no longer panics a debug build during a cold check's miss-gathering pass (first bad #661–#663, 775663c7c; fixed in 6e794f02c). (#754)
- A guard-returning method's mutable place reads compile again in the generated runner: two byte-string templates had kept `{copy}` as written (first bad #653, 8810b6fb5; fixed in 651ef5b8f, generator 127). (#755)
- quick-xml's `read_event_into` loop with attributes runs under `rawr run` as `cargo run` does: a borrowing call's receiver isn't moved by its frame binding, a closure's parameters are given their value at each body entry, lifetime-carrying types are read from the arena instead of cloned out of an accessor, and a frame's `&mut Vec` goes back into the place it came from. It had failed since 30b8a530c (2026-10-26). (#756)

## 0.3.104 (2026-11-01)

- std, core and alloc paths resolve as rustc resolves them, through every re-export and glob, from std's own table for the pinned toolchain. A path std doesn't have gets rustc's error (E0432, E0433, E0425), an unstable item gets E0658, and an item rawr can't run yet fails where it is used with "rawr doesn't run `X` yet". `HashMap<K, V, S>` and `HashSet<T, S>` take their hasher, and `core::collections`, `core::thread::sleep`, `core::time::Instant`, `core::rc::Rc`, `core::sync::Arc` and `core::string::FromUtf8Error` are refused, as rustc refuses them. (#742, #712)

## 0.3.103 (2026-11-01)

- A `&str` inside a tuple or `Option` input, such as an async fn's `(&str, &Catalog)`, and a string literal beside a widget in `column!` are typed `&str`, as rustc reads them, after #706 made `String` and `&str` distinct. (#750)

## 0.3.102 (2026-11-01)

- A `Vec` of crate values given to a crate function moves its elements, as Rust does, instead of cloning each one. (#698)
- A crate call's `&mut String` or `&mut Vec` is the program's own value, lent in place: room a call reserves is still there at the next call, and `+=` appends to it where it is. (#716)
- A crate trait implemented for `&str` takes a `&str` receiver, borrowed at the call's or view's lifetime, and binds even when the impl has no type parameters. (#717)

## 0.3.101 (2026-11-01)

- A type parameter bounded by `Into<String>`, `AsRef<str>`, `AsRef<Path>`, `Into<PathBuf>`, `AsRef<[u8]>` or `ToString` takes what std implements the bound for, however the parameter is written, and a string literal is lent rather than copied. (#713)
- An async fn whose future borrows `&Path`, `&[u8]`, `&String`, `&Vec<T>`, a number or a `Result` holding a borrow runs, with each borrow held by the future. (#714)
- A crate function taking an `FnMut` with no `'static` bound, such as image's `ImageBuffer::from_fn`, takes a closure that changes the locals it captures, and function pointers of any number of inputs take your functions. (#715)

## 0.3.100 (2026-11-01)

- The ?play universe runner starts again: a generated runner installs each crate's bindings after those of the crates whose types they use, so iced no longer waits on iced_test's bindings. (#738)

## 0.3.99 (2026-11-01)

- A closure or function whose parameter is `T` where the callable takes `&T` is refused with rustc's E0631, "type mismatch in closure arguments", naming both signatures. (#722)
- An `.into()` whose target nothing fixes is refused with rustc's "type annotations needed" (E0283) instead of converting to itself, and a `Vec` of widgets is no longer converted one by one where a list of Elements is expected. (#723)

## 0.3.98 (2026-11-01)

- A type missing a trait reads as rustc's E0277 ("the trait bound `T: X` is not satisfied", or "`T` doesn't implement `Debug`"), with a "required by a bound in" note, and suggests `#[derive(X)]` only for a derivable trait on the program's own type. (#718)
- A crate path that can't be used yet gives the Rust reason ("`x` can't be used yet: it takes a closure that isn't `'static`"), labelled where it is defined and exported, instead of an internal rejection name or "retained" evidence. (#719, #720)
- A failure in a crate call that only a rawr bug can cause panics as a rawr bug instead of printing a bare internal name. (#721)

## 0.3.97 (2026-11-01)

- `String` and `&str` are separate types, as in rustc. A string literal where a `String` is declared is E0308, a `&String` becomes a `&str` only by deref coercion, `String +` takes a `&str`, and `.into()`, `.to_string()` and `String::from` make the `String` a declaration needs. (#706)

## 0.3.96 (2026-11-01)

- A native value whose type is a future, such as iced's gallery's `impl Straw<..>`, can be passed to a function that takes `impl Future<Output = T>`, such as `Task::future`, and `T` is its output. (#637)
- A method whose input is an associated type its impl sets, such as palette's `shift_hue(self, amount: Self::Scalar)`, can be called. (#638)

## 0.3.95 (2026-11-01)

- rawr's nightly check build, which reads your dependencies' APIs, is kept once per machine in rawr's home instead of in each project's `target/rawr`. A second project checks only its own crates there: a fresh copy of iced's download_progress compiles 2 units instead of 274. `rawr cache clean` is back and also removes these build directories once they have gone unused. (#691)

## 0.3.94 (2026-11-01)

- `==` and `!=` choose the `PartialEq` impl rustc chooses: references compare through what they refer to, `String`, `str` and `&str` compare with each other, and `Vec`s, slices and arrays compare element by element. `&String == &str`, `&str == String`, `Vec<u8> == [u8; 3]` and `names.iter().position(|name| name == "Serif")` compile, and `&a < &b` compares what the references point to. A comparison rustc refuses gets its E0277, "can't compare `X` with `Y`", at the operator, with rustc's help and note. (#699)

## 0.3.93 (2026-11-01)

- A method that lends a slice of strings or crate values from `&self`, as iced's `combo_box::State::options()` does, works in place with nothing copied: `.len()`, indexing, `for` loops and iterator methods read the native elements. (#687)

## 0.3.92 (2026-11-01)

- A native module's function can take and give a dependency's types, by value or by reference: with `mod label;`, `label::describe(&version)` works when `label.rs` has `pub fn describe(version: &semver::Version) -> String`. (#685)

## 0.3.91 (2026-11-01)

- A pattern's binding of `o.as_mut()` on an `Option` or `Result` place writes through `*`, in `if let`, `match`, `while let` and `let .. else`, as `if let Some(n) = self.pending.as_mut() { *n -= 1; }` does in Rust. (#686)
- A tuple expression with an expected type coerces each field to it, so `pair((&name, &catalog))` passes a `&String` where `pair` takes `(&str, &Catalog)`; a tuple already held in a local isn't coerced, as in rustc. (#688)

## 0.3.90 (2026-11-01)

- An async function whose future borrows its arguments crosses whatever the arguments' types are, as fount's `google::load(family: &str, catalog: Option<&Catalog>)`: the future holds a crate value lent from its place, an `Option`'s `Some` payload, each member of a tuple of borrows, and the program's own string, with nothing copied. A path to a module's item rawr can't bind reads as rustc's E0425 ("cannot find function `load` in module `google`"). (#677)

## 0.3.89 (2026-11-01)

- `rawr check` of a program with its own Rust modules reuses its dependencies' shared bindings in a new project and extracts only the program's own crate: upstream download_progress takes 1 nightly build and 1 extraction instead of 3 and 3. (#672)

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
