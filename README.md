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

One command installs `rawr` and the extension that runs crates with
dependencies:

```sh
curl -LsSf https://rawr.airstrike.fyi/install.sh | sh
rawr new hello && cd hello && rawr run
```

The installer puts prebuilt `rawr`, `cargo-rawr`, `rawr-import` and
`rawr-import-driver` in `~/.cargo/bin`. `rawr new hello` writes a crate
with no dependencies whose `src/main.rs` starts with `// rawr`. `rawr run`
restarts it when you save. Arguments after `--` go to the program, as under
`cargo run`, and it reads the same stdin.

Extensions serve more programs. `rawr` hands a program to the one that serves
it, and says how to install it when it is missing.

- **rawr-import** runs programs with dependencies in `Cargo.toml`, or with
  native modules (files without the marker). Cargo compiles them, and the
  marked code calls them. A save that edits a function body reaches the
  running program in about 80 ms, in the same process with its state kept
  ([measurements](benches/README.md#release-hot-reload-measurements)).
  An [iced](https://github.com/iced-rs/iced) app is one of these programs.

rawr-import reads crate APIs with a nightly and its `rustc-dev` and
`rust-src` components: the nightly the project names, as rustup picks it
(`cargo +nightly`, `RUSTUP_TOOLCHAIN`, an override or `rust-toolchain.toml`),
or the pinned `nightly-2026-07-11` when it names none or names a stable or
beta release. Any nightly from
`nightly-2026-07-11` through `nightly-2026-11-02` works. The first program
that needs one installs it through [rustup](https://rustup.rs), once per
machine. The installed `rawr-import-driver` is the pinned nightly's; for
another nightly, rawr-import builds a driver once and keeps it in rawr's
home.

Prebuilt binaries cover macOS on Apple Silicon and Intel, and Linux on
x86_64 and aarch64. rawr-import has been tested on Apple Silicon Macs, and
CI runs it on x86_64 Linux. Windows is best effort. In PowerShell,
`irm https://rawr.airstrike.fyi/install.ps1 | iex` installs `rawr` and
`cargo-rawr` only, because rawr-import isn't built for Windows yet.

`rawr` alone also installs from source, with Rust 1.95 or newer:

```sh
cargo install --locked --git https://github.com/airstrike/rawr rawr-cli
```

A crate with dependencies needs `rust-toolchain.toml` and
`.cargo/config.toml`, so that `rawr run` and `cargo build` share one build.
`rawr run` prints both files when they are missing. A member of a Cargo
workspace needs neither, and `cargo build` then compiles its dependencies
separately. The [docs](https://rawr.airstrike.fyi/docs/imports) explain them.

## How do I start an iced app?

With rawr-import installed, make a crate and add iced to it:

```sh
rawr new counter && cd counter
cargo add iced --git https://github.com/iced-rs/iced --rev be88873d38544455e26c7a104eb757da2cbbac6b
```

Put iced's counter example in `src/main.rs` with `// rawr` as its first line,
and run `rawr run`. rawr-import reads iced's API and generates the bindings
the marked file calls. Edit `src/main.rs` while the window is open. A save
that passes the check reaches the running app at its next message. The app
keeps its state when the new code is compatible with it. `cargo run` compiles
the same app natively. The [iced docs](https://rawr.airstrike.fyi/docs/apps#iced)
say what works today.

## Can a coding agent use it?

Yes. `rawr mcp` is an MCP server for agents such as Claude Code and Codex. Run
`rawr init` in a project to register it. The agent then makes a crate with
`new`, checks it with `check`, and runs it with `run` or `start`. After each
save, `wait` says whether the program reloaded or failed. Each error comes with
its file and line. The [agents docs](https://rawr.airstrike.fyi/docs/agents)
list the tools.

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
It shows a program's output. It doesn't run iced programs yet. The `?play`
bot does the same in Discord channels where it is
turned on. [Add it to a server](https://rawr.airstrike.fyi/bot/invite).

## How far along is it?

Pre-alpha. rawr's API and commands may change.

rawr supports part of Rust. The
[Supported Rust](https://rawr.airstrike.fyi/docs/language) page lists what
works, what is missing, what rawr refuses and the few known cases that still
differ from Rust.

A native crate root may call marked modules, and a save reaches the running
program at its next call. The native `main` runs on the process's main
thread, so a winit app works. `examples/invaders` is a space-invaders game
whose rules and drawing reload while you play. A ratatui app works the same
way with no extra crate. `examples/tui` is a to-do list whose marked
`ui::draw` takes ratatui's `&mut Frame`, which rawr lends it for the call.
A save repaints the terminal at once, and rawr leaves the terminal as it
found it. The [ratatui docs](https://rawr.airstrike.fyi/docs/apps#ratatui) say
how. Planned and not built yet: calls from native modules other than the
root.

The examples are packages of this repository's Cargo workspace. From its
root, `cargo run -p invaders` plays the game natively, and `rawr run -p
invaders` plays it under rawr. [`examples/README.md`](examples/README.md)
lists them all.

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

rawr's evaluator began as a fork of [Rhai](https://github.com/rhaiscript/rhai).
The LICENSE files carry its attribution.
