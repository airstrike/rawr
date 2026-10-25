# Examples

## Small Examples

Each is one Rust file that links the rawr crate and runs a script from
[`scripts`](scripts). They are the root crate's examples, so
`cargo run --example <name>` runs one from the repository's root.

| Example                                 | Description                                                     | Run                                   |
| --------------------------------------- | --------------------------------------------------------------- | ------------------------------------- |
| [`cart`](cart.rs)                       | loads [`scripts/cart.rs`](scripts/cart.rs) and calls its `main` | `cargo run --example cart`            |
| [`nominal_counter`](nominal_counter.rs) | evaluates a typed script with a struct, an enum and a method    | `cargo run --example nominal_counter` |

## Apps for `rawr run`

Each is a package of the repository's Cargo workspace, as iced's examples
are. From the repository's root, `cargo run -p <name>` builds and runs one
natively, on the workspace's toolchain. `rawr run -p <name>` interprets its
marked files, and `rawr run examples/<name>` does the same. `rawr check -p
<name>` checks it without running it.

| App                    | Description                                                                                                 | Natively                         | Under rawr                        |
| ---------------------- | ----------------------------------------------------------------------------------------------------------- | -------------------------------- | --------------------------------- |
| [`islands`](islands)   | a native `main` that calls one marked module each tick                                                      | `cargo run -p islands`           | `rawr run -p islands`             |
| [`grep`](grep)         | a marked command-line tool with flags, files and stdin                                                      | `cargo run -p grep -- -n pie`    | `rawr run -p grep -- -n pie`      |
| [`invaders`](invaders) | a space-invaders game on winit and softbuffer, whose rules and drawing are marked and reload while you play | `cargo run -p invaders`          | `rawr run -p invaders`            |
| [`tui`](tui)           | a to-do list on ratatui, whose keys and view model are marked and repaint the terminal on save             | `cargo run -p tui`               | `rawr run -p tui`                 |

`cargo build --release -p <name>` builds one with no rawr in it.

`rawr run` needs rawr-import for the apps with native code or dependencies.
The repository's README says how to install it. A workspace member needs no
`rust-toolchain.toml` of its own: rawr builds it with rawr-import's pinned
nightly, and `cargo build` keeps the workspace's toolchain. rawr keeps what
it builds for a member in `target/rawr/members/examples/<name>`.

## An App That Embeds rawr

[`rules`](rules) is an app that adds the rawr crate. It totals bank
transactions by category, and its users write the categories in Rust. Run it
with `cargo run -p rules`. `scripts/embed-check.sh` checks what it prints.
