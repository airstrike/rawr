//! A program a [`Universe`] loaded, and the handle that reloads it from any
//! thread.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{mpsc, Arc, Mutex, MutexGuard, PoisonError};

use super::{compile, Error, Outcome, Universe};
use crate::host::runtime::Registry;
use crate::package::snapshot::Snapshot;
use crate::{future, island, reload, Dynamic, Engine, Shared};

thread_local! {
    /// The runtime each generation after the first binds a generation of,
    /// on the thread that owns the program (D475.2, D553.4), so native
    /// values keep their witnesses.
    static NEXT: RefCell<Option<Registry>> = const { RefCell::new(None) };
}

/// A marked file a [`Universe`] loaded: compiled against its bindings,
/// its top-level items evaluated, and published on the thread that loaded
/// it, which owns it. Native code on that thread calls it by name with
/// [`Program::call`], or runs its `main` with [`Program::run`].
///
/// ```ignore
/// let universe = rawr::universe::Universe::new(&Bindings, rawr::Access::all())?;
/// let program = universe.load("program/src/main.rs")?;
/// let reloader = program.reloader();
/// std::thread::spawn(move || {
///     for _save in saves {
///         eprintln!("{}", reloader.reload());
///     }
/// });
/// std::process::exit(program.run());
/// ```
pub struct Program {
    state: Arc<State>,
    /// It runs on the thread that loaded it.
    owner: PhantomData<Rc<()>>,
}

/// What reloads a [`Program`] from any thread: it compiles the new text
/// there and offers it to the thread that owns the program.
#[derive(Clone)]
pub struct Reloader {
    state: Arc<State>,
}

/// What the program and its reloaders share.
struct State {
    engine: Shared<Engine>,
    /// The crate root.
    entry: PathBuf,
    /// Whether a reload rereads the package from disk.
    on_disk: bool,
    current: Mutex<Current>,
    /// Who hears each outcome ([`Program::outcomes`]).
    listeners: Mutex<Vec<mpsc::Sender<Outcome>>>,
}

/// The text the program runs, or will once the owner takes it, and the
/// generation a next one must be able to replace.
struct Current {
    files: Vec<(PathBuf, Box<str>)>,
    generation: Arc<reload::Generation>,
    /// The files a reload refused last, which aren't compiled again.
    refused: Option<Vec<(PathBuf, Box<str>)>>,
}

/// A program's package as read, or as given.
pub(super) struct Sources {
    entry: PathBuf,
    files: Vec<(PathBuf, Box<str>)>,
    on_disk: bool,
}

impl Sources {
    /// The marked file at `path`, the `Cargo.toml` nearest above it, and
    /// the Rust files beside and below it.
    pub(super) fn read(path: &Path) -> Result<Self, Error> {
        let entry = std::path::absolute(path).map_err(|source| Error::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let directory = entry.parent().unwrap_or(Path::new("/")).to_path_buf();
        let mut files = Vec::new();
        if let Some(manifest) = directory
            .ancestors()
            .map(|ancestor| ancestor.join("Cargo.toml"))
            .find(|manifest| manifest.is_file())
        {
            files.push(read(manifest)?);
        }
        rust_files(&directory, &mut files)?;
        Ok(Self {
            entry,
            files: sorted(files),
            on_disk: true,
        })
    }

    /// The crate root at `entry` holding `text`, with `files` for the rest
    /// of its package.
    pub(super) fn given(
        entry: PathBuf,
        text: Box<str>,
        mut files: Vec<(PathBuf, Box<str>)>,
    ) -> Self {
        files.retain(|(path, _)| *path != entry);
        files.push((entry.clone(), text));
        Self {
            entry,
            files: sorted(files),
            on_disk: false,
        }
    }
}

impl Program {
    /// Compile `sources` against `universe`'s bindings, evaluate their
    /// top-level items and publish the program on this thread.
    pub(super) fn load(universe: &Universe, sources: Sources) -> Result<Self, Error> {
        let Sources {
            entry,
            files,
            on_disk,
        } = sources;
        let snapshot = Snapshot::new(entry.clone(), files.clone())?;
        let candidate = compile(&universe.engine, &snapshot)?;
        let generation = candidate.generation().clone();
        let bootstrap = bind(&universe.engine, candidate, universe.bind_runtime())?;
        bootstrap.evaluate_top_level().map_err(Error::Evaluate)?;
        island::publish(bootstrap);
        Ok(Self {
            state: Arc::new(State {
                engine: universe.engine.clone(),
                entry,
                on_disk,
                current: Mutex::new(Current {
                    files,
                    generation,
                    refused: None,
                }),
                listeners: Mutex::new(Vec::new()),
            }),
            owner: PhantomData,
        })
    }

    /// Call the program's function `name`, at its crate root, with
    /// `arguments`, as native code calls script code: a generation offered
    /// since the last call is taken first. A `&mut` parameter the call
    /// changes is changed in `arguments`.
    ///
    /// # Errors
    ///
    /// Returns E0425 when the crate root has no function `name`, and the
    /// call's own error when it fails as it runs.
    pub fn call(&self, name: &str, arguments: &mut [Dynamic]) -> Result<Dynamic, Error> {
        match island::invoke(name, arguments) {
            Some(result) => result.map_err(Error::Call),
            None => Err(Error::Function(name.into())),
        }
    }

    /// Run the program's `fn main` on this thread, as Rust runs it, and
    /// give the status the process would end with: 0, 1 for an `Err` from
    /// `main` (printed as `Error: {err:?}`), and 101 for a panic. A `main`
    /// that hands native code its loop, as iced's `run` takes it, runs it
    /// here, and each reload is taken at the loop's next call into the
    /// program. A program with no `main` ends at once with 0.
    #[must_use]
    pub fn run(&self) -> i32 {
        std::panic::catch_unwind(run_main).unwrap_or(101)
    }

    /// What reloads this program from any thread.
    #[must_use]
    pub fn reloader(&self) -> Reloader {
        Reloader {
            state: Arc::clone(&self.state),
        }
    }

    /// Reread the program's files, as [`Reloader::reload`] does.
    pub fn reload(&self) -> Outcome {
        self.reloader().reload()
    }

    /// Give the crate root `text`, as [`Reloader::replace`] does.
    pub fn replace(&self, text: impl Into<Box<str>>) -> Outcome {
        self.reloader().replace(text)
    }

    /// Every outcome from now on, in order: what each reload said, and
    /// [`Outcome::Swapped`] once the owner takes a queued one. The
    /// receiver ends when the program and its reloaders are gone.
    #[must_use]
    pub fn outcomes(&self) -> mpsc::Receiver<Outcome> {
        let (sender, receiver) = mpsc::channel();
        lock(&self.state.listeners).push(sender);
        receiver
    }
}

impl Reloader {
    /// Reread the program's package from disk and compile it when its text
    /// changed: [`Outcome::Queued`] when it compiled, which the owner takes
    /// at its next call into the program. A program given as text rereads
    /// nothing.
    pub fn reload(&self) -> Outcome {
        if !self.state.on_disk {
            return Outcome::Unchanged;
        }
        match Sources::read(&self.state.entry) {
            Ok(sources) => self.offer(sources.files),
            Err(error) => self.state.said(Outcome::Failed {
                message: error.to_string(),
            }),
        }
    }

    /// Give the crate root `text`, the rest of the package unchanged, and
    /// compile it as [`Reloader::reload`] does.
    pub fn replace(&self, text: impl Into<Box<str>>) -> Outcome {
        let files = lock(&self.state.current).files.clone();
        let sources = Sources::given(self.state.entry.clone(), text.into(), files);
        self.offer(sources.files)
    }

    /// Compile `files` beside the running program and offer the result to
    /// the thread that owns it. The same text twice compiles once, and so
    /// does a text refused last.
    fn offer(&self, files: Vec<(PathBuf, Box<str>)>) -> Outcome {
        let files = sorted(files);
        let mut current = lock(&self.state.current);
        if files == current.files {
            current.refused = None;
            return Outcome::Unchanged;
        }
        if current.refused.as_ref() == Some(&files) {
            return Outcome::Unchanged;
        }
        let candidate = match self.state.compile(&current, &files) {
            Ok(candidate) => candidate,
            Err(outcome) => {
                current.refused = Some(files);
                return self.state.said(outcome);
            }
        };
        current.generation = candidate.generation().clone();
        current.files = files;
        current.refused = None;
        drop(current);
        let state = Arc::clone(&self.state);
        // The owner takes it at its next call from native code; a newer
        // reload's replaces it first.
        future::offer(move || {
            let outcome = match swap(&state.engine, candidate) {
                Ok(None) => Outcome::Swapped,
                Ok(Some(reason)) => Outcome::Restarting { reason },
                Err(error) => Outcome::Failed {
                    message: error.to_string(),
                },
            };
            state.said(outcome);
        });
        self.state.said(Outcome::Queued)
    }
}

impl State {
    /// The next generation of `files`, which replaces `current`'s: its
    /// compile error, or why it can't replace it in this process, is the
    /// outcome instead.
    fn compile(
        &self,
        current: &Current,
        files: &[(PathBuf, Box<str>)],
    ) -> Result<reload::Candidate, Outcome> {
        let failed = |error: Error| Outcome::Failed {
            message: error.to_string(),
        };
        let snapshot = Snapshot::new(self.entry.clone(), files.to_vec())
            .map_err(|error| failed(error.into()))?;
        let candidate = compile(&self.engine, &snapshot).map_err(failed)?;
        if let Some(reason) = island::refusal(&current.generation, &candidate) {
            return Err(Outcome::Restarting { reason });
        }
        Ok(candidate)
    }

    /// Tell every listener `outcome`, and give it back.
    fn said(&self, outcome: Outcome) -> Outcome {
        lock(&self.listeners).retain(|listener| listener.send(outcome.clone()).is_ok());
        outcome
    }
}

/// Bind `candidate` to the runtime: `first`, the universe's own, for the
/// first program, and a next generation of it for each one after.
fn bind(
    engine: &Shared<Engine>,
    candidate: reload::Candidate,
    first: Option<Registry>,
) -> Result<reload::installation::Bootstrap, Error> {
    let runtime = match first {
        Some(runtime) => {
            NEXT.set(Some(runtime.next_generation()));
            runtime
        }
        None => NEXT
            .with_borrow(|next| next.as_ref().map(Registry::next_generation))
            .ok_or(Error::Unowned)?,
    };
    candidate.bind(engine, runtime).map_err(Error::Bind)
}

/// Bind `candidate` to a next generation of the running program's runtime,
/// evaluate its top level there and admit it: its next calls from native
/// code run in it. `Some` says why it can't replace the running program
/// now, which keeps running (#1746).
fn swap(engine: &Shared<Engine>, candidate: reload::Candidate) -> Result<Option<String>, Error> {
    let bootstrap = bind(engine, candidate, None)?;
    bootstrap.evaluate_top_level().map_err(Error::Evaluate)?;
    Ok(island::admit(bootstrap))
}

/// Run the published program's `main` to its status, as a generated runner
/// does: an async `main` hands its future to its runtime's entry (D513.4),
/// and a native entry the program registered runs here (D501).
fn run_main() -> i32 {
    let mut result = island::invoke_main();
    if let Some(Ok(value)) = &result
        && value.is::<future::Future>()
    {
        let Some(Ok(value)) = result.take() else {
            unreachable!("the result was just matched");
        };
        island::await_main(value.cast());
        return match island::take_entry() {
            Some(entry) => entry(),
            None => {
                eprintln!(
                    "error[E0752]: `main` function is not allowed to be `async` without a runtime's entry, such as `#[tokio::main]`"
                );
                101
            }
        };
    }
    if let Some(entry) = island::take_entry() {
        return entry();
    }
    match result {
        None => 0,
        Some(Ok(value)) => match value.try_cast::<Result<Dynamic, Dynamic>>() {
            Some(Err(error)) => {
                eprintln!("Error: {error:?}");
                1
            }
            _ => 0,
        },
        Some(Err(error)) => {
            eprintln!("error: {error}");
            101
        }
    }
}

/// `path`'s text.
fn read(path: PathBuf) -> Result<(PathBuf, Box<str>), Error> {
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok((path, text.into_boxed_str())),
        Err(source) => Err(Error::Read { path, source }),
    }
}

/// Every `.rs` file in `directory` and below it, but in `target`.
fn rust_files(directory: &Path, files: &mut Vec<(PathBuf, Box<str>)>) -> Result<(), Error> {
    let entries = std::fs::read_dir(directory).map_err(|source| Error::Read {
        path: directory.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let path = entry
            .map_err(|source| Error::Read {
                path: directory.to_path_buf(),
                source,
            })?
            .path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name != "target") {
                rust_files(&path, files)?;
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(read(path)?);
        }
    }
    Ok(())
}

/// `files` in path order, as a snapshot keeps them, so two reads of the
/// same package compare equal.
fn sorted(mut files: Vec<(PathBuf, Box<str>)>) -> Vec<(PathBuf, Box<str>)> {
    files.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    files
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
