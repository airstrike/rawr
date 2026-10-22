Rawr Tools
==========

Tools for working with Rawr scripts.

| Tool                                                                             | Required feature(s) | Description                                           |
| -------------------------------------------------------------------------------- | :-----------------: | ----------------------------------------------------- |
| [`rawr-run`](https://github.com/airstrike/rawr/blob/main/src/bin/rawr-run.rs)   |                     | runs each filename passed to it as a Rawr script      |
| [`rawr-repl`](https://github.com/airstrike/rawr/blob/main/src/bin/rawr-repl.rs) |     `rustyline`     | a simple REPL that interactively evaluates statements |
| [`rawr-dbg`](https://github.com/airstrike/rawr/blob/main/src/bin/rawr-dbg.rs)   |     `debugging`     | the _Rawr Debugger_                                   |

For convenience, a feature named `bin-features` is available which is a combination of the following:

* `decimal` &ndash; support for decimal numbers
* `metadata` &ndash; access functions metadata
* `serde` &ndash; export functions metadata to JSON
* `debugging` &ndash; required by `rawr-dbg`
* `rustyline` &ndash; required by `rawr-repl`


How to Run
----------

```sh
cargo run --features bin-features --bin sample_app_to_run
```


How to Install
--------------

To install these all tools (with full features), use the following command:

```sh
cargo install --path . --bins  --features bin-features
```

or specifically:

```sh
cargo install --path . --bin sample_app_to_run  --features bin-features
```
