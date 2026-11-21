//! The programs the client's files belong to, and the run each is attached
//! to: found from a file, attached to the newest run, followed through its
//! session's events, and let go once the run is over.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use super::hints::Hints;
use super::{Heard, Server, Stamp};
use crate::events::session;
use crate::mcp::session::{Session, Wait};

/// The name of a run's session socket in its place.
const SOCKET: &str = "session.sock";

/// A program the client has files of: its root, where its runs sit, and
/// the run it is attached to.
pub(super) struct Program {
    root: PathBuf,
    /// The name of the directory its runs sit in, which a watcher event
    /// names however it spells the directories above it.
    runs: Option<std::ffi::OsString>,
    pub(super) link: Option<Link>,
}

impl Program {
    /// Whether `path` is one of this program's files.
    fn holds(&self, path: &Path) -> bool {
        path.starts_with(&self.root)
    }
}

/// One run a program is attached to, and what the server took from its
/// session so far.
pub(super) struct Link {
    pub(super) serial: u64,
    pub(super) session: Arc<Session>,
    event: u64,
    output: u64,
    breakpoints: u64,
    /// Where the program is paused, as its session last said.
    pub(super) paused: Option<Value>,
    /// Counts the changes to where the program is: a request stamped with
    /// an older count is answered for a pause that is gone.
    pub(super) moment: u64,
    pub(super) hints: Hints,
    /// The last save's failure, as its event lists its diagnostics: null
    /// once a save swapped in.
    pub(super) failure: Value,
    /// The documents the failure was published to.
    pub(super) published: Vec<String>,
    /// The session hasn't greeted the server, so where the program is
    /// isn't known yet. The client's requests wait in `held` meanwhile.
    greeting: bool,
    held: Vec<Value>,
}

impl Link {
    fn new(serial: u64, session: Arc<Session>) -> Self {
        Self {
            serial,
            session,
            event: 0,
            output: 0,
            breakpoints: 0,
            paused: None,
            moment: 0,
            hints: Hints::default(),
            failure: Value::Null,
            published: Vec::new(),
            greeting: true,
            held: Vec::new(),
        }
    }

    /// A request sent now.
    pub(super) const fn stamp(&self) -> Stamp {
        Stamp {
            link: self.serial,
            moment: self.moment,
        }
    }
}

/// The native watcher on the user's session directory, which watches for
/// as long as it is kept.
pub(super) struct Watcher {
    _native: notify::RecommendedWatcher,
}

impl Server {
    /// The program that holds `path`, found from it the first time, and
    /// attached to its newest run when it has one and isn't attached.
    pub(super) fn program_of(&mut self, path: &Path) -> Option<usize> {
        let index = match self.held(path) {
            Some(index) => index,
            None => self.found(path)?,
        };
        if self.programs[index].link.is_none() {
            self.attach(index);
        }
        Some(index)
    }

    /// The program among those found that holds `path`: the innermost.
    fn held(&self, path: &Path) -> Option<usize> {
        self.programs
            .iter()
            .enumerate()
            .filter(|(_, program)| program.holds(path))
            .max_by_key(|(_, program)| program.root.as_os_str().len())
            .map(|(index, _)| index)
    }

    /// The program at `path` from rawr's discovery, added to those found.
    fn found(&mut self, path: &Path) -> Option<usize> {
        let directory = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent()?.to_path_buf()
        };
        if self.unserved.contains(&directory) {
            return None;
        }
        let path = path.to_path_buf();
        let Ok(target) =
            crate::base::discover(Some(&path), &mut rawr::project::cargo::Answers::new())
        else {
            self.unserved.insert(directory);
            return None;
        };
        let root = session::root(&target);
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        if let Some(index) = self
            .programs
            .iter()
            .position(|program| program.root == root)
        {
            return Some(index);
        }
        self.add(root);
        Some(self.programs.len() - 1)
    }

    /// Add the program at `root`, watching where its runs sit.
    pub(super) fn add(&mut self, root: PathBuf) {
        let runs = session::Runs::directory(&root).ok();
        if let Some(runs) = &runs {
            self.watch(runs);
        }
        self.programs.push(Program {
            runs: runs.and_then(|runs| runs.file_name().map(ToOwned::to_owned)),
            root,
            link: None,
        });
    }

    /// Watch the user's session directory, which holds `runs`, once.
    fn watch(&mut self, runs: &Path) {
        use notify::Watcher as _;

        if self.watcher.is_some() {
            return;
        }
        let Some(user) = runs.parent() else {
            return;
        };
        let heard = self.heard.clone();
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            let Ok(event) = event else {
                return;
            };
            let sockets: Vec<PathBuf> = event
                .paths
                .into_iter()
                .filter(|path| path.file_name().is_some_and(|name| name == SOCKET))
                .collect();
            if !sockets.is_empty() {
                let _ = heard.send(Heard::Sockets(sockets));
            }
        });
        if let Ok(mut watcher) = watcher
            && watcher
                .watch(user, notify::RecursiveMode::Recursive)
                .is_ok()
        {
            self.watcher = Some(Watcher { _native: watcher });
        }
    }

    /// A session socket appeared or went at `sockets`: attach each program
    /// whose runs sit there and isn't attached.
    pub(super) fn sockets(&mut self, sockets: &[PathBuf]) {
        for socket in sockets {
            let runs = socket
                .parent()
                .and_then(Path::parent)
                .and_then(Path::file_name);
            let Some(index) = self
                .programs
                .iter()
                .position(|program| runs.is_some() && program.runs.as_deref() == runs)
            else {
                continue;
            };
            if self.programs[index].link.is_none() {
                self.attach(index);
            }
        }
    }

    /// Attach program `index` to its newest run, when one serves it.
    fn attach(&mut self, index: usize) {
        if let Some(session) = Session::reach(&self.programs[index].root, None) {
            self.attach_to(index, session);
        }
    }

    /// Attach program `index` to `session`, and follow it once it greeted
    /// the server.
    fn attach_to(&mut self, index: usize, session: Arc<Session>) {
        self.links += 1;
        let serial = self.links;
        self.programs[index].link = Some(Link::new(serial, Arc::clone(&session)));
        self.park(&session, Wait::Greeting, move |server, _| {
            let Some(link) = server.link(serial) else {
                return;
            };
            if link.session.reached() {
                server.follow(serial);
            } else {
                server.detach(serial);
            }
        });
    }

    /// Add the program at `root`, attached to `session`, which greets the
    /// server once its peer answers `hello`.
    #[cfg(test)]
    pub(super) fn attached(&mut self, root: PathBuf, session: Arc<Session>) {
        self.programs.push(Program {
            root,
            runs: None,
            link: None,
        });
        self.attach_to(self.programs.len() - 1, session);
    }

    /// Add the program at `root`, attached to `session`, already greeted.
    #[cfg(test)]
    pub(super) fn linked(&mut self, root: PathBuf, session: Arc<Session>) {
        self.programs.push(Program {
            root,
            runs: None,
            link: None,
        });
        self.links += 1;
        let serial = self.links;
        if let Some(program) = self.programs.last_mut() {
            program.link = Some(Link::new(serial, session));
        }
        self.follow(serial);
    }

    /// Hold the client's `request` about the document at `uri` while the
    /// run its program is attached to hasn't greeted the server, to take it
    /// once the server knows where the program is. Whether it was held.
    pub(super) fn hold(&mut self, uri: &str, request: &Value) -> bool {
        let Some(path) = self
            .documents
            .get(uri)
            .map(|document| document.path.clone())
        else {
            return false;
        };
        let Some(index) = self.program_of(&path) else {
            return false;
        };
        match &mut self.programs[index].link {
            Some(link) if link.greeting => {
                link.held.push(request.clone());
                true
            }
            _ => false,
        }
    }

    /// The link numbered `serial`, while it holds.
    pub(super) fn link(&mut self, serial: u64) -> Option<&mut Link> {
        self.programs
            .iter_mut()
            .filter_map(|program| program.link.as_mut())
            .find(|link| link.serial == serial)
    }

    /// Whether a request stamped `stamp` is still answered for the pause it
    /// was asked at.
    pub(super) fn holds(&mut self, stamp: Stamp) -> bool {
        self.link(stamp.link)
            .is_some_and(|link| link.moment == stamp.moment)
    }

    /// Take what the session of link `serial` told since the server last
    /// looked, then wait for more, until the run is over.
    fn follow(&mut self, serial: u64) {
        let Some(link) = self.link(serial) else {
            return;
        };
        let session = Arc::clone(&link.session);
        link.breakpoints = session.breakpoints()["revision"]
            .as_u64()
            .unwrap_or(link.breakpoints);
        let answer = session
            .answer(Some(link.event), Some(link.output))
            .structured;
        link.event = answer["cursor"].as_u64().unwrap_or(link.event);
        link.output = answer["output_cursor"].as_u64().unwrap_or(link.output);
        // Where the program is paused comes from the same events as the
        // cursor, so a pause is taken once, whenever it lands.
        let mut failure = link.failure.clone();
        let mut paused = link.paused.clone();
        let mut moved = false;
        for event in answer["events"].as_array().into_iter().flatten() {
            let name = event["event"].as_str().unwrap_or_default();
            match name {
                "failed" => failure = event["diagnostics"].clone(),
                "running" | "queued" | "swapped" => failure = Value::Null,
                _ => {}
            }
            if name == "paused" {
                let mut at = event.clone();
                if let Some(at) = at.as_object_mut() {
                    at.remove("event");
                    at.remove("seq");
                }
                paused = Some(at);
                moved = true;
            } else if name != "failed" {
                moved = moved || paused.is_some();
                paused = None;
            }
        }
        if moved {
            link.paused = paused;
            link.moment += 1;
            self.read_hints(serial);
        }
        self.diagnose(serial, failure);
        let held = self.link(serial).map_or_else(Vec::new, |link| {
            link.greeting = false;
            std::mem::take(&mut link.held)
        });
        for request in held {
            self.take(request);
        }
        if session.phase().is_over() {
            return self.detach(serial);
        }
        let Some(link) = self.link(serial) else {
            return;
        };
        let change = Wait::Change {
            event: link.event,
            output: link.output,
            breakpoints: link.breakpoints,
        };
        self.park(&session, change, move |server, _| server.follow(serial));
    }

    /// Let link `serial` go: its run is over, or never greeted. What it
    /// showed goes with it.
    fn detach(&mut self, serial: u64) {
        self.diagnose(serial, Value::Null);
        let Some(program) = self.programs.iter_mut().find(|program| {
            program
                .link
                .as_ref()
                .is_some_and(|link| link.serial == serial)
        }) else {
            return;
        };
        if let Some(link) = program.link.take() {
            link.session.stop();
            if !link.hints.is_empty() {
                self.refresh();
            }
            for request in link.held {
                self.unanswered(&request);
            }
        }
    }

    /// Let every run go, as the client goes.
    pub(super) fn detach_all(&mut self) {
        for program in &mut self.programs {
            if let Some(link) = program.link.take() {
                link.session.stop();
            }
        }
    }
}
