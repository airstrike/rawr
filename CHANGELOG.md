# Changelog

Each merge to main bumps the patch version. A minor version marks a milestone.

## Unreleased

- The next milestone is 0.6, Rust as rustc has it: code rustc accepts runs in rawr with rustc's semantics and errors.
- A `&mut` view of a collection's elements kept past its statement reaches them as rustc's does: `&mut v[a..b]`, `as_mut_slice()`, `make_contiguous()`, `split_at_mut`'s halves (side by side, from closures and scoped threads), `chunks_mut`'s chunks, `first_mut` and `last_mut` of a view, and a kept `iter_mut()` walked, zipped, reversed, enumerated, filtered, collected into a `Vec<&mut T>` or passed to a function. rustc's E0499 and E0502 hold while a view lives, and a view out of bounds panics with std's message. (#800, #806, #805)

## 0.5.16 (2026-11-22)

- A `&mut` local can be given a new borrow of a field or an element (`r = &mut s.right`), and a `&mut` chosen by an `if` or a `match` (`let r = if c { &mut a } else { &mut b };`) runs as rustc's build runs it. (#121, #122)
- A branch's borrow is no longer live through the other branch or the next loop iteration, which ends false E0499 and E0502 reports for references chosen by an `if` or a `match` in a loop. (#122)
- A `&mut self -> &mut Self` method chain on a temporary of the program's own type (`Config::new().time().build()`) runs, and keeping its result past the statement is E0716. (#2074)
- A `&mut` local moved into `vec![..]` or `[..]` is E0382 when used again. (#2502)
- An untyped closure's arithmetic, such as `(|a, b| a + b)(3, 3)`, takes the types its call gives, with overflow checked at that kind. (#2512)
- `Trait::m("hi")` and `<str as Trait>::m("hi")` call a trait implemented for `str`. (#2514)
- A generic trait method called through its path with a turbofish (`Conv::conv::<u16>(&s)`) is tested. (#2513)
- A function's `I: Iterator` or `I: IntoIterator` parameter with no `Item` written has the iterator's methods. (#2526; part of #2507)
- A method whose impl bounds the receiver misses is rustc's E0599 'the method exists but its trait bounds were not satisfied', with its labels, note and derive help. (#2504)
- An impl header is checked against its trait's bounds on its parameters (E0277), and `Debug`'s E0277 carries rustc's derive note. (#2505)
- A returned value that breaks several lifetimes of its signature gets each of rustc's errors, then its help relating the bounds. (#2487)
- A lifetime error on a returned `if` or `match` marks the branch the borrow comes from. (#2488)
- A lifetime error between two elided lifetimes suggests a named lifetime, as rustc does. (#2489)
- A `use` of a name its module declares is rustc's E0255, with its labels, note and `as` suggestion. (#2497)
- A mismatched argument is reported at the argument, with rustc's callee label, the note at the function's definition and the help on a generic's return type. (#2498)
- Temporaries drop where rustc 2024 drops them: a `let` keeps `&D(..)`, `Some(&D(..))` and a tuple's borrowed member to the block's end, an `if let` subject's drop before the `else`, a `while let` subject's drop on every iteration, and `D("x").ok();` runs. (#870)
- A block's tail and a `Box::new` keep the borrows of their value, so E0716 and E0502 come from rawr, not `cargo check`. (#159, #147, #161)
- A move out of a `static` is E0507, and a static of a `Cell`, `RefCell` or `Rc` type is E0277 in rustc's words. (#192; part of #1563)
- A `&mut` stored in a struct, a tuple, an `Option` or a `vec!` refers to its place, so writes through it change the original, and `v.last_mut()` kept in a local then unwrapped writes the element. (#184, #195, #2503)
- Two `&mut`s stored in values that borrow disjoint parts of one local run without a data-race error, including a method on one part given a stored `&mut` of another. (#184)
- A `static` is one value: its initializer runs at its first read, and every read, call and thread shares it, so a `Mutex` static keeps what is pushed into it. A hot reload that edits other code keeps a static's state, and one that edits its initializer starts it fresh. (#2519, #192; part of #16)
- A closure that changes `self` may be kept in a local and called later, and changes the receiver where it is. (#189)
- A `&mut` local given whole where no reference is expected (`Some(r)`, a tuple, a generic field, `let s; s = r;`) moves as rustc's does, so a later use is E0382, and a `let s;` given a `&mut` reaches its place. (#2502)
- A function may keep a `&mut` parameter in the value it returns (`fn new(a: &'a mut i32) -> S<'a>`), a `&mut` passed to a type parameter or an `Option<&mut T>` passed to a function reaches the caller's place, and `#[derive(Hash)]` accepts a `&mut` field. (#184)
- A Box<dyn Error> keeps its error: source() chains walk, downcast_ref, is and downcast give the error back, and io::Error, VarError and std's other errors box with ?. A &e is a &dyn Error and an Arc of an error an Arc<dyn Error>. (#2091, #798)
- dyn Any runs: a boxed or borrowed value keeps its type for is, downcast_ref and downcast. (#1284)
- fn main() -> ExitCode ends with its status, as does Result<ExitCode, E>. (#2088)
- std::env::var and env::var_os read the environment a program is granted, and VarError matches and boxes as an error. (#645)
- `str::matches`, `match_indices` and `replacen` run with a `&str` or `char` pattern. (#2090)
- `String::drain` and `String::replace_range` run with any range, with std's panics off a char boundary. (#2521)
- `Path::new` runs on a `&str` or `&String`. Paths have `exists`, `is_file`, `is_dir`, `set_extension`, `with_extension`, `set_file_name`, `with_file_name`, `pop`, `starts_with`, `ends_with`, `is_absolute`, `is_relative`, `has_root` and `as_path`. A std path method rawr doesn't run yet says so, instead of E0599. (#2087)
- A `String` lends its text as a `&mut str` through `&mut *s`, `as_mut_str()` or a `&mut str` parameter, and `make_ascii_uppercase` and `make_ascii_lowercase` change it. A `String`-only method on a `&mut str` is rustc's E0599. (#821)
- `String::from_utf8_lossy` and a path's `to_string_lossy` give std's `Cow<str>`. (#2555; part of #58)
- A script's top-level statements name paths into its inline modules, as `picks::Count(2)`, the way a function body does. (#1141, #1348)
- A generic tuple struct's constructor takes a turbofish: `W::<i32>(1)`, `W::<u16>` as a function, `m::P::<i8>(3)`. (#2215)
- `#[path = ".."]` on `mod` names the module's file as rustc reads it, from the root, from another file, or inside an inline module. (#2559; part of #605)
- A non-prelude std trait's method, as `write_all` on a `Vec<u8>`, is rustc's E0599 with its import help where the trait isn't in scope, and `write!` into a `String` with only `io::Write` in scope is E0599. (#1428; part of #1427)
- `mem::swap` with no `use std::mem;` is rustc's E0433, and a path whose first segment names nothing offers rustc's imports and its "refer to it directly" help. (#808, #807)

## 0.5.15 (2026-11-22)

- Every range form is a value of std's range type: `a..b`, `a..=b`, `a..`, `..b`, `..=b` and `..` are `Range`, `RangeInclusive`, `RangeFrom`, `RangeTo`, `RangeToInclusive` and `RangeFull` of integers, chars and floats, 128-bit included, with std's fields, traits and methods, iterating where std iterates them and filling a crate's plain `T` as rustc fills it. (#1062, #1063, #1105, #1106, #1260)
- A program macro called inside a std macro's arguments no longer makes `rawr run` rebuild forever: a reload snapshot keeps the file's own text, not its laid-out expansions. (#2520)
- url moves to 2.5.9 for RUSTSEC-2026-0592, a stack overflow in `Url::origin()` on nested `blob:` URLs, and manifest package specs parse URLs as url 2.5.9 does. (#2518)

## 0.5.14 (2026-11-22)

- A non-exhaustive match or a refutable pattern names rustc's integer and char ranges below a constructor, such as `Some(2_i32..=i32::MAX)`, in rustc's order; a constant's path is a pattern and a range end (`i32::MIN..=-5`, `LO..=9`), and a pattern naming a `const` matches its value instead of binding a name. (#1287, #1306, #1288)
- A match on a type with no values, such as `Infallible`, needs no arms; function parameters take struct patterns and literals; and matches on crate enums and structs cover them by the patterns below their fields, with rustc's witnesses. (#620, #1307, #812, #1972, #1308, #342)
- A `macro_rules!` fragment is one of Rust's (an `expr` keeps a closure's commas, an `if` or a struct expression), a matcher its follow set refuses is rustc's error, `$(+ $x)*` expands inside a separated repetition, and hygiene leaves fields, paths and methods alone while renaming match-arm bindings and format captures. (#237, #2202, #2068, #2201)
- `#[cfg_attr]` nests and applies `cfg` and `default` to fields and variants, an attribute nothing in scope declares is rustc's "cannot find attribute", and a program's macros defined before a `mod` work in that module's marked files. (#603, #604, #2190)

## 0.5.13 (2026-11-22)

- A function body that reborrows through `&mut` references, returns a `&str` or slice range, or stores a borrow through a `&mut` parameter is checked against its signature's lifetimes as rustc checks it, with rustc's report; `*r = x` and `h.x = x` through a `&mut` parameter run, and a `&mut` moved by an untyped `let` or into a generic is moved (E0382). (#2476, #2477, #2478)
- A `fn(T)` field makes its struct contravariant in `T`, and a struct's fields imply their outlives bounds, as in rustc. (#2479)
- A `where` bound on another type (`where Vec<T>: Debug`) works on an impl, a struct, an enum and a trait; a bound met through the program's impl, a hand-written std impl or a derive proves that impl's own bounds; and E0277 names the innermost type that lacks the trait, with rustc's "required for … to implement …" notes. (#2470, #2471, #2472)

## 0.5.12 (2026-11-22)

- A generic bounded by `fmt::Write` or `io::Write` runs std's impls for `String` and `Vec<u8>`, and `write!` into any writer is its `write_fmt`, as rustc expands it; `fmt::Arguments` is a type a program can write. (#2483, #2484)
- A program's `Hasher` gets `write_u8` through `write_isize`, integers have `to_ne_bytes`, `to_le_bytes` and `to_be_bytes`, `Iterator::by_ref` on a program iterator goes on from where it stopped, and std's integer-to-float `From` conversions run. (#2481, #2482, #2475)

## 0.5.11 (2026-11-22)

- A path through another module's import of a std item resolves to that item, as rustc's does: `super::fmt::Display`, `crate::fmt::Formatter` and `super::super::HashMap::new()` after the parent's `use std::fmt;` or `use std::collections::*;`, in signatures, bodies and patterns. (#2468)
- `pub use` re-exports a std item or a module, as rustc does: `pub use std::fmt::Display;`, `pub use std::fmt;` and `pub use crate::a::b as c;` work from other modules, and a private import of a std item is E0603 naming the item's kind. (#2469)
- A function's `impl Trait` output is one type per type argument, as rustc's is: `wrap(Sq(1.0))` and `wrap(Ci(2.0))` no longer mix, and a bound naming a parameter (`impl Into<T>`, `impl Iterator<Item = T>`) works, read at each call's type arguments. (#2492)

## 0.5.10 (2026-11-22)

- A generic call whose `T::zero()` or `T::default()` needs the type an `impl Trait` value hides now runs, as rustc does, even when the caller comes first in the file. (#2491)
- An `impl Trait` value is `Send` exactly when the value it hides is: moving one that holds an `Rc` into `std::thread::spawn` is rustc's E0277. (#2494; part of #2480)

## 0.5.9 (2026-11-21)

- A function written `-> impl Trait` returns its own opaque type, known by every written bound as rustc knows it: `impl A + B`, `impl Clone`, `impl Copy`, `impl PartialEq`, `impl Default`, `impl Into<String>`, and traits with `-> Self`, generic, static or by-value methods all run. Two different hidden types in one body, an inherent call on the hidden type, naming the hidden type, and mixing two functions' opaque types are rustc's E0308 and E0599. (#2490; part of #2480)

## 0.5.8 (2026-11-21)

- A function body that breaks its own signature's lifetimes is reported with rustc's labels: where each lifetime is declared (or `'1` for an elided one), "returning this value requires that `'a` must outlive `'static`" or "function was supposed to return data with lifetime `'b`…", the bound to add, the variance notes for a `&mut`, a cell or an invariant struct, and E0621's explicit-lifetime suggestion. (#2486)

## 0.5.7 (2026-11-21)

- A program type that implements `io::Write`, `io::Read` or `fmt::Write` gets the trait's provided methods (`write_all`, `read_to_string`, `read_exact`, `write_char`), and `write!`/`writeln!` write into an `io::Write`. `lt`, `le`, `gt` and `ge` work on any `PartialOrd` type. (#2461)
- A function can return `impl Trait` of a program or std trait (`impl Shape`, `impl Display`), and a method call on its value is the trait's, never the hidden type's own namesake. (#2462; part of #2099)
- `r.clone()` of a `&T` with `T` unbounded copies the reference, and a tuple passed to a generic or boxed as a trait object prints as a tuple. (#2474, #2473)

## 0.5.6 (2026-11-21)

- A `where` clause bounds an associated type (`I::Item: AsRef<dyn Item>`, `<I as IntoIterator>::Item: Debug`) or any other type (`Vec<T>: Debug`, `&'a T: Display`), as rustc reads it: the body may rely on the bound, and each call checks it with rustc's E0277. The playground has a new example, "Traits: Display, Add, Iterator, dyn". (#383)
- A generic bounded by `FromStr` parses in its body with `s.parse()` or `T::from_str(s)`, and calls at `u8`, `i64`, `f64`, `String`, `bool` and `char` meet the bound by std's impls. Each integer kind parses as its own type, so `"300"` into `u8` fails as in rustc. (#2464, #2465)
- The borrow checker checks a function body against every lifetime in its signature, with variance, as rustc does: a returned `&'a &'a T` is no longer accepted as `&'a &'static T`, `&mut`, `Cell` and the structs that hold them are invariant, and bounds implied by parameter types, such as `'b: 'a` from `&'a &'b T`, make code legal as in rustc. (#170)

## 0.5.5 (2026-11-21)

- An impl of a native trait through its module, as `impl page::Page for BadgePage` after `use crate::page::{self}`, compiles as rustc resolves it, instead of being refused with E0404. (#2452)
- The playground has a New button beside Examples that starts from an empty `fn main`. (#2467)

## 0.5.4 (2026-11-21)

- A generic function's call takes its argument types from the let or return it is assigned to: `let b: P<&str> = f(&name)` coerces `&name` to `&str`. (#1419)
- A `vec![..]` at the end of an if or match arm coerces its elements to the type the function returns, as `Vec<Box<dyn Trait>>`. (#1293)
- Float literals converted into `(f32, f32)` or `Vec<f32>` are f32s, and `VecDeque::from(vec)` takes the vec's element type. (#2463; part of #1048)
- A `let` whose type nothing settles, as `let v = Vec::new();`, is rustc's E0282 "type annotations needed for `Vec<_>`". (#1257)

## 0.5.3 (2026-11-21)

- `(*w).push(1)` on a type with `DerefMut` calls `deref_mut`, and `(*f(&mut v)).push(1)` changes `v`, where the change used to go to a copy and be lost. (#873)
- `w[i]`, `w[i] = x` and `w[i] += 1` index through a program type's `Deref` and `DerefMut`, with rustc's E0596 when only `Deref` is there. (#817)
- Two traits in scope giving one method are rustc's E0034, and a glob-imported trait is found beside an out-of-scope trait of the same name. (#767)
- `&mut w` passed where `&mut Target` is expected coerces through a program type's `DerefMut`, so the callee changes `w`. (#2460; part of #816)

## 0.5.2 (2026-11-21)

- The build line says what Cargo reports: the step, the crates finished of the total, the crate that has compiled longest and the time, as in `building runner · 212 of 340 crates · iced_wgpu · 3m12s`. It no longer fills a bar on time, so a stalled build shows as stalled. When Cargo waits for another Cargo's lock the line says which directory and for how long, and a wait for rawr's shared extraction directory names it as rawr's, with the process using it and its project. rawr mcp, rawr tui and rawr gui show the same text in place of a percent. (#2456)

## 0.5.1 (2026-11-21)

- A method call on a program type calls its inherent method, as rustc does, when the type's impl of a dependency's trait with a method of the same name isn't in scope there: iced's game_of_life builds again. (#2455)

## 0.5.0 (2026-11-21)

This milestone is the debugger: one session any number of shells attach to, so rawr tui, rawr gui, VS Code through rawr dap, rawr lsp beside rust-analyzer, the playground and rawr mcp break, step, read values and evaluate Rust in the same paused program. A hover reads a value without running the program's code, every pane re-reads the paused program and keeps no copy, and rawr waits for another rawr's locks as Cargo does.

- The home page and Debugging show one program paused at the same line in rawr tui, rawr gui, the playground and VS Code, with values read in each and answers in their consoles. (#2454)

## 0.4.108 (2026-11-21)

- `rawr lsp` is a small language server beside rust-analyzer: while the program is paused, hovers and inlay hints show its values, read without running any of its code, and a save that doesn't reload shows rawr's errors under the code until one does. It attaches to the newest run of each program you open a file of, waits for one to start, and holds a hover asked before the run greets it. (#2440)

## 0.4.107 (2026-11-21)

- The reload's live lists, the owner's inbox and the program's statics each share one lock, the leaked texts, slices and locations are kept in the one lasting pin table, generic instance keys are literals in the generated runner, and a script impl's member takes no lock to find. Fifteen locks are gone. (#2450)
- rawr gui shows no hover over a call, a macro or anything else the hover read doesn't run, as over a blank spot, instead of a line saying why. (#2451)

## 0.4.106 (2026-11-21)

- `rawr tui` draws in `rawr gui`'s Oxocarbon colours: the paused line sits on a faint accent band with the gutter marker in the accent, code takes the GUI's syntax colours in 24-bit colour or the nearest of 256, pane borders are quiet, and the status line reads like the GUI's. (#2447)

## 0.4.105 (2026-11-21)

- A hover in rawr dap, rawr gui and VS Code reads the whole place under the pointer, such as player.score or names[turn as usize % names.len()], and runs none of the program's code; a call says to evaluate it in the console. rawr dap no longer runs a selected call such as tick() when the editor hovers it, and rawr gui cuts an inline value with … at the pane's edge. (#2442)
- A run hosts the session socket its shell handed it wherever the shell bound it, so a rawr mcp started before an upgrade hears its program run instead of waiting at "starting". (#2441)

## 0.4.104 (2026-11-21)

- Worktrees of one repository that share a target directory no longer build against each other's path crates: each project's runner places its path packages under its own key, so Cargo never takes another worktree's stale build of one as fresh. (#2423)

## 0.4.103 (2026-11-21)

- rawr gui edits its source in place: Cmd-S saves, and the bar shows whether the save swapped in, restarted or failed at a line. While paused, each binding's value shows at the end of its line, and a hovered name shows its value. The window draws in Oxocarbon, the console reads like DevTools', the panes meet at hairlines, and the title names the program. (#2434)

## 0.4.102 (2026-11-21)

- A build no longer stalls holding a shared target's build lock after cargo exits: a process cargo started that left its process group, such as a build script's daemon or a compiler cache client, no longer keeps the build waiting on cargo's output. (#2432)
- Two rawr runs of one project each host a session shells can attach to: rawr tui, gui, mcp and dap attach to the newest and name the others, and rawr tui --pid, rawr gui --pid and DAP's processId pick another. A run that Ctrl-C, SIGTERM or SIGHUP ends removes its session's socket directory, and a dead run's leftover socket is cleaned up. (#2430)

## 0.4.101 (2026-11-21)

- Two rawr runs on one project or one target directory wait for each other as two Cargo runs do: the second prints one line naming the lock, the holder's process and the project it builds, then goes on. A run no longer fails with "the build directory is locked by another build" or a busy runner cache, and a Cargo child's own wait for its build directory is printed. (#2429)

## 0.4.100 (2026-11-21)

- A runner whose program turns on a feature that reaches a patched package, such as iced_test through a project feature, settles its lock again: the runner keeps every project patch while Cargo settles it, then leaves out only the patches the settled lock lists as unused. (#2426)

## 0.4.99 (2026-11-21)

- The debugger's inspector keeps one generation's stops, lines and program in one state, and only the program's threads wait on its pause. Input sent before a program's first window is answered when the window shows, with no 50 ms poll, and every pause carries its count in its events and answers. (#2424)

## 0.4.98 (2026-11-21)

- `rawr gui` and `rawr tui` show a program under the home directory as `~/…` in the window title and the header. The GUI names the program once, in its title, and its toolbar keeps that space for the last error. (#2422)

## 0.4.97 (2026-11-21)

- An eval or a read asked at a pause that a continue or step ends first is answered at once as resumed, and never runs at the program's next pause. A screenshot given up while the window draws still gives the window's layer its settings back, and a click, drag or key press a pause interrupts still lets go of its button or key. (#2419)

## 0.4.96 (2026-11-21)

- The graph budget stays on the thread that owns the graph: the extraction plan and capture carry only whether the graph owns their artifacts, so the graph's account registry has no locks. (#2407)

## 0.4.95 (2026-11-21)

- The check's owner holds the list of kept macro expansions its checks read, not a process-wide global; a check that reuses a remembered lowering still keeps the expansions that lowering read. (#2387)

## 0.4.94 (2026-11-21)

- The format directories' process leases are kept in a write-once list, so opening a bindings, inventory or project format path takes no lock. (#2402)
- A nested native step no longer waits on its own lock forever, and a node started inside another node fails at once instead of hanging; heavy work takes the project's own lock instead of one lock for the whole store. (#2399)

## 0.4.93 (2026-11-21)

- Cargo's answers about a project (its targets, features and build units) are kept by the run, the runner or the command that asked, instead of in a process-wide lock; a save asks Cargo again only when the files it read changed. (#2401)

## 0.4.92 (2026-11-21)

- rawr's lines about a program that ended, a restart and a save's or start's round wait while the program holds the terminal raw, like every other rawr line. (#2415)

## 0.4.91 (2026-11-21)

- rawr's status line, the lines above it and the lines held while a program has the terminal are written by one printer thread: a line is on stderr when the call returns, the last line is flushed at exit, on a panic and after Ctrl-C or a kill, and a cold launch no longer starts a ticker thread. Four locks go. (#2414)

## 0.4.90 (2026-11-21)

- rawr mcp's screenshot takes a `region` in the window's points and a `scale` in pixels a point, cropped and resampled by the program itself; a bad scale or a region outside the window is refused with a message. (#2009; part of #1993)
- The session a run hosts keeps its state on one thread: shells, the program and the run's owner send it messages and each answer comes back on its own channel. A request whose shell closes ends at once and passes the control turn on, and a shell's hello never gives older breakpoints than a list it already heard. Five locks go. (#2412)

## 0.4.89 (2026-11-21)

- When a shell attaches to a run started at a terminal, `rawr mcp`'s `status` and `start` and `rawr dap`'s console say it hears rawr's prints but not what a dependency writes straight to the terminal; a run a shell starts sends that shell all of its output. (#2411; part of #2320)

## 0.4.88 (2026-11-21)

- rawr mcp's `wait` returns when the started program prints, not only at its next event; rawr's own progress lines don't end it. (#2267)
- rawr mcp's input and screenshot reach a program's minimized window instead of saying it shows no window. (#2255)

## 0.4.87 (2026-11-21)

- rawr mcp's `start`, `run` and `check` take `package`, `bin`, `features`, `no_default_features`, `all_features` and `env`, as `rawr run` takes `-p`, `--bin` and the feature flags; `env` sets variables for one call's build and program, and a started program keeps them across reloads. (#2398)
- rawr mcp's sessions each have one owner thread that answers requests and waits on reply channels, a cancel is a message on the call's own channel, and rawr dap runs on one adapter thread that never blocks on the program. A request is stamped when it is sent, so a debugger tool's answer never overrides a newer pause the program told, which could leave a later `wait` hanging. Eight locks go. (#2397)
- Debugger eval refuses a crate use the running build wasn't compiled with, such as a field the program never reads or a generic at a new type, at the use and before any of the code runs, and says that saving the use in the program rebuilds the runner. (#2406)

## 0.4.86 (2026-11-21)

- Read sets stay on the thread that owns them, and the compiler proves it: the importer's read-set validator is an Rc closure, and the resolution's capture, the owner's replay memos and the prepared closure's pins lose their locks. The background API rebuild and the classifier's workers take only the data they read. (#2405)

## 0.4.85 (2026-11-21)

- Evaluating code while paused at a match arm's body without braces runs instead of failing to parse. (#2265)
- The debugger shows `self` at every step of an expression on it, such as `self.items[0] += 1`. (#2256)
- The debugger no longer lists a binding the compiler made for an index, and keeps a function's locals listed at the steps inside an expression on one of them. (#2257)

## 0.4.84 (2026-11-21)

- A save no longer fails with "the build directory is locked by another build" when two Cargo operations of one run meet: the run owner holds one Cargo lane and lends it to each resolution, publication and runner build. (#1824)

## 0.4.83 (2026-11-21)

- rawr mcp's main loop owns its checks, and a call waits on one channel for its result or its cancel. A run waits for its child's exit in the kernel in place of a 10 ms poll, and ends the child's process group before reaping it. Five locks and a condvar are gone. (#2396)

## 0.4.82 (2026-11-20)

- An absent native import, as `use native::new_function`, requests a native build again instead of failing with E0432. (#2385)
- A project under a path Cargo percent-encodes, as one holding `{` or a space, keeps its runner's sources across saves instead of rendering them again. (#2374)

## 0.4.81 (2026-11-20)

- rawr dap, rawr mcp, rawr gui, the reload stream and the --web page server each hand their state to one owner thread, which removes 15 locks. rawr mcp no longer sleeps 20 ms after each child, and a pipe a process outside the child's group holds open no longer delays a call's output. (#2389)
- The engine drops locks one thread uses or nothing reads: the native account's baseline, the global constants map, the window-frame session, the reload publication slot and a second copy of the published program. A runner's pins and the host value tables each keep one lock in place of four. (#2390)

## 0.4.80 (2026-11-20)

- The generated runner frees native values and takes back lent values on a channel each arena owns, with no process-wide queue or lock, and a callback reads its script function and that function's witnesses together, so it never pairs one call's function with another call's witnesses. (#1826)

## 0.4.79 (2026-11-20)

- A `rawr run` that waits for a save no longer wakes on a clock: file events reach the owner on a channel, the program's end wakes it, and the only timed wait left is the 30 ms debounce after a save. (#1828)
- A long-running `rawr run` holds what each save leaves for the next and lends it to the save, and a background API read and a save no longer take each other's kept fragment reads. (#1825)

## 0.4.78 (2026-11-20)

- The bot's health check accepts the reports of a bot that kept running through a container start attempt, so a deploy after a connectivity blip no longer fails waiting for an image the running bot already reports. (#2381)
- A runner's parses and lowerings are a value its protocol loop owns and lends to each compile, so a save takes no lock of them (15 lock calls a save before, none now). Code a debugger evaluates compiles from its paused generation's parses and no longer evicts the paused file's parse and lowering, so the save after an evaluation parses nothing again. (#2382; part of #1604, #1646)
- The playground's modules load from their own storage, so a new module build goes live without a site deploy, and a deploy never puts older site code over newer. (#2383)

## 0.4.77 (2026-11-20)

- /play's iced examples are iced's own counter, bezier_tool and styling, unchanged from iced, in place of the cut-down to-do list, pick list and arc. ?play also runs iced's bezier_tool, clock and stopwatch as iced writes them. (#2378)

## 0.4.76 (2026-11-20)

- /play clears the debugger's breakpoints when you pick a different snippet, keeps an example's #anchor while you edit it, and shows a program that only prints without an empty frame above its text. (#2376)
- rawr no longer has a `sync` feature: values are always `Arc`/`RwLock`, so threads work in every build and manifests no longer ask for `sync`. (#2377)

## 0.4.75 (2026-11-20)

- std's `io::SeekFrom` variants work: `SeekFrom::Start(n)`, `End(n)` and `Current(n)` build values, serve as `fn` pointers and match in `match`, `if let` and `let`-`else`, and a missed variant is written with its fields. Messages name a std type as rustc does: `PathBuf` alone, `std::fmt::Alignment` where another type shares the name, and E0599 names the type's kind. (#2370)

## 0.4.74 (2026-11-20)

- A program's `Mutex` hands its lock to a waiting thread as soon as the guard ends, with no 1 ms poll: a handoff takes about 3 µs where it took about 1.2 ms, and a contended workload runs 22% faster. (#2371; part of #1828)
- `drop(guard)` of a `Mutex`, `RefCell` or `Cell` borrow ends it there, and a later use of the guard is rustc's E0382. (#2352)
- A read through a lock's temporary guard (`*m.lock().unwrap()`, a method call on it, a `match` on it) waits for the lock another thread holds. (#2357)
- The debugger reads a generation's lines, eval program and files together, so a request during a reload no longer pairs one generation's program with another's lines. (#2372)

## 0.4.73 (2026-11-20)

- A `match` on `fmt::Alignment`, `mpsc::TryRecvError` or `mpsc::RecvTimeoutError` with an arm for each variant is accepted, as rustc accepts it, and E0004 names the variants it misses as rustc does. A `#[non_exhaustive]` enum such as `IntErrorKind` or `io::ErrorKind` still needs `_`, and E0004 reports only `_` with rustc's note. Every stable `io::ErrorKind` variant can now be named. (#2369)

## 0.4.72 (2026-11-20)

- A path goes on through a module that a `use` or a glob binds, as rustc resolves it: `impl m::Tr for ..` after `use super::*;` reaches the parent's `use crate::m`, and `super::m::two()` reaches the parent's `use crate::m;`, in types, traits and expressions. (#2368)
- `f.align()` in a `fmt` body gives std's `fmt::Alignment` (Left, Right, Center) of the caller's spec. (#2349)

## 0.4.71 (2026-11-20)

- Under `rawr dap`, rawr's own progress and status lines reach the editor as console output; only the program's stderr shows as stderr. (#2367)
- A shared table or fact refused for its build-script output prepares again by itself instead of ending the run with 'run again'. (#2366)
- A trait another module declares, named in an `impl` header, a bound, a `where` clause, an `impl Trait` or a `dyn` without a `use`, is refused with rustc's E0405 and the `use` that imports it. A module's trait that shares a std trait's name no longer replaces std's trait where the scope doesn't import it. (#2364)

## 0.4.70 (2026-11-20)

- A build script that gives rawr's nightly build other output than the project's build no longer prints anything when rawr handles it, so VS Code's debug console shows no red lines for it. Once a rerun confirms the difference, every project that builds with the same nightly and gets the same output treats it as known, so a cleaned target or another workspace member no longer cleans and extracts again. (#2365; part of #2351)

## 0.4.69 (2026-11-20)

- rustc's diagnostics in more places: a struct's or struct variant's witness names its fields (`E::C { y: false, .. }`), every unresolved member of one `use` is one E0432 with a label at each, E0599 suggests a method, associated function or constant with a similar name and names lifetime parameters (`R<'a>`, `R<'_>`), and a missing trait (E0405) suggests its import, with core's candidates where rustc lists them. (#2347)
- An unbound `Reverse(1)` or `PhantomData` suggests `use std::cmp::Reverse;` or `use std::marker::PhantomData;`, as rustc does: the std table lists unit and tuple struct constructors in the value namespace. (#2363; part of #2349)

## 0.4.68 (2026-11-20)

- Every Run button on the site runs its snippet in your browser. The wide-integer and time examples now run there, and the threads example is a fixed transcript, since a browser tab has one thread. The site no longer sends code to a server, so it has no browser check. (#2361)

## 0.4.67 (2026-11-20)

- Run's "A native main" opens with a native main.rs and a marked module, then a table of what each save does and a table of what crosses between native and marked code, with one limits list. Native trait objects, statics, slice and text constants, generic functions and the value rules moved to a new Native boundary reference page, and the build copy to How it works. Stale limits were removed: marked code calls the native root's items, any file of the module tree can be marked, `&dyn` and `Rc<dyn>` of a marked type cross, generic methods cross, and the copy takes the feature flags. (#2355)
- A `?play` snippet in Discord can start with a cargo-script frontmatter block that declares its dependencies. The bot builds them and the bindings for the calls the snippet makes, offline against crates vendored into its image, and runs the snippet with the usual sandbox and limits. Only approved crates and features build: iced at the pinned rev, ratatui and tokio, each with an approved feature set. A snippet that names another crate or feature gets one line that says what is approved, and nothing is built. (#2358)
- The playground at /play runs every snippet in the browser. Run on server is gone, and so is every fallback to the server. A run that can't start in the browser says why and stops. A snippet with a frontmatter block runs there when each dependency it names is a crate of the browser's prebuilt runner; any other dependency gets one line naming those crates. (#2359)

## 0.4.66 (2026-11-20)

- The playground's console and `rawr mcp`'s eval take an assignment as Rust does, with or without its `;`, and answer `()`. Code evaluated in a file whose macro calls are expanded compiles in the paused frame, and its errors point at the code typed. (#2353)
- The playground's bindings panel edits a value in place: click it, type Rust and press Enter to assign it in the selected call. rustc's error shows under it when Rust refuses, and Escape cancels. (#2354)

## 0.4.65 (2026-11-20)

- Editors can set a value while the program is paused. The VS Code Variables pane and watch expressions assign the new value through eval in that value's frame, for fields, list items and map entries. A write Rust doesn't allow, such as to an immutable binding or a field behind `&self`, fails with rustc's error. (#2350)

## 0.4.64 (2026-11-20)

- A std type's name resolves only as rustc resolves it: through a `use` of the type or its module (`mpsc::Sender` after `use std::sync::mpsc;`, `collections::HashMap` after `use std::collections;`), its std path, or the prelude. A bare `Mutex`, `HashMap`, `Rc`, `Ordering` or `Duration`, or a path through a module nothing binds, is rustc's E0425 or E0433 with its import suggestions. (#2346)
- A `Display` impl formats under the spec its caller gives: `write!` and `write_str` ignore the width, `f.pad` applies it, and `f.width()`, `f.fill()`, `f.precision()`, `f.alternate()` and the sign flags read it. (#2348)

## 0.4.63 (2026-11-20)

- A parenthesized macro call among a file's items (`mk!();`) expands, as rustc expands it. (#2343)
- Running a file with an inline `mod` no longer prints "Rawr package snapshot contains a duplicate path". (#2344)
- `text.into()` from a `&str` or `String` where a `let` or a function's return type expects a `Box<dyn Error>` is std's `From` for the box. (#2345)
- A std type's associated function that rawr doesn't run, named through a `use` (`File::create`, `RwLock::new`), gives "rawr doesn't run `std::…` yet" instead of an unknown member on the wrong type. (#1254)
- `Formatter::pad` runs in a Display impl, under the format's width, fill, alignment and precision. (#1255)

## 0.4.62 (2026-11-20)

- The reference and embedding docs describe destructuring assignment, trait scope for method calls, impls that name a trait by path, std trait objects, macro calls in items, types and patterns, `rawr tui`, the debugger window's panes, the call-level default and `Outcome::Uncovered`, and drop limits that no longer hold. (#2342)
- E0599 for a program's own struct or enum names its kind and declared parameters and marks its definition, as rustc does. A name no `use` brings in suggests every item a `use` could import, the program's and std's (such as `HashMap`), as rustc's alternatives. A format argument lacking `Display` or `Debug` is labelled at its placeholder, and E0004 notes the program's enum with each uncovered variant. (#2337)
- E0432 marks a nested group member from the member and a std glob or `self` prefix at the missing segment, and names a missing module's parent as rustc does. (#2341)

## 0.4.61 (2026-11-20)

- An `impl` can name a trait through its module (`impl m::Shape`, `crate::m::Shape`, `self::m::Shape`, `super::Shape`) or by a name a `use` gives it, and so can a bound or a `dyn`, in a single source as in a package. An impl's trait path that names nothing is rustc's E0405, E0433 or E0404. (#2338)
- E0369 for an arithmetic or bit operator on a program type labels each operand with its type and notes the missing impl at the type. A compound assignment's E0368 marks the whole assignment. E0432 for a missing item marks the whole import, with its label on the last segment. (#2340)

## 0.4.60 (2026-11-20)

- Comparing values of a type with no `PartialEq` or `PartialOrd` reads as rustc's E0369: each operand is labelled with its type, a note at the type's declaration says the impl might be missing, and the `#[derive]` is suggested above it. A `use` through a function or a constant, as `use iced::Center::{x};`, says "expected type, found constant `Center` in `iced`". (#2339)

## 0.4.59 (2026-11-20)

- A program trait's methods are called only where the trait is in scope: declared in the module or a block around the call, imported by `use`, a glob, a rename or `as _`, or the trait of the impl the call is in. Elsewhere the call is rustc's E0599, with its help and the `use` it suggests. (#2335)
- `&mut c` of a local not declared `mut` is rustc's E0596 at the borrow, with its `mut` suggestion, whether or not the reference is used. (#2336)

## 0.4.58 (2026-11-20)

- rustc's suggestions print as code: a value for `return;` and for an uninitialized `let`, `let...else` or `if let` for a refutable `let`, the missing `match` arm, an `as` rename for a duplicate import, the explicit type for E0282, `ref mut` for a destructured binding, removing a called field's arguments, `use` lines for functions and `fmt::Write`, and `#[derive]` for format arguments. `&mut (place)` borrows the place, and `println!("{}", S)` of a type without `Display` is E0277. (#2330)

## 0.4.57 (2026-11-20)

- Wrong argument counts read as rustc's: the error is at the callee, each extra argument and missing parameter is labelled with its type, the missing parameters are marked where the callee is defined, and the fix is suggested. An unmet bound that a program's function writes points at the argument and at the bound. Private items read as rustc's: unit structs and tuple struct constructors by name, items past a private module as not publicly re-exported, the path to import instead, and `pub` for a constructor's private fields. `..base` giving private fields is E0451. (#2331)

## 0.4.56 (2026-11-20)

- `pub(in path)`, `pub(super)`, `pub(self)` and `pub(crate)` are enforced as rustc does for functions, types, methods, fields, tuple struct constructors and `use` re-exports, with rustc's E0603, E0624, E0616, E0451, E0364 and E0742. Field privacy is checked. (#2332)
- A type named through two modules takes its generic arguments where a type follows a pattern (`let _: m::q::S<i32>;`), `let _: T;` binds nothing, and a wrong argument count is rustc's E0107. (#2333)

## 0.4.55 (2026-11-20)

- Errors show rustc's notes that point at a place of their own as their own sections: E0382's note on the parameter that takes the value and on a type that could implement `Clone`, E0061's "function defined here", E0603's "the function `f` is defined here", and a match arm's "to declare a mutable binding use: `mut b`" with its suggestion to remove the `&mut`. A match on a number takes `&` patterns. `rawr mcp` diagnostics list each note among their `help`. (#2329)

## 0.4.54 (2026-11-20)

- Errors show rustc's code suggestions under their own `help:`, as the line the change makes, or beside the code for a short one. A slice pattern on a `Vec` suggests `v[..]`, and a `let`'s `&mut b` pattern on a `&` value gets "to declare a mutable variable use: `mut b`". Assigning twice to an immutable binding suggests `mut` where it is declared. A moved value suggests `.clone()`, a macro called as a function suggests its `!`, and a method read as a field suggests `()`. A value where a `Box<dyn Error>` is expected suggests `Box::new(..)` or `.into()`. `rawr mcp` diagnostics carry the first edit as `suggestion`. (#2328)

## 0.4.53 (2026-11-20)

- A pattern of another type than its value reads as rustc's E0308 in full: the label beside the value, written type or `for` iterator that typed it, the "expected … found …" note, and the pattern named by its own type (`(_, _)`, `Option<_>`, `&_`). A slice pattern on a value that is no array or slice is rustc's E0529. (#2327)

## 0.4.52 (2026-11-20)

- Let and match patterns of the wrong shape get rustc's errors: E0527 and E0528 for an array pattern of another length, "expected a tuple with N elements, found one with M elements" for a tuple of another arity, and rustc's two labels for a second `..`. E0005 and other pattern errors mark the whole pattern, such as `Some(a)`. Destructuring assignments report the same. (#2325)

## 0.4.51 (2026-11-19)

- ?play's universe binds iced's `text_input`, so a snippet can take typed text. It also holds iced's `styling` example, so snippets can use `checkbox`, `toggler`, `progress_bar`, `scrollable`, `container` and `rule`. (#2312)

## 0.4.50 (2026-11-19)

- The home page shows the debugger in every shell (VS Code, `rawr gui`, `rawr tui`, the playground), and the debugging docs explain the window pane by pane and list the terminal shell's keys. (#2321)
- /play says why the browser universe did not load when it falls back to the server, skips the browser at once on an engine without wasm exception references or WebAssembly.JSTag, and wraps its status line on a phone. The debugger's tooltip says only what this browser lacks. (#2322)
- The home page shows the debugger's four shells with the same tabs as the libraries above them. (#2323)
- Destructuring assignment works as in Rust: `(a, b) = (b, a);`, `[x, .., y] = arr;`, `S { f, .. } = s;`, `T(x, _) = t;`, `_ = value;` and nested forms, with the value evaluated first and the places left to right. `(x) = 1;` and `(*r) += 1;` assign the parenthesized place. Errors match rustc's E0384, E0070, E0067 and E0005. (#2318)

## 0.4.49 (2026-11-19)

- A shell attached to `rawr run` of a program with Rust dependencies, started in a terminal, hears what the program prints: what it printed before the attach, then each piece as it prints it. (#2319)

## 0.4.48 (2026-11-19)

- A shell attached to a `rawr run` started in a terminal shows the program's output: the last of what it printed before the attach, then each line as it prints. Programs with Rust dependencies get this when their output isn't a terminal. (#2288)

## 0.4.47 (2026-11-19)

- An assignment to a value that is no place, as `(x + y) = 42`, `foo(x) = 42`, `123 = 42` or `x.foo() = 42`, is rustc's E0070 "invalid left-hand side of assignment" with rustc's label and spans; a compound one, as `(x + y) += 1`, is E0067. (#2306)
- A block whose last statement ends in `;` has the value `()`, as in Rust, so native code calling a unit function gets `()` whatever its last statement leaves. (#2316)

## 0.4.46 (2026-11-19)

- `rawr tui` reads bindings with their parts from the inspector, so a map opens to one row per key, and `rawr gui` shows its Frame pane only once the program shows a window, giving the call stack and bindings the whole column otherwise. (#2317)

## 0.4.45 (2026-11-19)

- A debugger attached to a running program names the function it pauses in and reads that function's locals by their declared types: a `HashMap` reads as `{k: v}` with one row per key, and the frame says `main`, not `?`. A root function native code calls by its own name reads by its declared types too. (#2315)

## 0.4.44 (2026-11-19)

- The debugger reads a map, set or sequence holding a program's own structs or enums as Rust's `{:?}` prints it, such as `{"ann": Player { score: 3, level: 1, tags: ["new"] }}`, instead of a list of pairs with opaque `<Rawr struct …>` handles, so the panel opens such a map to one row per key, then its fields. (#2313)

## 0.4.43 (2026-11-19)

- The playground's Crates group has iced examples that run in the browser: a counter, a to-do list, a pick list and an animated arc. (#2311)

## 0.4.42 (2026-11-19)

- `rawr gui`'s debugger window splits into panes you resize by dragging (source, console, frame, call stack, bindings, output). Frames and bindings are compact tables; a binding or an eval answer opens to a row per field, item or map key, and a long value shows its whole text in a tooltip. The toolbar's controls are icons, on F8 (continue or pause), F10, F11 and Shift+F11. The window now runs on iced master, as the playground does. (#2310; part of #2270)

## 0.4.41 (2026-11-19)

- A shell that attaches to a `rawr run` of a program with Rust dependencies gets the debugger in the running program at its next step, even when its `main` loops without returning to native code. Every program now keeps its state and its process when a debugger attaches. (#2308)

## 0.4.40 (2026-11-19)

- The debugger's console runs code in the frame you select: a caller's code runs before the statement making the call, sees that caller's bindings, and a change stays in that caller. The playground, rawr gui, rawr tui, rawr dap (`frameId`) and rawr mcp's `eval` (`frame`) send the selected frame. (#2309)

## 0.4.39 (2026-11-19)

- A shell attaching to a `rawr run` of a program without Rust dependencies takes the debugger in the running program, which keeps its state and its process instead of restarting. (#2297)
- The playground's debugger and console are denser: bindings and console answers open as trees (a `HashMap` shows one row per key), and split bars between the code, output, debugger and console set sizes the page remembers. (#2307; part of #2304)

## 0.4.38 (2026-11-19)

- An assignment through a `&` reference (`*r = 5`, `r.field = 5`, `r[i] = 5` of a slice or array) gives rustc's E0594, "cannot assign to `*r`, which is behind a `&` reference", with rustc's label and span, in code and in the debugger's eval. A `Vec` element written through `&` gives rustc's E0596. (#2305)

## 0.4.37 (2026-11-19)

- The browser module can carry a debugger: built with `browser-debugger`, a page can pause a running program at a breakpoint, read its frames, evaluate, step and continue while the page stays live. The playground's own module is unchanged. (#2268)

## 0.4.36 (2026-11-19)

- A step into a closure whose body ends in a tail expression pauses at that expression, as rustc's debuggers do. (#2300)
- A breakpoint one thread reaches while another thread answers a debugger read pauses the program instead of being passed. (#2301)
- Scoped threads that borrow the same local read it at the same time, as in Rust, instead of failing with a data race under load. (#2303)

## 0.4.35 (2026-11-19)

- A step into a function no longer pauses on its signature for a `mut` binding of a `&mut` parameter or a parameter pattern, nor on a closure's or a `for` loop's pattern, and a breakpoint's line no longer pauses again after a call that lends a local `&mut` returns. (#2298)
- `rawr dap` tells a breakpoint a save moved as `changed` with the id the editor knows, so the editor keeps its own breakpoint. (#2299)

## 0.4.34 (2026-11-19)

- A breakpoint set on a running program takes effect for its next call: a click made while the debugger is still being added no longer passes it. (#2296)

## 0.4.33 (2026-11-19)

- A script whose last expression is a macro call, as `format!(..)`, or whose statement starts with one, as `format!(..).len();`, runs again. (#2283)
- A file that calls a crate's macro is kept in its parse cell, so it isn't parsed again on the next run. (#2283)
- A reload that adds a `use` of a dependency item the running build lacks prepares the build again, and a `use` through a function or a type is an unresolved import. (#2283)

## 0.4.32 (2026-11-19)

- A breakpoint set or cleared from any shell shows in every other at once: `rawr gui` and `rawr tui` mark it in the gutter, an editor through `rawr dap` adds or removes it, and a shell that attaches later shows those already set. Attaching a window no longer registers the debugger by itself. (#2278)

## 0.4.31 (2026-11-19)

- An editor launch holds the program at its first statement until its breakpoints are in (`configurationDone`), so a breakpoint in code that runs at startup is hit. (#2274)
- In VS Code, F5 with no launch.json runs the crate the active file belongs to, by its nearest Cargo.toml, and says so when the active .rs file isn't marked. (#2275)

## 0.4.30 (2026-11-19)

- A debugger step stops only at statements the program wrote, once per line: step into a function with a `&mut` parameter pauses at its first body line, not its signature, and a step or continue from a breakpoint no longer pauses on the same line again. (#2272)

## 0.4.29 (2026-11-19)

- A shell that attaches to a plain `rawr run` gets the debugger without a restart: the program registers it at its next call from native code and keeps its state. Screenshots, window input and menus need no debugger and no longer restart the program. (#2266)

## 0.4.28 (2026-11-19)

- An eval at a pause checks the program as its run did, so a crate's generic call whose type comes from an `impl Trait` return, such as `iced::run(update, view)`, no longer fails with E0282. (#2292)
- `rawr gui` shows the frames and bindings of the newest pause only; a slow answer about an earlier stop no longer replaces them. (#2295)

## 0.4.27 (2026-11-19)

- A continue ends a step that never pauses, and both are answered; the session no longer queues a continue behind a step. `rawr gui` runs on as soon as you step or continue, so Pause is available while a step runs. (#2294)
- `rawr gui` writes each capture of the program's window to its own file, so two captures in flight no longer show "No such file or directory". (#2293)

## 0.4.26 (2026-11-19)

- A step past the end of a function a window's event loop called is answered at once and pauses at the next statement the program runs, such as the next `view` or `update`, instead of leaving the shell waiting. (#2287)

## 0.4.25 (2026-11-19)

- A dependency's macro that calls std's macros through its own re-export of them, as `$crate::__support::format_args!(.., $crate::__support::line!())` does, expands again in marked code. A macro's `line!()` and `column!()` are where cargo's build puts them: the macro's own at its outermost call, and one its caller passes in where the caller wrote it. (#2284)

## 0.4.24 (2026-11-19)

- `rawr tui [path]` opens a terminal debugger on a program's run. It attaches to the run's shared debugger session, or starts one, and shows the phase and output, the source at the stop with breakpoints marked, the frames and their bindings, and an eval prompt. Keys set breakpoints, pause, step and continue, and it keeps working while another shell such as `rawr mcp` drives the same run. (#2259)
- A native module below a marked crate root can call the root's marked functions and use its marked types, public or private, as Cargo builds them. The calls go through the same trampolines as a native root's, so an edit to the marked code reaches the next call. (#2276)
- A native module below a marked one sees only the private items its paths name, so a save that changes a private field native code never reads still swaps in place. (#2282)
- `rawr gui [path]` opens a debugger window on the program's run, in its own process, so it keeps drawing while the program is paused. It attaches to a `rawr run` already going, or starts one, and shows the run's state and output, the program's latest frame, the source where it stopped with breakpoints set in the gutter, the frames and their bindings as folding trees, an eval console, and step, continue and pause. (#2260)
- `rawr gui` and `rawr tui` draw the source in syntax colours, and the window follows the system's light or dark mode. (#2286)

## 0.4.23 (2026-11-18)

- `rawr dap` serves the Debug Adapter Protocol on stdio as a shell of the run's debugger session: launch or attach, line and function breakpoints, next, step in and step out, pause, a call stack named by Rust paths, variables that expand field by field, and evaluate where the program is paused, while each save still reloads the program and says so as output. (#514)
- The VS Code extension adds a `rawr` debugger that runs `rawr dap`, with launch and attach configurations and snippets; a new Debugging docs page also covers Neovim, Helix and Zed. (#514)

## 0.4.22 (2026-11-18)

- `rawr mcp` reaches a program run with `rawr run` in a terminal: a tool naming it attaches to that run, `start` attaches instead of starting a second one, and `stop` lets go of it. Any number of servers attach to one run, and each `wait` returns every event from its own cursor, so a pause one server causes reaches the others. (#2258)
- `rawr mcp`'s calls wait on what they need, not on a clock: a read, a step, an input or a screenshot answers as soon as the program does, and a cancelled call ends at once. (#2258)

## 0.4.21 (2026-11-18)

- `rawr mcp` has `eval`: Rust code runs where the program is paused, with the bindings in scope there. It is compiled as a block written before the paused statement, so a type error, or a borrow that conflicts with one the program holds, is refused with rustc's error. The last expression's value prints as `{:?}` prints it, without moving it, and a change the code makes stays. (#490)
- rawr-repl runs its entries as the statements of one body: items and bindings from earlier entries keep their types, and a local whose type implements `Drop` lives until the session ends, then drops newest first, while temporaries, `let _` and `drop(x)` drop where Rust drops them. (#1207)

## 0.4.20 (2026-11-18)

- `rawr mcp inspect` prints each binding by its declared type: a tuple as a tuple, an `Ordering` as its variant, a `u64` and an `f32` as their kind. (#512)
- `rawr mcp` names each frame by its Rust path (`app::Counter::update`, `app::run::{{closure}}`), and leaves out frames the compiler wrote. (#573)
- `rawr mcp inspect` reads `self` while an expression on it runs, or says it is borrowed there, reads a caller's `self` during a call on a value it made, and says why a frame or a crate value can't be read instead of leaving it out or printing `NativeHandle`. (#541, #575, #533)
- `rawr mcp inspect` reads an iced app's state paused in `update` and in `view`. (#532)

## 0.4.19 (2026-11-18)

- rawr mcp's `type` sends text of any length in one turn of the window's event loop, so long text arrives whole and no longer waits two frames for each character. (#1415)
- A rawr mcp `wait` the client gave up on no longer takes the events a later `wait` needs: the later one takes its place and returns them. (#1542)
- rawr mcp's screenshot, input and debugger tools wait for a starting or restarting program and reach the restarted one, not the one it retires. (#1964)

## 0.4.18 (2026-11-18)

- `rawr test` catches a test that panics in a dependency's code, as `cargo test` does: the panic is printed on a thread named after the test with its output, that test fails alone, the others run on, and the failures list, result line and exit status are libtest's. With `--nocapture`, each test is named as libtest names it for the number of test threads. (#2254)

## 0.4.17 (2026-11-18)

- `use krate::{module::Type}` and nested groups, `self` and globs inside a `use` group reach a dependency's bindings, so a type a nested member binds no longer fails with E0433. iced's integration example builds and agrees with cargo again. (#2250)
- A directory rustc searches that another build changes during extraction restarts the build, then reports "`<path>` changed during the build; run it again", instead of "internal error: rawr misread rustc's output". A completion that truly disagrees with what was observed is still a misread. (#2251)

## 0.4.16 (2026-11-18)

- A change to the surface format evicts old resolved-demand entries when the store opens, instead of leaving them for size-based eviction. (#2247)
- Projects whose path has a space, braces or non-ASCII characters reuse Cargo's metadata and unit graph across source-only edits. (#2249)

## 0.4.15 (2026-11-18)

- Remembered Cargo outputs aren't saved when a manifest, config or target directory Cargo read changed after Cargo started, so the next run asks Cargo again instead of pairing old metadata with the edited manifest. (#2246)
- A build script without rerun-if inputs is proved by the files Cargo lists for its package (Git's tracked and unignored files, else a walk without hidden names, with `include` and `exclude`), so ignored files written at run time no longer re-resolve it. (#1209)

## 0.4.14 (2026-11-18)

- Cached profile and checked entries written in an older format are evicted, never read, and a profile entry that doesn't decode under its own key stops the warm launch as a rawr bug, as the full path already did. (#2245)

## 0.4.13 (2026-11-18)

- A Cargo.toml, Cargo.lock, Cargo config, toolchain or source file edited during a build starts the build again silently, as `cargo watch` does. A file that keeps changing ends with Cargo's message: "`<path>` changed during the build; run it again". (#657)

## 0.4.12 (2026-11-18)

- A program with dependencies runs again: rawr's shared runtime compiles, which it didn't on 0.4.11. (#2244)
- A workspace manifest edited while Cargo reads metadata is reported as drift, never paired with the old metadata. (#656)
- A running program sees a symlinked input pointed at another file, not only edits to the file it pointed at. (#1172)

## 0.4.11 (2026-11-18)

- A change to a manifest above a workspace's root, such as the checkout around a project, no longer stops a build as changed Cargo inputs. (#894, #411, #658)
- A relative `RAWR_HOME` is read from the current directory, so the first check of a project with dependencies no longer fails on a doubled path. (#2226)
- A damaged remembered resolution, or the document beside it, is resolved again instead of failing the run. (#484)
- A stale bundled cache entry at a fresh build's action is replaced instead of failing the build. (#615)
- Profile, profile-proof and checked entries an older rawr wrote are evicted when the store opens. (#631)
- A declared path with `..` after a symlinked directory names the link target's sibling, as Cargo reads it. (#1173)
- A failed runner build shows its own error first; a build-script difference follows as a possible cause. A failed reservation release reports the failure it follows. (#1423, #641)

## 0.4.10 (2026-11-18)

- A library that is also built for a build script or a procedural macro of its closure shares its bindings and its intrinsic facts across projects, so a second project using it runs no nightly build. (#471)

## 0.4.9 (2026-11-18)

- A change to the `[profile.dev]` settings in your `~/.cargo/config.toml`, or another Cargo config file rawr's runtime build reads, compiles a new shared runtime instead of reusing one compiled under the old settings. (#304)
- Shared bindings no longer refuse to share a crate because a build script reached only as another build script's dependency gives the nightly other output than the project's build. (#1525)
- A registry crate whose dependencies a `[patch]` or `[replace]` points at a path keeps its item decisions in the project's `target/rawr`, never in rawr's home. (#2235)

## 0.4.8 (2026-11-18)

- A later demand round writes each item it takes from an earlier classification into the stored classification as it was, and its plan takes each kept contribution without digesting outcomes again. On chrono, encoding and digests fall from 117 ms to 37 ms per round. (#2242; part of #2221)

## 0.4.7 (2026-11-18)

- A rawr that waits for another rawr's build of the shared runtime, the nightly build directory, a toolchain's driver or the project's runner build says so once, naming the process that holds it. A wait for other runs' leases names none. (#1408; part of #2239)

## 0.4.6 (2026-11-18)

- A process rawr starts no longer holds a shared bindings lock for a moment after rawr lets go of it, so a reader no longer misses and extracts again, and a publication or eviction is no longer skipped. (#1026)
- A crate's facts captured where rawr's nightly build had a stale build-script output are captured again after that output is cleaned, rather than served again from the project. (#1025)

## 0.4.5 (2026-11-18)

- A crate that several of a program's dependencies reach keeps rawr's decisions about its items under its own key, so a project that reaches it through any one of them decides none of them again. (#2223)
- A dependency path that names no item, such as a derive macro's, is kept with the crate it starts at, so a second project no longer regenerates it each round. (#2233)
- Saving a crate's decisions writes only what a run decided anew, not the crate's whole entry: on chrono a second project writes 755 KB, not 3.2 MB. (#2224)
- A check's later demand rounds take and keep decisions in rawr's home too, so a second project checking the same program decides nothing in any round, where it decided 293 items again in round 2. (#2234)

## 0.4.4 (2026-11-18)

- A second project that uses a crate, and a later demand round of a check, write none of the crate's trait and impl headers or its paths' sites again: each is kept beside the crate's item decisions with what it read, and taken where it reads the same. (#2222)
- Each demand round digests every impl's identity once, and writes the stored classification in one pass instead of two. (#2232; part of #2221)

## 0.4.3 (2026-11-18)

- The browser's ratatui to-do list draws on every animation frame through ratzilla's `draw_web`, so an edit to `draw` shows at once, with its to-dos kept. (#1457)
- A method of a generic impl that names its type's parameters its own way, as ratzilla's `impl<T> WebRenderer for Terminal<T>` names `Terminal<B>`'s `B`, takes the receiver's type arguments, so `terminal.draw_web(..)` builds. (#2228)

## 0.4.2 (2026-11-17)

- A shared crate's bindings entry lists only the crates its table names. (#1128)
- A later demand round classifies and generates only the items its new paths change, and a second project classifies and generates none of a crate another project already did: each item's classification and facade contribution are kept beside the crate's shared table in rawr's home. (#1618, #271)

## 0.4.1 (2026-11-17)

- An array's length may be a const parameter: `struct Buf<const N: usize> { data: [u8; N] }`, `[0; N]` in a body, and `fn sum<const N: usize>(a: [i32; N])`, whose `N` is read from the array it's given. (#2213; part of #1152)
- The landing page's plain loop is a ball that bounces forever in your browser. Edit it while it runs and it reloads in place, keeping its state; a compile error shows while the last good code keeps running. (#1206)
- `std::thread::sleep` works in the browser. A program in a frame of its own gives the page its time at each sleep, and a one-off run waits on the clock within its time limit instead of stopping. A browser without JSPI still runs every other program, and says so in the frame of one that sleeps. (#1206)
- A `main` that loops in plain Rust and sleeps takes a reload at its next sleep and runs the new code from its next statement, with its locals kept. (#2216)
- An enum variant imported by `use` can be passed to a macro, as `column![row![..].align_y(Center)]` does. (#2217)

## 0.4.0 (2026-11-17)

- Const generics work: a program's `struct S<const N: usize>`, `impl<const N: usize> S<N>` and `fn f<const N: usize>()` read `N` as a value, and a const argument may be a literal, a `{ .. }` block of constants or a `const`'s name. A crate's const-generic type, such as tokio's `Rotator::<3>`, is reached at each value the program names. (#2210; part of #1152)

## 0.3.250 (2026-11-17)

- A closure passed to a crate or std function that changes what it captures keeps each change across calls, so `std::future::poll_fn(move |cx| ..)` that counts its polls ends as it does under cargo rather than polling forever. (#2208; part of #1152)

## 0.3.249 (2026-11-17)

- A `&mut` destructuring inside a closure binds the place it names, whether of a `&mut` reference the closure captured, one that shadows its place's local, or the closure's own local. Each was refused with a false E0596 or panicked at run time. (#2200; part of #1152)
- `tokio::select!` expands from tokio's own definition, so every form tokio accepts runs as tokio runs it, and rawr's separate `select!` expansion is gone. (#2204; part of #1152)
- A match arm that an earlier arm makes unreachable is accepted, as rustc accepts it with a warning, rather than rejected. (#2205)
- A crate macro that another of its macros calls through `$crate::` is extracted with it, and a dependency's macro written where a pattern goes expands there. (#2206; part of #1152)
- An assignment in parentheses, such as `(sum += v)`, is an expression of type `()`, and macro hygiene no longer renames a closure parameter's type. (#2207)

## 0.3.248 (2026-11-17)

- The iced parity harness reports an example whose tests can't create iced_test's headless renderer (integration draws only with wgpu, and a GitHub runner has no GPU) as not runnable headless rather than as a mismatch. (#2197)
- The agent cursor that rawr's input tools show in a program's window travels to each move, click, drag and scroll along a short eased curve of about 200 ms, sending pointer moves along the way so hover follows, and a button goes down only once it has landed. It is drawn as a rounded orange dart with a white outline and a soft shadow, as screenshots show it too. (#2199)
- The agent cursor that rawr's input tools show in a program's window travels to each move, click, drag and scroll along a short eased curve, sending pointer moves along the way, and a button goes down only once it has landed. It is drawn as a rounded orange dart with a white outline and a soft shadow. (#2199)
- A dependency's macro_rules! call used as an expression lays out its expansion, so its items are declared and its paths resolve, and an inline mod a dependency macro declares loads as a module whose macro calls expand. (#2196, #2189; part of #1152)

## 0.3.247 (2026-11-17)

- A dependency declared in `[dependencies]` and again in a `[target.'cfg(..)'.dependencies]` table that holds gets the features of both, as Cargo unifies them. rawr kept only the first entry, so iced's integration example lost winit's x11 and wayland on Linux. (#2195)
- A save during `rawr test` keeps the program's tests: the reload is parsed with `cfg(test)` and the run's features as the start was, so the next test still runs and no restart runs `main`. (#2194)

## 0.3.246 (2026-11-17)

- A call of the program's own `macro_rules!` used as an expression declares the items of the blocks it expands to, so the expansion can name them, as rustc does. (#2191; part of #2188)
- A path through a module in a macro's expansion resolves, and an item a macro expands to in a block can name the items beside it, as rustc resolves both. (#2192, #2193; part of #2188)

## 0.3.245 (2026-11-17)

- The landing page's ratatui demo keeps drawing after a key press. A program in the browser's universe starts once its frame has been laid out at its shown size, so ratzilla no longer rebuilds an empty grid on the first key. (#2185; part of #2116)
- While a program in the site's browser frame has focus, the arrow keys, Space, PageUp, PageDown, Home and End go to the program only and no longer scroll the page. (#2186)
- The landing page's iced, ratatui and plain loop tabs can be linked to: `#iced`, `#ratatui` or `#loop` opens that tab in view, and choosing a tab writes its anchor. (#2187)
- A macro call where an item goes, of the program's own `macro_rules!` or a dependency's `macro_rules!` or proc macro, declares the items it expands to in its module or block, as rustc does. (#2183; part of #1152)

## 0.3.244 (2026-11-17)

- The bundle seal flushes each file before it measures its allocation, so a large runner binary on Linux is no longer reported as corrupted when the filesystem settles its blocks after the seal. (#2181; part of #2146)
- A crate macro that calls another macro of its own crate through `$crate::`, one the program never names, expands as cargo expands it rather than failing with 'expected expression'. (#2182; part of #1152)

## 0.3.243 (2026-11-17)

- A program's `&mut Context<'_>` crosses into crate functions, such as tokio's `poll_budget_available`, and the `Poll<T>` a crate gives comes back as std's `Poll`. (#2175; part of #1152)
- `Future::poll` polls a crate future, such as tokio's `Sleep` or `JoinHandle`, where it is, so it keeps its own methods. `Poll` is `Copy`, `Clone` and `Debug` as std's is and supports `map`, and `Box::pin` pins a future to poll by hand. (#2176; part of #1152)

## 0.3.242 (2026-11-17)

- When a sealed build bundle's file count or size changes, rawr's error names the total that changed, its sealed and measured values, and the file behind it. (#2177; part of #2146)
- Type parameters and variants named with an underscore and digits, such as `enum Out<_0, _1> { _0(_0), _1(_1) }`, parse as rustc accepts them. (#2174)
- A dependency's macro that calls itself or another of its crate's macros through `$crate::name!` expands with the crate's own `macro_rules!` rules rather than going to rustc. (#2173; part of #1152)

## 0.3.241 (2026-11-17)

- Marked code polls futures by hand: `std::future::poll_fn`, `Pin::new` and `Pin::new_unchecked`, `Future::poll`, `Context`, `Waker`, `Poll::Ready` and `Poll::Pending` in expressions and patterns, and `ready!`. Script futures are polled directly, and crate futures after `IntoFuture::into_future`. (#2172; part of #1152)

## 0.3.240 (2026-11-17)

- A stored API writes each re-export hop once, in a table beside its compiler units: counter's API value fell from 30.2 MB to 23.1 MB. (#2169)
- Stored classifications, facade plans and inventory entries write 32-byte digests as 64 hex digits rather than arrays of 32 numbers: counter's stored classification is 92 MB rather than 110 MB. (#2170)

## 0.3.239 (2026-11-17)

- A demand round rewrites less of the stored classification, because its support, definition plans and implementation ledgers are written in source order: counter's second round writes 30.8 MB of new parts rather than 41.9 MB. (#2165)
- `rawr check` retires the API, classification, facade and overlay values of the demand rounds its final demand superseded, as a bind does. (#2168)

## 0.3.238 (2026-11-17)

- A stored API writes each definition, implementation and export once, and each compiler unit once in a table, so counter's API value is 30 MB rather than 156 MB. (#2166, #2167)

## 0.3.237 (2026-11-17)

- A crate-facts capture deletes the extraction driver's protocol output once its facts are built, and a crate's facts promoted to rawr's home no longer keep a second copy in the project. (#2161, #2162)
- A cold `rawr test` keeps only the last demand round's API, classification, facade and overlay values, because binding the program retires the rounds it superseded. A round that grows the API writes far fewer new stored parts. On download_progress the project's stored parts went from 606 MB to 288 MB, and the last round's new API parts from 41.3 MB to 10.9 MB. (#2160)

## 0.3.236 (2026-11-17)

- A cold `rawr run` or `rawr test` of a program with large dependencies stores each crate's API facts about 6.5 times smaller and serializes them once rather than three to five times. pokedex's facts went from 9.1 GB to 1.4 GB, and its cold test from 21m38s to 12m45s in a debug build. (#2153)
- A `mod` declared in a block is named only in that block, as in rustc: two blocks, or a block and its module, may each declare a module of one name, and using it outside its block is E0433. (#2159; part of #1152)

## 0.3.235 (2026-11-17)

- A `mod` item inside a block expression works as in rustc: its items, `use super::*` reaching the enclosing module, and the privacy of its items. A file module in a block is rustc's error. (#2157; part of #1152)
- A `tokio::select!` branch may have a precondition (`pattern = future, if condition => body`). Every condition is evaluated before any future is polled, and a branch whose condition is false is disabled and its future never polled. (#2158; part of #1152)

## 0.3.234 (2026-11-17)

- A rebuild of a crate whose proc macro a marked module calls checks the program again rather than replaying the check of the old expansion. (#2156)
- A `use` of a dependency's macro that doesn't exist is rustc's E0432, even when nothing calls it. (#2155)

## 0.3.233 (2026-11-17)

- A marked module calls a dependency's function-like proc macro by the name a `use` binds, such as `dec!(1.5)` after `use rust_decimal_macros::dec;`, from the proc macro crate or a library's re-export. (#2149)
- A dependency's function-like proc macro called where a type or a pattern goes expands through rustc, as calls in expression and statement position do. (#2150; part of #1152)
- Editing a dependency's proc-macro crate expands each of its calls and derives again against the new build, and expansions no passing check reads are deleted from `target/rawr/expand`. (#2152)
- Every new call of a dependency's function-like proc macro in a check goes to rustc in one expansion run, rather than one run and one round per call. (#2151)

## 0.3.232 (2026-11-17)

- The iced parity harness gives an example's direct iced_winit or winit dependency x11 and wayland on Linux and the BSDs, as iced's workspace does, so integration builds there. The nightly sweep keeps each example's logs. (#2148)
- A marked module can call a dependency's function-like proc macro by its path, such as `rust_decimal_macros::dec!(1.5)`, directly or through a library's re-export. rustc expands the call, and a save that changes its input expands it again. (#1125; part of #1152)
- When an iced example's build fails with no compiler error, the parity harness shows the last line the build printed rather than "no error line". (#2154; part of #2146)

## 0.3.231 (2026-11-16)

- A browser program whose `main` panics shows Rust's panic on its page. `std::thread::spawn` there panics with cargo's own wasm32 message. (#1160)
- The import pipeline builds a runner for a package with no library and no dependency, so `rawr run --web`'s shared runner for plain programs is rawr and std alone. (#2137)

## 0.3.230 (2026-11-16)

- `vec![return x]` and `[return x, ..]` type their element as a variable a later use settles, so a later `v.push(String::new())` compiles as it does with rustc. (#2139)
- A `select!` branch whose pattern doesn't match is disabled and the others keep running. With every branch disabled the select runs `else`, and branches are polled from a random one unless `biased;` is given, as tokio does. (#2140; part of #1152)
- A `match` on `()` or a reference to it takes `()`, `_` and binding patterns. (#2145)
- A git dependency's percent-encoded branch, tag or rev in Cargo.lock (`feature%2Fx`) is decoded as cargo reads it. (#2143)

## 0.3.229 (2026-11-16)

- A runner that compiles rawr in the project settles its lock when the project locks an older version of a crate rawr needs. It takes rawr's version of that crate and keeps every other version and git revision the project pins. (#2144)

## 0.3.228 (2026-11-16)

- A runner keeps the git revisions the project's Cargo.lock pins. After `cargo update -p <crate> --precise <sha>` it builds on that revision, and it no longer mixes two revisions of one git source. (#2142)

## 0.3.227 (2026-11-16)

- A crate future inside what a call gives, such as a `JoinHandle` in an `Option`, a `Vec`, a channel or from `JoinSet::join_next`, can be awaited. (#2135)
- `tokio::time::timeout` works on an async block, a `Sleep` or a `JoinHandle`, and its `Timeout` is awaited as a `Result` of the future's output. (#2136)
- `tokio::select!` takes several branches and runs the body of the first future to finish. (#2138; part of #1152)
- `Some(return)` and similar payloads that never come are settled by later code or fall back to `!`, and a value holding `!` is `Copy`, as rustc reads it. (#2134)

## 0.3.226 (2026-11-16)

- `rawr run --web` runs a program with no dependencies, such as one marked .rs file, in the browser and reloads its saves there. Every such program shares one runner of rawr and std, built once per rawr version, and the page shows what the program prints. (#2122)

## 0.3.225 (2026-11-16)

- `return` and `break` with no value before `)`, `]`, `,` or `}`, as in `Some(return)`, parse as rustc parses them. (#2133)
- `{:?}` of a value whose type only a diverging expression gives, such as `Vec<!>` from `.map(|_| panic!()).collect()`, prints as rustc's build does rather than giving E0282. (#2131)
- A crate's type that implements `Future` keeps its own type and methods. tokio's `JoinHandle` has `abort` and `is_finished`, a program's function can return one, `Sleep` has `is_elapsed`, and each is still awaited or joined. (#2132; part of #1714)

## 0.3.224 (2026-11-16)

- A type that only a diverging expression gives (`loop {}`, `panic!()`, `todo!()`) is `!` once the body is read, as in rustc's edition 2024. `tokio::spawn(async move { loop { .. } })`, `id(loop {})` and a closure returning `panic!()` check where they gave E0282, and `!` fails a bound as it does in rustc (`!: Default`). (#2128)

## 0.3.223 (2026-11-16)

- An async `main` keeps running across a save under `rawr run`. The new code takes over the next time one of its futures resumes, such as a timer loop at its next tick, and the futures keep their locals. An edit to `main`, or to the body of an async function or block a future waits in, restarts the program with a one-line reason. (#2127)

## 0.3.222 (2026-11-16)

- A save that only moves an async function a suspended future waits in, such as a line added above it, keeps the program running with its state. A restart for an edited async block names the block. (#2126)

## 0.3.221 (2026-11-16)

- An edit to the async function a running program is suspended in restarts the program with a one-line reason, under `rawr run` and on `rawr run --web`'s page, rather than failing at its next poll. (#2117)
- `rawr run --web` compiles a project with the features and cfg options Cargo resolves for it on wasm32, a workspace member's inherited settings included. A save that changes them restarts the page. (#2121)

## 0.3.220 (2026-11-16)

- `rawr run --web` names errors' paths from the directory it ran in, as `rawr run` does. For a crate API the runner doesn't cover yet, the page says the runner builds again and reloads onto it, and when that build fails, the page says so. (#2120)
- `rawr run --web script.rs` runs a cargo script with dependencies in its frontmatter in a browser, and reloads it on each save. (#2123; part of #2122)
- `rawr run --web` packs a runner for a project that doesn't itself depend on wasm-bindgen. (#2124)

## 0.3.219 (2026-11-16)

- `rawr run --web` builds a project's crates and runner for wasm32, serves its page on localhost and reloads each save there with its state kept. Compile errors show on the page and in the terminal, and a save that uses a crate API the runner doesn't cover builds the runner again. `--out <dir>` writes the page's files instead of serving them. (#2119; part of #1395, #1387)

## 0.3.218 (2026-11-16)

- The landing page's ratatui snippet runs in your browser as an app. It draws with ratzilla's DomBackend and awaits its keys, which reach it only while its frame has focus. (#1384, #1184)
- A crate method that lends a closure `&mut` of a type with a lifetime and returns a borrow of its receiver takes a script closure. ratatui's `Terminal::draw(|frame| ..)?` is one. (#2112; part of #1457)
- A future waiting at an `.await` keeps running after an edit that left its own function unchanged. (#2113)

## 0.3.217 (2026-11-16)

- Runner builds no longer wait for each other when rawr's shared runtime is already built, so several `rawr run` or `rawr test` builds on one machine run side by side. (#2094)
- A crate trait's method reaches a slice through a blanket impl bounded on `&I`, as rayon's `par_iter()` does on a slice, `Vec` or array with `use rayon::prelude::*;`. A crate type that derives `Clone` and holds a boxed script closure, as rayon's `Map`, no longer fails the runner build. (#2079, #2064)
- iced_test Emulator tests build and run under `rawr test` as under `cargo test`: a program's events, channels and presets are compiled where rustc knows the program's type, including a type a builder method gives. (#2060, #2078)
- Moving a value bound by a crate enum's pattern inside a loop is no longer reported as a use of a value moved in a previous iteration. (#2077)
- A method call on a value a function's `impl Trait` gives, and `Trait::method(&value)`, call the trait's method as rustc does, even where the value's type has an inherent method of the same name. (#2105; part of #2099)
- A crate trait's `&self` method that lends a slice of values naming the trait's associated types, as `presets(&self) -> &[Preset<Self::State, Self::Message>]`, works on an `impl Trait` value. The slice is lent in place even when its elements don't clone or hold the program's own types, and an element `find` gives passes back to a crate function as `Option<&T>`. (#2102)
- A crate method called on a closure's `&&T` item, as in `.iter().find(|p| p.name() == ..)` over a generic crate type, is called at that type's arguments. (#2102)
- A glob import in a marked module brings in what its parent's own globs and imports bind, such as a native module the grandparent declares, as rustc does. (#2110)

## 0.3.216 (2026-11-15)

- A view a program or test calls itself, outside the application, builds a widget that joins a button and a `&str` label, such as a tooltip, instead of panicking as a rawr bug. (#2057)
- A crate method that calls the program's own widgets back through an erased element, such as a test simulator's snapshot of a custom widget, reaches the program's values instead of panicking with `Stale`. (#2058)
- Changing state that a view called outside the application borrowed, once that view is dropped, no longer fails with a false E0502. (#2059)
- A project's rawr build keeps only each target's current generation. A new run or test run retires the runner and resolution builds it supersedes, and their parts, so a project's store no longer grows by hundreds of MB per change. Switching between `rawr run` and `rawr test` still reuses both runners. (#1407)
- Compiling rawr's runtime for a new rawr removes the runtime builds it replaces on the same compiler, unless an installed rawr still links them, so the rawr home no longer grows about 1.1 GB per rawr change. (#2046)
- The site is readable by coding agents: every page has a Markdown copy at its address plus `.md` (such as /docs/run.md, /play.md, /todo.md and /changelog.md), /llms.txt is an index of them with the essentials, and /llms-full.txt holds every docs page in one file. (#2072)
- A crate whose method gives an opaque `impl Trait + '_` with a borrowing associated type, as polars' `par_iter` gives `impl ParallelIterator<Item = Option<&str>> + '_`, imports. Its trait's members, such as `chain`, wrote the type with an argument its declaration didn't have, and the import panicked before the program started. (#2073)
- A builder chain whose methods take and return `&mut Self`, as tokio's `runtime::Builder::new_current_thread().enable_time().build()`, runs on a temporary or a local, for crate types without `Clone`. (#2061)
- `.into()` of a `&[u8]` or `Vec<u8>` fills a crate field of type `Cow<'static, [u8]>`, as `Settings { fonts: vec![FONT.into()], .. }` does, and a program struct can hold a `Cow` field. (#2063)
- A call that takes a value holding the type a crate function returned around an opaque type, such as a sender of events of `Wrapper<impl Trait>`, builds under `rawr run` and `rawr test`. (#2075; part of #2060)
- A crate trait's provided method whose type parameter shares a name with its impl's is callable, and a missing method on a slice of an imported type names the slice as rustc does (`[Theme]`). (#2076; part of #2064)
- A crate trait's provided `map` takes a closure of the impl's own item, and `collect()` into a `Result` fixes the closure's error type from the crate's impl for `Result`, as rustc infers it. (#2080; part of #2064)
- An unannotated closure given to an opaque program's builder method, such as `.subscription(|state| ..)` or `.scale_factor(|_| ..)`, takes its input from the program's `State` binding again instead of panicking the import. (#2084; part of #2073)
- rawr-import-cli's integration suite keeps fixture lanes, the shared rawr home and started slots between runs, and replaces a slot only when its stamp (the fixture's files and the rawr-import that ran it) no longer matches. This amends how #2045 cleaned up: only stale parts go. (#2085)
- A crate's callable value fills a callable parameter again, a crate method returning a `Cow` inside a scope frame compiles again, and a second run of an unchanged project writes nothing to `target/`. (#2086)

## 0.3.215 (2026-11-15)

- The playground runs a snippet that uses iced in the browser: its window is drawn in the output pane and takes clicks and keys, with no server round trip. (#435; part of #1395, #1384)
- `rawr-import run` of a project built for a target this machine can't run, such as wasm32, builds its runner and exits 0 without starting it. (#1753)
- The browser universe builds and covers its programs' calls again: one proxy holds a universe runner's script type, and a script impl crosses only with its own program's uses. (#2027; part of #1395)
- A Rust dependency that declares several crate types, as a library exposing a C ABI does with lib, cdylib and staticlib, no longer stops `rawr run` with an internal error. The runner also builds for crates whose async methods give futures that borrow their receiver; calling one says it isn't supported yet, instead of the native build failing. (#2029; part of #2024, #2030)
- In the playground's browser mode, an edit to a running iced snippet reloads it in place and keeps its state. A compile error shows under the window while the program keeps running, and an edit that can't reload starts it over and says why. (#1455; part of #1395)
- A save that adds, removes or reorders a marked type's private fields keeps the program running: each live value takes the new fields in place, a new field at its type's `Default`, and one line says what changed ("`App` gained `filter` (default); state kept"). A native loop that passes its state as `&mut` keeps it too, with no native build. (#2026)
- An async fn or async `&mut self` method can use its `&mut` parameters across `.await`: its future holds the caller's place, as in Rust, and using the place while the future lives is rustc's borrow error. (#176; part of #2024)
- A save that first names a crate module's item through a `use` that also binds a function of the same name, as `.style(button::success)` after `use iced::widget::button`, in a view that returns `impl Widget`, now rebuilds and runs, as a fresh start does, instead of failing with E0599. (#2004)
- A closure whose `?` converts into an error type nothing in the body names is rustc's E0282 "type annotations needed", as rustc reports it; `unwrap` on its result no longer says "`_` doesn't implement `Debug`". (#2039)
- A project that uses smallvec, smartstring or thin-vec without serde keeps its one copy of the crate: the runner no longer turns on serde in them. (#2041)
- After an edit to a crate that a trait-method call's proof reads, the next run lowers that call again instead of failing with "missing runtime binding". (#2042)
- A crate method that returns a borrow guard such as `Ref` builds and is read through its guard, as `*meter.borrow()` is. Before, only a method returning a plain `&T` built. (#2047)
- `&Some(..)` of a string or number passed as a `&dyn Debug` or `&dyn Display` reaches the crate, as `both("count", &count, &Some("two"))` does. (#2048)
- A boxed region value whose type is invariant in its own lifetime takes another value of its region again, as a pane's `title_bar(container)` does. (#2049)
- A call made in a view's region that takes a crate value holding an opaque type, such as `hover(scrollable(preview), right(..))`, takes that value at the region's lifetime. iced's markdown example starts again; it panicked with "a value reached an adapter as a type its signature doesn't take". A method that ties the value's own lifetime to a region value, as a pane's `title_bar` does, compiles there too. (#2050)
- A Rust dependency or `[patch]` path reached through a symlink, such as a sibling directory that links elsewhere, builds under `rawr run` as it does under `cargo build`. So does a symlinked `Cargo.lock` or a symlinked file inside a dependency. (#2055)

## 0.3.214 (2026-11-14)

- A use after a move gives rustc's note on the parameter that took the value and its clone suggestion, and a borrow of a moved local says 'borrow of moved value' over the borrow. (#921)
- `let x;` follows rustc: a second assignment on any path is E0384, a match arm can assign it, `let (a, b);` works, and a Drop type drops only where it was assigned. (#946)
- Arithmetic on literals that overflows or divides by zero, and an array index past its length, are refused with rustc's arithmetic_overflow and unconditional_panic errors. (#947)
- A type mismatch underlines the whole expression, as rustc does. (#1729)
- A project built with `split-debuginfo = "packed"` runs under rawr: the `.dSYM` debug info Cargo names or links beside a build script is no longer read as a build output, a crate rustc can find, or part of the runner's build. (#1923)
- The std table records each constant's type, and the std facade declares `UNIX_EPOCH` and the numeric constants from it. (#925)
- `Option::insert`, `get_or_insert`, `get_or_insert_with` and `get_or_insert_default` give the payload's place, so `*o.get_or_insert_with(f) += 1` works. (#1001)
- `Receiver::try_recv`, `recv_timeout`, `sync_channel`, `thread::scope` and `thread::Builder` run as std's, and a receive's errors are std's own. (#950)
- A `Mutex`'s `lock().unwrap_or_else(PoisonError::into_inner)`, `is_poisoned`, `clear_poison` and `into_inner()`'s `LockResult` work. (#1927; part of #949)
- Errors print as rustc and cargo print theirs: rawr's diagnostics use rustc's layout, with the source line under every label, cargo's errors keep their colours, and rawr's internal failures read `error: rawr could not ...` with the detail in a `= note:`. Colour follows cargo's rules (`--color`, `CARGO_TERM_COLOR`, `NO_COLOR`); pipes, logs and `rawr mcp` stay plain. (#1928)
- A crate function that takes a shared borrow of a tuple holding borrows, as log's `&(&str, &'static str, &Location)`, or a `fn(&T) -> U` pointer is no longer refused as having a higher-ranked lifetime; marked code passes the tuple and a script function as rustc does. (#1559, #966)
- A project's Cargo.lock never has to change for rawr: rawr's own dependency floors no longer raise a package the project pins, such as toml_datetime. (#1914)
- A string literal or `&'static str` const passed where a crate takes `impl Into<T>` lives for the program when T converts only from `&'static str`, as iced's `.id(..)`, `operation::focus(..)` and `scrollable::snap_to(..)` take one. (#726)
- A float pair passed as `impl Into<Size>`, as iced's `window_size((500.0, 800.0))`, takes f32s, as rustc infers them. (#1941)
- A crate method that returns `&Path`, as directories' `ProjectDirs::data_dir()`, and a tokio future that takes `&[u8]`, as `fs::write(path, json.as_bytes())`, no longer fail to build. (#1942, #1943)
- A path that starts with `::` after a keyword, as gallery's `match ::image::load_from_memory(..)`, names its crate. (#1935)
- A crate trait that a `use` inside a function brings into scope, as game_of_life's `use itertools::Itertools;`, gives its methods to calls in that function. (#1936)
- `std::convert::identity` runs, a std function passed as a value (`.on_link_click(std::convert::identity)`, `.map(std::mem::take)`) is the closure that calls it, and a one-argument call accepts a trailing comma. (#1944)
- `log::error!`, `warn!` and `info!` in marked code run as under Cargo: `std::panic::Location` crosses as std's own value, and a crate's `#[track_caller]` function sees the program's call site, so a logger prints the user's file and line. (#1560)
- Crate structs with a PhantomData field, as palette's Rgb and Hsl, build in the runner when the program has moved another of their fields out, so iced's color_palette runs. (#1931)
- `image(&handle)` boxed at iced's renderer takes iced's Handle, as rustc infers it, so iced's screenshot runs. (#1939)
- A crate's async fn whose elided lifetime only fills a type's lifetime argument, as wgpu's `initialize_adapter_from_env_or_default(.., Option<&Surface<'_>>)`, can be called. (#1937)
- A marked enum whose variants carry nothing and which derives Copy reaches native generic code as a Copy value that compares, orders and hashes as the enum derives, so iced's `radio` takes it and iced's scrollable builds unchanged. (#1940)
- An assignment evaluates its value before its place, as rustc does, so `*value.part_mut() = value.part() + 1` writes through the borrow the method lends, as iced's ferris does when it spins. (#1945)
- A future of yours that a crate's executor polls on its own threads, as iced's `Task::perform` does on tokio's workers, runs there as under cargo, and the native values it holds across an `.await` go with it. (#1949)
- A crate function that borrows a value whose type holds a borrow inside a type argument, as wgpu's `request_device(&DeviceDescriptor<'_>)` does, can be called. (#1958; part of #1953)
- A builder that keeps a borrow of a `?Sized` key, as iced's `Sensor::key_ref(url.as_str())` does, can be called, and a native module's path works beside a `use` that binds the crate's own name, as iced's markdown calls `icon::copy()` beside `use iced::widget::markdown`. (#1959; part of #801)
- A `&dyn Trait` type argument, as rfd's `set_parent(&window)` takes, reaches the crate as the trait object. (#1960; part of #28)
- A struct declared in a function body with a crate's derive, as pokedex's `#[derive(Deserialize)] struct Entry`, works again, and its derived impl's generic functions build in the runner. (#1938)
- A native module can name what the marked crate root re-exports from a crate or a native module, as custom_shader's `use crate::wgpu;`, and marked code reads native fields whose types live in private native modules. (#1932)
- A crate's `static X: LazyLock<T>` of a crate value is read as the T it derefs to. (#1963; part of #610)
- rawr mcp: a click, key or other input sent right after `start` waits for the program's first window instead of saying the program shows no window. (#1947)
- Projects that share one target directory keep separate rawr caches, so one project no longer builds from another's generated crate or fails with 'Cargo.toml changed during the build'. (#1934)
- A start whose cached entries were removed since they were read starts again instead of failing with an internal store error, and a failure while starting a program says rawr could not start it. (#1933)
- A crate builder method that keeps a borrow in Self, as loading_spinners' `Linear::easing(&'a Easing)` does, takes a static or the program's value for as long as the value it is kept in. (#610)
- A script closure can take a crate trait's object that native code lends for the call, as iced's `window::run` lends `&dyn Window`, and pass it on to another crate call. (#1967; part of #28)
- A crate function's opaque value that borrows a string it is given, such as iced's `markdown::parse(&text)`, lives beside the string, so iced's changelog builds and runs. (#1966)
- A struct literal or pattern written through a crate's type alias, as wgpu's `DeviceDescriptor { label: None, .. }` and `SurfaceConfiguration { .. }`, builds the aliased struct with the alias's arguments, and a `&self` method's `impl Future` that borrows nothing, as `request_device`, can be awaited. (#1961; part of #1953)
- Calling a `&self` method on a crate struct's field, as wgpu's `capabilities.formats.iter()` does, borrows the field where it is and leaves the struct usable, as in Rust. (#1969)
- A crate struct's `Option<&str>` field can be read in a pattern and filled from `..Default::default()`. (#1970)
- A script function that returns an async block can fill a crate's generic type, a crate function that takes a future, as `.then(Task::future)`, runs it, and an `Rc` or `Arc` of a string or number crosses as Rust's own pointer to it, so iced's editor builds and starts unchanged. (#551)
- std's `io::Cursor` runs: a program makes one over its bytes and reads, seeks and rewinds it, and passes one to a crate, as image's `ImageReader::new(io::Cursor::new(bytes))` takes it; `io::SeekFrom` is std's own. (#1976; part of #87)
- A full-path std call with a turbofish, as `std::sync::mpsc::channel::<u8>()`, is a call again beside a std function named as a value. (#1980)
- A closure's `?` converts its error through From into the error the closure's tail gives, as rustc infers it, instead of fixing the error type at the first `?`. (#1977)
- A value that keeps a `&'a mut` and a `&'a` of the crate values it was made from, as integration's `Scene::clear(&view, &mut encoder, color)` gives a `RenderPass<'a>`, lives in its block and holds both places for its life, as rustc's borrow does. (#1973)
- A function or closure, borrowed or boxed, can be used as a crate trait's object when the crate implements the trait for every function of one signature, as iced's markdown takes `&Code::highlight` as a `&dyn Highlighter<Code>`; native code calling the object's methods runs the program's function. (#1981)
- A `&self` method that lends its receiver to a value it takes by `&mut`, as integration's `scene.draw(&mut render_pass)`, works, and the receiver stays borrowed while that value lives, so changing it meanwhile is rustc's E0502. (#1982)
- `&mut` of a crate value made for the call, a `&mut` binding where a crate takes `&dyn Trait`, an `Arc` of a crate value passed as `&dyn Trait`, `for` over a crate type through its IntoIterator, and a public field of a crate struct that implements Drop all work. (#1982)
- rawr mcp's `screenshot` is drawn by the program itself through its own renderer, so it needs no Screen Recording permission and shows the window's current state even while another window covers it. (#1989)
- A new build of rawr recompiles only the runtime crates whose sources changed: builds that agree on toolchain, lockfile, features and seeds share one build directory, and an edit outside the runtime's crates recompiles nothing. (#1983)
- A window program `rawr run` runs on macOS keeps its own name in the menu bar and gets a `rawr` menu beside its own, with Restart, Take Screenshot, About rawr and Exit. (#1990)
- Projects that share a target directory no longer reuse each other's runner build scripts, so after rawr changes each runner links the new runtime instead of failing with E0425 on the old one. (#1984)
- A `&self` method whose `impl Trait` result captures the receiver, as edition 2024 does by default or as `+ '_` writes it, keeps the receiver borrowed for as long as the result lives. (#1991)
- A crate call takes by value a result that borrows its call's values even when its trait has type arguments and supertraits and its methods borrow, as iced's `UserInterface::build(controls.view(), ..)` does, and the values it borrowed come back when native code drops it. (#1998; part of #1997)
- A program's impl of a crate trait whose function returns `impl Trait` returns the type its body gives, as markdown's `Viewer::image` and `code_block` do. (#1999; part of #83)
- An if, else if or match that picks one of a crate's functions, such as text::warning or container::danger, passes the function it picked, and two calls given different crate functions each run their own, so table colours its cells and toast its headers as cargo does. (#2003)
- An associated function's arguments take its parameter types as their expected types, so `State::with_life(cells.collect())` collects into the type it takes and game_of_life starts. (#2006)
- iced's integration example draws frames and takes input: a field borrow in a let whose value borrows nothing ends with the statement, and a value bound out of a crate enum's variant moves rather than cloning, so a SurfaceTexture is presented once. (#2000)
- iced's editor example takes key presses: a match arm that binds nothing of a borrowing subject runs after the borrow ends, so key_binding's `_ => Binding::from_key_press(press)` moves the press. (#2005)
- A program's impl of a crate trait can return a reference to a crate value it holds, as iced markdown's `fn theme(&self) -> &Theme` does, and native code borrows the value where it is for as long as `&self` lives. (#2001; part of #83)
- A future or a Send closure that captured a crate value can use it on the thread that polls or calls it, so iced's gallery loads its images. (#2014)
- A program's From impl into a std type, such as `From<Language> for String`, reaches a crate's `impl Into<String>`, so iced's tour shows its language radio. (#1987)
- `rich_text![..]` and other `FromIterator::from_iter` calls on a crate type take the program's array of spans. (#1988)
- A future polled on another thread brings back the crate values it made, inside an Option or a Result too, so pokedex's images and todos' saved task ids survive. (#1986, #1957)
- A widget made inside a view and passed in an Option, as gallery's `stack![content, viewer]` does, reaches the call that boxes it, so gallery's image viewer opens. (#2015)
- A closure can compare parameters whose types its later calls fix, and keyed_column takes (key, element) pairs made inside a view, so iced's todos adds, checks, filters, deletes and saves its tasks. (#2019; part of #249)
- A program's impl of a crate trait whose `impl Trait` function returns a crate type holding another crate function's opaque output compiles: the runner gives the boxed value, which implements the trait and its supertraits. (#2013; part of #83)
- A crate call whose result keeps a slice a crate method lent, as `view_with(content.items(), ..)`, borrows it where it is, and a crate value that borrows its inputs can join a value made in a view's region in one call, as `hover(code_block(..), right(..))`. (#2020, #2021; part of #83)
- A trait method a program implements, whose result borrows a slice iced passes it, can return a crate value that keeps that slice, as markdown's `Viewer::code_block` does with `markdown::code_block(self, settings, lines, ..)`. (#2022; part of #83)
- A crate call whose opaque result keeps a slice borrowed from a field no longer leaves that field borrowed once the view is gone, so the next `update` can change it through `&mut`, as markdown's stream does. (#2023; part of #83)

## 0.3.213 (2026-11-13)

- A restarted program's runner reads its image's facade outline from disk instead of decoding every facade body again, and the first runner decodes each body without building JSON trees. (#1897; part of #1755)
- A save that adds a private field of a plain type to a struct no longer lowers again the other modules that name it: their kept lowerings are reused with the struct's new schema. (#1885)
- A save to a marked file of an idle windowed program on macOS is applied at once on its main thread, with no input, and code its frames call shows on the next redraw. (#1889)
- A restart no longer names rawr's own generated files, such as the store's Cargo.lock, as the change that caused it, and rawr never watches them. (#1890)
- A marked call whose argument a crate's `impl Into<T>` can't take, such as `.spacing("zero")`, fails at once with rustc's E0277 at the argument instead of minutes later in rawr's generated code; generated crates no longer warn about build metadata in their pinned versions. (#1891)
- After a save whose native build failed, a save back to what the running program was built from swaps in place, with no new native build and no restart. (#1892)
- Assigning a field of a native field that isn't Copy, as `theme.colors.accent = theme.colors.error`, changes it in place, as rustc does. (#1893)
- Calling a method on a Copy field through a native field path, as `theme.colors.error.clone()`, no longer moves the path; a later `theme.colors.clone()` checks as rustc checks it. (#1894)
- A native struct literal with `..base` runs its fields in source order before the base's borrow, as `Colors { accent: theme.colors.error.clone(), ..theme.colors.clone() }`. (#1895)
- A save whose first `.into()` needs a crate's From impl, as `text(..).into()` into an Element, prepares again with it instead of failing with an unknown member. (#1896)
- An app that keeps its view, as iced does, can show a save of view-only code at once: the new `rawr-hot` crate tells the app each save it takes, and `Subscription::run(rawr_hot::generations).map(|_| Message::Reloaded)` turns that into a message. Under cargo it never fires. (#1903)
- A path dependency whose manifest names a file above its own folder, as a workspace member's `readme = "../README.md"`, no longer stops `rawr run` with an internal error. (#1904)
- rawr's runner build reuses the units `cargo build -p <member>` made in a workspace whose root is a package, as icedxdy's: it resolves features with the workspace's resolver when the root inherits its edition, keeps every version the project's Cargo.lock pins, places the root package and its path dependencies in the runner's workspace, and never prunes a path package's unit that cargo built. (#1911, #1912, #1913)
- A respawned runner reads each source's interface outline, the walks over the program's declarations and each item's and instance's digest from what the exiting runner kept on disk, so a restart after a save it already compiled lexes, walks and digests only what changed. (#1898, #1899; part of #1755)

## 0.3.212 (2026-11-13)

- A cold `rawr run` on a large program no longer stalls in reveal: each instance round digests again only the instances it added or revealed, not every instance signature with its facades' whole proofs. (#1887; part of #1755)

## 0.3.211 (2026-11-13)

- A save's compile resolves again only the files whose names or reads changed, reusing the rest of the program's resolution from the last save. (#1876)
- A save that adds a private field to a struct no longer reveals the other marked files again, each instance round reads again only the bodies whose calls gained an instance, and a host call's type lookup no longer searches or builds paths to compare. (#1884; part of #1755, #1850)
- A save that changes a declaration lowers again only the files whose names can reach it; a file that merely writes a name of the same kind, such as its own `State`, keeps its lowering. (#1886)

## 0.3.210 (2026-11-13)

- A restart that needs no native build starts the program sooner: the new runner skips its walk of the program's dependencies when the file watcher saw none change, unchanged source files aren't read again, and the runner looks up its adapters instead of scanning for them. (#1879; part of #1755)
- A restart starts the new runner sooner: the runner is spawned with posix_spawn, so the owner's memory is no longer copied at each runner start, and the runner clears its own signal mask; a runner the owner ends for a restart or a refusal leaves at once, without the GPU and windowing teardown. A program's own exit still runs its teardown, and Ctrl-C still reaches it as under cargo run. (#1880; part of #1755)
- A save in a running program parses again only the lowered text of the parts that changed, and hashes again only the declarations that changed, so a save of one file no longer reparses the whole program. (#1877, #1878)
- A marked function that takes `&mut` of a native type with an elided lifetime, as ratatui's `&mut Frame`, builds again (it failed with E0521), and a whole new value it gives that place, also for a type with a named lifetime, comes back to native code. (#1881)

## 0.3.209 (2026-11-13)

- A save that changes only Rawr code no longer installs every crate's facades again for the program's check: the check shares the registry the last check built for the same API, and a facade's install now merges its entries instead of re-sorting the whole registry. (#1870; part of #1755, #1850)
- A start or save of a large program does less repeated work: a reused body's text is never built, each body's text is as long as the body, a source's interface reads only the declarations it reaches, and resolution neither copies each unit's items nor scans every declaration per module. (#1871; part of #1850, #1755)
- A marked struct that native code holds by value but never opens (no field read, struct literal, pattern or layout read, and no `&mut` of it outside a method's receiver) crosses as a handle of the program's value, so adding or removing one of its private fields builds nothing native: the program restarts from its runner, or swaps when no value of the type is alive. Its auto traits still follow its fields, so `rawr check` rejects what `cargo check` rejects, and a native module that starts reading a field gets the real layout back. (#1872; part of #1865)

## 0.3.208 (2026-11-13)

- A native value the runner protocol handed out is dropped once the program that took it back drops it, not kept until the next program; repeated calls find their adapter, kept slices, releases and pinned strings without a scan, and a crossing call's witnesses no longer copy their types. (#1852)

## 0.3.207 (2026-11-13)

- A save does less repeated work in the runner build and the generator: the runner executable is hashed once, Cargo.lock and each manifest are parsed once, Cargo's messages are decoded once, and each plan's signature and opaque facts are computed once. (#1853)
- A kept overlay of runner sources no longer re-hashes every generated file on a hit. (#1831)

## 0.3.206 (2026-11-12)

- A start whose API is current reads it, its classification and its facade once: identities share their unit, each definition is digested once, and stored bytes are trusted by their digest instead of being encoded again to check them. (#1866; part of #1851)

## 0.3.205 (2026-11-12)

- A save or round that reads shared bindings no longer reads, hashes and parses every crate's table again: inputs are checked first, and a table replayed for the same demand is kept by the running owner. (#1845)
- A save no longer reads every file of rawr's copy of the crate to compare it: the owner remembers what it wrote and reads and writes only copies whose text changed. The program the save loaded is checked without loading it again, and the rawr directory is resolved once per run. (#1863; part of #1849)
- A native build's input capture indexes the build once for all the crates it extracts: each input file is observed once per build, and once per worker in a round's checks of shared tables, instead of once per crate; artifact joins use sorted lookups, a dependency's closure is walked from one index, and dep-info is parsed once. (#1846)

## 0.3.204 (2026-11-12)

- A native save's store work no longer grows with the whole store: the installation ledger is decoded only when it changes, a reservation reads no index rows and repeats no recovery, an entry takes one full flush instead of four, and a freshly computed node is used as computed instead of being read back. (#1843, #1844)

## 0.3.203 (2026-11-12)

- `rawr run` no longer fails to settle the runner's lockfile when the program's crate has an optional dependency the project patches (inboard's `iced_test`). The runner drops a `[patch]` only when no package of its workspace names it, optional and target-specific dependencies included. (#1840)
- A save that builds no longer parses Cargo's metadata and unit graph a third time: the replay that admits the new bundle takes what the build parsed. (#1836)
- A save whose Cargo view is remembered no longer runs or parses the provisional `cargo metadata`. (#1837)
- A save that changes one private field checks again only the sources it changed: the owner keeps the compiler that checks the program, and each source's uses under what its lowering reads. On the islands-theme fixture a private-field save lowers 1 of 5 sources and its check's lowering takes 0.6 s instead of 1.4 s. With RAWR_VERBOSE each round says how many sources its check lowered and how long each step of the check took, and a start says how long each of its rounds' phases took. (#1857; part of #1850, #1815)

## 0.3.202 (2026-11-12)

- A native save back to an earlier save's sources, or the launch's, reuses that bundle's parsed Cargo view instead of parsing it again, and the metadata probe resolves each loaded crate's paths once per run, not once for every crate rawr names. RAWR_VERBOSE also times a native save's harness preparation, output copy, input capture, seal, bundle replay and the program's check. (#1835; part of #1794)
- A native edit that changes no crate's API, such as a private field, no longer rebuilds the API graph or decodes the classification on a save: the owner reuses the graph it kept, with the new build's details, and the classification the last save read. (#1839; part of #1786, #1799)

## 0.3.201 (2026-11-12)

- A save's restart compiles rawr's copy of the program's crate with the profile `cargo build` gives the crate. A project whose workspace dependency lies outside its workspace, or that has no path packages, had the copy built with its `[profile.dev.package."*"]` settings, which on inboard meant opt-level 3 at every restart. (#1833; part of #1789, #1801)

## 0.3.200 (2026-11-12)

- rawr-import's driver builds again on the newest nightly in its window (nightly-2026-11-02), so projects on that nightly can import. (#1829)
- A rawr run save that needs the API graph shares the one its owner built or is still building for the same API, with no lock and no copy; a save for a different API builds its own at once instead of waiting. (#1822)
- Shared values no longer sleep up to 50 ms when a borrow conflicts: a conflict fails at once, a Mutex's own state waits for the other thread, and a borrowed value is never mistaken for one of another type. (#1823)
- A function taking a struct parameter checks it without copying it, so a native program's state lent to marked `update` is no longer copied on every call. (#1823)
- A native edit that changes nothing the runner's sources are rendered from, such as a private field added to a struct native code holds, restarts the program without rendering or storing those sources again. `RAWR_VERBOSE` says how many of the runner's files each generation rendered. (#1821)

## 0.3.199 (2026-11-12)

- A save that puts back sources an earlier save prepared reuses that save's native build: no new harness, copy, seal or probe. (#1817; part of #1794)
- rawr's copy of a crate with marked modules no longer type-checks the island's support code on every rebuild: the support is a package built once and shared across saves. (#1808)
- rawr's copy of a crate gives a marked function's trampoline its body only when native code calls the function, so the copy type-checks about a quarter less code; a new native call writes the body and checks again before anything runs. (#1807)
- With RAWR_VERBOSE, a save says what its nightly check compiled and how long it took, how long the probe and each phase took, what the start is still doing in the background, and how long generating and staging the runner's sources took and how many files that wrote. (#1820; part of #1815, #1816)

## 0.3.198 (2026-11-12)

- A marked `main` whose save needs a new native build keeps running while the runner builds, and ends just before the new program starts, so its terminal, ports and locks are free; a build that fails leaves it running and prints the error. (#1810)
- The runner protocol no longer carries the unused state-carry messages; no state crosses a restart. (#1812)
- On macOS, a program `rawr run` restarts, or starts again later, opens its windows where you left them, unless the program now asks for a different size or position. (#1811)

## 0.3.197 (2026-11-12)

- rawr's copy of a crate with marked modules builds its generated glue only where it is called, roughly halving the copy's codegen: 3,522 functions instead of 9,124 on a 13-screen test app. (#1806; part of #1801)
- A crate function whose output is `impl Trait<T>` of another crate's trait, as elevato's `view` over iced's `Widget`, takes `T` from the expected type even where the trait's defaulted parameters name types the crate can't, so `elevato::view(building)` needs no `::<Message>`. (#1804)

## 0.3.196 (2026-11-12)

- A save that changes the program compiles two runner crates instead of three: what names the program is part of the runner's own binary. (#1793; part of #1789)
- A program that returns from `main` ends `rawr run` with status 0, as under `cargo run`; one that panics or exits with an error still waits for the save that fixes it. Under `rawr mcp` a clean exit ends the session and `start` runs it again. (#1800)
- Deleting a project's `target` while `rawr run` runs it no longer stops the run with 'node store entry is corrupt'; the missing entries rebuild, as on a cold start. (#1802)

## 0.3.195 (2026-11-12)

- An opaque widget taken through a later `.map(..)` compiles again, so `badge(..).map(..)` inside `column!` runs as cargo runs it. (#1797)
- A second `rawr run` of an unchanged project writes nothing under `target`, the digest table included. (#1780)

## 0.3.194 (2026-11-12)

- With `RAWR_VERBOSE`, each runner build says how many units Cargo compiled, which ones, and how long it took. (#1795)
- A save after one that built native code probes once, and a save that changes no crate's API, such as a private field, reads no definitions again. (#1798; part of #1794, #1786)

## 0.3.193 (2026-11-12)

- A marked fn can return a crate's opaque widget that borrows what native code lent it through a higher-ranked trait, as iced's `ViewFn` takes `Program::view`; the lent value is borrowed in place, never copied. (#1770; part of #965)

## 0.3.192 (2026-11-12)

- After an edit to one crate, a `rawr run` save re-reads only the API of crates whose API changed, and `RAWR_VERBOSE` prints each round of a save's preparation. (#1776)
- rawr removes the build units it made that no current build names: in a project's target after each runner build, keeping every unit cargo's own build uses, and in rawr home's shared nightly directory after each extraction build. Incremental directories keep only their last session. (#1783, #1771)
- rawr's generated runner crates compile without debug info, so a save that moves lines no longer recompiles most of the runner's code: 5 and 3 codegen units where it was 47 and 37 on a test project. (#1792; part of #1789)

## 0.3.191 (2026-11-12)

- A save that changes a held type in your program rebuilds only rawr's copy of your crate, the runner code that names it, and the runner: 3 units where it was 6 on a test project. (#1775)
- rawr and `cargo build` share the units of your project's path packages, so neither rebuilds them after the other. (#1777)

## 0.3.190 (2026-11-12)

- A running owner reuses the Cargo view its last check replayed while no watched file changed, parsing no Cargo metadata and hashing no recorded input again. `RAWR_VERBOSE` says how many Cargo views each round parsed, compared and kept. (#1759)
- The start's status line names the step the start is in (reading the API, checking, building the runner, starting) instead of 'resolving dependencies' throughout. (#1763)
- A new context schema, a moved Cargo home or a changed build variable no longer classifies a project's API or selects its bindings again when the API is unchanged. (#1766)
- A crate function whose generic appears only in its returned `impl Trait<T>` takes that type from where its value goes, the enclosing function's `impl Trait<T>` return or a crate function's `impl Trait<T>` parameter, so `fn view(&self) -> impl Widget<Message> { crate::view(..) }` needs no turbofish. (#1769)
- A start walks its runner's read set once after its pins instead of six times, and a restart in a new process of the same runner keeps its pins with no new transaction. (#1760)
- A restart after a full preparation starts from that preparation, or in a new process of the running runner when its build is unchanged. (#1761)
- Each session of a run loads the program through its parse cells, and rustc's version is asked once per run. (#1762)
- An edit to the project's crate is checked incrementally on the nightly, as cargo check checks it, instead of from scratch on every edit. (#1779; part of #1772)

## 0.3.189 (2026-11-11)

- Checking a project from a terminal and then from an editor or script that sets `CARGO_HOME` to its default path no longer rebuilds the dependencies with the nightly every round. An unset `CARGO_HOME` and its default path are the same home, a `CARGO_HOME` reached through a symlink works, and a shared table settled under other inputs costs one build, then replays. (#1754)
- A start after a save that dropped a crate call from a marked function keeps the runner it ran on, which still covers the program, instead of building a new one. (#1743)
- Removing a pub item from a marked module swaps into the running program unless native code names it, and a new marked item that shadows a name native code resolves through a glob import rebuilds native code, as cargo would. (#1756; part of #1744)
- A restart that needs no new native build, such as a save the running program refuses or a panic after a swap, starts a new process of the running runner with no check, in about 0.1 s on a small project instead of a full start. The new program starts with fresh state. (#1764; part of #1755)

## 0.3.188 (2026-11-11)

- Removing an unused type, or changing its fields, swaps into the running program instead of restarting it. The program restarts only when a live value of the type exists, or a closure native code holds can still make the old one, and it says which. (#1746)
- A declaration added above a `macro_rules!` lowers again only its own file, and an edit to a path crate of the project lowers again only the marked files that name what changed. (#1730, #1735, #1734)
- Editing a path crate, such as a workspace member, and starting again from a copy or restore of older files no longer breaks the first start: rawr's shared extraction build checks a path crate's files by their contents, not their times. (#1750)
- A universe runner build writes its runner to `<dir>/runner` when `RAWR_UNIVERSE_OUT=<dir>` is set, whether rawr built it or reused it, so a web build takes the module from a fixed place; Cargo's leftover copies in rawr's work directory are removed after each compile, so an old module no longer sits where a web build used to look. (#1752)

## 0.3.187 (2026-11-11)

- `RAWR_VERBOSE` says what each start of the program ran: Cargo resolutions, nightly builds, probes, runner builds and the check's rounds. (#1742; part of #1739)
- A new pub item no native code names (a struct, fn, trait, impl or re-export) swaps into the running program instead of rebuilding native code. A new `Drop` impl, or a new impl of the crate's own trait that native code could hold as a `dyn` value, still rebuilds. An item taken away rebuilds, and the restart line names each one. (#1741)
- A new pub struct native code sees no longer makes the restarted runner lower every marked file; only the files that name it are lowered again. (#1740)
- A new build of rawr builds its own runner instead of reusing an older one, and the program ends with its `rawr run`: a killed run, or a run whose parent process ends, no longer leaves the program behind. (#1712)

## 0.3.186 (2026-11-11)

- A refutable `let` of a tuple holding a crate value names rustc's missing pattern again, and a signature naming a crate type rawr hasn't prepared asks to prepare it instead of failing. (#1724)
- A save that adds or removes a struct or enum lowers again only its own file and the files that name it. (#1726)
- After a save that changes what native code sees, such as a new public function, a derive or a `cfg`, the restarted runner lowers again only the marked files that name what changed. (#1725)
- Adding or removing a derive on a private type in a marked file swaps into the running program instead of restarting it. A save the running program can't take, such as a new field or a removed type, restarts it at once with its reason, without first preparing a native build. (#1728)
- A pub marked type's new `#[derive(..)]` swaps into the running program instead of preparing a native build. A derive that goes away still builds again, and the restart line names it instead of saying the fields changed. (#1737)

## 0.3.185 (2026-11-11)

- A save that adds or edits a doc comment or an inert attribute (`#[allow]`, `#[inline]`, `#[must_use]`, `#[deprecated]`, tool attributes), adds blank lines, or writes a signature over more lines lowers only that file under `rawr run`, and no longer prepares a new native build for an item native code sees. (#1716; part of #1646)
- A crate's `impl From<Its> for PathBuf`, or for another std type rawr holds, is seen as rustc sees it: `impl Into<PathBuf>` takes `Its` in a program's function or method and in a crate's, and `PathBuf::from(its)` calls the crate's impl. iced's editor gets past `load_file(picked_file)`. (#1713)
- Rebuilding a project's runner uses several cores and recompiles only what changed: each imported crate's adapters are a crate of their own on a shared base, so a new call into one crate recompiles only that crate's adapters and the runner. (#1670)
- A runner build removes the build state of an imported crate the program no longer imports, so a project's build directory doesn't keep growing after an import is removed. (#1722)
- `rawr run` lowers again only the marked files that name what a save changed: a new function, a derive or a `cfg` on something native code doesn't see re-lowers its own file and the files that name it. (#1718; part of #1646)

## 0.3.184 (2026-11-11)

- iced's `Task::sip`/`finish` shape works again: a closure takes `<S as Future>::Output` of an `S: Core`, because a crate trait whose one blanket implementation also needs a crate trait, as sipper's `Core` needs a stream, no longer reads as a plain future. (#1699)
- A closure over iced_test's lifetime-carrying `Candidate<'_>` fills a selector, as in `ui.click(|candidate: Candidate<'_>| ..)`, as rustc allows. (#1694)
- A crate future's output that holds a `&'static str`, given to a script closure as in `Task::perform(passed_beside(async { .. }, "beside"), move |(waited, beside)| ..)`, builds and runs as it does under cargo. Before, the generated runner stopped with E0521. (#1700)
- A cold extraction reads each crate's API facts across the machine's cores. The reads join in crate order, so the API's bytes and fingerprint are unchanged. (#1687)
- A crate's async function that returns a `Result` or `Option` of its own types gives values the program can call methods on, so `reqwest::get(&url).await?.json().await` decodes as `cargo run` does, and iced's pokedex runs its whole search path. A crate type the program never names, such as the `Bytes` of `response.bytes().await?`, has the `Debug` and other traits its crate implements. (#1018)
- A view whose widget holds an opaque widget a crate function made of the program's values, as iced's `center(component(..))` in the component example, now builds and runs: the runner names the widget's type through the function that makes it. (#1075; part of #965)
- `rawr test` links the package's `[dev-dependencies]` into its tests, general and per-target ones, as `cargo test` does. `rawr run` still builds without them. (#1703)
- A compound assignment with a number on the left and a crate's type on the right, as `y += turn` with the crate's `impl AddAssign<Turn> for f32`, calls the crate's impl, on a local or a field, as rustc does. (#987)
- A generic function written `-> impl Trait` takes its type parameters from the bound of the parameter its value is passed to, as iced's editor's `action(new_icon(), ..)` infers `new_icon`'s message. (#1633)
- A conversion's target that only a later renderer fixes, through an impl's `Renderer: image::Renderer<Handle = Handle>`, is that handle, also through a method call such as `image(handle).boxed()`, as in iced's screenshot. (#1632)
- A string literal's `.into()` into a crate type whose `From<&str>` borrows the text runs where only a later use gives the target, as in `Row::with_children([.., "b".into()])`. (#1328)
- Items in a function body are scoped to their block, as rustc scopes them. A body's `const` is visible to the `fn` and `impl` items nested in the same block, even before its declaration. Two functions can each declare their own `const MAX`, `struct Point`, `fn helper` or `use … as name`. A nested item can't be named outside its block (E0425), and `{:?}` of a nested struct prints its bare name. (#1631, #1429)
- A native generic call that takes a program value works in the head of a `for`, `if`, `match` or `while`, so iced's download_progress runs its downloads as cargo does. (#314)
- serde's `Deserialize` on a unit-variant enum runs. `from_utf8_lossy(&[u8])` gets a scope frame that owns a copy of the bytes, and a match pattern infers the type a call leaves open, as `V` of `EnumAccess::variant::<V>()`. iced's todos now passes `rawr check`. (#1709; part of #1017)
- A leading `::` path names its crate, as rustc's does: `use ::image as img;` and `use ::image::ColorType;` run beside an `image` the program imports from another crate. iced's screenshot now passes `rawr check`. (#1706)
- `Option<PathBuf>::as_deref` gives an `Option<&Path>`, as std's `Deref for PathBuf` does, and `Path::extension` works as a function value after it, as in iced's editor. (#1705)
- A crate's thread `JoinHandle` crosses inside an `Option`, a `Result` or a tuple, and a crate can take one, whether the crate's thread or the program's own `std::thread::spawn` made it. (#1274)
- A crate's `JoinHandle<T>` whose `T` holds the crate's values, alone or in an `Option`, gives them to the program when it joins the thread. (#1275)
- A program's thread `JoinHandle` can be a crate generic's type argument, as in `Held::new(std::thread::spawn(..))`. (#1276)
- A crate's own derive macro whose expansion names `alloc` through a re-export runs, as cargo runs it. (#1710)
- `std::mem::take` empties a `Vec` of a type without `Default`, and a hand-written `impl<'de> serde::de::Deserialize<'de>` with a parameter named `D` runs. (#1711)

## 0.3.183 (2026-11-11)

- A crate function whose output future's `Output` is its future parameter's `S::Output`, as futures' adapters keep their input's output, is imported. Before, the parameter's output was declared twice and Bind stopped with E0403 "the name `SOutput` is already used for a generic parameter", which stopped inboard's check. This had regressed with #1674. (#1685)
- A new rawr build no longer classifies and generates an unchanged project's API again, also when the project's crates came from a cold extraction or a later run reads them from shared tables. The API names the resolution universe without the digest of the rawr build that compiled it, and a shared-table run names its resolution as a complete one does. After an install, inboard's "building API" had taken 237 s on one core. (#1686)
- A save to a marked file that names a crate type through one of several re-export routes, as inboard's theme.rs names `iced::theme::Mode`, now swaps into the running program in about a second instead of rebuilding it (18-97 s in inboard). Every use that names a type, its variant, field or constructor names it by the type's one path, in rawr-import's compile and in the runner's own. (#1688)
- Under `RAWR_VERBOSE=1`, when a save can't swap in place, the runner names the file, line and column of the use its build doesn't cover, and the item. (#1689)
- A new rawr install, such as one that only bumps the generator version, no longer canonicalizes an unchanged project's crate API again. Before, that took about 235 s on one core in inboard. The API read from shared tables is no longer keyed by the host directory's path or by the driver binary, so an install regenerates bindings and rebuilds the runner, nothing more. `RAWR_VERBOSE`'s round lines now count canonicalized sidecars. (#1690)
- A block's last expression, such as a `#[cfg(..)] { .. }` block, is the block's value when a false `#[cfg]` removes every statement after it. iced's pokedex now passes `rawr check`. (#1684)
- A `let` on the only variant of a crate enum destructures it, unless the enum is `#[non_exhaustive]`, with rustc's E0005 witnesses where it can't. (#1681)
- A `match` on a `#[non_exhaustive]` crate enum that names every variant but has no wildcard arm is E0004 with `_`, or `&_` through a reference, as rustc has it outside the enum's crate. (#1692)
- iced_test's `ui.click("Increment")` works: a crate method that takes a selector, which the crate also implements for closures rawr can't write, takes a string, and the selector's `Output` is the one the crate's `&str` impl gives. (#1678)
- An opaque crate iterator fills a crate's `impl IntoIterator` parameter, so iced_test's `ui.simulate(click())` needs no `.collect()`. (#1679)
- `iced_test::simulator(counter.view())` needs no annotation: a crate bound on what the program's `fn .. -> impl Trait` gives takes its theme and renderer from that bound, as rustc does. iced's counter, driven through the simulator, prints what `cargo run` prints. (#1680)
- A crate method on a region-held `&mut self` receiver that takes a `&str`, as iced_test's `typewrite`, builds: the generated adapter pins the string before it reaches the receiver. (#1693)
- rawr cleans up after itself by state, with no timer. Another extraction protocol's directory goes the first time a run opens rawr's home or a project's `target/rawr`, unless an installed rawr uses it or a run is reading it. A development or test build no longer counts as installed, so it no longer keeps old protocols alive. A project's own bindings, staged records, trait facts, extraction tables and API memo now live in `target/rawr/formats/<protocol>`, and the copies an older rawr kept straight under `target/rawr` go at the next run. (#1677)
- A check after installing a new rawr build starts from the native instances the last build recorded, so it no longer builds the project's crates twice in two rounds. (#1695)
- A crate function whose own generic parameter is named like one rawr invents, such as `SOutput` beside a future parameter `S` or `impl_1` beside an argument's `impl Trait`, is imported and called. Before, Bind stopped with E0403 "the name `SOutput` is already used for a generic parameter". rawr now gives its own parameters reserved names derived from their identity, and its messages spell them as rustc does: `impl Into<u32>` or `<S as Future>::Output`. (#1698)
- After installing a rawr whose manifests are unchanged, `rawr check` reuses the last resolution: no harness, no Cargo resolution, no nightly build and nothing copied. The resolution names rawr's crate by its manifests in rawr's home, not by the install's host directory. (#1696)

## 0.3.182 (2026-11-10)

- Bind classifies a project's definitions and exports across the machine's cores, with byte-identical contracts. An opaque type's left-out captures are now set by the function that gives it, whatever is classified first. (#1669)
- `s.as_ref()` and `s.borrow()` on a String, str, Vec, array or number lend the view the use expects: a `let` annotation, a parameter, a return, or a later crate trait bound such as iced's `column![self.description.as_ref(), ..]`. The view comes from std's `AsRef` and `Borrow` impls, with rustc's E0282, E0277 and E0505 where rustc gives them. (#1672; part of #1632, #1018)
- A `match` or `if` whose arms give `&str` and `&String` is a `&str`, as in rustc: one arm deref-coerces to the other's type, in either order. (#1673; part of #1018)
- A program that depends on both iced and iced_test runs: an opaque output such as `explain`'s, reached through two crates' paths that name its parameters differently, is one type. (#1660)
- A canvas program's `update` keeps what it writes to its `&mut` state, as in bezier_tool's `*state = Some(Pending::One { .. })`, so a third click on its canvas draws a curve, as it does under cargo. (#990)
- A cold `rawr check` or `rawr run` extracts each dependency only for what the program uses, instead of walking every big crate whole for its shared table (about 36,000 units for iced). A later program that uses other items of the crate extracts just its own demand once, and the shared table grows to hold it. Programs whose demand the table already holds still read it with no nightly build. (#1665)
- A second run of an unchanged program compiles nothing again. A demand round the store already holds now reads its stored resolution instead of rebuilding its API from the shared tables, so the check, profile and runner keep the keys the next run looks for. This had regressed on 2026-11-10 with 3fc6645a9. (#1682; part of #1618)
- `futures::future::try_join` takes an async block's and an async fn's futures and gives what each yields inside its `Ok`, as iced's pokedex joins them. (#1674)
- A `String`'s or `str`'s `as_ref()` lends a `&Path` or `&OsStr`, as std's impls do, and a type std has no such impl for is rustc's E0277 naming `Path`. (#1675)
- A generic `T: AsRef<[u8]>` given a `String` lends its bytes, as rustc does. (#1676)

## 0.3.181 (2026-11-10)

- A marked function that panics in a call from native `main` prints the panic cargo's build prints: its place in the marked file and Rust's message, with nothing of rawr's. (#1657)
- An error a save's compile finds in a marked module of a native app names its own file, line and column in rustc's form, such as `error[E0425]` in `src/widget/sidebar.rs`. Before, it named the crate root, as a framework error. (#397)
- A crate's callable value, such as the `impl Fn` that iced's `Function::with` gives, fills a generic callable that the call's output type names, as in `self.grid.view().map(Message::Grid.with(version))` in iced's game_of_life. (#1060)
- A crate value whose type elides a lifetime, such as iced's `Pin<&str>` from `pin("..")`, passes to a generic crate function such as `container`, and the program builds and runs. iced's layout example builds again. (#991)
- A crate function that takes a future and a closure over its output, such as iced's `Task::perform(tokio::time::sleep(..), move |_| ..)`, runs with a crate future or an async block beside a capturing closure. iced's toast example now builds and starts its runner. (#1073)
- `{:?}` of a crate value prints the crate values held by a program value inside it, as cargo does: `Action { message_to_publish: Some(Curve { from: Point { .. }, .. }) }` in iced's bezier_tool. (#1658; part of #990)
- A new `rawr run` runner recompiles only the marked files that changed since the last runner, and reads the parses rawr already made instead of parsing those files again. A start with no edits compiles none of them. (#1638)
- A cold `rawr check` or `rawr run` extracts, reads, classifies and binds only the crates the marked code names, and the crates defining the types and traits their signatures reach. A dependency it never names is still built by cargo, but rawr records only its names. (#1663; part of #1618)
- A new rawr build, such as one with a generator or classifier bump, reads, classifies and generates nothing again for an unchanged project. The API now names rawr's own package by name and version, and a shared resolution by its lockfile, instead of by the path of rawr's build directory. Only the runner rebuilds, because it links the new rawr. (#1666; part of #1604)
- Each selected crate's shared extraction table is read, checked and replayed on its own core. (#1667; part of #1604)
- A driver built for a nightly other than the pinned one compiles, because its embedded sources now include `sidecar.rs` and `expand/request.rs`. (#1668)

## 0.3.180 (2026-11-10)

- rawr-import's driver builds again on nightly-2026-11-10 and every other nightly in its window. CI now builds it on the window's newest nightly too. (#1643)
- A save of one marked file in `rawr run` reveals and lowers only that file again. That now holds when another marked file reads a file with `include_str!` or `include_bytes!`, or a variable with `env!`, as inboard's theme does, and when the edit is to a method's body. With `RAWR_VERBOSE`, the runner says how many files each save revealed. (#1654; part of #1646)
- `rawr test` of a program whose package has Rust dependencies runs its tests in the program's runner, as `rawr run` runs its `main` there, and reports as `cargo test` does. That covers each test's result, a failure's output and panic, libtest's filters and options, the exit status, and `cargo check --profile test`'s errors after the report. (#1653)
- A save that adds or removes a line in a marked file keeps every other marked file's lowering, as long as the line is in a function body or is a blank or comment line. The save lowers only its own file. (#1655; part of #1646)
- A save reads each marked file's outline once per text and keeps it across passes and saves. Before, it blanked every file's text against every body on each pass. (#1656; part of #1646)

## 0.3.179 (2026-11-10)

- A `Box<dyn Fn>`, a `&dyn Fn` or a `&Box<dyn Fn>` fills a function's `impl Fn` or `F: Fn` input, as in Rust, so a boxed style closure given to iced's `container::style` checks and runs. An `Rc` or `Arc` of one stays an error, as in rustc. Messages name a box or reference of a function `dyn Fn(..)`, and a crate method on a borrowed receiver can call back a closure the receiver holds. (#1650; part of #222)
- A type alias of an associated type takes its defaulted parameters written, so `fn f() -> canvas::Geometry<Renderer>` checks and runs, as iced's sandpiles writes it. (#317)
- A crate function's or method's type parameter that only an argument's std-trait bound names is inferred from std's impl for the argument's type, as in rustc. `Link` of iced's `rich_text(spans)` given a `Vec<Span<'static, ()>>`, an array or a slice is `()`, through `AsRef`, `Borrow` and the other std impls. (#1651)
- A crate function a `use` names can be called with a turbofish, as `span::<()>("e")`. (#1652)
- An expression nests and a function recurses as deep as a debug build of Rust allows. There is no expression-depth limit, and the call depth is 100 000. (#962, #1624)
- A spawned thread's panic, and a runner's main panic, name the file, line and column where they panicked. (#948)
- `collect()` into a `Result` or an `Option` of a map or a set runs. (#937)
- `rawr test` runs `cargo check` on the tests beside it and reports what only rustc rejects. (#929)

## 0.3.178 (2026-11-10)

- A check parses the program once and keeps the parse across its demand rounds, where each round used to parse every source again. `RAWR_VERBOSE=1` also prints, per round, the harnesses bound, build outputs copied, bundles sealed and sources parsed. (#1634; part of #1618)
- A demand round no longer resolves, builds or seals a new bundle when an earlier round built the same crates: it probes the new demand over that build and replays the grown tables. A cold check of a marked theme builds the nightly twice instead of four times. (#1636; part of #1618)
- `RAWR_VERBOSE=1 rawr run` says what preparing the run did: its rounds, Cargo resolutions, nightly builds, probes, runner builds and the units they compiled. A second run of an unchanged project shows all of them at zero. (#1637; part of #1604)
- rawr compiles the dependency versions your Cargo.lock pins. A project without a resolution universe was resolved without its lock and could get newer compatible versions than cargo builds. The runner also no longer warns about a `[patch]` only the rest of your workspace uses. (#1635)
- A project whose Cargo.lock is over 1 MiB resolves from its lock. A crate type's `&'static [&'static str]` const can be read. `Some(&value)` where an `Option<&dyn Trait>` is expected coerces to the trait object, as rustc does. Borrowing a local for a crate's `&'static dyn` field is rustc's E0597 before the run. (#1639, #1626, #1623, #1620)
- A crate whose types nest deeply, such as a long combinator chain, no longer fails `rawr check` with "rawr misread rustc's output (`sidecar-schema`): recursion limit exceeded". The compiler's output also writes each definition id once, which cuts its definitions to about 57% of their former size. (#1640)
- A crate's `use Item as _` of an item that is surely not a trait, such as a module, struct or function, no longer fails the extraction with "rawr misread rustc's output (`anonymous-import-kind`)": as in rustc it adds nothing. A trait's `use Trait as _` still brings its methods into scope, and a declaration whose shape failed closed is refused by name, since it may be a trait. (#1642)
- A save of a marked file in a running `rawr run` no longer does work that grows with the program's dependencies. Each native use finds its binding by one search of an index built when the runner starts, and the owner and the runner parse only the file that changed. (#1644; part of #1604)
- A save of a marked file no longer re-reads, re-encodes or re-stages the running build's native watch: a body edit registers no watcher root. (#1645; part of #1604)
- A method with elided lifetimes whose output borrows its receiver, as `fn user(&self, id: &Id) -> Option<&User>`, can be called from marked code: its lifetimes are read as the call's own, not as a higher-ranked signature. A method that gives `&T` or `Option<&T>` also works on a receiver native code lent, or on a field of one, and may take arguments. (#1647)
- A save that changes a script impl's shape, such as a provided method it now overrides or a signature it changes, now reaches native code. `rawr run` used to swap in the new code while native code kept calling the old shape; the save now rebuilds the runner, and body edits still apply in place. (#326)
- An expected type reaches every arm of `if`/`else` and `match`, a block-like tail of a block or function body, and every `break` value of a `loop`, and each arm coerces to it as rustc does. `let style: Box<dyn Fn(&Theme) -> Style> = if c { Box::new(|t| ..) } else { Box::new(|t| ..) };` checks, as do `&dyn` and `fn`-pointer arms. (#1648)
- A save of one marked file lowers only that file again; every other file reuses its lowering from the last compile. `RAWR_VERBOSE` says how many files a save lowered. (#1649; part of #1646, #1604)

## 0.3.177 (2026-11-10)

- A check whose first build records new instances of marked generics no longer probes that build, and a check keyed by proved facts proves each imported call in place instead of starting again for each call. A toolchain or driver version mismatch stops a check at once. (#446, #470; part of #1604)
- An operator on a type without an impl of its trait is rustc's error in rustc's words, at the operator: E0369 for `+`, `|`, `<=` and `==`, E0368 for `+=`, E0600 for unary `-`, and E0308 for a right operand the one impl doesn't take. A `.into()` that nothing fixes is E0283 in every build. (#1615, #1617)
- A project's record of native instances is keyed by the rawr build that kept it, and the runner a check builds reads the same key, so a record from an older rawr is never reused. (#1606)
- Marked code calls a crate trait's `&self` methods on a trait object, as `held.0.area()` on a `&'static dyn Shape` or a `Box<dyn Shape>` a crate gave. A marked value is lent to native code as a native trait's `&dyn`, written `&page as &dyn Page` or `&page`, and `Rc<dyn Trait>` and `Arc<dyn Trait>` cross for marked types, numbers and crate values. (#1593, #1304)
- `&format_args!(..)` needs no `as &dyn Value` where a crate call takes `&dyn Value`. A slice of objects lent to a call whose result keeps none of them is accepted. (#1621; part of #1592)
- inboard's marked files that name a std trait beside tracing's macros pass `rawr check` again. The extraction error now names the item it misread, not a bare hash. (#1622)
- `-` of an unsigned integer or literal is rustc's E0600 ("unsigned values cannot be negated"), and changing a std collection through a `&` reference is rustc's E0596 in its words. (#945, #938)
- More std paths run as cargo's build runs them: `std::num::ParseIntError` written in full in a `let`, an imported `Duration` or `Instant` in a `let`, `Instant::now` as a value, `fmt::Formatter::write_str(f, s)`, `Iterator::sum::<i64>(it)`, `<i64 as FromStr>::from_str`, `<T as TryFrom<U>>::try_from`, and `SystemTime`'s `+`, `-`, `+=`, `-=` with a `Duration` and its ordering. (#922, #1002, #978, #977, #924)
- A panic in a project's `main`, sync or async, prints Rust's `thread 'main' panicked:` and its message instead of rawr's internal text. (#969)
- A frame-held element lends itself to a `&mut Vec<String>` call such as `labels` and to a crate function's `&T`, two values of one opaque type go to one call such as `hover(a, b)`, and `Column::with_children([span(&name), span("right")])` runs again inside a view. (#1369, #1368, #1366)
- A generic crate trait's method is called by its path with the trait's arguments, as in `Counted::<u8>::doubled(x)`. (#1282)
- A marked module that logs through a crate's re-export of tracing's macros, as inboard's inbox calls its own `log::warn!`, passes `rawr check` and runs as cargo runs it, even when the package names neither tracing nor tracing-core. (#1627)
- Assigning through `&self` is rustc's E0594, and a `&mut self` call through it is rustc's E0596, where rawr gave its own RAWR-RESOLVE-RECEIVER-MUTATION. (#1625)
- A program with marked generic instances keeps its extracted tables for the next demand round again, so a round whose items the tables already hold needs no nightly build or probe. A check no longer generates its bindings plan twice per round, and `rawr check` with `RAWR_VERBOSE=1` prints what each round's stages did. (#1629; part of #1618)
- `std::env::current_dir()` runs, a generic function's type parameter is inferred through an `impl Trait` argument as rustc does, a borrowed crate field derefs at a call even when the program never names its type, and `Rectangle::<u32> { .. }` literals work. iced's screenshot passes `rawr check`, and todos and integration get further. (#1061, #1218, #1100, #1320)
- A crate macro that includes a file relative to a native module, as `wgpu::include_wgsl!("shader/vert.wgsl")`, finds it: a native module's non-`.rs` files sit beside its copy. iced's integration gets past its shaders. (#1630; part of #314)

## 0.3.176 (2026-11-10)

- Native code can call a marked generic function at a type that sits below a private module, as inboard's `workspace::spreadsheet` uses its private `sidebar` module's `Message`. (#1598)
- Native code can call a generic marked method or function taking an impl of a marked trait, as inboard's `Sidebar::content(impl IntoContent<'a, Message>)`, with the trait's own type or any iterator of elements. A body edit swaps in place. (#1586; part of #1539)
- A crate function that takes trait objects in a slice, each alone or in an `Option`, and gives back a value that borrows them, as tracing-core's `fields.value_set_all(&[Some(&n as &dyn Value), None])`, runs from marked code. The value can be read, passed to a crate function such as `Event::dispatch(meta, &set)`, or passed to a closure. (#1591)
- A generic marked function's call to another marked generic takes its type arguments from the expected type (a return, `let` annotation, argument or field), as rustc does, so inboard's `Sidebar::default()` in a function returning `Sidebar<'a, Message>` passes `rawr check`. A turbofish no longer counts an argument-position `impl Trait`. (#578; part of #408)
- A `Box<dyn Iterator<Item = T>>` that marked code makes, as inboard's sidebar `Header::new` keeps, crosses to native code as an iterator that pulls one item from marked code at each `next`. Native code and marked `&mut self` methods pull from the same iterator. (#1600)
- When `cargo check` also fails, `rawr check` and `rawr run` show cargo's errors alone, with rustc's own text and positions. A supervised run's `failed` event carries rustc's diagnostics. (#1605)
- A marked type boxed as a native trait object works again when the crate's root imports the trait, as with `use page::Page;`. The trait is now compared by the one path it's known by. (#1607; part of #1578)
- A crate's re-exported enum variant reads by its path in marked code, as in `.align_y(iced::Center)` with iced's `pub use Alignment::Center;`. (#1608)
- inboard's sidebar builders run from native code. A marked `&mut self` view passes its lent `Pixels` to a crate call, moves the elements native code keeps into a crate column, and mixes widgets that two crates' paths make in one row. A marked function borrows native code's `&Theme` through the crate root's `pub use` without a clone. (#1584, #1587, #1588)
- A method taking `self` or `mut self` can move a part below its fields out of the receiver, as in `let items = self.content.items;`, as rustc allows. Behind `&self` or `&mut self` the move is rustc's E0507, naming the place. (#1589)
- A crate trait's function can be called through the trait's module path: `home::Shape::size(&x)`, `crate::home::Shape::size(..)` and `<T as home::Shape>::size(..)` resolve, as in rustc. (#1596)
- `rawr check` on a large workspace joins each loaded crate's artifact to its unit once, walking the build graph once per check instead of once for each crate that loads it. In inboard this was 46% of the check's single-threaded time after an install. (#1610; part of #1604)
- `rawr check` shows which phase it's in on one line, updated in place: resolving, extracting N of M, reading API, classifying, generating, checking. (#1611; part of #1604)
- inboard's sidebar style builders run under `rawr run`. A crate calls marked style closures with the lent `&Theme` without copying it. An `impl Fn(&Theme, Status)` that a native function or `Ternary::with` gives is held boxed. A crate trait with its own `from` in scope no longer breaks the runner. (#1583, #1612; part of #766)
- Real `tracing::warn!("..")` and `info!("..")` events, and `level_enabled!`, run in marked code and reach a native subscriber as cargo's build gives them, also through a crate's re-export, as inboard's `log::warn!` is. `format_args!` is `fmt::Arguments`: its `{:?}` is unquoted, and it coerces to a crate's `&dyn Value`. (#1561, #1602, #1603)
- `rawr check` classifies a crate's API without re-reading a trait's implementations for every implementation and function that bounds one of its associated types, and finds a type's definition through an index instead of hashing every definition. In inboard's post-bump check this work was about 40% of rawr-import's single-threaded time. (#1616; part of #1604)
- A use of an owned `self`'s field, or of `self`, after a field moved out of it is rustc's E0382. A use after only part of a value moved says "partially moved", as rustc does. (#1609)
- A crate trait's methods work on a std number, `bool` or `char` with the trait in scope, as `count.to_u32()` with num-traits' `NumCast`, and through a qualified path, as `<u64 as NumCast>::from(n)`. (#1614, #1613)

## 0.3.175 (2026-11-09)

- A save near a call to a crate function generic over a const length, as `total([a, b])` is, now swaps into the running program instead of preparing again. (#1536)
- Native code can call a marked function that takes an `impl IntoIterator<Item = T>`, `impl Into<T>`, `impl AsRef<str>` or `impl Display`. The argument may be any value, including one that borrows locals. The program builds as `cargo check` does and runs as `cargo run` does. (#1537)
- When rawr's native build fails in code it generated for a marked function, the error now points at that function's signature in the marked file and names the generated file and line, not just `src/main.rs`. (#1538)
- A program under `rawr run` now goes on from the call that admitted a save only once the run's events include `swapped`. A `wait` that the client cancelled no longer takes the events it found, so the next `wait` returns them. (#1541)
- A `&self` method of a marked type that crosses by copy, such as inboard's `Id::as_str`, now gives native code a `&str` into its own value, as rustc does, instead of panicking. Methods of marked types that hold native parts also return their copied text fields in place. (#1543)
- A marked type's `HashSet`, `HashMap`, `BTreeSet` or `BTreeMap` over values that cross, such as inboard's `read_ids: HashSet<Id>`, now crosses by copy as a `Vec` does. Marked code's `contains`, `insert` and `extend` run on the interpreter's own collection, hashed and compared as Rust does. (#1544)
- A `static` item inside a block or function body now builds, as in `fn place() -> &'static u32 { static PLACE: u32 = 9; &PLACE }`. It is the module's item, as a `static` written there is. A `static` that a macro call expands to is reported as not supported yet, not as a syntax error. (#1546)
- A marked fn taking an `impl` of a trait its own file declares now builds and runs when native code calls it. The trait and its impls are declared natively, and each value is given by the marked impl for its type. Before, the native build failed with E0405. (#1551)
- When the native build fails in the signature of a marked fn that native code can't call yet, the error now names the marked file's line and the word it is at. (#1552)
- A program's blanket impl over a type parameter, such as `impl<T> Trait for T where T: IntoIterator<Item = X>` beside `impl Trait for Content`, now resolves and runs the way rustc selects it. A call on the concrete type runs its own impl. A call on any type the where-clauses hold for, such as a Vec, an array, an iterator or a Display value, runs the blanket impl, directly or through an `impl Trait` bound. (#1553)
- A crate type now compares with another type through its PartialOrd<Rhs> or PartialEq<Rhs>, as tracing's `Level <= LevelFilter::current()` does. (#702)
- std's macros now work by their paths and through a crate's re-export, as log's `__private_api::module_path!()` and tracing's `__macro_support::concat!` do. `format_args!` is supported. (#1548)
- Native code can now format a marked value it is lent as `&dyn Debug`, `&dyn Display` or another std formatting trait. It also takes `format_args!` as `fmt::Arguments`. (#1558)
- A `static` declared in a marked body, or by a crate macro at each call, is named only in its block. One of a native type is a native value with a fixed `'static` address for the program's life, made the first time its block runs. A save that leaves its declaration alone keeps it, and a save that changes it makes a new one without a restart. Statics that refer to each other, as tracing's `META` and `__CALLSITE` do, are refused at `rawr check` for now. (#1547)
- A crate function that takes a `&'static T` of a native type, or a method that takes `&'static self`, now takes a static's own place. It used to fail the runner build with E0597. (#1562; part of #1440)
- A marked function that native code calls with its own `&str`, and that returns a crate value borrowing it, now builds and keeps native code's own text. inboard's `subtitle<'a>(label: &'a str) -> Text<'a>` is one. It used to fail the native build with "lifetime may not live long enough", because iced's `Text` is invariant in its lifetime. Each `&str` also stops leaking one pinned copy per call. (#1566)
- A marked struct or enum with a lifetime parameter, such as a `Panel<'a>` of `&'a str` and `Element<'a, Message>`s, now crosses to native code. Native code builds one through a chain of marked builder methods, by value and by `&mut`, and keeps it between calls. Its borrows are native code's own. (#1557)
- A bound naming a generic trait the crate declares, with a lifetime among its arguments, now resolves as rustc resolves it. Examples are `impl IntoContent<'a, Message>` on a method of `impl<'a, Message> Sidebar<'a, Message>`, `C: IntoContent<'a, M>`, and a free fn's `impl IntoContent<'a, Message> + 'a`. When such a bound isn't satisfied, rawr writes the trait as rustc does, with `'_` for each lifetime. (#1568)
- Statics declared in a body that refer to each other, as tracing's `META` and `__CALLSITE` do, now work in marked code. Each points at the other, registration by address works, and a save keeps or replaces them without a restart. A cycle whose type keeps the pointer inside an enum or a union is refused at `rawr check`. (#1547)
- `concat!` and other compile-time macros now take a std macro named by a path from the crate root, as in `::core::line!()`. (#1569)
- Native code can call a marked function that is generic over its message, at each type rustc's check saw it called with, directly or through a generic native helper. A body edit to it swaps in place. (#1556; part of #1539)
- A std macro that a crate re-exports and names only inside its own macros, as tracing does with `line!()` inside `concat!`, now resolves in marked code. (#1570; part of #1547)
- A `macro_rules` `expr` fragment no longer starts at its own stop token, so a matcher such as tracing's `fieldset!` matches exactly what rustc matches. (#1573; part of #1547)
- A marked fn whose result's elided lifetime borrows a marked value native code lent it, as `sidebar(theme: &Theme) -> Element<'_, Message>`, can give it on to another marked fn that hands it to native code. The last native call gets native code's own reference instead of panicking. (#1581; part of #1539)
- A marked module of iced builders shaped as inboard's sidebar passes `rawr check` as `cargo check` does. That covers `[4, 0].into()` into a crate type, `impl IntoFragment<'a>` bounds, a blanket program-trait impl calling generic builders, closures annotated `&Theme` through a re-export, `native_fn.with(x)` from a glob-imported trait, a native fn's `impl Fn(&Theme, Status)` given to `style`, and `column(&mut boxed_iterator)`. (#1582; part of #766, #767, #1555)
- Native code can build and keep a marked type generic over its message, as inboard's `Sidebar<'a, Message>`, through its generic methods, and call marked generic functions that take an `impl Trait` argument. A body edit swaps in place. (#1574; part of #1539)
- Marked code builds crate values that hold trait objects, as tracing's `Identifier(&__CALLSITE)`, and holds or passes back a crate's `Box<dyn Trait>`. A number, `bool`, `char` or crate value coerces to a crate trait's `&dyn` or `Box<dyn>`, and the impl for its exact type runs. Native code makes every object. (#1577; part of #1578, #1547)
- Two modules of a crate may each declare their own trait of one name, as rustc allows. Each is known by its path, as `home::IntoContent` beside `sidebar::IntoContent`, in bounds, impls, `impl Trait`, `dyn Trait` and calls by the trait's name. A crate trait a module imports with `use` can be implemented by that name. (#1595)

## 0.3.174 (2026-11-09)

- A marked view can pass a lent value to a crate fn that also takes a `&str`, as in `inbox::view(title, theme).map(..)`, including when the output keeps both the string and the lent value. (#1509)
- A lent value reached at the wrong type is reported as a rawr bug, not as a runtime error in the user's program. (#1528)
- A new engine installs std's declarations as they were written once from the std table, instead of reading the table at every start. That is about 29 ms less per browser instance and 11 ms less per native thread. (#1526; part of #1384)
- A marked function that takes `Option<Element<'a, Message>>` by value from native code, as inboard's `home::State::view` takes `home_grid`, can now call `.map` on the element. Before, a value given this way was checked against the marked theme's native mirror, while `.map` expected the marked type itself, so the call failed. A native value whose type doesn't match what a call expects is now reported as a rawr bug, not as an error in the program. (#1530)
- A marked screen that passes a native function by name, as inboard's `home.rs` passes `theme::text::base` to `.style`, no longer restarts on a body edit above that call; it swaps in place. The runner names the function, not where the program writes it. When a save does need a new native build, `rawr run` says which native input or native use changed instead of "native code changed". (#1531)
- When a save can't compile inside the running program, `rawr run` says why in one line, `rawr: preparing again: <reason>`, and the events stream carries `{"event":"preparing","reason":…}`. The reason is one of: a refresh, the native input that changed since the running build was checked, `Cargo.toml` or what native code sees of the marked modules, an earlier save's pending native build, or the running program's own miss. (#1533)
- A marked fn whose element borrows only native inputs now runs, as `fn view<'a>(threads: &'a Store) -> Element<'a, Message>` does. So does a fn that reads a crate call's output right where it makes it, from values native code lent it. (#1491)
- A lent slice passed beside a lent value to a crate fn that keeps neither now works. (#1500)
- An edit to a marked file that passes a crate function by name to a crate method, as `.style(theme::text::base)` does, now swaps into the running program. It used to take the full preparation on every save: about 2.5 minutes for the first edit on inboard. (#1534)

## 0.3.173 (2026-11-09)

- A call that passes a closure to a widget held boxed in a view builds, as when iced's modal styles its `center(..)` container. iced's modal runs. (#955)
- A marked module's path through a `use` that binds a marked module now reaches that module's native children, as rustc does. inboard's `use crate::theme;` followed by `theme::text::base` is an example. A native signature's `&Theme` of a marked theme is now the marked type itself, not a different one. Each crate function item passed to a generic, as in `.style(theme::text::base)` beside `.style(theme::container::base)`, calls its own function. Before, they could share one adapter. (#1504)
- iced's `pane_grid` example runs again. A `&self` method on a crate value generic over the program's own type, such as `self.panes.len()` on a `pane_grid::State<Pane>`, can read it while a view still borrows it, as rustc allows. It no longer stops with a false E0502 before the window opens. This had regressed in #1432. (#1505)
- An `if`, `while`, `for` or `match` missing its `{` now reads as rustc's "expected `{`, found `2`", and nothing follows it. Before, it shifted the braces and surfaced as a `self` error in a later method. A free function's `self` parameter reads as rustc's "`self` parameter is only allowed in associated functions". A `self` value outside a method reads as E0424, replacing RAWR-RESOLVE-SELF. (#1507)
- A marked view that native code lends a marked value to, as inboard's `home::view` gets `theme: &Theme`, can pass it on by `&Theme` to native functions, crate generics and `'a` parameters that its result keeps. Native code gets its own reference back, where it used to panic. A native type alias over a marked type, such as inboard's `Element<'a, Message>`, names the marked type wherever marked code uses it. That removes a false E0308 on `return crate::widget::void();`. (#1484)
- A universe in a browser, such as elevato.rs's page, prepares its bindings about eight times faster: 10.8 s down to about 1.4 s, measured natively. Installing a crate's items now sorts once, instead of shifting the registry for each item. Each item and type that re-exports reach at many paths is stored and converted once. The embedded bindings plan shrinks from 97.6 MB to about 26 MB. (#1512; part of #1360, #1362, #435, #1384)
- A crate's `&mut self` method now works on a temporary, as in inboard's `rand::rng().random_range(0..100)`. The call's value is borrowed where it is made and lives to the end of its statement, as rustc does. This replaces a RAWR-RESOLVE-HOST-OPERATION "not supported yet" error. (#1514; part of #143, #870)
- A crate's `&self` or `&mut self` method now reaches an element of a slice that a native method lends where it is. Examples are `entry.is_shared()` in `for entry in registry.entries()`, and on a `Vec<&Entry>` collected from it. It used to panic at run time with "a value reached an adapter as a type its signature doesn't take". (#1515)
- A browser universe prepares about four times faster: elevato's `prepare()` goes from 1.25 s to about 0.29 s, measured natively. A whole-crate or whole-module re-export is now one alias that the runner's registry follows, not a copy of its entries. The facade plan also writes identities as hex. elevato's plan drops from 25.8 MB to 14.8 MB, and its wasm from 51.9 MB to 41.0 MB. (#1516; part of #1360, #1384, #435)
- A call inside a crate macro's arguments now runs in Rust's order around a call whose value borrows a local, as in inboard's `stack![page, opaque(wizard.view(catalog, theme).map(..))]`. It used to be refused as "a call or assignment run before a call whose value borrows a local". (#1521)
- A marked view whose scope frame captures values that aren't `Send` or `Sync` now builds under `rawr run`. Examples are a native view of borrows into a type with `RefCell` or `OnceCell` caches, and an owned iced `Element`, as in inboard's `wizard.view(catalog, ..)` and `sidebar::header_row`. Before, the generated runner failed with E0277. A frame's opener stays on the thread that made it. A native value lent to a marked call by `&` or `&mut` no longer has to be `Send` or `Sync`. (#1523)
- Projects now share generated bindings for crates whose build scripts leave native build files, such as openpnp_capture_sys's CMake tree and aws-lc-sys's stray objects. rawr compares only the OUT_DIR files a compile read. `rawr check` reads the project's build-script outputs when its cargo check ends, before another toolchain's build can rerun them. Not sharing names its real reason, and each note prints once. In a fixture, a second project's check went from 10.5 s, not shared, to 4.0 s, shared. (#1524)
- A runner's bindings now load in about a millisecond. Each binding is built the first time a program looks it up, from a binary image the generator writes. elevato's page prepares in under 30 ms in Chrome instead of about 0.7 s, and native runners start the same way. (#1518, #1519)
- A marked view can now map the element that a native function returns over a lent marked type, as inboard's `home::State::view` does with `inbox::view(.., theme).map(Message::Inbox)`. It used to fail with "the lent value is not of the type this call takes". A marked type now has one spelling in the runner, so the returned element and the receiver of `map` are the same type. (#1527)

## 0.3.172 (2026-11-09)

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
- rawr no longer forgets an installed rawr-import: a manifest probe that fails, as one could under heavy load, is asked again on the next run instead of being remembered as 'not installed'. (#1230)
- A crate type whose impls at different arguments each have a method of one name giving an `impl Trait`, as swash's `Setting<u16>` and `Setting<f32>` do with `parse_list`, imports again, so iced's examples that draw text check. (#1232)
- The landing page is shorter: each example shows its code, its command and its output, and the bouncing ball is drawn across the page. (#1233)
- A value of yours that holds one of a view's values, as iced's loupe holds what `explain` gives of its `column![..]`, is reached after the view returns: the view's values live as long as the state the view borrows stays unchanged. (#1217)
- Your impl of a crate's trait at a type that holds a crate's opaque value can delegate to it, as `self.content.labels(labels)` or `Widget::press(&self.content, label)`: a `&mut Vec<String>` the crate lent your method reaches the crate's own vector. (#913)
- iced's pane_grid runs: a closure that gives a value at its call's own lifetime, as `PaneGrid::new(&state, |id, pane, max| Content<'a, ..>)` does, makes it in a region of its own at that lifetime, and `content.title_bar(title_bar)` joins values of that region. (#1191, #1082)
- A crate whose generic type's impls each give an opaque iterator by one name, as swash's `Setting<u16>::parse_list` and `Setting<f32>::parse_list`, imports: every iced program that draws text checks again. (#1232)
- A function written `-> impl Widget<Message>` or `-> Option<impl Widget<Message>>` fixes its renderer before an `impl Into<Handle>` argument's own type would fix the handle, as rustc does, so gallery's images take iced's `image::Handle` and gallery passes `rawr check`. A crate's opaque widget whose trait names a `String`, as `markdown::view`'s `impl Widget<Uri, ..>`, takes the renderer a later use gives it. (#1213, #1099; part of #917)
- rawr runs a package's binaries as Cargo lists them: one under src/bin/ or named by [[bin]], and a library runs from whichever binary cargo run would run. With several binaries and no default-run, rawr asks for --bin, as cargo run does. (#1116)
- `rawr run path/to/file.rs` runs a file its package lists as a binary as that package's binary, with its dependencies and profile, and `rawr run --bin NAME` (and check and test) picks one as cargo run --bin does. A file no package lists still runs alone. (#1175)
- A package's `panic = "abort"` profile aborts a panic, as the binary Cargo builds does: status 134. (#1176)
- A [target.'cfg(..)'.dependencies] table joins a program's dependencies when Cargo's own build links it on the host. rawr asks Cargo's unit graph and evaluates no cfg itself. (#1214)
- A large project is never refused for its size: the semantic API, the node store, the extraction driver and the attempt record no longer have byte or count caps. (#1231)
- Each runnable snippet on the site had a hand-written output that nothing checked. One error showed only its first line, six compile errors were not marked as errors, and the drop-order output showed what Rust prints rather than what rawr prints. The site's build now runs every snippet in rawr-web, or on the server through /play/render for the three blocks marked data-where="server", and draws each output with the same function the Run button uses (site/src/lib/output.ts). A hand-written output fails the build. /play/render skips Turnstile for the smoke token, and the deploy's smoke test checks each server block's output against the deployed server. (#1246)
- The reference pages were checked claim by claim against the current rawr: overview, requirements, iced, std, closures, generics, items, moves, play and commands. These were stale: gaps that now run (serde derives, crate functions taking your type, async without a runtime, try_join! and select!, consts captured in format strings, entry borrows, std::fs, char::from_u32), a removed difference (the index panic's column), lenient cases rustc's errors now catch (HashMap without use, a field returned by value), a Send bound said to go unchecked, the &dyn gap (it is std traits' trait objects), and quoted errors rawr no longer gives (unknown import, RwLock's not-yet message). Requirements lists rawr-reload, and commands gives exit status 134 under panic = "abort" and the default-run rule. Merged as e94629ca8. (#1250)
- Calling a macro without `!`, as in `print("hi")`, `vec(1)` or `std::print("hi")`, now gives rustc's E0423 "expected function, found macro" with the help "use `!` to invoke the macro". It used to say rawr doesn't run it yet. (#1252)
- docs/reference/iced said what rawr doesn't do for iced (nothing hand-written, nothing prebuilt) and listed gaps such as 'a #[widget] written in a marked file', an attribute that exists nowhere. The page is deleted; /docs/reference/iced redirects to /docs/iced, its bindings and gaps anchors go to Imports, its examples anchor to the iced guide, and the values-cross-both-ways and one-iced-per-project facts stay in Imports and Run. (#1265)
- A marked crate's `cfg!(feature = "..")`, `#[cfg(feature = "..")]` and `#[cfg_attr(..)]` follow the features Cargo enables for it, and `rawr run`, `rawr check` and `rawr test` take `--features`, `--no-default-features` and `--all-features` as `cargo run` does. (#1259)
- A message that a closure gives on iced's executor thread, such as `keyboard::listen().filter_map(..)`'s `Some(Message::SplitFocused(Axis::Vertical))`, keeps the crate values it holds. pane_grid's hotkeys no longer panic. (#1234)
- A crate function returning `impl Trait`, where the trait has a supertrait like `Add<Output = Self>` (as num-traits' `Num` does), now imports. Before, rawr-import overflowed its stack on it. (#1262)
- A method of an opaque output that gives another opaque output of the same method, as `explain(..).explain(..)` does, can be used again. The fix for #1262 had left it unusable at `rawr check`. (#1270)
- A marked module names a native module's trait by path (`Box<dyn page::Page>`, `use crate::page::Page;`) and takes, keeps and returns its objects. (#1263)
- A path into a dependency none of whose items rawr can use yet names the item and says why, as it does for a crate with other items, instead of rustc's "cannot find module or crate". (#1267)
- A crate's function that gives a thread's `JoinHandle`, as `open::that_in_background` does, runs: the program can ignore the handle or `.join()` it for what the thread gave. (#1266)
- A marked crate with Rust dependencies follows Cargo's features: `cfg(feature)` in marked code and in native modules holds as under `cargo run`, with `--features`, `--no-default-features` and `--all-features`. (#1268)
- A crate whose generic trait has a provided method giving an opaque type, where that type's supertrait binds an associated type to Self (as `Add<Output = Self>` does), no longer panics rawr check while building its API. An item whose signature names another item's type parameter is left out with a can't-be-used-yet note. (#1278)
- A std trait works as a trait object, as in Rust. `Box<dyn Error>` with `?` from `parse`, `&dyn Debug` of a derived or builtin value, `&dyn Display` of the program's own impl, and `Vec<Box<dyn Display>>` of mixed values all work, with no `impl Error` needed in the program. Formatting, `to_string()` and the trait's methods call the value's own impl. (#1251)
- A `match` that misses values reports rustc's E0004 in rustc's words: the patterns not covered (`Message::Decrement`, `None`, `i32::MIN..=0_i32` and `2_i32..=i32::MAX`, `&_`), the label on the scrutinee, the matched type and the arm to add. Integer and `char` arms whose ranges cover the whole type no longer need a `_`. (#1256)
- A marked module builds a native module's structs and enums whose fields are `&'static str`, `Option<&'static str>` and `&'static [T]` from literals, and a marked `pub static` of them is compiled natively, so native code reads it as a Rust item. (#1283)
- An array or `vec!` literal's elements coerce to the trait object a parameter, a `let` or a function's return type expects: `all(&[&A, &B])` for `&[&dyn Named]`, and `vec![Box::new(A), Box::new(B)]` returned as `Vec<Box<dyn Named>>`. (#1285)
- A refutable pattern in a `let`, a `for` loop or a function argument is rustc's E0005, naming the values it misses and suggesting `let else` or `if let`. (#1289)
- Importing a crate no longer panics when a type in an item's values form, a type's fields, an alias, a default, a trait or impl header, a relation or an application names a type parameter its declaration doesn't have. One walker (undeclared_parameter in src/package/import.rs) checks everything the build converts for an item; the item is left out with the usual can't-be-used-yet note, and RAWR_VERBOSE=1 prints one line naming the crate, the item, the parameter index, the declared parameter count and the type. The second shape behind #1278 is found with that line on the reporting workspace. (#1296; part of #1295)
- A type alias from a native module, such as `type Width = usize`, now crosses a marked function's signature as its target does: by value, by `&` and as a result. Native code calling the function no longer fails with `Function not found`. (#1272)
- A marked type that implements one of your crate's native traits becomes its object: `Box::new(BadgePage { .. })` gives a `Box<dyn page::Page>` that native code keeps and calls. `&mut self` changes stay for the next call, and a saved method body takes effect at the next call. (#1264)
- A `let` with `ref mut` and a refutable part, a slice pattern on a slice of unknown length, a native struct pattern with refutable fields, and an enum variant in a function parameter are rustc's E0005, with rustc's witnesses. Their irrefutable forms (`let [all @ ..] = s;`, `fn f(One::Only(x): One)`) now run. (#1294)
- A tuple variant or tuple struct pattern with the wrong number of fields is rustc's E0023 (`this pattern has 1 field, but the corresponding tuple variant has 2 fields`), with the declared fields marked and rustc's help to add `_` or wrap the fields in parentheses. (#1290)
- A marked function's `&mut` parameter whose type is a native alias of a plain type, such as `&mut Width` for `type Width = usize`, now crosses as `&mut usize` does. Native callers see the change after the call. A `&mut` of a native type is still lent. (#1297)
- `rawr check` no longer stalls after reading parse cells on an app over iced: a type from another crate's facade is named by that facade's fingerprint, never by re-walking that facade's contract at each mention. (#1309)
- A native trait with supertraits works as a marked type's trait object: `Debug`, `Display`, `Clone`, `PartialEq`, `Eq`, `Hash`, `Default`, `PartialOrd` and `Ord`, derived or written, run the marked type's own impl, and a supertrait the type lacks is rustc's E0277. (#1301)
- A `&self` method of a marked type's native trait object can call back into the same object through native code: `&self` calls share the value, as shared borrows do. (#1302)
- A crate's impl with a `Future<Output = T>` bound is no longer left out when another future's `Output` names a parameter only its where clause fixes. futures_util's forwarding impls for `TakeWhile`, `Fold`, `PollImmediate` and the `&mut`/`Pin`/`Box` blankets are declared now. (#1295)
- Marked code reads a marked `pub static` of native values, such as a page's `BLOCKS: &[page::Block]`, where native code holds it. It reads the length, indexes, iterates, matches, and reads a block's `children`, with no copy. `&BLOCKS[1..2]` lends a range of it to a native function. (#1291)
- A crate's unit struct is a value, as `badge::Secondary` is, through a module or a re-export: it is passed where a method takes `impl Into<T>` through the crate's `From`, compared, printed and matched as a pattern. A tuple struct's constructor is a function value, as in `.map(Wrap)`. (#1317)
- A native trait's `&self` method can return a borrow of the marked value, as `fn label(&self) -> &str` or `fn items(&self) -> &[String]`; native code reads it after the call, and calling a `&mut self` method while it is still in use is rustc's E0502, as under cargo. (#1305)
- `rawr check` on a marked type that lacks a native trait's supertrait shows only cargo's error at the program's own paths, not a failed build of rawr's copy of the crate with generated names. (#1310)
- A `&mut` that a crate method returns, such as image's `get_pixel_mut(x, y)`, is a place: `.0 = v`, `*p = v`, `.0[3] += 1` and passing it on to another crate call change the crate's own value, as in Rust. (#1318)
- `use m::badge::{self, badge};` binds the module and the function of one name, as Rust does: `badge::Secondary` goes through the module and `badge("..")` calls the function. Two `use` lines and a glob beside the module's import do the same. Two imports of one name in one namespace are rustc's E0252. (#1319)
- A crate macro that converts its children with `Into::<Slot<'_, _>>::into(child)`, as a component crate's `badge!` and `row!` do, now takes a string literal through the crate's `From<&'a str>` and a `String` through its `From<String>`, as cargo does. So do `"x".into()`, `From::from("x")` and `<T as From<_>>::from(..)` wherever the target type is known. (#1326)
- `rawr run` of a program that rustc rejects, such as a marked type missing a native trait's supertrait, now shows only `cargo check`'s error, at the program's own paths, without rawr's report of its copy of the crate. A save that breaks the program the same way shows cargo's error too. (#1323)
- A `&mut` a crate method returns is held for its statement or binding, as Rust holds it: `buffer.get_mut(i).0 = v` and `let p = buffer.get_mut(i);` call the method once, not once per access. (#1325)
- A call can take borrowed crate values from several `let`s at once, as `Row::with_children([a, b])` takes two `Slot`s each made from its own string: its output lives as long as the shortest borrow, as in Rust. (#1329)
- `#[cfg(..)]` and `#[cfg_attr(..)]` on a struct expression's field, a match arm, an array or tuple element or a call argument leave it out where they don't hold, as in rustc. This covers crate structs such as iced's `window::Settings { #[cfg(target_os = "macos")] platform_specific: .., ..Default::default() }`. `target_os`, `unix`, `debug_assertions` and every other cfg follow `rustc --print cfg` for the target and profile Cargo builds with. (#1331)
- A crate function's array types work as in Rust: `[T; N]` by value with a const-generic `N` taken from the array you pass (`Row::with_children([a, b])`), `&[T; N]`, `&mut [T; N]` whose writes reach your array or a crate struct's field (`fill(&mut pixel.0, 7)`), and `-> [u8; 4]` results. (#1330, #1324)
- A crate macro's children convert through the crate's From impl for their own full type, as rustc selects it: a widget crate's generic Text whose theme only the impl fixes, the crate's own builders, a program's own From impl, and a type reached through a re-export. A child no impl takes gets rustc's E0277 with the types the impls take, never "unknown member". (#1336)
- A crate method's `&mut` of a value a scope frame's region holds, and an `Option<&mut T>` a crate method returns (as pane_grid's `state.get_mut(pane)`), are now called once and held for the statement or binding, as rustc holds them. Such methods are now places whatever their other inputs are. (#1333, #1334)
- A program's unit struct named by a module path is its value in a `match` scrutinee and an `if let` value too, as `match badge::Secondary { badge::Secondary => .. }`. (#1327)
- A program's generic `impl<'a, M> From<Mine> for Slot<'a, M>` of a crate type can pass its own `M` to the crate in its body: the impl is compiled for each type its conversions take, as rustc monomorphises it. (#1341)
- A leading `self::` in a path names the current module, as rustc reads it: in values, calls, patterns, types, `use` lists and `self::super::`. `self` after a path's first segment is rustc's E0433. (#1342)
- A native crate root's `mod` declarations are read under the `cfg`s Cargo builds the package with for this target, `cfg_attr` included: `#[cfg(windows)] mod x;` on macOS is no longer scanned, loaded or built, and a `#[path]` applied by `#[cfg_attr(..)]` resolves as rustc resolves it. (#1337)
- A single-file program names its own items through `crate::`, `self::` and `super::` as rustc does, in types, values, calls, patterns and `use`, with rustc's error for `super::` at the root. (#1344)
- `pub use` of an enum variant, by name or by glob, re-exports it from its module, for the program's enums and a crate's. (#1345)
- `rawr run` watches the program's inputs, not the project tree: nothing under `target/` reaches the file watcher, so rawr's own builds no longer count as saves, and a project with a huge `target/` starts its watcher. If macOS refuses the watcher's stream, the error says so and lists the paths rawr asked for. (#1346; part of #1222)
- `clock.strftime("%H:%M")` fills `F: ?Sized` with the unsized `str`; instance_surface_type had no case for that type, so the call carried no instance key and fell back to the item's shared binding, the adapter of whichever instance registered first, in an order set by hashes. New crates in the crate-shapes fixture flipped that order and the literal reached the `[u8]` adapter: the 'a value reached an adapter as a type its signature doesn't take' panic, deterministic in one lane and absent in another, which is also the shape of the #1087 flake. Ty::UnsizedStr now keys as Builtin::Str, as the adapter generator names it. Merged in 1777280a7. (#1352; part of #1087)
- A call of a crate's generic item always reaches the adapter of its own instance. A parameter an associated type reads no longer misses its adapter, and a type argument that keys no instance is a check error at the call, never a call into another instance's adapter at run time. (#1353, #1087)
- A marked type behind a native trait object lends its strings in place: `fn label(&self) -> &str` reads the program's own text with no copy, and a `&self` method returning a native type that borrows the value, as `view(&self) -> Element<'_, Message>` does, works and keeps what it borrows only to the next `&mut self` call. (#1322, #1321)
- Two `let`s of a crate value that borrows what it takes, such as an invariant `Text<'a>` or what `explain` gives, go to one call outside a view too, as Rust infers one lifetime for both. (#1335)
- rawr keeps one copy of each crate's bindings: the home's for a shared crate, target/rawr/bindings for a path crate, and the file the nightly compiler wrote for bindings that haven't settled. target/rawr/staged holds only a small record for each, keeps those of the last four resolutions that ran the nightly compiler, and drops the rest. A check whose bindings are unchanged writes none of them again. (#1354, #1355)
- The iced tab is iced's counter, and Run shows its window's first frame. The ratatui tab draws a to-do screen with ratatui's widgets and prints it as text. Both carry a Run button and the server marker, and the build shows what the server returns. The server's universe gains ratatui 0.30 (play/universe). A block that needs the server's next image (`data-image="next"`) says so while that image isn't live; the ratatui block carries it and its real output appears once the release workflow rebuilds the image and the site builds again. Merged as 7777bd005. (#1372)
- A project's target/rawr now grows by an edit's changes, not by a full copy per edit. The API, generic contract, facade, overlay and a runner's generated sources are kept as shared parts in target/rawr/store.parts, so a new generation writes only the parts it changed. When a runner leaves the four the project keeps, the units only its generation used go with it, and so does every part nothing lists any more. An undo to any of the four newest generations still rebuilds nothing. (#1356, #1358)
- Shared bindings and trait facts are kept in one directory per extraction protocol in rawr's home (`formats/<protocol>`). Each directory records the installed rawr executables that use it. An older protocol's directory is removed once it has gone a day unused and no installed rawr uses it, so a protocol bump no longer leaves gigabytes of unreachable entries. `rawr cache clean` now also removes idle shared trait facts, and the home's stale `digests` file is removed. (#1359)
- The docs are grouped as Get started, Guides, Reference and Tools. On desktop they show a sidebar tree; on phones a drawer opens from the bar's Docs control. Every page has breadcrumbs, and every heading on the site links to its own anchor. /docs opens with four reader paths. The iced, iced reload and ratatui pages are now worked examples in Apps and libraries, rewritten as plain documentation, and previews moved to Tools as Editor previews. Old docs URLs no longer redirect. (#1378)
- A push to main that changes `play/universe`, the bot's code or `play/Dockerfile` builds the ?play image and deploys it by itself. A snippet on the site that needs a crate the universe gained no longer waits for the release workflow. Every other change to the site or the docs deploys on push, as before. (#1380)
- `rawr run` starts its file watcher on apps with a thousand dependencies: the OS watches the project's source directories, each path dependency's and each manifest's directory, never every dependency file, and registry and git dependencies get no watch, since a changed one arrives through `Cargo.lock`. (#1375)
- Installing a contract counted its facade roots by rescanning every prior outcome for each outcome, which took minutes on a workspace of a thousand packages. The roots are collected once. (#1383)
- `#[widget]` / `#[rawr::widget]` is gone from rawr-macros and from rawr's parser. It did nothing; `#[preview]` is the one Rawr attribute. (#1379)
- A string literal passed to a crate call that borrows it, as `badge![icon(), "Verified"]` or `Slot::from("x")` does, is `'static` text, as in Rust: no scope frame opens for it, so nested view code such as `row![badge![..].style(..), ..].spacing(8).into()` checks and runs as cargo runs it. (#1363)
- A long-running rawr (such as `rawr mcp`) no longer loses its host directory when another rawr prunes old hosts. Each process leases its host for as long as it runs, and prune and `rawr cache clean` skip leased hosts. (#1389)
- The landing page gets its snippets ready once it has loaded: it fetches rawr-web and runs each snippet in the background. Run then shows the result at once instead of waiting for the download or the server. (#1385)
- The site's Run strips no longer say where a snippet runs: the "on rawr's server" label is gone from the landing page and the docs. (#1392; part of #1384)
- A `use` or path whose crate isn't a dependency now reads as rustc's E0433, with rustc's `cargo add` help. A missing item or module in a present module reads as rustc's E0432 or E0425, with rustc's labels. The help "isn't available in Rawr yet" is gone. (#1386)
- `rawr cache warm` builds what the first run with dependencies used to build: it installs the nightly rawr reads crate APIs with (the one the current directory names, or the pinned one), gets that nightly's driver and std table, and compiles rawr's runtime for your Rust compiler. It prints the same progress lines, or one line when everything is built. The installer runs it, so a fresh install's first `rawr run` no longer stops to compile rawr's runtime. (#1388)
- A crate value's `.into()` whose target type is known but has open arguments, as `Slot<'_, _>` in a row whose message type only a later child fixes, picks the one `From` impl that fits, as rustc does, instead of reporting E0283. (#1403)
- A crate's `From<String>`, `From<&str>` and `From<Cow<str>>` impls stay three: a `String` reaches `From<String>`, a literal `From<&str>`, and only a program's own `Cow<str>` (`Cow::Borrowed`, `Cow::from`) reaches `From<Cow<str>>`, as rustc selects them. Before, a `String` could reach the crate's Cow impl. (#1390)
- A crate value passed to `impl Into<T>` where it already is a `T`, as a component crate's `badge!` passes its `Slot::new(row)` to `badge(content: impl Into<Slot<'a, Message>>)`, takes the target's type arguments, so a later use fixes the row's message as rustc does. A crate value whose type nothing settles is rustc's E0282 at its `let` instead of a runner cargo refuses. (#1404)
- A crate that calls your function with a `&` or `&mut` of its own value, as a simulation's `update(&mut self, message, building: &Building)` does, lends you that value for the call: its type needs no `Clone`, nothing is copied, and each call sees the value as the crate holds it then. Its methods, its fields, `{}`, `{:?}` and `==` reach it where it is, and you can pass it on to the crate's functions that take `&Building`, as a view's `draw(building)` does. (#1405, #1406)
- The ?play universe runner builds again: a widget made from a borrowed `&str`, as iced's `button(label)` gives a `Button<'_, Message, &str>`, no longer fails E0597 in its generated adapter, and the generated runner compiles without warnings. (#1396)
- Running one workspace member after another no longer fails with E0463 ("can't find crate for `rawr`"). Each member's runner is now its own Cargo unit in the workspace's build directory, so one member never reuses another member's build script and the shared runtime it names. (#1421)
- `rawr::universe` loads a marked file against a binary's linked generated bindings and runs it in that process. `call(name, args)` or `run()` executes it, and `reload()` or `replace(text)` on any thread reports unchanged, queued, swapped, failed or restarting. ?play's universe runner serves its snippets through it. (#1424; part of #1387)
- `write!` with no `Write` trait in scope is rustc's E0599 "cannot write into `String`", with its help naming `use std::fmt::Write;`. (#5)
- A `use` inside a block is in scope in that block alone; outside it its names are rustc's E0425. (#7)
- A bare value where a `Box<dyn Error>` is expected is rustc's E0308, with rustc's `Box::new` or `Into::into` help. (#6)
- An unsuffixed literal that several `From` impls fit takes rustc's fallback type, i32 or f64, and selects that impl. (#8)
- An index past a slice's end panics at the expression's start, as rustc places it. (#35)
- Integer division and remainder by zero or overflow, out-of-range slice ranges, `pow` overflow and a reversed `clamp` panic in rustc's words and at rustc's position. (#37)
- A project whose `.cargo/config.toml` sets `build.target` (or that runs with `CARGO_BUILD_TARGET`) is read, extracted and generated for that target, as Cargo builds it, such as wasm32-unknown-unknown. (#1394)
- A crate value generic over a program's types can call a script function it kept, such as elevato's `Simulation::frame` calling the program's `update`, and that function can make crate values like a `Task` and give them back. Before, the program panicked with `Stale`. (#1432)
- A view whose message type only its declared return type fixes, as `fn view(..) -> Slot<'static, Message> { match id { .. => row![badge![..].style(..)].into() } }`, runs instead of failing with E0282 at the badge when the crate's Slot is also an Element. A crate type with `From` impls for several generic targets, as a badge that is both a Slot and an Element, converts with `.into()` instead of failing at start with "the bindings have no item". (#1422)
- A function bounded by a trait with more than 64 impls, such as icedxdy's `badge(content: impl Into<Slot<'a, Message>>)`, now ties its argument's types to the callee's, so `badge![icon, "Verified"]` no longer reports E0282. (#1435)
- A native function that keeps the blocks it is given, such as `State::new(page_index, blocks: &'static [Block])`, takes a marked module's `static BLOCKS`, a range of it, or a slice literal, as Rust does. Before, the generated runner failed to compile with E0713/E0521. (#1437)
- A marked type that native code holds as a `Box<dyn Trait>` can return references: a `&'static Meta` or `&'static [Block]` that is a static gives native code the static itself, and `&self.state` is lent where the value holds it. A reference result rawr can't give yet is an error at `rawr check` instead of a panic at the call. (#1442)
- A marked type that native code holds as a `Box<dyn Trait>` can clone a field of a value it lent in place, as `fn scrollable_id(&self) -> Id { self.state.scrollable_id.clone() }` does while native code still holds `&State` from `state()` or a view from `preview()`. icedxdy's browser no longer exits when a page is selected. (#1445)
- A local array or slice borrowed where a crate takes `&'static [T]` is now rustc's E0597, as a borrowed string already was; statics, ranges of them and constant literals still pass. (#1064)
- An unused `use crate::module;` in a marked file is no longer refused with E0432; as in rustc, it is at most a warning. (#1436)
- ?play's universe (iced, tokio and ratatui with their generated bindings) builds for wasm32, and the site can run an iced snippet in your browser on it: each Run gets a fresh instance in a frame of its own, drawn by iced's web shell, answering clicks with no round trip to the server. The landing page keeps its iced counter on the server for now, since the browser draws it at the wrong scale on high-density displays. (#1448; part of #1395, #1384)
- The landing page's iced counter runs in your browser, in a frame of its own, at any display scale, answering clicks with no round trip to the server. (#1449)
- A method a crate's trait gives another crate's type by a generic impl now resolves, as ratzilla's `WebRenderer` gives ratatui's `Terminal` its `on_key_event`; a block that ends in a closure passed to a crate keeps the closure's parameter types and captures. (#1450)
- `?` converts a crate's error through the crate's `impl From<Error> for io::Error`, and a crate error with a written or derived (thiserror) `std::error::Error` impl boxes as a `Box<dyn Error>`. (#1451)
- When one definition is read under two names of the same toolchain, as `nightly` and the dated nightly it resolves to, rawr treats it as one definition instead of failing with `conflicting-definition`; that error also names the item and both sources. (#1461)
- A crate whose `lazy_static!` declares a hidden struct beside a static of the same name, as `colored`'s `SHOULD_COLORIZE` does, imports instead of failing as declared twice. (#1462)
- A crate that re-exports `core` under another name, as bytemuck's `pub use ::core as __core;`, registers its bindings instead of failing at a module with no parent. (#1463)
- A marked function that native code calls on another thread, as an iced app's `Task::perform` output closure runs on a tokio worker, makes and uses crate values as it does on the main thread, instead of failing with `Stale`. (#1464)
- The playground deploys again: the browser's universe ships as a 5 MB brotli-compressed module that the browser decompresses as it downloads, since the 53 MB raw module was over Cloudflare's limit on one file. (#1459)
- A marked file can define a type over a native one, as inboard's `theme.rs` does with `enum Theme { Selected(themedef::Theme) }`. Its values cross with their native parts, and every impl native code relies on stays: `From` in both directions with native and crate types, a crate trait's impl, and `impl From<&Theme> for markdown::Settings`. (#1466)
- In ?play, a snippet's statements outside `fn main` can call std's path, io, time and fs items again, as the std page's `PathBuf` example does. (#1460)
- rawr checks a project built for another target, such as wasm32, when a dependency's build script uses a build-dependency with host-only dependencies. Before, it stopped with an internal error that a Cargo unit's package was absent from metadata. (#1454)
- Any file in a native crate's module tree can be marked with `// rawr`, not only one declared by the crate root or a marked module. Marked code there reaches its parent's private items, its siblings and its native children as rustc allows, and native code calls into it. (#1467, #1150)
- A marked type over a native one, as inboard's `enum Theme { Selected(themedef::Theme) }`, lends its native part to the interpreter where it is when native code borrows it, `&mut self` too, and moves it for a value: no clone, and a part that isn't `Clone` crosses. (#1469)
- A marked type's `&self` method returning a reference into its native part, as `fn colors(&self) -> &Colors`, gives native code that reference where the part is, and marked code can call it as `self.colors().accent`. A `&str` result such as `iced::theme::Base::name` works too. (#1468)
- Slices and `Vec`s run `binary_search`, `binary_search_by`, `binary_search_by_key` and `partition_point` by std's own loop, so each `Ok` among equal elements and each `Err` insertion point is the one rustc's build gives. (#1456)
- A marked module reads a native crate's `const` or `static` of `&str` as `&'static str` and passes it to a crate function that takes one, such as `Font::new(fonts::UI_FAMILY)`, keeping one string for the program's life. A constant of a slice of numbers reads as `&[T]`, as in Rust. An item a module can't give yet is now rustc's E0425 for the module. (#1146)
- A `match` on a marked enum whose variant holds a crate enum (`Message::Wizard(wizard::Message::Step(step))`) is exhaustive where rustc's is, instead of a false E0004. (#1481)
- Native code can keep a marked `State` between calls and poll the `Task<Message>` a marked `update` returns, mapped with a marked variant such as `Message::Wizard`: the task and element types are native code's own. (#1485; part of #1249, #44)
- A marked type can hold a native value that isn't `Clone` inside an `Option`, a `Vec`, a `Box` or a tuple, as inboard's `home::State` holds `wizard: Option<wizard::State>`. Native code lends the value to marked methods where it is, so `self.wizard = Some(..)`, `self.wizard = None`, `self.tabs.push(..)` and `self.tabs.remove(i)` work as in Rust. (#1475, #1482)
- A marked function that returns `()` gives native code `()` even when its last statement leaves a value, such as the native part `self.tabs.remove(index);` removes. That case used to fail with E0515. (#1486)
- A save to a program with a native `main` and an idle window loop now shows at once on macOS. rawr asks the program's windows to redraw when it queues a save, so the next frame takes it with no input. Before, the save waited for the next pointer or key event. (#1479)
- Native code can hold the `Element<'a, Message>` that a marked `view<'a>(&'a self, ..)` or `view(&State) -> Element<'_, Message>` returns while it borrows the state. The element's borrowed text lives as long as Rust says it does, and `.map(Message::Wizard)` on such an element works inside the view. (#1473)
- A project that links a native library rawr's runtime also links, such as psm through stacker, now reuses the runtime rawr warmed when both take it from one package at Cargo-compatible versions. Its first run after an install no longer compiles rawr in the project: inboard's 11-minute first start goes away. (#1480)
- A marked function can take a native value of borrows by value, as inboard's `catalog: View<'_>`, for the call alone, and a marked method on it works too. (#1225)
- Inside a marked `view<'a>`, a crate method called on a native `&'a T` lent to the view, such as `inbox.view()`, returns an element that lives as long as the view's own. A crate fn with an `Option<&'a str>` parameter that its output keeps, such as inboard's `sidebar::nav_section(None, ..)`, now imports and runs. (#1496; part of #1413)
- An or-pattern whose alternatives differ only in the crate values they test, such as `Message::Floor(_, floor::Message::Up) | Message::Floor(_, floor::Message::Down)`, no longer fails with a false RAWR-RESOLVE-MATCH. A name missing from one alternative is rustc's E0408, and a binding of another type is E0308. elevato.rs's browser universe now checks. (#1497; part of #435, #1384)
- A marked function takes native values inside an `Option`, a `Vec`, a `Box` or a tuple by value, such as inboard's `home_grid: Option<Element<'a, grid::Message>>`. (#1224)
- A marked function takes a `&[T]` of native values, lent where it is, and passes it on to native functions. (#1292)
- In a marked view, an element from a native function may borrow the view's lent arguments, as inboard's `inbox::view(inbox_state, .., browser_updates, ..)` does. An element that a native part of `self` gives lasts as long as the view's result. (#1413)
- A marked module can name a crate's type alias through a module its `use` binds, such as `extension::registry::Update` after `use model::extension;`. That works in signatures and in `let`s. A crate's or a native module's alias of a std type, such as `type Names = Vec<String>`, now resolves too. Both gave E0425 before. (#694, #1202)
- A crate method whose output borrows its receiver can also take a `&'a T` at that lifetime, as inboard's `wizard.view(catalog, theme)` does from a marked view. (#1499)
- Moving a field out of a crate struct held in a local, as `let task = action.task;`, leaves the struct's other fields for later reads and moves, as rustc does. So `match action.instruction` afterwards works, and so does a destructuring `let Action { instruction, task } = ..` of a crate value. (#1502; part of #834)

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
