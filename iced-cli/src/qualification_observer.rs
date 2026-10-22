//! Explicitly built private observer; never installed or dispatched by Rawr.

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod qualification;

fn main() -> std::process::ExitCode {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    match qualification::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        eprintln!("qualification observer requires a reviewed aarch64 Darwin ABI");
        std::process::ExitCode::FAILURE
    }
}
