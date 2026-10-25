# rawr

rawr is rust augmented with rust: a Rust interpreter for your dev builds.

Site: <https://rawr.airstrike.fyi>
Docs: <https://rawr.airstrike.fyi/docs>

## What is rawr?

Put `// rawr` on the first line of a file in your Cargo crate. `rawr run`
checks the marked files and then interprets them. Cargo compiles the same
crate for release. The marker is a comment, so the release build has no
interpreter in it.

rawr checks names, types, traits and `match` arms before anything runs. Its
errors use rustc's codes and point to your source. `rawr check` also runs
`cargo check`, so rustc has the final word on whether the code is valid Rust.

An app can also embed the `rawr` crate to run Rust scripts. That is a second,
separate use, described [below](#can-my-app-run-rust-scripts).

## What does it look like?

This is `src/main.rs` of a Cargo crate:

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
$ rawr run
total: 27
$ cargo run
total: 27
```

`rawr run cart.rs` runs the same code as a single file.

## How do I install it?

The `rawr` command needs Rust 1.95 or newer:

```sh
cargo install --locked --git https://github.com/airstrike/rawr rawr-cli
rawr new hello && cd hello && rawr run
```

`rawr-cli` installs `rawr` and `cargo-rawr`. `rawr new hello` writes a crate
with no dependencies whose `src/main.rs` starts with `// rawr`. A program like
this runs once and exits. Reload on save for it is planned.

Extensions serve more programs. `rawr` hands a program to the one that serves
it, and says how to install it when it is missing.

- **rawr-import** runs programs with dependencies in `Cargo.toml`, or with
  native modules (files without the marker). Cargo compiles them, and the
  marked code calls them.
- **rawr-iced** runs [iced](https://github.com/iced-rs/iced) apps and reloads
  them when you save a marked file.

Both need a Unix system and the pinned nightly `nightly-2026-07-11` with the
`rustc-dev` and `rust-src` components. They have been tested on Apple Silicon
Macs.

```sh
rustup toolchain install nightly-2026-07-11 --component rustc-dev,rust-src

# rawr-import
cargo +nightly-2026-07-11 install --locked --git https://github.com/airstrike/rawr rawr-import-cli
cargo +nightly-2026-07-11 install --locked --git https://github.com/airstrike/rawr rawr-import --features rustc-driver

# rawr-iced
cargo +nightly-2026-07-11 install --locked --git https://github.com/airstrike/rawr rawr-iced-cli
cargo +nightly-2026-07-11 install --locked --git https://github.com/airstrike/rawr rawr-import --features rustc-driver
```

A crate with dependencies needs `rust-toolchain.toml` and
`.cargo/config.toml`, so that `rawr run` and `cargo build` share one build.
`rawr run` prints both files when they are missing. The
[docs](https://rawr.airstrike.fyi/docs/imports) explain them.

## How do I start an iced app?

With rawr-iced installed:

```sh
rawr new counter --template iced && cd counter && rawr run
```

The template writes iced's counter with `// rawr` as its first line, a
`Cargo.toml` that depends on the iced revision rawr-iced supports, and the two
files above. Edit `src/main.rs` while the window is open. A save that passes
the check swaps the new code into the running app. The app keeps its state
when the new code is compatible with it. `cargo run` compiles the same app
natively.

rawr-iced also adds `rawr preview`, `rawr snapshot`, `rawr mcp`, `rawr init`
and `rawr cache`. The [rawr-iced docs](https://rawr.airstrike.fyi/docs/iced)
cover them.

## Can my app run Rust scripts?

Yes. That is embedding, a second use of rawr. It lets the people who use your
Rust app change its behaviour in Rust, without rebuilding it. Your app adds
the crate, exports its own types with `#[rawr::export]`, loads the user's
script and calls its functions through typed handles:

```sh
cargo add --git https://github.com/airstrike/rawr rawr
```

```rust
use rawr::{Engine, Exports};

#[derive(Clone)]
#[rawr::export]
pub struct Transaction {
    pub payee: String,
    pub amount: f64,
}

fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    let mut bank = Exports::new("bank");
    bank.add::<Transaction>();
    let mut engine = Engine::new();
    engine.export(bank)?;

    let script = engine.load("rules.rs")?;
    let category = script.func::<(Transaction,), Option<String>>("category")?;
    let tx = Transaction { payee: "Blue Bottle Coffee".into(), amount: -4.5 };
    println!("{:?}", category.call(tx)?);
    Ok(())
}
```

The user's `rules.rs` is typed Rust:

```rust
use bank::Transaction;

fn category(tx: Transaction) -> Option<String> {
    if tx.payee.contains("Coffee") { Some("coffee".into()) } else { None }
}
```

It prints `Some("coffee")`. A mistake in the rules fails when they load, or
when the app looks up `category`, with the error rustc would give.
`script.reload()` picks up an edit. The interpreter is part of your app, so it
ships in your release build. `examples/rules` is a fuller example, and the
[embedding docs](https://rawr.airstrike.fyi/docs/embedding) cover the API.

## Can I try it without installing anything?

The [playground](https://rawr.airstrike.fyi/play) runs Rust in your browser.
It shows a program's output, and renders iced code as an image or an
animation. The `?play` bot does the same in Discord channels where it is
turned on. [Add it to a server](https://rawr.airstrike.fyi/bot/invite).

## How far along is it?

Pre-alpha. rawr's API and commands may change.

rawr supports part of Rust. The
[Supported Rust](https://rawr.airstrike.fyi/docs/language) page lists what
works, what is missing, what rawr refuses and the few known cases that still
differ from Rust.

A native crate root may call marked modules, and a save reaches the running
program at its next call. Planned and not built yet: the same from native
modules other than the root, reload on save for other programs that aren't
iced apps, and a second UI host for ratatui.

If rawr gives a different result than Rust, or rejects Rust it says it
supports, please [report it](https://github.com/airstrike/rawr/issues) with a
small reproduction.

## How do I work on rawr itself?

The repository checks its commits with git hooks in `.githooks/`. Turn them
on once per clone:

```sh
git config core.hooksPath .githooks
```

Before a commit, `pre-commit` runs `cargo audit` and refuses a new `mod.rs`
or a new Markdown file at the root. When Rust files are staged, it also runs
`cargo +stable fmt --all --check` and
`cargo +nightly-2026-07-11 clippy --workspace --all-targets -- -D warnings`.
`pre-push` runs the same checks on everything being pushed. `commit-msg`
and `pre-merge-commit` refuse messages that mention Claude. Run
`cargo +stable fmt --all` before committing.

## What's the license?

MIT or Apache-2.0, at your option. See [`LICENSE-MIT.txt`](LICENSE-MIT.txt) and
[`LICENSE-APACHE.txt`](LICENSE-APACHE.txt).
