# Rawr

Rust Augmented With Rust.
Site: <https://rawr.airstrike.fyi>

## What is Rawr?

A scripting language that reads like Rust: the same structs, enums, traits and
`match`, checked up front with rustc-style errors. Your program stays Rust; the
parts you want to change while it runs are written in Rawr.

The `rawr` crate is the language, and any Rust program can embed it. Layers
add a domain on top. The first is `rawr-iced`, for
[iced](https://github.com/iced-rs/iced) apps; it is brought in when your code
uses iced.

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
- It's still Rust to read and to review. Errors look like rustc's: a code, a
  span, and what is missing.
- With iced, your own Rust crates are imported with no bindings to
  write, and `#[preview]` plus `rawr snapshot` render views to PNG for review.

If you later want plain Rust back, `rawr check --as-rust` copies the project
under `target/` with `.rawr` renamed to `.rs` and runs `cargo check` on it.

## How do I install it?

To embed the language, add the crate. It builds on stable Rust 1.91 or newer:

```sh
cargo add --git https://github.com/airstrike/rawr rawr
```

The `rawr` command is more demanding, even for scripts without iced: it
carries rawr-iced, runs on Apple Silicon Macs for now, and needs one pinned
Rust nightly, `nightly-2026-07-11`, with the `rustc-dev` and `rust-src`
components. Rawr builds iced and your own crates with that toolchain too.

```sh
rustup toolchain install nightly-2026-07-11 --component rustc-dev,rust-src
cargo +nightly-2026-07-11 install --locked --git https://github.com/airstrike/rawr rawr-cli
cargo +nightly-2026-07-11 install --locked --git https://github.com/airstrike/rawr rawr-import --features rustc-driver
```

`rawr-cli` installs `rawr`. `rawr-import` installs `rawr-import-driver` next
to it, which reads your Rust crates for imports. The first `rawr run` of an
iced app compiles iced, so it takes a few minutes. Later launches reuse that
build. A script without iced runs once and exits, with no build and no hot
reload.

## How do I start an iced project?

```sh
rawr new hello && cd hello && rawr run
```

`rawr new` writes one file, `src/main.rawr`, a counter. There is no manifest
until you want one. Edit it while the window is open.

The other commands you will use:

- `rawr check`: type-check without opening a window.
- `rawr preview [name]`: open one `#[preview]` view on its own.
- `rawr snapshot --check` / `--update`: render every preview headlessly and
  compare with, or replace, the committed PNGs.
- `rawr doctor`: report on the environment and the project.

Every command takes a project directory or a single file. `rawr --help` lists
them all.

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

A renderer for snippets. In a Discord channel where the bot is turned on, write
`?play` above a code block holding an expression, a view or a whole app, and it
replies with the rendered frame.

````text
?play 320x200 light msg=Message::Increment
```rust
column![text("hi"), button("go")]
```
````

Options are `WxH` (up to 2048 per side), `dark` or `light`, and repeated
`msg=EXPR` messages applied before rendering. The
[playground](https://rawr-bot.airstrike.fyi/play) does the same in a browser.
To add the bot to a server, use the
[invite link](https://rawr-bot.airstrike.fyi/invite).

## How far along is it?

Pre-alpha. The `rawr` command runs on Apple Silicon Macs, rawr-iced follows
iced master, and both pin one nightly. APIs, commands, file formats and error
codes will change without notice.

It covers a lot of Rust, not all of it. Known gaps include:

- nested `fn` items and `let .. else`;
- `impl Fn(..) -> T + 'a` and a few other closure and `fn`-value shapes;
- generic functions over iced types, such as a `fn padded<Message>(..)` helper;
- parts of iced not yet exposed, such as `rule`, `rich_text!` and canvas.

When Rawr rejects valid Rust that it claims to cover, that is a bug. A small
reproduction is the most useful report.

## What's the license?

MIT or Apache-2.0, at your option. See [`LICENSE-MIT.txt`](LICENSE-MIT.txt) and
[`LICENSE-APACHE.txt`](LICENSE-APACHE.txt).
