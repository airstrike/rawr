//! `cargo rawr <args>` runs `rawr <args>`. Cargo starts this binary as
//! `cargo-rawr rawr <args>`: the `rawr` it passes first is dropped.

#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    let mut arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.first().is_some_and(|word| word == "rawr") {
        arguments.remove(0);
    }
    rawr_cli::main(arguments)
}
