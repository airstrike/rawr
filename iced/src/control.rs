//! Interpreter-independent in-process controls for one prepared preview host.

mod error;
mod preview;
pub mod reload;
mod status;

pub use error::Error;
pub use preview::Preview;
pub use status::{Selection, Status};

use std::path::PathBuf;
use std::sync::Arc;

/// Ask the next `rawr preview` of the project at `root` to open the preview
/// named `name` (its name or `group/name`).
pub fn remember(root: &std::path::Path, name: &str) -> std::io::Result<()> {
    let directory = root.join(".rawr");
    std::fs::create_dir_all(&directory)?;
    std::fs::write(directory.join("preview"), name)
}

/// The sole in-process owner of one prepared application/preview host.
pub struct Host {
    state: crate::preview::host::Host,
    snapshots: Option<crate::snapshot::Suite>,
}

impl Host {
    pub(crate) fn new(
        state: crate::preview::host::Host,
        catalog: Arc<rawr::preview::BoundCatalog>,
        root: Option<PathBuf>,
    ) -> Self {
        let snapshots = root.map(|root| crate::snapshot::Suite::new(catalog, root));
        Self { state, snapshots }
    }

    /// Transform one native-bound reload bootstrap into a sealed control host.
    pub fn bootstrap(
        bootstrap: rawr::reload::installation::Bootstrap,
        source: rawr::diagnostic::Source,
    ) -> Result<Self, rawr::diagnostic::Report> {
        let state = crate::preview::host::Host::bootstrap(bootstrap, source, None)?;
        Ok(Self {
            state,
            snapshots: None,
        })
    }

    /// Transform one native-bound bootstrap with explicit timing observation.
    #[doc(hidden)]
    pub fn bootstrap_observed(
        bootstrap: rawr::reload::installation::Bootstrap,
        source: rawr::diagnostic::Source,
        timing: rawr::timing::Session,
    ) -> Result<Self, rawr::diagnostic::Report> {
        let state = crate::preview::host::Host::bootstrap(bootstrap, source, Some(timing))?;
        Ok(Self {
            state,
            snapshots: None,
        })
    }

    /// Present this host as `rawr run`: the application starts selected under its
    /// own title, and the preview footer is the only added chrome.
    #[must_use]
    pub fn into_run(self) -> Self {
        Self {
            state: self.state.into_run(),
            snapshots: self.snapshots,
        }
    }

    /// Present this host as `rawr preview`: open straight into the preview
    /// named in `<root>/.rawr/preview` (see [`remember`]), else the first, and
    /// remember each later pick there.
    #[must_use]
    pub fn into_preview(self, root: &std::path::Path) -> Self {
        Self {
            state: self.state.into_preview(root),
            snapshots: self.snapshots,
        }
    }

    /// Return the deterministic compiler-proven preview list.
    #[must_use]
    pub fn previews(&self) -> &[Preview] {
        self.state.control_previews()
    }

    /// Return the current application/preview session status.
    #[must_use]
    pub fn status(&self) -> Status {
        Status::from_host(&self.state)
    }

    /// Select one listed preview through the native host's transaction boundary.
    pub fn select(&mut self, key: &rawr::preview::Key) -> Result<Status, Error> {
        self.state.select_control(key).map_err(Error::Rejected)?;
        Ok(self.status())
    }

    /// Reset the selected preview while preserving any retained application.
    pub fn reset(&mut self) -> Result<Status, Error> {
        if matches!(
            self.state.selection(),
            crate::preview::host::Selection::Application
        ) {
            return Err(Error::Application);
        }
        self.state.reset_control().map_err(Error::Rejected)?;
        Ok(self.status())
    }

    /// Close the optional preview and reveal the retained application, when present.
    #[must_use]
    pub fn close(&mut self) -> Status {
        self.state.close_preview();
        self.status()
    }

    /// Install one candidate through a single host-owned transaction.
    pub fn install(
        &mut self,
        gate: &rawr::reload::scheduler::Gate,
        publication: &rawr::reload::scheduler::publication::Slot,
        candidate: rawr::reload::scheduler::Candidate<(
            rawr::reload::Candidate,
            rawr::reload::scheduler::publication::Staged,
        )>,
        engine: &rawr::Shared<rawr::Engine>,
        runtime: rawr::host::runtime::Registry,
    ) -> Result<rawr::reload::scheduler::Admission<(), reload::Outcome>, reload::Error> {
        if self.snapshots.is_some() {
            return Err(reload::Error::Snapshots);
        }
        match self
            .state
            .install_control(gate, publication, candidate, engine, runtime)
            .map_err(reload::Error::host)?
        {
            rawr::reload::scheduler::Admission::Current((evidence, inputs)) => Ok(
                rawr::reload::scheduler::Admission::Current(reload::Outcome::new(evidence, inputs)),
            ),
            rawr::reload::scheduler::Admission::Obsolete(()) => {
                Ok(rawr::reload::scheduler::Admission::Obsolete(()))
            }
        }
    }

    /// Return read-only canonical reports retained by every live host session.
    #[must_use]
    pub fn diagnostics(&self) -> Vec<&rawr::diagnostic::Report> {
        self.state.reports()
    }

    /// Execute one deterministic snapshot request for this validated target.
    pub fn snapshot(&self, mode: crate::snapshot::Mode) -> Result<crate::snapshot::Outcome, Error> {
        self.snapshots
            .as_ref()
            .ok_or(Error::Root)?
            .execute(mode)
            .map_err(Error::Snapshot)
    }

    /// Run the native low-chrome host from this exact controlled state.
    pub fn run(self) -> iced::Result {
        crate::preview::host::Application::new(self.state).run()
    }

    /// Continue the running application from the State a previous runner
    /// process exported; `false` keeps the booted State.
    pub(crate) fn seed_state(&mut self, bytes: &[u8]) -> bool {
        self.state.seed_state(bytes)
    }

    /// Run this preview host with its private live-candidate service.
    pub fn run_live(self, service: crate::reload::Service) -> iced::Result {
        self.state.run(service)
    }

    #[cfg(test)]
    pub(crate) fn into_state(self) -> crate::preview::host::Host {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{Error, Selection};
    use crate::preview::Compiler;
    use crate::snapshot::Mode;

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn preview_opens_the_remembered_preview_and_remembers_picks() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../target/granita/preview-memory-{}",
            std::process::id()
        ));
        let open = || {
            Compiler::new()
                .unwrap()
                .check_named_source("control-app.rs", app_source())
                .unwrap()
                .prepare()
                .unwrap()
                .into_control()
        };
        let primary = open()
            .previews()
            .iter()
            .find(|preview| preview.name() == "Primary")
            .unwrap()
            .key()
            .clone();

        super::remember(&root, "COUNTER/primary").unwrap();
        let control = open().into_preview(&root);
        assert_eq!(
            control.status().selection(),
            &Selection::Preview(primary.clone())
        );

        // A pick in the host is what the next `rawr preview` opens.
        super::remember(&root, "nothing").unwrap();
        let mut state = open().into_preview(&root).into_state();
        state.pick(primary);
        assert_eq!(
            fs::read_to_string(root.join(".rawr/preview")).unwrap(),
            "counter/Primary"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn app_catalog_lists_and_controls_an_optional_preview() {
        let mut prepared = Compiler::new()
            .unwrap()
            .check_named_source("control-app.rs", app_source())
            .unwrap()
            .prepare()
            .unwrap();
        let control = prepared.control();

        assert_eq!(
            control
                .previews()
                .iter()
                .map(|preview| preview.name())
                .collect::<Vec<_>>(),
            ["Broken", "Primary"]
        );
        let primary = control
            .previews()
            .iter()
            .find(|preview| preview.name() == "Primary")
            .unwrap();
        assert_eq!(primary.group(), "counter");
        assert_eq!(primary.themes(), ["dark"]);
        assert!(!primary.snapshot());
        assert_eq!((primary.width(), primary.height()), (120, 80));
        let primary = primary.key().clone();
        let broken = control
            .previews()
            .iter()
            .find(|preview| preview.name() == "Broken")
            .unwrap()
            .key()
            .clone();

        let initial = control.status();
        assert_eq!(initial.selection(), &Selection::Application);
        assert!(initial.is_active());
        assert_eq!(initial.diagnostics(), 0);
        assert!(matches!(control.reset(), Err(Error::Application)));

        let selected = control.select(&primary).unwrap();
        assert_eq!(selected.selection(), &Selection::Preview(primary.clone()));
        assert!(selected.is_active());
        assert_eq!(selected.diagnostics(), 0);

        let error = control.select(&broken).unwrap_err();
        let report = error.report().unwrap();
        assert_eq!(report.code(), rawr::diagnostic::code::RUNTIME_THROW);
        assert!(report.message().contains("broken candidate"));
        assert_eq!(report.labels().len(), 1);
        assert_eq!(report.labels()[0].source().identity(), "control-app.rs");
        assert_eq!(
            report.labels()[0].span().start(),
            rawr::Position::new(17, 8)
        );
        assert_eq!(
            control.status().selection(),
            &Selection::Preview(primary.clone())
        );
        assert!(control.status().is_active());
        assert_eq!(control.diagnostics().len(), 1);

        assert_eq!(
            control.select(&primary).unwrap().selection(),
            &Selection::Preview(primary)
        );
        assert!(control.diagnostics().is_empty());
        let closed = control.close();
        assert_eq!(closed.selection(), &Selection::Application);
        assert!(closed.is_active());
    }

    #[test]
    fn preview_only_fallback_recovers_then_close_and_reset_reuse_the_same_owner() {
        let mut control = Compiler::new()
            .unwrap()
            .check_named_source("control-standalone.rs", standalone_source())
            .unwrap()
            .prepare()
            .unwrap()
            .into_control();
        let initial = control.status();
        assert!(matches!(initial.selection(), Selection::Preview(_)));
        assert!(!initial.is_active());
        assert_eq!(initial.diagnostics(), 1);
        assert!(control.diagnostics()[0]
            .message()
            .contains("broken standalone"));

        let healthy = control
            .previews()
            .iter()
            .find(|preview| preview.name() == "Healthy")
            .unwrap()
            .key()
            .clone();
        let selected = control.select(&healthy).unwrap();
        assert_eq!(selected.selection(), &Selection::Preview(healthy.clone()));
        assert!(selected.is_active());
        assert_eq!(selected.diagnostics(), 0);

        let closed = control.close();
        assert_eq!(closed.selection(), &Selection::Preview(healthy.clone()));
        assert!(!closed.is_active());
        assert!(control.diagnostics().is_empty());

        let reset = control.reset().unwrap();
        assert_eq!(reset.selection(), &Selection::Preview(healthy));
        assert!(reset.is_active());
    }

    #[test]
    fn rootless_control_rejects_snapshot_requests() {
        let control = Compiler::new()
            .unwrap()
            .check_named_source("control-rootless.rs", snapshot_source())
            .unwrap()
            .prepare()
            .unwrap()
            .into_control();

        assert!(matches!(control.snapshot(Mode::Check), Err(Error::Root)));
    }

    #[test]
    fn rooted_control_requests_the_same_complete_snapshot_suite() {
        let project = Project::new(snapshot_source());
        let target = rawr::project::Target::discover(project.path()).unwrap();
        let control = Compiler::new()
            .unwrap()
            .check(&target)
            .unwrap()
            .prepare()
            .unwrap()
            .into_control();

        let updated = control.snapshot(Mode::Update).unwrap();
        assert!(updated.updated());
        assert_eq!(updated.artifacts(), 1);
        let checked = control.snapshot(Mode::Check).unwrap();
        assert!(checked.is_successful());
        let root = fs::canonicalize(project.path()).unwrap();
        assert_eq!(checked.baseline(), root.join("snapshots"));
        assert_eq!(checked.actual(), root.join("target/rawr/snapshots"));
    }

    struct Project {
        root: PathBuf,
    }

    impl Project {
        fn new(source: &str) -> Self {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let root =
                std::env::temp_dir().join(format!("rawr-control-{}-{id}", std::process::id()));
            fs::create_dir_all(root.join("src")).unwrap();
            fs::write(
                root.join("Cargo.toml"),
                "[package]\nname = \"control-fixture\"\n",
            )
            .unwrap();
            crate::write_marked(&root, "src/main.rs", source);
            Self { root }
        }

        fn path(&self) -> &Path {
            &self.root
        }
    }

    impl Drop for Project {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).ok();
        }
    }

    fn app_source() -> &'static str {
        r#"
use iced::Theme;
use iced::Element;
use iced::widget::text;

#[derive(Default)] struct State;
#[derive(Debug, Clone, Copy)] enum Message { Tick }

impl State {
    fn update(&mut self, message: Message) {}
    fn dark() -> Theme { Theme::Dark }

    #[preview(name = "Primary", group = "counter", themes = [State::dark], snapshot = false, size = (120, 80))]
    fn view(&self) -> Element<'_, Message> { text("primary") }

    #[preview(name = "Broken")]
    fn broken(&self) -> Element<'_, Message> { throw "broken candidate" }
}

pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
    }

    fn standalone_source() -> &'static str {
        r#"
use iced::Element;
use iced::widget::text;

#[preview(name = "Broken")]
fn broken() -> Element<'_, ()> { throw "broken standalone" }

#[preview(name = "Healthy")]
fn healthy() -> Element<'_, ()> { text("healthy standalone") }
"#
    }

    fn snapshot_source() -> &'static str {
        r#"
use iced::Element;
use iced::widget::text;

#[preview(name = "Snapshot", size = (80, 60))]
fn sample() -> Element<'_, ()> { text("snapshot") }
"#
    }
}
