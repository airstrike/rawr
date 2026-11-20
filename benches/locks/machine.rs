//! What a result was measured with and on: the build (embedded by
//! build.rs when it was compiled), the toolchain, the machine and its
//! load. Load is recorded, never used to refuse a run.

use serde_json::{json, Value};
use std::process::Command;

/// The build: its revision, whether its tree had changes and their
/// digest, its profile, whether it counts, and the binary's hash.
pub fn build() -> Value {
    use sha2::Digest as _;
    let hash = std::env::current_exe()
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .map(|bytes| hex(&sha2::Sha256::digest(bytes)));
    let changes = env!("LOCK_BENCH_CHANGES");
    json!({
        "revision": env!("LOCK_BENCH_REVISION"),
        "dirty": env!("LOCK_BENCH_DIRTY") == "true",
        "changes": (!changes.is_empty()).then_some(changes),
        "profile": env!("LOCK_BENCH_PROFILE"),
        "opt_level": env!("LOCK_BENCH_OPT_LEVEL"),
        "debug_assertions": cfg!(debug_assertions),
        "counters": cfg!(feature = "counters"),
        "rawr": rawr::VERSION,
        "binary_sha256": hash,
    })
}

/// The toolchain on `PATH`, which `cargo bench` built with unless a
/// toolchain was named.
pub fn toolchain() -> Value {
    json!({
        "rustc": output(Command::new("rustc").arg("-V")),
        "cargo": output(Command::new("cargo").arg("-V")),
    })
}

/// The machine: its system, processor and how many threads it runs.
pub fn machine() -> Value {
    let processor = if cfg!(target_os = "macos") {
        output(Command::new("sysctl").args(["-n", "machdep.cpu.brand_string"]))
    } else {
        std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|info| {
                info.lines()
                    .find_map(|line| line.strip_prefix("model name")?.split_once(':'))
                    .map(|(_, name)| name.trim().to_owned())
            })
    };
    json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "processor": processor,
        "model": output(Command::new("sysctl").args(["-n", "hw.model"])),
        "threads": std::thread::available_parallelism().ok().map(std::num::NonZero::get),
    })
}

/// The 1, 5 and 15 minute load averages now.
pub fn load() -> Value {
    let text = if cfg!(target_os = "macos") {
        output(Command::new("sysctl").args(["-n", "vm.loadavg"]))
    } else {
        std::fs::read_to_string("/proc/loadavg").ok()
    };
    let averages: Vec<f64> = text
        .as_deref()
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|field| field.parse().ok())
        .take(3)
        .collect();
    json!(averages)
}

/// Now, in UTC, as `YYYYMMDDTHHMMSSZ`.
pub fn utc_now() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let (days, time) = (seconds / 86_400, seconds % 86_400);
    let (year, month, day) = civil(days as i64);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        time / 3600,
        time / 60 % 60,
        time % 60
    )
}

/// The proleptic Gregorian date `days` after 1970-01-01 (Howard Hinnant's
/// `civil_from_days`).
fn civil(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let of_era = shifted.rem_euclid(146_097);
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * of_year + 2) / 153;
    let day = (of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The trimmed standard output of `command`, when it ran and succeeded.
fn output(command: &mut Command) -> Option<String> {
    let output = command
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// `bytes` in lowercase hexadecimal.
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}
