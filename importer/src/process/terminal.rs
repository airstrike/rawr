//! The terminal on stdin, handed between this process's group and a
//! runner's, and kept sane across the runner's end (D534.2).
//!
//! A runner that ends by SIGKILL, as every restart does, runs no `Drop`
//! and no `ratatui::restore()`. So this process saves the terminal's modes
//! before the first runner gets it. After any runner that had it ends, it
//! puts them back when they changed and writes a fixed reset. While a
//! program holds the terminal raw, the lines rawr prints wait for that
//! restore, so they never land inside a full-screen program.

/// What a program that drew the whole screen may have left on: the kitty
/// keyboard flags it pushed on the alternate screen, mouse reports, focus
/// reports, bracketed paste and the alternate screen itself, then the
/// flags pushed on the main screen, a hidden cursor and colours.
#[cfg_attr(not(unix), allow(dead_code))]
const RESET: &[u8] = b"\x1b[<99u\
\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\
\x1b[?1004l\x1b[?2004l\
\x1b[?1049l\
\x1b[<99u\x1b[?25h\x1b[0m";

/// Print `line` on stderr, or hold it until the terminal is restored when a
/// program holds it raw.
pub fn say(line: impl std::fmt::Display) {
    let line = line.to_string();
    if imp::raw()
        && let Ok(mut held) = imp::HELD.lock()
    {
        held.push(line);
        return;
    }
    eprint!("{line}");
    if !line.ends_with('\n') {
        eprintln!();
    }
}

/// Whether a program holds the terminal raw, so rawr writes nothing to it.
pub(crate) fn raw() -> bool {
    imp::raw()
}

#[cfg(unix)]
pub(super) use imp::{give, restore, take_back, wake};

#[cfg(unix)]
mod imp {
    use std::io::{IsTerminal as _, Write as _};
    use std::sync::Mutex;

    use rustix::process::{getpgrp, kill_process_group, Pid, Signal};
    use rustix::termios::{
        tcgetattr, tcgetpgrp, tcsetattr, tcsetpgrp, LocalModes, OptionalActions, SpecialCodeIndex,
        Termios,
    };

    /// The terminal's modes before the first runner had it.
    static SAVED: Mutex<Option<Termios>> = Mutex::new(None);
    /// Lines [`super::say`] held while a program had the terminal raw.
    pub(super) static HELD: Mutex<Vec<String>> = Mutex::new(Vec::new());

    /// Whether stdin is a terminal a program holds raw: without line
    /// editing, as every full-screen program sets it.
    pub(super) fn raw() -> bool {
        let stdin = std::io::stdin();
        stdin.is_terminal()
            && tcgetattr(&stdin).is_ok_and(|modes| !modes.local_modes.contains(LocalModes::ICANON))
    }

    fn leader(leader: u32) -> Option<Pid> {
        i32::try_from(leader).ok().and_then(Pid::from_raw)
    }

    /// Give stdin's terminal to the group `leader` leads, when this
    /// process's group has it in the foreground. Whether it did. The
    /// terminal's modes are saved the first time.
    pub(in crate::process) fn give(leader: u32) -> bool {
        let stdin = std::io::stdin();
        let Some(leader) = self::leader(leader) else {
            return false;
        };
        if !tcgetpgrp(&stdin).is_ok_and(|foreground| foreground == getpgrp()) {
            return false;
        }
        if let Ok(mut saved) = SAVED.lock()
            && saved.is_none()
        {
            *saved = tcgetattr(&stdin).ok();
        }
        if tcsetpgrp(&stdin, leader).is_err() {
            return false;
        }
        // A read before the hand-off stopped the program with SIGTTIN.
        let _ = kill_process_group(leader, Signal::CONT);
        true
    }

    /// Take stdin's terminal back from the group `leader` leads, when it
    /// still has it.
    pub(in crate::process) fn take_back(leader: u32) {
        let stdin = std::io::stdin();
        let Some(leader) = self::leader(leader) else {
            return;
        };
        if tcgetpgrp(&stdin).is_ok_and(|foreground| foreground == leader) {
            let _ = tcsetpgrp(&stdin, getpgrp());
        }
    }

    /// Tell the group `leader` leads that the terminal changed size, so a
    /// loop blocked reading the terminal draws again. Its default action
    /// is to be ignored.
    pub(in crate::process) fn wake(leader: u32) {
        if let Some(leader) = self::leader(leader) {
            let _ = kill_process_group(leader, Signal::WINCH);
        }
    }

    /// After a runner that had the terminal ended: put back its saved modes
    /// when they changed, with the fixed reset, then print the held lines.
    pub(in crate::process) fn restore() {
        let stdin = std::io::stdin();
        let saved = SAVED.lock().ok().and_then(|saved| saved.clone());
        if let (Some(saved), Ok(now)) = (saved, tcgetattr(&stdin))
            && !same(&saved, &now)
        {
            let _ = tcsetattr(&stdin, OptionalActions::Now, &saved);
            write_reset();
        }
        let held = HELD
            .lock()
            .map(|mut held| std::mem::take(&mut *held))
            .unwrap_or_default();
        for line in held {
            super::say(line);
        }
    }

    fn same(left: &Termios, right: &Termios) -> bool {
        left.input_modes == right.input_modes
            && left.output_modes == right.output_modes
            && left.control_modes == right.control_modes
            // The kernel sets `PENDIN` on any switch back to line
            // editing, until the next read; it is no change of the program's.
            && left.local_modes.difference(LocalModes::PENDIN)
                == right.local_modes.difference(LocalModes::PENDIN)
            && left.special_codes[SpecialCodeIndex::VMIN]
                == right.special_codes[SpecialCodeIndex::VMIN]
            && left.special_codes[SpecialCodeIndex::VTIME]
                == right.special_codes[SpecialCodeIndex::VTIME]
    }

    /// Write [`super::RESET`] to the controlling terminal.
    fn write_reset() {
        let written = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/tty")
            .and_then(|mut tty| tty.write_all(super::RESET).and_then(|()| tty.flush()));
        if written.is_err() {
            let mut stderr = std::io::stderr().lock();
            let _ = stderr.write_all(super::RESET).and_then(|()| stderr.flush());
        }
    }
}

#[cfg(not(unix))]
mod imp {
    use std::sync::Mutex;

    pub(super) static HELD: Mutex<Vec<String>> = Mutex::new(Vec::new());

    pub(super) fn raw() -> bool {
        false
    }

    pub(in crate::process) fn give(_leader: u32) -> bool {
        false
    }

    pub(in crate::process) fn take_back(_leader: u32) {}

    pub(in crate::process) fn wake(_leader: u32) {}

    pub(in crate::process) fn restore() {}
}

#[cfg(not(unix))]
pub(super) use imp::{give, restore, take_back, wake};
