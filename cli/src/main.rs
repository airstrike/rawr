#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    rawr_cli::main(std::env::args_os().skip(1).collect())
}
