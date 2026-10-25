# Sample Applications

## Standard Examples

| Example                                                   | Description                                                                                                                                         |
| --------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| [`arrays_and_structs`](arrays_and_structs.rs)             | shows how to register a Rust type and using it with arrays                                                                                          |
| [`callback`](callback.rs)                                 | shows how to store a Rawr closure and call it later within Rust                                                                                     |
| [`custom_types_and_methods`](custom_types_and_methods.rs) | shows how to export a Rust type, its fields and its methods to a typed script                                                                      |
| [`custom_types`](custom_types.rs)                         | shows how to register a Rust type and its functions using the `CustomType` trait                                                                    |
| [`grain_bench`](grain_bench.rs)                           | times the `grain` bytecode VM against the tree walker (requires the `grain` feature; run with `--release`)                                          |
| [`hello`](hello.rs)                                       | simple example that evaluates an expression and prints the result                                                                                   |
| [`nominal_counter`](nominal_counter.rs)                   | evaluates a typed script with a struct, an enum and a method                                                                                        |
| [`pause_and_resume`](pause_and_resume.rs)                 | shows how to pause/resume/stop an `Engine` running in a separate thread via an MPSC channel                                                         |
| [`reuse_scope`](reuse_scope.rs)                           | evaluates two pieces of code in separate runs, but using a common `Scope`                                                                           |
| [`serde`](serde.rs)                                       | example to serialize and deserialize Rust types with [`serde`](https://crates.io/crates/serde) (requires the `serde` feature)                       |
| [`simple_fn`](simple_fn.rs)                               | shows how to register a simple Rust function                                                                                                        |
| [`strings`](strings.rs)                                   | shows different ways to register Rust functions taking string arguments                                                                             |
| [`threading`](threading.rs)                               | shows how to communicate in duplex with an `Engine` running in a separate thread via a pair of MPSC channels                                        |

## Apps for `rawr run`

Each is a Cargo crate with its own workspace. `rawr run examples/<name>`
interprets its marked files, and `cargo build --release` builds it natively.

| App                     | Description                                                                                                  |
| ----------------------- | ------------------------------------------------------------------------------------------------------------ |
| [`islands`](islands)    | a native `main` that calls one marked module each tick                                                       |
| [`grep`](grep)          | a marked command-line tool with flags, files and stdin                                                       |
| [`invaders`](invaders)  | a space-invaders game on winit and softbuffer, whose rules and drawing are marked and reload while you play |

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
