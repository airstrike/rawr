# Rawr

Rust Augmented With Rust.
Site: <https://rawr.airstrike.fyi>
Docs: <https://rawr.airstrike.fyi/docs>

## What is Rawr?

A scripting language whose programs are Rust. You write Rust. Rawr runs the
files marked `// rawr` while you develop. It reloads them when you save. Cargo
builds the same crate for release.

A Rawr file is a `.rs` file whose first line is `// rawr`. A Rawr project is an
ordinary Cargo crate. Rawr checks your code before running it and reports
errors the way rustc does.

Rawr code runs on an interpreter. Everything it calls into is native Rust.

The `rawr` crate is the language. Any Rust program can embed it. The `rawr`
command runs Rawr programs. rawr-iced adds support for
[iced](https://github.com/iced-rs/iced) apps. Install it and `rawr` hands
programs that use iced to it.

## What does it look like?

On its own, `cart.rs`:

```rust
// rawr
enum Item {
    Book { price: f64 },
    Coffee { cups: i64 },
}

trait Priced {
    fn price(&self) -> f64;
}

impl Priced for Item {
    fn price(&self) -> f64 {
        match self {
            Item::Book { price } => *price,
            Item::Coffee { cups } => 3.5 * (*cups as f64),
        }
    }
}

fn total(items: &[Item]) -> f64 {
    let mut sum = 0.0;
    for item in items {
        sum += item.price();
    }
    sum
}

fn main() {
    let cart = vec![Item::Book { price: 20.0 }, Item::Coffee { cups: 2 }];
    println!("total: {}", total(&cart));
}
```

```sh
$ rawr run cart.rs
total: 27
```

The same file can be `src/main.rs` of a Cargo crate. Then `cargo run` prints the
same line.

A Rust program can also embed rawr and load the same code as a script. A
script is typed Rust too, in a `.rs` file. This one is
`examples/scripts/cart.rs`, the program above without its `// rawr` line.

```rust
use rawr::{Engine, Scope};

fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    let engine = Engine::new();
    let ast = engine.compile(include_str!("scripts/cart.rs"))?;
    engine.call_fn::<()>(&mut Scope::new(), &ast, "main", ())
}
```

That's `examples/cart.rs`. `cargo run --example cart` prints `total: 27`.

With iced, this is iced's own counter example with `// rawr` as its first line.
It is `src/main.rs` in a crate that depends on iced. `rawr check` accepts it.
`rawr run` opens the app and reloads it when you save. `cargo run` builds the
same app natively.

```rust
// rawr
use iced::Center;
use iced::widget::{Widget, button, column, text};

pub fn main() -> iced::Result {
    iced::run(Counter::update, Counter::view)
}

#[derive(Default)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
    Decrement,
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => {
                self.value += 1;
            }
            Message::Decrement => {
                self.value -= 1;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            button("Increment").on_press(Message::Increment),
            text(self.value).size(50),
            button("Decrement").on_press(Message::Decrement)
        ]
        .padding(20)
        .align_x(Center)
    }
}
```

## Why would I use it instead of plain Rust?

- Code you change while the program runs doesn't need a rebuild. With iced,
  saving a marked file reloads the running app in about 0.2 s, and the
  window stays open and keeps its state.
- It's still Rust to read and to review. Errors look like rustc's. They show
  the error code, the span and what is missing.
- Rawr code uses the crates in your Cargo.toml without bindings.
  `#[preview]` and `rawr snapshot` render views to PNG for review.

The cost is speed. Rawr code is interpreted, so its own logic runs slower than
compiled Rust. Keep hot loops and heavy work in native modules or Rust crates.

For release, `cargo build --release` builds the same crate natively. No
interpreter ships with it. `rawr check` runs `cargo check` on the crate after
its own check, so rustc tells you whether the code is valid Rust.

## How do I install it?

To embed the language, add the crate. It builds on stable Rust 1.95 or newer:

```sh
cargo add --git https://github.com/airstrike/rawr rawr
```

The `rawr` command runs Rawr programs. It builds on stable Rust too:

```sh
cargo install --locked --git https://github.com/airstrike/rawr rawr-cli
```

`rawr-cli` installs `rawr`. It also installs `cargo-rawr`, so `cargo rawr run`
works too. A program without iced runs once and exits. It doesn't hot reload.

rawr-iced adds iced support. Install it and `rawr` hands programs that use
iced to it. It needs a Unix system. It has only been tested on Apple Silicon
Macs so far. The rest of rawr-iced should work wherever iced runs. It has only
been tested on macOS. It needs one pinned Rust nightly, `nightly-2026-07-11`,
with the `rustc-dev` and `rust-src` components. Rawr reads the API of your
Rust dependencies with that nightly.

```sh
rustup toolchain install nightly-2026-07-11 --component rustc-dev,rust-src
cargo +nightly-2026-07-11 install --locked --git https://github.com/airstrike/rawr rawr-iced-cli
cargo +nightly-2026-07-11 install --locked --git https://github.com/airstrike/rawr rawr-import --features rustc-driver
```

`rawr-iced-cli` installs `rawr-iced`. `rawr-import` installs
`rawr-import-driver` in the same directory. The driver reads your Rust crates
for imports. `rawr` finds `rawr-iced` next to itself or on `PATH`.

An iced program is a Cargo project. `rawr run` builds a runner against the
project's own iced and its features, in the project's `target/rawr/`, so the
first `rawr run` of a project compiles iced and Rawr's crates. Edits to Rawr
code need no build. A program that uses iced without a Cargo.toml is refused;
`rawr new` starts a project.

## How do I start an iced project?

With rawr-iced installed:

```sh
rawr new hello && cd hello && rawr run
```

`rawr new hello` writes a Cargo crate. `hello/Cargo.toml` depends on iced at the
revision rawr-iced supports, with its `advanced` feature, which Rawr's runner
needs. `hello/rust-toolchain.toml` pins the nightly Rawr builds with, so
`cargo build` uses the same compiler. `hello/src/main.rs` starts with `// rawr`
and holds iced's counter. Edit it while the window is open. `cargo run` builds
and runs the same app.

The other commands you will use:

- `rawr check`: check with Rawr and run `cargo check`, without opening a window.
- `rawr preview [name]`: open one `#[preview]` view on its own.
- `rawr snapshot --check` / `--update`: render every preview to a PNG, then
  compare them with the committed images or replace them.
- `rawr doctor`: report on the environment and the project.

`rawr new`, `rawr preview` and `rawr snapshot` come with rawr-iced. Every
command takes a crate directory, its Cargo.toml or a single `.rs` file.
`rawr --help` lists them all.

## Can I use my own Rust code?

Yes. Add the crate to `[dependencies]` in Cargo.toml and `use` it:

```toml
[dependencies]
gallery-ui = { path = "rust/gallery-ui" }
```

```rust
use gallery_ui::Dial;
```

Rawr reads only the items your code touches. A custom iced widget written in
Rust is just another widget.

A module whose file doesn't start with `// rawr` is native Rust. `rawr run`
builds it with cargo and calls it from Rawr. Saving a native module or a
dependency triggers a native rebuild. Saving a marked file does not.

`#[preview]` and `#[widget]` come from the `rawr-macros` crate. Its library
name is `rawr`. Add it to `[dependencies]`:

```toml
rawr-macros = { git = "https://github.com/airstrike/rawr" }
```

Then write `#[rawr::preview(..)]`, or `use rawr::preview;` and `#[preview(..)]`.
Under cargo the attributes leave the item as written.

## What's ?play?

A runner for snippets. In a Discord channel where the bot is turned on, write
`?play` above a code block. A snippet that uses iced can be an expression, a
view or a whole app. The bot replies with its rendered frame.

````text
?play 320x200 light msg=Message::Increment
```rust
column![text("hi"), button("go")]
```
````

A plain program is one that does not use iced. The bot runs it and replies
with its output as text. It shows the exit status when that is not 0. A bare
expression such as `1 + 2` prints its value.

Options are `WxH` (up to 2048 per side), `dark` or `light`, and repeated
`msg=EXPR` messages applied before rendering. The
[playground](https://rawr.airstrike.fyi/play) does the same in a browser.
To add the bot to a server, use the
[invite link](https://rawr.airstrike.fyi/bot/invite).

## How far along is it?

Pre-alpha. The `rawr` command builds on stable Rust. Importing your own Rust
crates has only been tested on Apple Silicon Macs. rawr-iced follows iced master
and pins one nightly. APIs,
commands, file formats and error codes will change without notice.

Rawr doesn't cover all of Rust yet. Known gaps include:

- type aliases, glob imports and `pub(crate)` or `pub(super)` visibility;
- generic functions over iced types, such as a `fn padded<Message>(..)` helper;
- `std::time` outside iced apps;
- parts of iced not yet exposed, such as canvas.

The [docs](https://rawr.airstrike.fyi/docs/language) keep the full list.

Unsupported Rust in a marked file fails the Rawr check with an error. If Rawr
rejects valid Rust that it claims to support, that's a bug. Please
report it with a small reproduction.

## What's the license?

MIT or Apache-2.0, at your option. See [`LICENSE-MIT.txt`](LICENSE-MIT.txt) and
[`LICENSE-APACHE.txt`](LICENSE-APACHE.txt).
