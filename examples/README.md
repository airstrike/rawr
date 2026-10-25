# Sample Applications

## Small Examples

Each is one Rust file that links the rawr crate and runs a script from
[`scripts`](scripts).

| Example                                 | Description                                                     |
| --------------------------------------- | --------------------------------------------------------------- |
| [`cart`](cart.rs)                       | loads [`scripts/cart.rs`](scripts/cart.rs) and calls its `main` |
| [`nominal_counter`](nominal_counter.rs) | evaluates a typed script with a struct, an enum and a method    |

## Apps for `rawr run`

Each is a Cargo crate with its own workspace. `rawr run examples/<name>`
interprets its marked files, and `cargo build --release` builds it natively.

| App                     | Description                                                                                                  |
| ----------------------- | ------------------------------------------------------------------------------------------------------------ |
| [`islands`](islands)    | a native `main` that calls one marked module each tick                                                       |
| [`grep`](grep)          | a marked command-line tool with flags, files and stdin                                                       |
| [`invaders`](invaders)  | a space-invaders game on winit and softbuffer, whose rules and drawing are marked and reload while you play |
| [`tui`](tui)            | a to-do list on ratatui, whose keys and view model are marked and repaint the terminal on save              |

## An App That Embeds rawr

[`rules`](rules) is an app with its own Cargo workspace. It totals bank
transactions by category, and its users write the categories in Rust. Run it
with `cargo run --manifest-path examples/rules/Cargo.toml`.
`scripts/embed-check.sh` checks what it prints.

## Running Examples

Examples can be run with the following command:

```sh
cargo run --example {example_name}
```
