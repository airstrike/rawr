Testing Scripts
===============

Sample scripts written in typed Rust. Each one is a list of statements, with any
items (`fn`, `struct`, `trait`, `impl`, `const`, `use`) it needs, and runs as if
it were the body of `fn main`.


Install `rawr-run` Tool
-----------------------

Use the following command to install the `rawr-run` tool:

```sh
cargo install --path . --bin rawr-run
```


How to Run
----------

Run scripts using the `rawr-run` tool:

```sh
rawr-run ./scripts/test_script_to_run.rs
```

or

```sh
cargo run --bin rawr-run ./scripts/test_script_to_run.rs
```

The timing scripts (`fibonacci`, `for2`, `mat_mul`, `primes`, `speed_test` and
`strings_map`) are meant for a release build:

```sh
cargo run --release --bin rawr-run ./scripts/fibonacci.rs
```

A debug build allows only 8 nested function calls, so `fibonacci` stops with a
stack overflow there.
