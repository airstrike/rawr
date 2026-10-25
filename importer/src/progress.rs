//! One status line on stderr while a cold launch builds its Rust dependencies.
//!
//! The line carries one overall bar for the whole launch. Each stage owns a
//! weight: the previous cold launch's stage durations for this project (kept in
//! `<project>/target/rawr/cache/progress`), else [`DEFAULT_WEIGHTS`]. Within a stage,
//! progress is 90% of the crate fraction (finished crates over the unit-graph
//! total, else over the previous launch's count for that stage), with the gap
//! to the stage end closed by a time fraction (elapsed over the previous
//! launch's stage duration, else an easing toward [`DEFAULT_SECONDS`]). It never
//! reaches the stage end on time alone, and a slow tail of large crates still
//! advances. Counts come from Cargo's own output: one stdout
//! JSON line per finished unit (`--message-format json`), or one `Compiling`
//! stderr line per crate. The shown percent never goes backwards. A launch whose
//! first stage settles within [`QUIET`] prints nothing, so warm launches stay
//! silent; once shown, the line stays up until the window appears or the launch
//! fails.

use std::io::{IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const STAGES: [&str; 4] = [
    "resolving dependencies",
    "building API",
    "building runner",
    "starting",
];
/// First-launch stage weights: resolve, build API, build runner, start.
const DEFAULT_WEIGHTS: [f64; 4] = [0.10, 0.45, 0.40, 0.05];
/// First-launch stage time estimates, in seconds, when no record exists.
const DEFAULT_SECONDS: [f64; 4] = [20.0, 150.0, 120.0, 10.0];
/// Crates alone carry a stage this far; time closes the rest.
const CRATE_SHARE: f64 = 0.9;
/// Time alone never carries a stage past this.
const TIME_CAP: f64 = 0.98;
const QUIET: Duration = Duration::from_secs(1);
const TICK: Duration = Duration::from_millis(250);

/// One stage as a launch saw it: crates finished and how long it took.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Record {
    crates: usize,
    millis: u64,
}

struct State {
    launch: Option<Instant>,
    stage: usize,
    since: Option<Instant>,
    lines: usize,
    compiling: usize,
    units: Option<usize>,
    tick: usize,
    drawn: bool,
    announced: usize,
    notes: Vec<String>,
    file: Option<PathBuf>,
    previous: Option<[Record; 4]>,
    current: [Record; 4],
    compiled: bool,
    shown: f64,
}

const RECORD: Record = Record {
    crates: 0,
    millis: 0,
};

static STATE: Mutex<State> = Mutex::new(State::new());

fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Start showing progress for one launch of the project at `project` until
/// [`finish`].
pub fn begin(project: &Path) {
    let file = project.join("target/rawr/cache/progress");
    let previous = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| parse(&text));
    let mut state = lock();
    let notes = std::mem::take(&mut state.notes);
    *state = State::new();
    state.notes = notes;
    state.start(Instant::now(), Some(file), previous);
    drop(state);
    std::thread::spawn(|| loop {
        std::thread::sleep(TICK);
        let mut state = lock();
        if state.launch.is_none() {
            return;
        }
        draw(&mut state);
    });
}

/// Clear the status line: the window is up (`success`), or the launch failed.
/// A successful launch that compiled anything records its stages for the next.
pub fn finish(success: bool) {
    let mut state = lock();
    if state.launch.is_some() {
        state.close(Instant::now());
        if let Some(file) = state.file.as_ref().filter(|_| success && state.compiled) {
            if let Some(parent) = file.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(file, format(&state.current));
        }
    }
    clear(&mut state);
    state.launch = None;
}

/// Print `line` once per process, above the status line.
pub fn note(key: &str, line: &str) {
    let mut state = lock();
    if state.notes.iter().any(|seen| seen == key) {
        return;
    }
    state.notes.push(key.into());
    let shown = state.drawn;
    clear(&mut state);
    crate::process::say(line);
    if shown {
        state.drawn = true;
        draw(&mut state);
    }
}

/// Enter `stage` (1-based); stages only move forward.
pub(crate) fn stage(stage: usize) {
    lock().advance(stage, Instant::now());
}

/// Record the planned unit count from a `--unit-graph` listing.
pub(crate) fn plan(unit_graph: &[u8]) {
    let units = count(unit_graph, b"\"pkg_id\"");
    if units > 0 {
        lock().units = Some(units);
    }
}

/// Observe one chunk of a building Cargo child's stdout or stderr.
pub(crate) fn observe(stdout: bool, chunk: &[u8]) {
    let mut state = lock();
    if stdout {
        state.lines += count(chunk, b"\n");
    } else {
        state.compiling += count(chunk, b"Compiling ");
    }
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}

impl State {
    const fn new() -> Self {
        Self {
            launch: None,
            stage: 0,
            since: None,
            lines: 0,
            compiling: 0,
            units: None,
            tick: 0,
            drawn: false,
            announced: 0,
            notes: Vec::new(),
            file: None,
            previous: None,
            current: [RECORD; 4],
            compiled: false,
            shown: 0.0,
        }
    }

    fn start(&mut self, now: Instant, file: Option<PathBuf>, previous: Option<[Record; 4]>) {
        self.launch = Some(now);
        self.stage = 1;
        self.since = Some(now);
        self.file = file;
        self.previous = previous;
    }

    /// Close the current stage's record at `now`.
    fn close(&mut self, now: Instant) {
        let Some(since) = self.since else { return };
        let index = self.stage.clamp(1, STAGES.len()) - 1;
        self.current[index] = Record {
            crates: self.lines.max(self.compiling),
            millis: u64::try_from(now.saturating_duration_since(since).as_millis())
                .unwrap_or(u64::MAX),
        };
        self.compiled |= self.compiling > 0;
    }

    fn advance(&mut self, stage: usize, now: Instant) {
        if self.launch.is_some() && stage > self.stage && stage <= STAGES.len() {
            self.close(now);
            self.stage = stage;
            self.since = Some(now);
            self.lines = 0;
            self.compiling = 0;
        }
    }

    /// Fraction of the current stage done at `now`, in `[0, 1)`.
    fn fraction(&self, now: Instant) -> f64 {
        let index = self.stage.clamp(1, STAGES.len()) - 1;
        let previous = self.previous.map(|records| records[index]);
        let total = (self.stage == 2)
            .then_some(self.units)
            .flatten()
            .or(previous.map(|record| record.crates))
            .filter(|total| *total > 0);
        // Cargo may report more finished units than the plan listed; cap rather
        // than dropping the bar.
        let crates = total.map_or(0.0, |total| {
            (self.lines.max(self.compiling) as f64 / total as f64).min(1.0) * CRATE_SHARE
        });
        let elapsed = self.since.map_or(0.0, |since| {
            now.saturating_duration_since(since).as_secs_f64()
        });
        let time = match previous.filter(|record| record.millis > 0) {
            Some(record) => (elapsed / (record.millis as f64 / 1000.0)).min(1.0),
            None => 1.0 - (-elapsed / DEFAULT_SECONDS[index]).exp(),
        };
        // At least max(crates, time × cap), and moving whenever time passes.
        crates + (TIME_CAP - crates).max(0.0) * time
    }

    /// Overall launch fraction at `now`, never lower than any shown before.
    fn percent(&mut self, now: Instant) -> f64 {
        let overall = overall(self.stage, self.fraction(now), &weights(self.previous));
        self.shown = self.shown.max(overall).min(1.0);
        self.shown
    }
}

/// Stage weights from the previous launch's durations, else the defaults.
fn weights(previous: Option<[Record; 4]>) -> [f64; 4] {
    let Some(records) = previous else {
        return DEFAULT_WEIGHTS;
    };
    let sum: u64 = records.iter().map(|record| record.millis).sum();
    if sum == 0 {
        return DEFAULT_WEIGHTS;
    }
    records.map(|record| record.millis as f64 / sum as f64)
}

/// Completed stage weights plus the current stage's weight times `fraction`.
fn overall(stage: usize, fraction: f64, weights: &[f64; 4]) -> f64 {
    let index = stage.clamp(1, STAGES.len()) - 1;
    let before: f64 = weights[..index].iter().sum();
    (before + weights[index] * fraction.clamp(0.0, 1.0)).clamp(0.0, 1.0)
}

fn format(records: &[Record; 4]) -> String {
    records
        .iter()
        .map(|record| format!("{} {}\n", record.crates, record.millis))
        .collect()
}

fn parse(text: &str) -> Option<[Record; 4]> {
    let mut records = [RECORD; 4];
    let mut lines = text.lines();
    for record in &mut records {
        let mut fields = lines.next()?.split_whitespace();
        *record = Record {
            crates: fields.next()?.parse().ok()?,
            millis: fields.next()?.parse().ok()?,
        };
    }
    Some(records)
}

fn draw(state: &mut State) {
    let (Some(launch), Some(since)) = (state.launch, state.since) else {
        return;
    };
    if !state.drawn && since.elapsed() < QUIET {
        return;
    }
    // A program that has the screen draws it alone.
    if crate::process::terminal::raw() {
        return;
    }
    let percent = state.percent(Instant::now());
    let mut stderr = std::io::stderr().lock();
    if stderr.is_terminal() {
        let width = rustix::termios::tcgetwinsize(&stderr).map_or(80, |size| size.ws_col);
        let utf8 = ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .find_map(|key| std::env::var(key).ok().filter(|value| !value.is_empty()))
            .is_some_and(|value| value.to_ascii_lowercase().replace('-', "").contains("utf8"));
        let style = Style {
            width: Some(usize::from(if width == 0 { 80 } else { width })),
            unicode: utf8,
            color: std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty()),
            truecolor: palette::truecolor(),
        };
        state.tick = state.tick.wrapping_add(1);
        let line = render(state.stage, percent, launch.elapsed(), state.tick, &style);
        let _ = write!(stderr, "\r\x1b[2K{line}");
        state.drawn = true;
    } else if state.announced < state.stage {
        let style = Style {
            width: None,
            unicode: false,
            color: false,
            truecolor: false,
        };
        let line = render(state.stage, percent, launch.elapsed(), 0, &style);
        let _ = writeln!(stderr, "{line}");
        state.announced = state.stage;
    }
    let _ = stderr.flush();
}

fn clear(state: &mut State) {
    if std::mem::take(&mut state.drawn) && !crate::process::terminal::raw() {
        let _ = write!(std::io::stderr(), "\r\x1b[2K");
    }
}

/// How one status line is drawn: `width` is `None` for plain (non-terminal) lines.
struct Style {
    width: Option<usize>,
    unicode: bool,
    color: bool,
    truecolor: bool,
}

/// The CLI's palette: hot pink, soft pink and grays.
pub mod palette {
    /// One terminal color: 24-bit RGB with a 256-color fallback.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Color {
        pub rgb: (u8, u8, u8),
        pub ansi256: u8,
    }

    /// #FF1493: the one colored swatch (spinner, bar fill).
    pub const HOT_PINK: Color = Color {
        rgb: (255, 20, 147),
        ansi256: 198,
    };
    /// #FF8FC8: the `rawr` mark and percentages.
    pub const SOFT_PINK: Color = Color {
        rgb: (255, 143, 200),
        ansi256: 211,
    };
    /// #C8C8C8: labels.
    pub const LIGHT_GRAY: Color = Color {
        rgb: (200, 200, 200),
        ansi256: 251,
    };
    /// #8A8A8A: secondary figures such as elapsed time.
    pub const MID_GRAY: Color = Color {
        rgb: (138, 138, 138),
        ansi256: 245,
    };
    /// #4A4A4A: tracks and empty space.
    pub const DARK_GRAY: Color = Color {
        rgb: (74, 74, 74),
        ansi256: 239,
    };

    impl Color {
        /// Wrap `text` in this foreground color (`bold` optional), then reset.
        #[must_use]
        pub fn paint(self, text: &str, bold: bool, truecolor: bool) -> String {
            let weight = if bold { "1;" } else { "" };
            let (r, g, b) = self.rgb;
            if truecolor {
                format!("\x1b[{weight}38;2;{r};{g};{b}m{text}\x1b[0m")
            } else {
                format!("\x1b[{weight}38;5;{}m{text}\x1b[0m", self.ansi256)
            }
        }
    }

    /// Whether the terminal advertises 24-bit color.
    #[must_use]
    pub fn truecolor() -> bool {
        std::env::var("COLORTERM")
            .is_ok_and(|value| matches!(value.as_str(), "truecolor" | "24bit"))
    }
}

fn render(stage: usize, percent: f64, elapsed: Duration, tick: usize, style: &Style) -> String {
    use palette::{DARK_GRAY, HOT_PINK, LIGHT_GRAY, MID_GRAY, SOFT_PINK};
    const SPIN: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let name = STAGES[stage.clamp(1, STAGES.len()) - 1];
    let percent = percent.clamp(0.0, 1.0);
    let secs = elapsed.as_secs();
    let time = if secs >= 60 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    };
    let share = format!("{:.0}%", (percent * 100.0).floor());
    let Some(width) = style.width else {
        return format!("rawr > {name}  {share}  {time}");
    };
    let spin = if style.unicode {
        SPIN[tick % SPIN.len()]
    } else {
        ['|', '/', '-', '\\'][tick % 4]
    };
    let spin = spin.to_string();
    // "rawr S name  " + bar + "  " + "P%  T"
    let used = 5 + 2 + name.chars().count() + 2 + share.chars().count() + 2 + time.chars().count();
    let cells = width.saturating_sub(used + 3).min(24);
    let filled = ((percent * cells as f64) as usize).min(cells);
    let (fill, empty) = if style.unicode {
        ('━', '─')
    } else {
        ('=', '-')
    };
    let bar: String = std::iter::repeat_n(fill, filled).collect();
    let rest: String = std::iter::repeat_n(empty, cells - filled).collect();
    if cells < 8 || !style.color {
        let middle = if cells < 8 {
            String::new()
        } else {
            format!("{bar}{rest}  ")
        };
        let line = format!("rawr {spin} {name}  {middle}{share}  {time}");
        return if line.chars().count() >= width {
            line.chars().take(width.saturating_sub(1)).collect()
        } else {
            line
        };
    }
    let tc = style.truecolor;
    format!(
        "{} {} {}  {}{}  {}  {}",
        SOFT_PINK.paint("rawr", true, tc),
        HOT_PINK.paint(&spin, false, tc),
        LIGHT_GRAY.paint(name, false, tc),
        HOT_PINK.paint(&bar, false, tc),
        DARK_GRAY.paint(&rest, false, tc),
        SOFT_PINK.paint(&share, false, tc),
        MID_GRAY.paint(&time, false, tc),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDE: Style = Style {
        width: Some(120),
        unicode: true,
        color: false,
        truecolor: false,
    };

    fn launch(previous: Option<[Record; 4]>) -> (State, Instant) {
        let now = Instant::now();
        let mut state = State::new();
        state.start(now, None, previous);
        (state, now)
    }

    #[test]
    fn renders_spinner_bar_and_plain_lines() {
        assert_eq!(count(b"{\"pkg_id\":1}{\"pkg_id\":2}", b"\"pkg_id\""), 2);
        assert_eq!(count(b"   Compiling a\n   Compiling b\n", b"Compiling "), 2);
        assert_eq!(
            render(3, 0.67, Duration::from_secs(108), 2, &WIDE),
            "rawr ⠹ building runner  ━━━━━━━━━━━━━━━━────────  67%  1m48s"
        );
        let colored = Style {
            width: Some(120),
            unicode: true,
            color: true,
            truecolor: true,
        };
        assert_eq!(
            render(3, 0.5, Duration::from_secs(9), 3, &colored),
            "\x1b[1;38;2;255;143;200mrawr\x1b[0m \x1b[38;2;255;20;147m⠸\x1b[0m \
             \x1b[38;2;200;200;200mbuilding runner\x1b[0m  \
             \x1b[38;2;255;20;147m━━━━━━━━━━━━\x1b[0m\x1b[38;2;74;74;74m────────────\x1b[0m  \
             \x1b[38;2;255;143;200m50%\x1b[0m  \x1b[38;2;138;138;138m9s\x1b[0m"
        );
        let colored = Style {
            truecolor: false,
            ..colored
        };
        assert!(render(3, 0.5, Duration::ZERO, 0, &colored).contains("\x1b[38;5;198m━"));
        let narrow = Style {
            width: Some(42),
            unicode: false,
            color: false,
            truecolor: false,
        };
        assert_eq!(
            render(2, 0.52, Duration::from_secs(72), 1, &narrow),
            "rawr / building API  ====----  52%  1m12s"
        );
        let tiny = Style {
            width: Some(20),
            unicode: false,
            color: false,
            truecolor: false,
        };
        assert_eq!(render(3, 0.1, Duration::ZERO, 0, &tiny).chars().count(), 19);
        let plain = Style {
            width: None,
            unicode: false,
            color: false,
            truecolor: false,
        };
        assert_eq!(
            render(1, 0.04, Duration::from_secs(4), 0, &plain),
            "rawr > resolving dependencies  4%  4s"
        );
    }

    #[test]
    fn overall_percent_sums_finished_stages_and_the_current_fraction() {
        assert!((overall(1, 0.5, &DEFAULT_WEIGHTS) - 0.05).abs() < 1e-9);
        assert!((overall(2, 0.5, &DEFAULT_WEIGHTS) - 0.325).abs() < 1e-9);
        assert!((overall(3, 0.0, &DEFAULT_WEIGHTS) - 0.55).abs() < 1e-9);
        assert!((overall(4, 1.0, &DEFAULT_WEIGHTS) - 1.0).abs() < 1e-9);
        assert!((overall(2, 7.0, &DEFAULT_WEIGHTS) - 0.55).abs() < 1e-9);

        let (mut state, now) = launch(None);
        state.advance(2, now);
        state.units = Some(200);
        state.lines = 100;
        assert!((state.percent(now) - (0.10 + 0.45 * 0.45)).abs() < 1e-9);
        // More finished units than the plan listed caps the crate share.
        state.lines = 284;
        assert!((state.percent(now) - (0.10 + 0.45 * 0.9)).abs() < 1e-9);
        let line = render(2, state.percent(now), Duration::ZERO, 0, &WIDE);
        assert!(line.contains('━') && line.contains("  50%"), "{line}");
    }

    #[test]
    fn unknown_totals_use_the_previous_count_else_ease_short_of_the_end() {
        let mut previous = [RECORD; 4];
        previous[2] = Record {
            crates: 100,
            millis: 100_000,
        };
        let (mut state, now) = launch(Some(previous));
        state.advance(3, now);
        state.compiling = 50;
        assert!((state.fraction(now) - 0.45).abs() < 1e-9);
        // Time over the previous duration closes the gap to the cap.
        let expected = 0.45 + (TIME_CAP - 0.45) * 0.6;
        assert!((state.fraction(now + Duration::from_secs(60)) - expected).abs() < 1e-9);
        assert_eq!(state.fraction(now + Duration::from_secs(600)), TIME_CAP);

        let (mut state, now) = launch(None);
        state.advance(3, now);
        let late = state.fraction(now + Duration::from_secs(6_000));
        assert_eq!(late, TIME_CAP);
        assert!(state.fraction(now + Duration::from_secs(30)) < late);
    }

    #[test]
    fn many_fast_crates_then_a_slow_tail_keeps_advancing() {
        let api = Record {
            crates: 283,
            millis: 264_000,
        };
        for previous in [None, Some([RECORD, api, RECORD, RECORD])] {
            let (mut state, now) = launch(previous);
            state.advance(2, now);
            state.units = Some(283);
            // Nearly every crate lands in the first 25 s...
            state.lines = 280;
            let mut last = state.percent(now + Duration::from_secs(25));
            // ...then a few large crates compile for minutes.
            for secs in (60..=240).step_by(30) {
                let shown = state.percent(now + Duration::from_secs(secs));
                assert!(
                    shown > last,
                    "stalled at {shown} after {secs}s ({previous:?})"
                );
                last = shown;
            }
            let end = overall(3, 0.0, &weights(previous));
            assert!(last < end, "time alone never reaches the stage end");
        }
    }

    #[test]
    fn shown_percent_never_goes_backwards() {
        let (mut state, now) = launch(None);
        state.advance(2, now);
        state.units = Some(100);
        state.lines = 80;
        let high = state.percent(now);
        // A larger plan arriving later would lower the raw fraction.
        state.units = Some(400);
        assert_eq!(state.percent(now), high);
        state.lines = 0;
        state.advance(1, now);
        assert_eq!(state.percent(now), high);
    }

    #[test]
    fn stage_transition_keeps_the_bar_and_records_the_stage() {
        let (mut state, now) = launch(None);
        state.drawn = true;
        state.advance(2, now);
        state.units = Some(10);
        state.compiling = 10;
        let end = state.percent(now);
        let later = now + Duration::from_secs(5);
        state.advance(3, later);
        assert!(state.drawn, "a stage change never clears the line");
        let next = state.percent(later);
        assert!(next >= end);
        assert!(render(3, next, Duration::ZERO, 0, &WIDE).contains('━'));
        assert_eq!(
            state.current[1],
            Record {
                crates: 10,
                millis: 5_000
            }
        );
        assert!(state.compiled);
    }

    #[test]
    fn learned_weights_round_trip() {
        let records = [
            Record {
                crates: 0,
                millis: 2_000,
            },
            Record {
                crates: 284,
                millis: 10_000,
            },
            Record {
                crates: 120,
                millis: 7_000,
            },
            Record {
                crates: 0,
                millis: 1_000,
            },
        ];
        assert_eq!(parse(&format(&records)), Some(records));
        assert_eq!(parse("1 2\n3 4\n"), None);
        assert_eq!(weights(None), DEFAULT_WEIGHTS);
        assert_eq!(weights(Some([RECORD; 4])), DEFAULT_WEIGHTS);
        assert_eq!(weights(Some(records)), [0.1, 0.5, 0.35, 0.05]);

        let (mut state, now) = launch(None);
        state.advance(2, now);
        state.compiling = 3;
        state.advance(3, now + Duration::from_millis(1_500));
        state.close(now + Duration::from_millis(2_000));
        let read = parse(&format(&state.current)).unwrap();
        assert_eq!(
            read[1],
            Record {
                crates: 3,
                millis: 1_500
            }
        );
        assert_eq!(read[2].millis, 500);
        let next = weights(Some(read));
        assert!((next[1] - 0.75).abs() < 1e-9);
    }
}
