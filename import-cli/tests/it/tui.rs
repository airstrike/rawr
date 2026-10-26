//! A ratatui program whose `main` is native and whose view model is marked
//! (D534): a body edit swaps at its next call, and at a terminal the owner
//! wakes the program to draw it, holds rawr's lines while the program has
//! the terminal raw, and leaves the terminal as it found it however the
//! program ends. Build the driver first, as for `plain`.

use std::fs;
use std::io::{Read as _, Write as _};
use std::os::fd::{AsRawFd as _, OwnedFd};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::support::{copy, start_with};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/islands-tui");

fn fixture(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(FIXTURE), &root);
    root
}

fn edit(path: &Path, from: &str, to: &str) {
    let text = fs::read_to_string(path).unwrap();
    assert!(text.contains(from), "{text}");
    fs::write(path, text.replace(from, to)).unwrap();
}

/// The headless half of the fixture draws on ratatui's `TestBackend`, one
/// frame per line it reads. A body edit of the marked view changes the
/// next frame, and the app's state and the native `ListState` carry on.
#[test]
fn a_test_backend_frame_shows_a_body_edit_with_its_state_unbroken() {
    let root = fixture("islands-tui-headless");
    let app = root.join("src/app.rs");
    let mut run = start_with(
        &root,
        &[("TUI_HEADLESS", PathBuf::from("1"))],
        &[],
        Stdio::piped(),
    );
    let mut stdin = run.child.stdin.take().unwrap();
    let mut frame = |run: &mut crate::support::Run, line: &str, limit| {
        stdin.write_all(format!("{line}\n").as_bytes()).unwrap();
        let from = run.seen.len();
        run.until(limit, |line| line == "end");
        run.seen[from..run.seen.len() - 1]
            .iter()
            .filter(|line| line.starts_with('|'))
            .cloned()
            .collect::<Vec<_>>()
    };

    let first = frame(&mut run, "down", Duration::from_secs(900));
    assert_eq!(first[0], "|┌tui-one───────────────┐|", "{first:?}");
    assert_eq!(first[2], "|│> beta                │|", "{first:?}");
    let typed = frame(&mut run, "x", Duration::from_secs(30));
    assert_eq!(typed[6], "|│keys=1:x              │|", "{typed:?}");

    // An edit as the program starts may start it again (D531.9); this one
    // comes once it runs.
    std::thread::sleep(Duration::from_secs(3));
    edit(&app, "\"tui-one\"", "\"tui-two\"");
    let deadline = Instant::now() + Duration::from_secs(120);
    let swapped = loop {
        let next = frame(&mut run, "", Duration::from_secs(30));
        if next[0].contains("tui-two") {
            break next;
        }
        assert!(Instant::now() < deadline, "{}", run.seen.join("\n"));
        std::thread::sleep(Duration::from_millis(250));
    };
    let seen = run.seen.join("\n");
    assert_eq!(swapped[0], "|┌tui-two───────────────┐|", "{seen}");
    assert_eq!(swapped[2], "|│> beta                │|", "{seen}");
    assert_eq!(swapped[6], "|│keys=1:x              │|", "{seen}");
    assert!(
        !run.seen.iter().any(|line| line.contains("restarting")),
        "{}",
        run.seen.join("\n")
    );
}

const DRAW: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tui-draw");
const EXAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/tui/src");

/// The keys of each frame the comparison draws: one line a frame.
const KEYS: &[&str] = &[
    "",
    "down",
    "down down down",
    "a x y",
    "enter",
    "up space",
    "d",
];

/// `examples/tui`'s marked `app.rs` and `ui.rs` beside the headless
/// driver, which draws each frame on a `TestBackend`.
fn draw_fixture(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(DRAW), &root);
    for file in ["app.rs", "ui.rs"] {
        fs::copy(Path::new(EXAMPLE).join(file), root.join("src").join(file)).unwrap();
    }
    root
}

/// The frames in the driver's output: the lines before each `end`.
fn frames(output: &str) -> Vec<Vec<String>> {
    let mut frames = vec![Vec::new()];
    for line in output.lines() {
        if line == "end" {
            frames.push(Vec::new());
        } else if let Some(frame) = frames.last_mut() {
            frame.push(line.to_owned());
        }
    }
    frames.pop();
    frames
}

/// D558's first slice: `examples/tui`'s `ui::draw(frame: &mut Frame, app:
/// &mut App)`, idiomatic ratatui with a `Layout`, `Paragraph`s, `Block`s and
/// a stateful `List`, runs on the native `&mut Frame` lent to it. Every
/// frame's buffer, styles and cursor match `cargo run`'s, and a body edit of
/// `ui.rs` repaints the next frame with the app's state unbroken.
#[test]
fn a_lent_frame_draws_the_example_ui_as_cargo_run_does_and_a_body_edit_repaints() {
    let root = draw_fixture("tui-draw");
    let keys = KEYS
        .iter()
        .map(|keys| format!("{keys}\n"))
        .collect::<String>();
    let mut native = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    native
        .stdin
        .take()
        .unwrap()
        .write_all(keys.as_bytes())
        .unwrap();
    let native = native.wait_with_output().unwrap();
    assert!(native.status.success());
    let native = frames(&String::from_utf8(native.stdout).unwrap());
    assert_eq!(native.len(), KEYS.len());

    let mut run = start_with(&root, &[], &[], Stdio::piped());
    let mut stdin = run.child.stdin.take().unwrap();
    let mut frame = |run: &mut crate::support::Run, keys: &str, limit| {
        stdin.write_all(format!("{keys}\n").as_bytes()).unwrap();
        let from = run.seen.len();
        run.until(limit, |line| line == "end");
        run.seen[from..run.seen.len() - 1]
            .iter()
            .filter(|line| !line.starts_with("stderr: "))
            .cloned()
            .collect::<Vec<_>>()
    };
    for (index, keys) in KEYS.iter().enumerate() {
        let drawn = frame(&mut run, keys, Duration::from_secs(900));
        assert_eq!(drawn, native[index], "frame {index} after `{keys}`");
    }
    let last = native.last().unwrap();

    // An edit as the program starts may start it again (D531.9); this one
    // comes once it runs.
    std::thread::sleep(Duration::from_secs(3));
    edit(&root.join("src/ui.rs"), "\" New to-do \"", "\" New task \"");
    let deadline = Instant::now() + Duration::from_secs(120);
    let swapped = loop {
        let next = frame(&mut run, "", Duration::from_secs(30));
        if next[0].contains("New task") {
            break next;
        }
        assert!(Instant::now() < deadline, "{}", run.seen.join("\n"));
        std::thread::sleep(Duration::from_millis(250));
    };
    let seen = run.seen.join("\n");
    assert_eq!(
        swapped[0],
        last[0].replace(" New to-do ", " New task ─"),
        "{seen}"
    );
    // The list and its selection are as they were.
    assert_eq!(swapped[1..10], last[1..10], "{seen}");
    assert!(
        !run.seen.iter().any(|line| line.contains("restarting")),
        "{seen}"
    );
}

const STATE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tui-state");

/// A value native code lent a marked function goes on to a native call as
/// an argument, as it does as a receiver: `main` keeps a `ListState` and
/// passes `&mut state` with the frame to a marked `ui::draw`, which hands
/// it to `render_stateful_widget`. The frames and the state the draws
/// leave are what `cargo run` prints.
#[test]
fn a_lent_list_state_goes_to_render_stateful_widget_as_cargo_run_does() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("tui-state");
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(STATE), &root);
    let native = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    assert!(native.status.success(), "{}", text(&native.stderr));
    let expected = text(&native.stdout);
    assert!(expected.contains("|│> delta       │|"), "{expected}");
    assert!(expected.contains("selected Some(4) offset 2"), "{expected}");

    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let run = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(&root)
        .env("RAWR_HOME", directory.join("rawr-home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&run.stdout),
        text(&run.stderr)
    );
    assert_eq!(
        text(&run.stdout),
        expected,
        "stderr:\n{}",
        text(&run.stderr)
    );
}

/// What a terminal shows, from the bytes written to it: the alternate
/// screen's cells, and the text written on the main screen.
#[derive(Default)]
struct Screen {
    alternate: bool,
    cells: Vec<Vec<char>>,
    row: usize,
    column: usize,
    main: String,
    /// The end of the last read: a character or an escape it cut.
    pending: Vec<u8>,
}

impl Screen {
    const ROWS: usize = 24;
    const COLUMNS: usize = 80;

    fn feed(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        let mut whole = match std::str::from_utf8(&self.pending) {
            Ok(_) => self.pending.len(),
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            Err(_) => self.pending.len(),
        };
        // An escape sequence the read cut waits for the rest.
        if let Some(escape) = self.pending[..whole].iter().rposition(|&byte| byte == 0x1b) {
            let rest = &self.pending[escape + 1..whole];
            let complete = match rest.first() {
                Some(b'[') => rest[1..]
                    .iter()
                    .any(|byte| byte.is_ascii_alphabetic() || *byte == b'~'),
                Some(_) => true,
                None => false,
            };
            if !complete {
                whole = escape;
            }
        }
        let bytes: Vec<u8> = self.pending.drain(..whole).collect();
        let text = String::from_utf8_lossy(&bytes);
        let mut chars = text.chars().peekable();
        while let Some(char) = chars.next() {
            match char {
                '\x1b' => {
                    if chars.next_if_eq(&'[').is_none() {
                        chars.next();
                        continue;
                    }
                    let mut parameters = String::new();
                    let mut last = None;
                    for char in chars.by_ref() {
                        if char.is_ascii_alphabetic() || char == '~' {
                            last = Some(char);
                            break;
                        }
                        parameters.push(char);
                    }
                    self.control(&parameters, last);
                }
                '\r' => self.column = 0,
                '\n' if self.alternate => self.row += 1,
                '\n' => self.main.push('\n'),
                char if char.is_control() => {}
                char if self.alternate => {
                    if self.row < Self::ROWS && self.column < Self::COLUMNS {
                        self.cells[self.row][self.column] = char;
                    }
                    self.column += 1;
                }
                char => self.main.push(char),
            }
        }
    }

    fn control(&mut self, parameters: &str, last: Option<char>) {
        match (parameters, last) {
            ("?1049", Some('h')) => {
                self.alternate = true;
                self.cells = vec![vec![' '; Self::COLUMNS]; Self::ROWS];
            }
            ("?1049", Some('l')) => self.alternate = false,
            // A line rewritten in place, as rawr's progress line is.
            ("2", Some('K')) if !self.alternate => {
                let line = self.main.rfind('\n').map_or(0, |at| at + 1);
                self.main.truncate(line);
            }
            ("" | "0", Some('K')) if self.alternate => {
                if let Some(row) = self.cells.get_mut(self.row) {
                    for cell in row.iter_mut().skip(self.column) {
                        *cell = ' ';
                    }
                }
            }
            ("2", Some('J')) if self.alternate => {
                self.cells = vec![vec![' '; Self::COLUMNS]; Self::ROWS];
            }
            (position, Some('H')) => {
                let mut numbers = position.split(';').map(|part| part.parse().unwrap_or(1));
                self.row = numbers.next().unwrap_or(1).max(1) - 1;
                self.column = numbers.next().unwrap_or(1).max(1) - 1;
            }
            _ => {}
        }
    }

    fn rows(&self) -> Vec<String> {
        self.cells
            .iter()
            .map(|row| row.iter().collect::<String>().trim_end().to_owned())
            .collect()
    }
}

/// `rawr-import run` at a pseudo-terminal, as a shell runs it.
struct Terminal {
    child: Child,
    master: Arc<OwnedFd>,
    slave: PathBuf,
    screen: Arc<Mutex<Screen>>,
}

impl Terminal {
    fn start(root: &Path) -> Self {
        let size = nix::pty::Winsize {
            ws_row: Screen::ROWS as u16,
            ws_col: Screen::COLUMNS as u16,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let pty = nix::pty::openpty(&size, None).unwrap();
        let slave = nix::unistd::ttyname(pty.slave.as_raw_fd()).unwrap();
        let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let tty = || Stdio::from(pty.slave.try_clone().unwrap());
        let mut command = Command::new(env!("CARGO_BIN_EXE_rawr-import"));
        command
            .arg("run")
            .arg(root)
            .env("RAWR_HOME", directory.join("rawr-home"))
            .env("TERM", "xterm-256color")
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("RUSTFLAGS")
            .env_remove("RAWR_VERBOSE")
            .stdin(tty())
            .stdout(tty())
            .stderr(tty());
        // SAFETY: only async-signal-safe calls between fork and exec. The
        // child leads a new session whose controlling terminal is the pty.
        // TIOCSCTTY is a u32 on macOS and already the request type on Linux.
        #[allow(clippy::useless_conversion)]
        unsafe {
            command.pre_exec(|| {
                nix::unistd::setsid()?;
                if nix::libc::ioctl(0, nix::libc::TIOCSCTTY.into(), 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn().unwrap();
        drop(pty.slave);
        let master = Arc::new(pty.master);
        let screen = Arc::new(Mutex::new(Screen::default()));
        let (reader, shown) = (Arc::clone(&master), Arc::clone(&screen));
        std::thread::spawn(move || {
            let mut file = fs::File::from(reader.try_clone().unwrap());
            let mut buffer = [0_u8; 65536];
            while let Ok(count @ 1..) = file.read(&mut buffer) {
                shown.lock().unwrap().feed(&buffer[..count]);
            }
        });
        Self {
            child,
            master,
            slave,
            screen,
        }
    }

    fn keys(&self, keys: &[u8]) {
        let mut file = fs::File::from(self.master.try_clone().unwrap());
        file.write_all(keys).unwrap();
    }

    /// `stty -g` of the terminal, as a shell would see it. macOS sets
    /// `PENDIN` in `lflag` on any switch back to line editing, until the
    /// next read, as the shell's would be; it is left out.
    fn stty(&self) -> String {
        const PENDIN: u64 = 0x2000_0000;
        let output = Command::new("stty")
            .arg("-g")
            .stdin(fs::File::open(&self.slave).unwrap())
            .output()
            .unwrap();
        let settings = String::from_utf8(output.stdout).unwrap().trim().to_owned();
        settings
            .split(':')
            .map(|field| match field.strip_prefix("lflag=") {
                Some(flags) => {
                    let flags = u64::from_str_radix(flags, 16).unwrap() & !PENDIN;
                    format!("lflag={flags:x}")
                }
                None => field.to_owned(),
            })
            .collect::<Vec<_>>()
            .join(":")
    }

    /// Wait until `accept` holds for the screen, up to `limit`. How long it
    /// took.
    fn until(&self, limit: Duration, what: &str, accept: impl Fn(&Screen) -> bool) -> Duration {
        let started = Instant::now();
        loop {
            {
                let screen = self.screen.lock().unwrap();
                if accept(&screen) {
                    return started.elapsed();
                }
                assert!(
                    started.elapsed() < limit,
                    "timed out waiting for {what}; the screen:\n{}\nthe main screen's text:\n{}",
                    screen.rows().join("\n"),
                    screen.main
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn shows(&self, limit: Duration, text: &'static str) -> Duration {
        self.until(limit, text, move |screen| {
            screen.alternate && screen.rows().iter().any(|row| row.contains(text))
        })
    }

    fn main_text(&self) -> String {
        self.screen.lock().unwrap().main.clone()
    }

    fn ended(&self, times: usize) {
        self.until(Duration::from_secs(60), "the program's end", |screen| {
            screen.main.matches("a save runs it again").count() >= times
        });
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// D534.6's terminal acceptance, in a pseudo-terminal: a body edit
/// repaints with no key pressed and keeps the selection; a failing save
/// writes nothing while the program has the screen; a new field restarts
/// with one line on a restored screen; and after `q`, Ctrl-C, a panic and
/// a restart, `stty -g` is what it was before the run.
#[test]
fn a_terminal_program_repaints_on_a_save_and_leaves_the_terminal_as_it_found_it() {
    let root = fixture("islands-tui-terminal");
    let app = root.join("src/app.rs");
    let terminal = Terminal::start(&root);
    let before = terminal.stty();

    terminal.shows(Duration::from_secs(900), "tui-one");
    assert_ne!(terminal.stty(), before, "ratatui holds the terminal raw");
    terminal.keys(b"\x1b[B");
    terminal.keys(b"x");
    terminal.shows(Duration::from_secs(30), "keys=1:x");

    // A body edit: no key is pressed, and the program draws it.
    edit(&app, "\"tui-one\"", "\"tui-two\"");
    let took = terminal.shows(Duration::from_secs(60), "tui-two");
    eprintln!("a body edit repainted in {took:?}");
    let rows = terminal.screen.lock().unwrap().rows();
    assert!(rows.iter().any(|row| row.contains("> beta")), "{rows:#?}");
    assert!(rows.iter().any(|row| row.contains("keys=1:x")), "{rows:#?}");

    // A failing save: nothing reaches the screen while the program has it.
    edit(&app, "\"tui-two\"", "\"tui-two\" +");
    std::thread::sleep(Duration::from_secs(15));
    {
        let screen = terminal.screen.lock().unwrap();
        assert!(screen.alternate);
        assert!(!screen.main.contains("error"), "{}", screen.main);
        assert!(
            !screen.rows().iter().any(|row| row.contains("error")),
            "{:#?}",
            screen.rows()
        );
    }

    // A new field restarts the program. The line and the held error print
    // on the main screen, after the terminal is restored.
    let text = fs::read_to_string(&app).unwrap();
    let text = text
        .replace("\"tui-two\" +", "\"tui-three\"")
        .replace(
            "    pub keys: i64,\n}",
            "    pub keys: i64,\n    pub extra: i64,\n}",
        )
        .replace(
            "            keys: 0,\n",
            "            keys: 0,\n            extra: 0,\n",
        );
    fs::write(&app, text).unwrap();
    terminal.until(Duration::from_secs(300), "the restart line", |screen| {
        screen
            .main
            .contains("rawr: `app::App` changed its fields; restarting the program")
    });
    let shown = terminal.main_text();
    assert!(shown.contains("error"), "{shown}");
    assert_eq!(
        shown.matches("restarting the program").count(),
        1,
        "{shown}"
    );
    terminal.until(
        Duration::from_secs(600),
        "the restarted program",
        |screen| screen.alternate && screen.rows().iter().any(|row| row.contains("keys=0:")),
    );

    // `q` ends it, and the terminal is as it was.
    terminal.keys(b"q");
    terminal.ended(1);
    assert_eq!(terminal.stty(), before);

    // A save runs it again; Ctrl-C ends it.
    edit(&app, "\"tui-three\"", "\"tui-four\"");
    terminal.shows(Duration::from_secs(300), "tui-four");
    terminal.keys(b"\x03");
    terminal.ended(2);
    assert_eq!(terminal.stty(), before);

    // And a panic ends it.
    edit(&app, "\"tui-four\"", "\"tui-five\"");
    terminal.shows(Duration::from_secs(300), "tui-five");
    terminal.keys(b"!");
    terminal.ended(3);
    assert!(terminal.main_text().contains("asked to panic"));
    assert_eq!(terminal.stty(), before);
}
