# Rawr

Rust Augmented With Rust.
Site: <https://rawr.airstrike.fyi>
Docs: <https://rawr.airstrike.fyi/docs>

## What is Rawr?

A scripting language that reads like Rust. It has Rust's structs, enums,
traits and `match`. It checks your code before running it and reports errors
the way rustc does. Your program stays Rust. You write the parts you want to
change while it runs in Rawr.

Rawr code runs on an interpreter. Everything it calls into is native Rust.

The `rawr` crate is the language. Any Rust program can embed it. The `rawr`
command runs Rawr programs. rawr-iced adds support for
[iced](https://github.com/iced-rs/iced) apps. Install it and `rawr` hands
programs that use iced to it.

## What does it look like?

On its own, `cart.rawr`:

```rust
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
$ rawr run cart.rawr
total: 27
```

It is also a Rust program: rename it to `cart.rs` and rustc prints the same.

Embedded in a Rust program, the same file:

```rust
use rawr::{Engine, Scope};

fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    let engine = Engine::new();
    let ast = engine.compile(include_str!("cart.rawr"))?;
    engine.call_fn::<()>(&mut Scope::new(), &ast, "main", ())
}
```

That's `examples/cart.rs`; `cargo run --example cart` prints `total: 27`.

With iced, this is iced's own counter example. Save it as
`src/main.rawr` and `rawr check` accepts it unchanged. `rawr run` opens the
app and reloads it when you save.

```rust
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
  saving a `.rawr` file reloads the running app in about 0.2 s, and the
  window stays open and keeps its state.
- It's still Rust to read and to review. Errors look like rustc's. They show
  the error code, the span and what is missing.
- With iced, you import your own Rust crates without writing bindings.
  `#[preview]` and `rawr snapshot` render views to PNG for review.

The cost is speed. Rawr code is interpreted, so its own logic runs slower than
compiled Rust. Keep hot loops and heavy work in Rust crates.

Rawr has no release mode that compiles to machine code. Because the code is
Rust-shaped, `rawr check --as-rust` copies the project under `target/` with
`.rawr` renamed to `.rs` and runs `cargo check` on it. If that passes, cargo
can build the same files as a normal Rust program.

## How do I install it?

To embed the language, add the crate. It builds on stable Rust 1.95 or newer:

```sh
cargo add --git https://github.com/airstrike/rawr rawr
```

The `rawr` command runs Rawr programs. It builds on stable Rust too:

```sh
cargo install --locked --git https://github.com/airstrike/rawr rawr-cli
```

`rawr-cli` installs `rawr`. A program without iced runs once and exits. It
doesn't build anything or hot reload.

rawr-iced adds iced support. Install it and `rawr` hands programs that use
iced to it. Importing your own Rust crates currently works only on Apple
Silicon Macs. The rest of rawr-iced should work wherever iced runs, but it has
only been tested on macOS. It needs one pinned Rust nightly, `nightly-2026-07-11`, with the `rustc-dev` and `rust-src`
components. Rawr builds iced and your own crates with that toolchain too.

```sh
rustup toolchain install nightly-2026-07-11 --component rustc-dev,rust-src
cargo +nightly-2026-07-11 install --locked --git https://github.com/airstrike/rawr rawr-iced-cli
cargo +nightly-2026-07-11 install --locked --git https://github.com/airstrike/rawr rawr-import --features rustc-driver
```

`rawr-iced-cli` installs `rawr-iced`. `rawr-import` installs
`rawr-import-driver` in the same directory. The driver reads your Rust crates
for imports. `rawr` finds `rawr-iced` next to itself or on `PATH`.
`cargo install` builds iced into `rawr-iced`, so it takes a few minutes.
After that, an iced app opens in seconds. A project with its own Rust crates
compiles them on its first run.

## How do I start an iced project?

With rawr-iced installed:

```sh
rawr new hello && cd hello && rawr run
```

`rawr new` writes a counter app to `src/main.rawr`. You don't need a manifest
until you add Rust crates. Edit it while the window is open.

The other commands you will use:

- `rawr check`: type-check without opening a window.
- `rawr preview [name]`: open one `#[preview]` view on its own.
- `rawr snapshot --check` / `--update`: render every preview to a PNG, then
  compare them with the committed images or replace them.
- `rawr doctor`: report on the environment and the project.

`rawr new`, `rawr preview` and `rawr snapshot` come with rawr-iced. Every
command takes a project directory or a single file. `rawr --help` lists them
all.

## Can I use my own Rust code?

Yes. List the crate in `rawr.toml` and `use` it:

```toml
[rust.dependencies]
gallery_ui = { package = "gallery-ui", path = "rust/gallery-ui" }
```

```rust
use gallery_ui::Dial;
```

Rawr reads only the items your code touches. A custom iced widget written in
Rust is just another widget. Changing the Rust crate triggers a native rebuild;
changing `.rawr` files does not.

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
crates works only on Apple Silicon Macs for now. rawr-iced follows iced master
and pins one nightly. APIs,
commands, file formats and error codes will change without notice.

Rawr doesn't cover all of Rust yet. Known gaps include:

- type aliases, glob imports and `pub(crate)` or `pub(super)` visibility;
- generic functions over iced types, such as a `fn padded<Message>(..)` helper;
- `std::time` outside iced apps;
- parts of iced not yet exposed, such as canvas.

The [docs](https://rawr.airstrike.fyi/docs/language) keep the full list.

If Rawr rejects valid Rust that it claims to support, that's a bug. Please
report it with a small reproduction.

## What's the license?

MIT or Apache-2.0, at your option. See [`LICENSE-MIT.txt`](LICENSE-MIT.txt) and
[`LICENSE-APACHE.txt`](LICENSE-APACHE.txt).
