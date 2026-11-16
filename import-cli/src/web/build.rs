//! The program's runner built for a browser and packed with its page.
//!
//! This executable builds the runner again in a process of its own, with
//! Cargo's target set to wasm32: `run` of a runner for a target this
//! machine can't run builds it and ends (#1753), and writes it, with the
//! `Cargo.lock` it was built from, to the directory `RAWR_UNIVERSE_OUT`
//! names (#1752). Its own output says what it builds and why it fails, as
//! `rawr run`'s does. The build has a Cargo target directory of its own,
//! under the project's rawr directory, so it shares no units or records with
//! the project's native build.
//!
//! wasm-bindgen then turns the runner into `universe_bg.wasm` and the glue
//! `universe.js` imports, beside rawr-web's page for a project
//! (`web/www/run.html`) as `index.html`.

use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{bindgen, Error};

/// The target a page's runner is built for.
const TARGET: &str = "wasm32-unknown-unknown";

/// rawr-web's page for a project, which runs its runner.
const PAGE: &str = include_str!("../../../web/www/run.html");

/// The files the page's directory holds.
pub(super) const GLUE: &str = "universe.js";
pub(super) const MODULE: &str = "universe_bg.wasm";
pub(super) const INDEX: &str = "index.html";

/// Where a project's runner for a browser is built and packed.
pub(super) struct Build {
    /// The program's location, as `rawr run` is given it.
    location: PathBuf,
    /// The Cargo target directory the build uses.
    cargo: PathBuf,
    /// Where the build writes the runner and its lock.
    runner: PathBuf,
    /// Where wasm-bindgen writes, before the files move to the page.
    pack: PathBuf,
    /// The page's directory.
    site: PathBuf,
    /// rawr's home, where wasm-bindgen's CLI is installed.
    home: PathBuf,
    /// Whether the module keeps its name section, as a page served here
    /// does for the panics it shows, or is packed small to deploy.
    names: Names,
    /// The digest of the runner packed last.
    packed: Option<u64>,
}

/// Whether a packed module keeps the names of its functions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Names {
    Kept,
    Removed,
}

/// What a build did to the page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Built {
    /// The page has a new runner.
    Packed,
    /// The runner is the one the page has.
    Unchanged,
}

impl Build {
    /// The build of the program at `location`, whose package's root is
    /// `root`, with its page in `site`, or in the project's rawr directory.
    pub(super) fn new(location: &Path, root: &Path, site: Option<&Path>) -> Self {
        let target = rawr_cli::cells::target_directory(root);
        let web = rawr_location::rawr_directory_in(&target, root).join("web");
        let cargo_home = std::env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".cargo")))
            .unwrap_or_default();
        let names = if site.is_some() {
            Names::Removed
        } else {
            Names::Kept
        };
        Self {
            location: location.to_path_buf(),
            cargo: web.join("target"),
            runner: web.join("runner"),
            pack: web.join("pack"),
            site: site.map_or_else(|| web.join("site"), Path::to_path_buf),
            home: rawr_import::store::home(&cargo_home),
            names,
            packed: None,
        }
    }

    /// The page's directory.
    pub(super) fn site(&self) -> &Path {
        &self.site
    }

    /// Build the runner and pack it with the page, unless the page has it.
    pub(super) fn run(&mut self) -> Result<Built, Error> {
        let runner = self.runner.join(rawr_import::universe::RUNNER);
        let _ = std::fs::remove_file(&runner);
        let executable = std::env::current_exe().map_err(|source| Error::Spawn {
            program: "rawr-import".to_owned(),
            source,
        })?;
        let mut command = Command::new(executable);
        rawr::project::cargo::features::Selection::selected().hand(&mut command);
        rawr_cli::color::hand(&mut command);
        let status = command
            .arg("run")
            .arg(&self.location)
            .env("CARGO_BUILD_TARGET", TARGET)
            .env("CARGO_TARGET_DIR", &self.cargo)
            .env(rawr_import::universe::DIRECTORY, &self.runner)
            .status()
            .map_err(|source| Error::Spawn {
                program: "rawr-import run".to_owned(),
                source,
            })?;
        if !status.success() {
            return Err(Error::Build);
        }
        let bytes = std::fs::read(&runner).map_err(|_| Error::Missing(self.runner.clone()))?;
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        let digest = hasher.finish();
        if self.packed == Some(digest) {
            return Ok(Built::Unchanged);
        }
        self.pack(&runner)?;
        self.packed = Some(digest);
        Ok(Built::Packed)
    }

    /// Pack `runner` with the page, each file replaced whole.
    fn pack(&self, runner: &Path) -> Result<(), Error> {
        let lock = self.runner.join(rawr_import::universe::LOCK);
        let tool = bindgen::tool(&lock, &self.home, &self.pack.with_extension("tools"))?;
        let _ = std::fs::remove_dir_all(&self.pack);
        let mut command = Command::new(&tool);
        command.args([
            "--target",
            "web",
            "--no-typescript",
            "--out-name",
            "universe",
        ]);
        if self.names == Names::Removed {
            command.arg("--remove-name-section");
        }
        let packed = command
            .arg("--out-dir")
            .arg(&self.pack)
            .arg(runner)
            .status()
            .map_err(|source| Error::Spawn {
                program: tool.to_string_lossy().into_owned(),
                source,
            })?;
        if !packed.success() {
            return Err(Error::Pack(runner.to_path_buf()));
        }
        let file = |path: &Path| {
            let path = path.to_path_buf();
            move |source| Error::File { path, source }
        };
        std::fs::create_dir_all(&self.site).map_err(file(&self.site))?;
        for name in [GLUE, MODULE] {
            let site = self.site.join(name);
            std::fs::rename(self.pack.join(name), &site).map_err(file(&site))?;
        }
        let index = self.site.join(INDEX);
        std::fs::write(&index, PAGE).map_err(file(&index))?;
        let _ = std::fs::remove_dir_all(&self.pack);
        Ok(())
    }
}
