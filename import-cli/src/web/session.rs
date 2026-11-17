//! One `rawr run --web`: the program's runner built for a browser, its page
//! served, and each save handed to the page.
//!
//! Three threads share the work. This one waits for saves and sends the
//! package's new files to the page. The builder builds the runner again
//! when a save uses an API the running one doesn't cover, or when no
//! runner built yet, and tells the page to load the new one. The reporter
//! says in the terminal what the page reported.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, PoisonError};

use rawr::project::cargo::features::Enabled;

use serde_json::{json, Value};

use super::base;
use super::build::{self, Build, Built};
use super::error::Failure;
use super::package::Package;
use super::server::{Event, Handle, Report, Server};
use super::Error;

/// The name of the program's package in a page's directory.
const PROGRAM: &str = "program.json";

/// A program served to a page, ready to hear its saves.
pub(super) struct Session {
    /// The program's path, as the command was given it.
    path: Option<PathBuf>,
    /// The program's crate root.
    entry: PathBuf,
    graph: rawr::package::Graph,
    /// What the package's `cfg(..)`s read as Cargo builds it for the page,
    /// which the builder reads too.
    features: Arc<Mutex<Enabled>>,
    package: Package,
    handle: Handle,
    /// Whether the page has a runner: a build succeeded.
    built: Arc<AtomicBool>,
    /// Asks the builder for a build.
    builds: mpsc::Sender<()>,
}

impl Session {
    /// Build the program at `path` for a browser and serve its page, or
    /// write the page to `options.out` and serve nothing: `None`.
    pub(super) fn start(
        path: Option<&PathBuf>,
        options: &rawr_cli::web::Options,
    ) -> Result<Option<Self>, Failure> {
        let loaded = rawr_cli::base::load(path, rawr_cli::base::Build::Program)?;
        rawr_cli::cells::keep("rawr-import", &loaded.cells, &loaded.graph);
        // A package builds in its target directory, a script in its
        // directory in rawr's area, as `rawr run` builds them. A program
        // that uses no crates runs on the runner of rawr and std that every
        // such program shares.
        let (location, root) = match &loaded.target {
            _ if !loaded.graph.declares_rust_import() => {
                let package = base::package(&build::home())?;
                (package.clone(), package)
            }
            rawr::project::Target::Project(project) => (
                project.location().to_path_buf(),
                project.root().as_path().to_path_buf(),
            ),
            target @ rawr::project::Target::File(source) => (
                source.path().to_path_buf(),
                rawr::package::Graph::root_of(target),
            ),
        };
        let entry = loaded.target.source().path().to_path_buf();
        let mut build = Build::new(&location, &root, options.out.as_deref());
        let built = build.run();
        let features = features(&loaded.graph)?;
        let package = Package::read(&entry, &features).map_err(Error::from)?;
        if options.out.is_some() {
            built?;
            let path = build.site().join(PROGRAM);
            std::fs::write(&path, package.program().to_string())
                .map_err(|source| Error::File { path, source })?;
            rawr_import::process::say(format_args!(
                "rawr: the program's page is in {}",
                build.site().display()
            ));
            return Ok(None);
        }
        let built = match built {
            Ok(_) => true,
            // The build said why; a save builds it again.
            Err(Error::Build) => false,
            Err(error) => return Err(error.into()),
        };
        let (reports, heard) = mpsc::channel();
        let server = Server::bind(
            options.port,
            build.site().to_path_buf(),
            program(&package, built),
            reports,
        )?;
        rawr_import::process::say(format_args!("rawr: the program runs at {}", server.url()));
        let handle = server.handle();
        server.spawn();
        let built = Arc::new(AtomicBool::new(built));
        let features = Arc::new(Mutex::new(features));
        let builds = builder(
            build,
            entry.clone(),
            Arc::clone(&features),
            handle.clone(),
            Arc::clone(&built),
        );
        reporter(heard, builds.clone());
        Ok(Some(Self {
            path: path.cloned(),
            entry,
            graph: loaded.graph,
            features,
            package,
            handle,
            built,
            builds,
        }))
    }

    /// Hand each save to the page, for as long as the process runs.
    pub(super) fn serve(mut self) -> ExitCode {
        loop {
            let Some(watcher) = rawr_cli::supervise::Watcher::open(&self.graph) else {
                // Saves can't be heard; the page runs what it has.
                loop {
                    std::thread::park();
                }
            };
            watcher.wait();
            // A program that no longer parses still goes to the page,
            // which reports its error as it compiles it.
            if let Ok(loaded) =
                rawr_cli::base::load(self.path.as_ref(), rawr_cli::base::Build::Program)
            {
                self.graph = loaded.graph;
                match features(&self.graph) {
                    Ok(features) => *lock(&self.features) = features,
                    Err(error) => eprintln!(
                        "{}: {error}",
                        rawr_cli::color::label(rawr_cli::color::Level::Error)
                    ),
                }
            }
            let read = Package::read(&self.entry, &lock(&self.features));
            let package = match read {
                Ok(package) => package,
                Err(error) => {
                    eprintln!(
                        "{}: {error}",
                        rawr_cli::color::label(rawr_cli::color::Level::Error)
                    );
                    continue;
                }
            };
            if package == self.package {
                continue;
            }
            // The running program's `cfg(..)`s can't change in place: the
            // page starts it again with the new ones.
            let same_cfg = package.same_cfg(&self.package);
            self.package = package;
            let built = self.built.load(Ordering::Acquire);
            self.handle.set_program(program(&self.package, built));
            if built && !same_cfg {
                rawr_import::process::say(
                    "rawr: the program's features changed; restarting the program",
                );
                self.handle.send(&Event::Reload);
            } else if built {
                self.handle.send(&Event::Files(self.package.files()));
            } else {
                let _ = self.builds.send(());
            }
        }
    }
}

/// The program as the page loads it: its package once the page has a
/// runner, and that it hears saves.
fn program(package: &Package, built: bool) -> Value {
    let mut program = if built { package.program() } else { json!({}) };
    program["watch"] = Value::Bool(true);
    program
}

/// Start the thread that builds the runner again each time it is asked,
/// once for all the asks that came while it built, and tells the page to
/// load a runner that changed.
fn builder(
    mut build: Build,
    entry: PathBuf,
    features: Arc<Mutex<Enabled>>,
    handle: Handle,
    built: Arc<AtomicBool>,
) -> mpsc::Sender<()> {
    let (builds, asked) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        while asked.recv().is_ok() {
            while asked.try_recv().is_ok() {}
            match build.run() {
                Ok(Built::Packed) => {
                    let read = Package::read(&entry, &lock(&features));
                    if let Ok(package) = read {
                        handle.set_program(program(&package, true));
                    }
                    built.store(true, Ordering::Release);
                    rawr_import::process::say(
                        "rawr: the runner is built again; the page restarts on it",
                    );
                    handle.send(&Event::Reload);
                }
                Ok(Built::Unchanged) => {}
                // The build said why, in the terminal.
                Err(Error::Build) => handle.send(&Event::Unbuilt),
                Err(error) => {
                    eprintln!(
                        "{}: {error}",
                        rawr_cli::color::label(rawr_cli::color::Level::Error)
                    );
                }
            }
        }
    });
    builds
}

/// What the page's `cfg(..)`s read: the features and options Cargo builds
/// the program's crate with for the page's target, as `cargo build
/// --target wasm32-unknown-unknown` builds it.
fn features(graph: &rawr::package::Graph) -> Result<Enabled, Error> {
    graph
        .entry_features_on(build::TARGET)
        .map_err(|error| Error::Cargo(error.to_string()))
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Start the thread that says in the terminal what the page reported, as
/// `rawr run` says what a program it runs did, and asks for a build when
/// a save needs one.
fn reporter(heard: mpsc::Receiver<Report>, builds: mpsc::Sender<()>) {
    std::thread::spawn(move || {
        for report in heard {
            match report {
                Report::Output { stderr: true, text } => eprint!("{text}"),
                Report::Output {
                    stderr: false,
                    text,
                } => print!("{text}"),
                Report::Answer(answer) => {
                    if said(&answer) {
                        let _ = builds.send(());
                    }
                }
            }
        }
    });
}

/// Say what `answer`, the page's report of a start or a reload, says, and
/// whether its program needs a runner built for its text.
fn said(answer: &Value) -> bool {
    for chunk in answer["output"].as_array().into_iter().flatten() {
        let text = chunk["text"].as_str().unwrap_or_default();
        if chunk["stream"] == "stderr" {
            eprint!("{text}");
        } else {
            print!("{text}");
        }
    }
    let text = |key: &str| answer[key].as_str().unwrap_or_default().to_owned();
    match answer["kind"].as_str() {
        Some("error") if answer["uncovered"] == true => {
            rawr_import::process::say(
                "rawr: preparing again: the program uses an API its runner doesn't cover",
            );
            return true;
        }
        Some("error") => {
            let text = text("text");
            eprint!("{text}");
            if !text.ends_with('\n') {
                eprintln!();
            }
        }
        Some("restart") => rawr_import::process::say(format_args!(
            "rawr: {}; restarting the program",
            text("reason")
        )),
        Some("panic") => {
            rawr_import::process::say("rawr: the program panicked; a save runs it again")
        }
        Some("text") if answer["status"] != 0 => rawr_import::process::say(format_args!(
            "rawr: the program's `main` ended with status {}",
            answer["status"]
        )),
        _ => {}
    }
    false
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    /// Only an error at an API the runner doesn't cover asks for a build.
    #[test]
    fn an_uncovered_use_asks_for_a_build() {
        assert!(super::said(
            &json!({ "kind": "error", "text": "e", "uncovered": true })
        ));
        assert!(!super::said(
            &json!({ "kind": "error", "text": "error[E0308]: mismatched types\n" })
        ));
        assert!(!super::said(&json!({ "kind": "swapped", "output": [] })));
        assert!(!super::said(
            &json!({ "kind": "restart", "reason": "a field changed" })
        ));
    }

    /// The page hears saves whether or not a runner built.
    #[test]
    fn the_program_says_it_hears_saves() {
        let root = std::env::temp_dir().join(format!("rawr-web-session-{}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"app\"\n").unwrap();
        std::fs::write(root.join("src").join("main.rs"), "// rawr\nfn main() {}\n").unwrap();
        let features = super::Enabled::default();
        let package = super::Package::read(&root.join("src").join("main.rs"), &features).unwrap();
        assert_eq!(super::program(&package, false), json!({ "watch": true }));
        let built = super::program(&package, true);
        assert_eq!(built["watch"], true);
        assert_eq!(built["files"].as_array().map(Vec::len), Some(2));
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A workspace member's page reads its `cfg(..)`s as Cargo builds it
    /// for the browser: the features Cargo enables for it, its inherited
    /// workspace settings resolved, and wasm32's options (#2121).
    #[test]
    fn a_workspace_members_cfg_is_cargos_for_the_browser() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("web-workspace");
        let root = std::env::temp_dir().join(format!("rawr-web-workspace-{}", std::process::id()));
        for file in ["Cargo.toml", "app/Cargo.toml", "app/src/main.rs"] {
            let to = root.join(file);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::copy(fixture.join(file), to).unwrap();
        }
        let app = root.join("app");
        let Ok(loaded) = rawr_cli::base::load(Some(&app), rawr_cli::base::Build::Program) else {
            panic!("the member loads");
        };
        let features = super::features(&loaded.graph).expect("Cargo describes the member");
        assert!(
            features.holds("loud"),
            "{:?}",
            features.lines().collect::<Vec<_>>()
        );
        assert!(features.option("target_arch", Some("wasm32")));
        let package = super::Package::read(&app.join("src").join("main.rs"), &features).unwrap();
        let cfg = package.program()["cfg"].clone();
        assert!(
            cfg.as_array()
                .unwrap()
                .contains(&json!("target_arch=\"wasm32\"")),
            "{cfg}"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
