//! `rawr-play <file.rs|-> [--size WxH] [--theme dark|light] [--message M]...
//! [--format png|jpg|webp] [--time-ms N] [--memory-mb N] [--fonts DIR] -o <out|->`
//!
//! `--fonts DIR` draws with iced's embedded faces and the fonts in `DIR`
//! only, never the system's (the `?play` container's fonts; tests use it so
//! a Mac's fonts cannot hide a missing glyph).
//!
//! `-` reads the snippet from stdin or writes the result to stdout. The bot
//! runs one of these per render, so `--memory-mb` caps the whole process.
//!
//! A snippet that uses iced writes its image. A plain program writes its
//! output instead, as one JSON object (`rawr_play::Output::to_json`):
//! `{"kind":"text","status":0,"output":[{"stream":"stdout","text":".."}],..}`.
//! A result starting with `{` is text; no image starts with it. Errors
//! (diagnostics, bad options) go to stderr with exit status 1, as before.
//!
//! `rawr-play decrypt-server <token>...` prints the guild ID behind each
//! server token on the bot's dashboard, and `rawr-play encrypt-server <id>...`
//! the token for a guild ID, both with the key in `~/.config/rawr-play/log-key`
//! (or the file at `PLAY_LOG_KEY_FILE`).

use std::io::{Read, Write};
use std::process::ExitCode;

const USAGE: &str = "usage: rawr-play <file.rs|-> [--size WxH] [--theme dark|light] \
[--message EXPR]... [--format png|jpg|webp] [--time-ms N] [--memory-mb N] [--fonts DIR] -o <out|->";

/// Print the guild ID behind each token, or the token for each guild ID.
fn server_tokens(decrypt: bool, values: &[String]) -> Result<(), String> {
    if values.is_empty() {
        return Err("usage: rawr-play decrypt-server <token>... | encrypt-server <guild id>...".into());
    }
    let path = std::env::var_os("PLAY_LOG_KEY_FILE")
        .map(std::path::PathBuf::from)
        .or_else(rawr_play::server::ServerKey::default_path)
        .ok_or("set HOME or PLAY_LOG_KEY_FILE")?;
    let key = rawr_play::server::ServerKey::from_file(&path)?;
    for value in values {
        if decrypt {
            println!("{value} {}", key.decrypt(value)?);
        } else {
            let id: u64 = value.parse().map_err(|_| format!("not a guild ID: {value}"))?;
            println!("{id} {}", key.encrypt(id));
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    if let Some(command @ ("decrypt-server" | "encrypt-server")) = args.first().map(String::as_str) {
        return server_tokens(command == "decrypt-server", &args[1..]);
    }
    let mut input = None;
    let mut output = None;
    let mut format = None;
    let mut options = rawr_play::Options::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .ok_or_else(|| format!("{name} needs a value\n{USAGE}"))
        };
        let number = |text: String| {
            text.parse::<u64>()
                .map_err(|_| format!("`{text}` is not a number\n{USAGE}"))
        };
        match arg.as_str() {
            "-o" | "--output" => output = Some(value("-o")?),
            "--size" => options.size = Some(rawr_play::parse_size(&value("--size")?)?),
            "--theme" => options.theme = value("--theme")?.parse()?,
            "--message" => options.messages.push(value("--message")?),
            "--format" => format = Some(value("--format")?.parse()?),
            "--time-ms" => {
                options.limits.time = std::time::Duration::from_millis(number(value("--time-ms")?)?);
            }
            "--memory-mb" => limit_memory(number(value("--memory-mb")?)?)?,
            "--fonts" => {
                let dir = value("--fonts")?;
                rawr_iced::snapshot::fonts::use_only(std::path::Path::new(&dir))?;
            }
            "-h" | "--help" => return Err(USAGE.to_owned()),
            _ if arg.starts_with('-') && arg != "-" => {
                return Err(format!("unknown option `{arg}`\n{USAGE}"));
            }
            _ if input.is_none() => input = Some(arg),
            _ => return Err(format!("unexpected argument `{arg}`\n{USAGE}")),
        }
    }
    let input = input.ok_or(USAGE)?;
    let output = output.ok_or(USAGE)?;
    let format = match format {
        Some(format) => format,
        None => output
            .rsplit_once('.')
            .and_then(|(_, extension)| extension.parse().ok())
            .unwrap_or_default(),
    };
    let mut source = String::new();
    let read = if input == "-" {
        std::io::stdin().read_to_string(&mut source).map(drop)
    } else {
        std::fs::read_to_string(&input).map(|text| source = text)
    };
    read.map_err(|error| format!("cannot read {input}: {error}"))?;
    let (bytes, summary) = match rawr_play::run(&source, &options).map_err(|error| error.to_string())? {
        rawr_play::Outcome::Image(png) => {
            let bytes = rawr_play::encode(&png, format).map_err(|error| error.to_string())?;
            let summary = format!("{}x{}, {} bytes", png.width, png.height, bytes.len());
            (bytes, summary)
        }
        rawr_play::Outcome::Text(output) => {
            let bytes = output.to_json().into_bytes();
            let summary = format!("text, exit status {}, {} bytes", output.status, bytes.len());
            (bytes, summary)
        }
    };
    if output == "-" {
        let mut stdout = std::io::stdout().lock();
        stdout
            .write_all(&bytes)
            .and_then(|()| stdout.flush())
            .map_err(|error| format!("cannot write the result: {error}"))?;
    } else {
        std::fs::write(&output, &bytes)
            .map_err(|error| format!("cannot write {output}: {error}"))?;
        println!("{output} ({summary})");
    }
    Ok(())
}

/// Cap this process's address space, so an allocation past it fails and the
/// process aborts instead of exhausting the host.
#[cfg(target_os = "linux")]
fn limit_memory(megabytes: u64) -> Result<(), String> {
    let bytes = megabytes.saturating_mul(1024 * 1024);
    let limit = libc::rlimit {
        rlim_cur: bytes,
        rlim_max: bytes,
    };
    // SAFETY: setrlimit reads a valid rlimit for the calling process.
    if unsafe { libc::setrlimit(libc::RLIMIT_AS, &limit) } == 0 {
        Ok(())
    } else {
        Err(format!("cannot limit memory: {}", std::io::Error::last_os_error()))
    }
}

/// macOS does not enforce `RLIMIT_AS`; the cap applies on Linux hosts.
#[cfg(not(target_os = "linux"))]
fn limit_memory(_megabytes: u64) -> Result<(), String> {
    Ok(())
}
