//! `rawr-play <file.rs|-> [--size WxH] [--theme dark|light] [--message M]...
//! [--step N[=TEXT]]... [--format png|jpg|webp] [--still] [--time-ms N] [--animation-ms N] [--memory-mb N] [--fonts DIR] -o <out|->`
//!
//! `--step N` clicks a full app's target `N` after its messages, counting
//! from 0 in the targets of the view as it then is. `--step N=TEXT` types
//! `TEXT` over text input `N`'s content and taps ⏎. A full app's image
//! comes with an `action: <label>` line for each target of its last frame
//! (`input: <label>` for a text input), in that order, and a
//! `click: <label>` line for each step's target; they go where the note
//! goes.
//!
//! `--time-ms N` is the wall-clock budget for a render. `--animation-ms N`
//! is a full app's budget once its view changes (never less than
//! `--time-ms`).
//!
//! `--fonts DIR` draws with iced's embedded faces and the fonts in `DIR`
//! only, never the system's (the `?play` container's fonts; tests use it so
//! a Mac's fonts cannot hide a missing glyph).
//!
//! `-` reads the snippet from stdin or writes the result to stdout. The bot
//! runs one of these per render, so `--memory-mb` caps the whole process.
//!
//! A snippet that uses iced writes its image. A full app whose view changes
//! writes a GIF in any format, and a file named `.png`, `.jpg` or `.webp`
//! is written with a `.gif` extension instead. `--still` writes the last
//! frame as a still image. A note on the image (such as why an animation
//! was cut) goes to stderr as a `note: ` line when writing to stdout. A
//! plain program writes its
//! output instead, as one JSON object (`rawr_play::Output::to_json`):
//! `{"kind":"text","status":0,"output":[{"stream":"stdout","text":".."}],..}`.
//! A result starting with `{` is text; no image starts with it. Errors
//! (diagnostics, bad options) go to stderr with exit status 1, as before.
//!
//! `SIGUSR1` stops an animation's recording before its next frame. The
//! frames so far are kept, and the note says someone else was waiting.
//!
//! `rawr-play decrypt-server <token>...` prints the guild ID behind each
//! server token on the bot's dashboard, and `rawr-play encrypt-server <id>...`
//! the token for a guild ID, both with the key in `~/.config/rawr-play/log-key`
//! (or the file at `PLAY_LOG_KEY_FILE`).

use std::io::{Read, Write};
use std::process::ExitCode;

const USAGE: &str = "usage: rawr-play <file.rs|-> [--size WxH] [--theme dark|light] \
[--message EXPR]... [--step N[=TEXT]]... [--format png|jpg|webp] [--still] [--time-ms N] [--animation-ms N] [--memory-mb N] [--fonts DIR] -o <out|->";

/// Print the guild ID behind each token, or the token for each guild ID.
fn server_tokens(decrypt: bool, values: &[String]) -> Result<(), String> {
    if values.is_empty() {
        return Err(
            "usage: rawr-play decrypt-server <token>... | encrypt-server <guild id>...".into(),
        );
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
            let id: u64 = value
                .parse()
                .map_err(|_| format!("not a guild ID: {value}"))?;
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
    if let Some(command @ ("decrypt-server" | "encrypt-server")) = args.first().map(String::as_str)
    {
        return server_tokens(command == "decrypt-server", &args[1..]);
    }
    stop_on_signal();
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
            "--step" => {
                let step = rawr_play::message::parse_step(&value("--step")?)?;
                options.steps.push(step);
            }
            "--format" => format = Some(value("--format")?.parse()?),
            "--still" => options.still = true,
            "--time-ms" => {
                options.limits.time =
                    std::time::Duration::from_millis(number(value("--time-ms")?)?);
            }
            "--animation-ms" => {
                options.limits.animation =
                    std::time::Duration::from_millis(number(value("--animation-ms")?)?);
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
    let mut output = output;
    let mut lines = Vec::new();
    let (bytes, summary) = match rawr_play::run(&source, &options)
        .map_err(|error| error.to_string())?
    {
        rawr_play::Outcome::Image(image) => {
            lines =
                rawr_play::message::lines(image.note.as_deref(), &image.actions, &image.clicked);
            match image.picture {
                rawr_play::Picture::Still(png) => {
                    let bytes =
                        rawr_play::encode(&png, format).map_err(|error| error.to_string())?;
                    let summary = format!("{}x{}, {} bytes", png.width, png.height, bytes.len());
                    (bytes, summary)
                }
                rawr_play::Picture::Animated(gif) => {
                    if let Some((stem, "png" | "jpg" | "jpeg" | "webp")) = output.rsplit_once('.') {
                        output = format!("{stem}.gif");
                    }
                    let play = if gif.looping { "loops" } else { "plays once" };
                    let summary = format!(
                        "GIF {}x{}, {} frames, {play}, {} bytes",
                        gif.width,
                        gif.height,
                        gif.frames,
                        gif.bytes.len()
                    );
                    (gif.bytes, summary)
                }
            }
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
        for line in lines {
            eprintln!("{line}");
        }
    } else {
        std::fs::write(&output, &bytes)
            .map_err(|error| format!("cannot write {output}: {error}"))?;
        println!("{output} ({summary})");
        for line in lines {
            println!("{line}");
        }
    }
    Ok(())
}

/// Set [`rawr_play::CUT`] on `SIGUSR1`: the bot asks a long animation to
/// stop and keep its frames.
#[cfg(unix)]
fn stop_on_signal() {
    extern "C" fn cut(_: libc::c_int) {
        rawr_play::CUT.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    // SAFETY: the handler only stores to an atomic, which is
    // async-signal-safe.
    unsafe {
        libc::signal(
            libc::SIGUSR1,
            cut as extern "C" fn(libc::c_int) as libc::sighandler_t,
        );
    }
}

#[cfg(not(unix))]
fn stop_on_signal() {}

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
        Err(format!(
            "cannot limit memory: {}",
            std::io::Error::last_os_error()
        ))
    }
}

/// macOS does not enforce `RLIMIT_AS`; the cap applies on Linux hosts.
#[cfg(not(target_os = "linux"))]
fn limit_memory(_megabytes: u64) -> Result<(), String> {
    Ok(())
}
