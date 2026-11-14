//! Whether rawr colours what it prints on stderr, by cargo's rules: the
//! `--color <when>` option (`auto`, `always` or `never`), else
//! `CARGO_TERM_COLOR`, else `auto`. `auto` colours a terminal, as
//! `anstream` decides for cargo: `NO_COLOR` turns colour off,
//! `CLICOLOR_FORCE` on, `CLICOLOR=0` off, and a terminal whose `TERM` is
//! `dumb` gets none.
//!
//! The option goes on to each `rawr` process this one starts as
//! `CARGO_TERM_COLOR`, which is how a process `rawr` hands a program to
//! reads it.

use std::ffi::OsString;
use std::io::IsTerminal as _;
use std::process::Command;
use std::sync::OnceLock;

use rawr::diagnostic::Color;

/// The variable cargo reads its colour choice from.
const VARIABLE: &str = "CARGO_TERM_COLOR";

/// A colour choice as cargo takes it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Choice {
    /// Colour a terminal only.
    Auto,
    /// Colour always.
    Always,
    /// Never colour.
    Never,
}

impl Choice {
    fn new(word: &str) -> Result<Self, String> {
        match word {
            "auto" => Ok(Self::Auto),
            "always" => Ok(Self::Always),
            "never" => Ok(Self::Never),
            other => Err(format!(
                "argument for `--color` must be auto, always, or never, but found `{other}`"
            )),
        }
    }

    const fn word(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Always => "always",
            Self::Never => "never",
        }
    }
}

/// The choice `--color` gave this process, if it gave one.
static CHOSEN: OnceLock<Choice> = OnceLock::new();

/// The arguments without `--color <when>` or `--color=<when>` before the
/// first `--`, whose choice holds for the rest of this process.
///
/// # Errors
///
/// Returns cargo's message for a choice other than `auto`, `always` or
/// `never`, or a `--color` with nothing after it.
pub fn take(arguments: Vec<OsString>) -> Result<Vec<OsString>, String> {
    let (chosen, rest) = split(arguments)?;
    if let Some(choice) = chosen {
        // A second `take` in one process keeps the first choice.
        let _ = CHOSEN.set(choice);
    }
    Ok(rest)
}

/// The choice `--color` gives in `arguments`, and the arguments without it.
fn split(arguments: Vec<OsString>) -> Result<(Option<Choice>, Vec<OsString>), String> {
    let end = arguments
        .iter()
        .position(|argument| argument == "--")
        .unwrap_or(arguments.len());
    let mut rest = Vec::with_capacity(arguments.len());
    let mut chosen = None;
    let mut arguments = arguments.into_iter().enumerate();
    while let Some((index, argument)) = arguments.next() {
        if index >= end {
            rest.push(argument);
            continue;
        }
        let text = argument.to_string_lossy();
        if let Some(word) = text.strip_prefix("--color=") {
            chosen = Some(Choice::new(word)?);
        } else if text == "--color" {
            let word = arguments
                .next()
                .filter(|(index, _)| *index < end)
                .map(|(_, word)| word.to_string_lossy().into_owned())
                .ok_or("`--color` needs a value: auto, always, or never")?;
            chosen = Some(Choice::new(&word)?);
        } else {
            rest.push(argument);
        }
    }
    Ok((chosen, rest))
}

/// Hand this process's `--color` choice to `command`, a `rawr` process.
pub fn hand(command: &mut Command) {
    if let Some(choice) = CHOSEN.get() {
        command.env(VARIABLE, choice.word());
    }
}

/// Whether what this process prints on stderr is coloured.
#[must_use]
pub fn stderr() -> Color {
    let choice = CHOSEN.get().copied().or_else(|| {
        std::env::var(VARIABLE)
            .ok()
            .and_then(|word| Choice::new(&word).ok())
    });
    match choice.unwrap_or(Choice::Auto) {
        Choice::Always => Color::Always,
        Choice::Never => Color::Never,
        Choice::Auto => auto(std::io::stderr().is_terminal()),
    }
}

/// `auto` for a stream that is a terminal or not, as `anstream` decides.
fn auto(terminal: bool) -> Color {
    let set = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
    let clicolor = std::env::var_os("CLICOLOR");
    if set("NO_COLOR") {
        return Color::Never;
    }
    if set("CLICOLOR_FORCE") {
        return Color::Always;
    }
    if clicolor.as_deref().is_some_and(|value| value == "0") {
        return Color::Never;
    }
    let dumb = std::env::var_os("TERM").is_none_or(|term| term == "dumb");
    if terminal && (!dumb || clicolor.is_some() || set("CI")) {
        Color::Always
    } else {
        Color::Never
    }
}

/// `text` in the style cargo gives `style`'s lines, when `color` colours.
#[must_use]
pub fn paint(text: &str, style: Style, color: Color) -> String {
    match color {
        Color::Never => text.to_owned(),
        Color::Always => format!("{}{text}\u{1b}[0m", style.escape()),
    }
}

/// A style of cargo's own lines.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Style {
    /// `error`: bold bright red.
    Error,
    /// `warning`: bold yellow.
    Warning,
    /// A status, as `Checking` and `Finished`: bold bright green.
    Status,
    /// A headline's message, after `error: `: bold.
    Emphasis,
}

impl Style {
    const fn escape(self) -> &'static str {
        match self {
            Self::Error => "\u{1b}[1m\u{1b}[91m",
            Self::Warning => "\u{1b}[1m\u{1b}[33m",
            Self::Status => "\u{1b}[1m\u{1b}[92m",
            Self::Emphasis => "\u{1b}[1m",
        }
    }
}

/// The level a line rawr prints on stderr starts with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Level {
    /// `error`.
    Error,
    /// `warning`.
    Warning,
}

/// The word `level` starts a line on stderr with, before its `: `, in
/// cargo's colours when stderr takes them: `error` as cargo prints its
/// own errors.
#[must_use]
pub fn label(level: Level) -> String {
    match level {
        Level::Error => paint("error", Style::Error, stderr()),
        Level::Warning => paint("warning", Style::Warning, stderr()),
    }
}

#[cfg(test)]
mod tests {
    use super::{paint, split, Choice, Style};
    use rawr::diagnostic::Color;

    #[test]
    fn color_is_taken_from_the_arguments_before_the_program_s() {
        let arguments = |words: &[&str]| words.iter().map(Into::into).collect::<Vec<_>>();
        assert_eq!(
            split(arguments(&["check", "--color", "never", "."])),
            Ok((Some(Choice::Never), arguments(&["check", "."])))
        );
        assert_eq!(
            split(arguments(&["run", "--color=always", "--", "--color", "x"])),
            Ok((
                Some(Choice::Always),
                arguments(&["run", "--", "--color", "x"])
            ))
        );
        assert_eq!(
            split(arguments(&["check"])),
            Ok((None, arguments(&["check"])))
        );
        assert!(split(arguments(&["check", "--color=sometimes"])).is_err());
        assert!(split(arguments(&["check", "--color"])).is_err());
    }

    #[test]
    fn paint_colours_only_when_asked() {
        assert_eq!(paint("error", Style::Error, Color::Never), "error");
        assert_eq!(
            paint("error", Style::Error, Color::Always),
            "\u{1b}[1m\u{1b}[91merror\u{1b}[0m"
        );
    }
}
