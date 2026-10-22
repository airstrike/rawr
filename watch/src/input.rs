//! Notify-specific filesystem event normalization.

use std::fs;
use std::path::{Component, Path, PathBuf};

use notify::event::ModifyKind;
use notify::{Event, EventKind};

/// One filesystem event normalized before entering the neutral revision gate.
pub(super) enum Change {
    Irrelevant,
    Rescan,
    Exact(Box<[PathBuf]>),
    /// Attributes changed on these paths themselves; contents below an
    /// ancestor directory are untouched (Cargo tags its caches this way).
    Metadata(Box<[PathBuf]>),
    Tree(Box<[PathBuf]>),
    Unknown(Box<[PathBuf]>),
}

impl Change {
    pub(super) fn from_event(event: &Event) -> Self {
        if event.need_rescan() {
            return Self::Rescan;
        }
        if matches!(event.kind, EventKind::Access(_)) {
            return Self::Irrelevant;
        }
        if event.paths.is_empty() || matches!(event.kind, EventKind::Any | EventKind::Other) {
            return Self::Rescan;
        }
        let mut paths = Vec::new();
        let mut redirected = false;
        for path in &event.paths {
            let Some(expanded) = retain_event_paths(path, &mut paths) else {
                return Self::Rescan;
            };
            redirected |= expanded;
        }
        let paths = paths.into_boxed_slice();
        match event.kind {
            EventKind::Access(_) => Self::Irrelevant,
            EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_)) => Self::Tree(paths),
            EventKind::Modify(ModifyKind::Any | ModifyKind::Other) => Self::Unknown(paths),
            EventKind::Any | EventKind::Other => Self::Rescan,
            // A redirected directory identity can name an ancestor of an
            // exact native input, even if the backend reports data or
            // metadata rather than an explicit rename.
            EventKind::Create(_)
            | EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_))
                if redirected =>
            {
                Self::Tree(paths)
            }
            EventKind::Modify(ModifyKind::Metadata(_)) => Self::Metadata(paths),
            EventKind::Create(_) | EventKind::Modify(ModifyKind::Data(_)) => Self::Exact(paths),
        }
    }

    pub(super) fn known_paths(&self) -> Option<&[PathBuf]> {
        match self {
            Self::Exact(paths) | Self::Metadata(paths) | Self::Tree(paths) => Some(paths),
            Self::Irrelevant | Self::Rescan | Self::Unknown(_) => None,
        }
    }

    pub(super) fn relevant(&self, inputs: &rawr::reload::scheduler::publication::Read<'_>) -> bool {
        match self {
            Self::Irrelevant => false,
            Self::Rescan => true,
            Self::Exact(paths) | Self::Metadata(paths) => {
                paths.iter().any(|path| inputs.contains(path))
            }
            Self::Tree(paths) | Self::Unknown(paths) => {
                paths.iter().any(|path| inputs.affects(path))
            }
        }
    }

    pub(super) fn relevant_inputs(&self, inputs: &rawr::reload::source::Inputs) -> bool {
        match self {
            Self::Irrelevant => false,
            Self::Rescan => true,
            Self::Exact(paths) => paths.iter().any(|path| {
                inputs.contains(path) || inputs.paths().iter().any(|input| input.starts_with(path))
            }),
            Self::Metadata(paths) => paths.iter().any(|path| inputs.contains(path)),
            Self::Tree(paths) | Self::Unknown(paths) => paths.iter().any(|path| {
                inputs.contains(path)
                    || inputs.paths().iter().any(|input| input.starts_with(path))
                    || inputs.trees().iter().any(|tree| tree.starts_with(path))
            }),
        }
    }
}

fn retain_event_paths(path: &Path, paths: &mut Vec<PathBuf>) -> Option<bool> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let original = normalize(&absolute);
    let start = paths.len();
    if let Ok(canonical) = fs::canonicalize(&absolute) {
        if canonical == original {
            paths.push(original);
            return Some(false);
        }
        paths.push(canonical);
    }
    paths.push(original);
    // Current resolution must not erase the watched identity replaced by a
    // symlink. Canonicalize prefixes separately: an earlier prefix can retain
    // the old input even when a later ancestor now resolves somewhere else.
    for ancestor in absolute.ancestors().skip(1) {
        let Ok(canonical) = fs::canonicalize(ancestor) else {
            continue;
        };
        let remainder = absolute.strip_prefix(ancestor).ok()?;
        let identity = normalize(&canonical.join(remainder));
        if !paths[start..].contains(&identity) {
            paths.push(identity);
        }
    }
    Some(paths.len() - start > 1)
}

fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = normalized.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::Change;
    use notify::event::{
        AccessKind, CreateKind, DataChange, Flag, ModifyKind, RemoveKind, RenameMode,
    };
    use notify::{Event, EventKind};
    use rawr::reload::compatibility::{Owner, Owners};
    use rawr::reload::scheduler::Admission;
    use std::fs;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    #[test]
    fn access_is_irrelevant_and_backend_rescan_is_conservative() {
        assert!(matches!(
            Change::from_event(&Event::new(EventKind::Access(AccessKind::Read))),
            Change::Irrelevant
        ));
        assert!(matches!(
            Change::from_event(&Event::new(EventKind::Other).set_flag(Flag::Rescan)),
            Change::Rescan
        ));
    }

    #[test]
    fn remove_retains_a_lexical_destination_after_its_parent_disappears() {
        let root = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("rawr-watch-remove-parent-{}", std::process::id()));
        let path = root.join("nested/source.rawr");
        let change = Change::from_event(
            &Event::new(EventKind::Remove(RemoveKind::Folder)).add_path(path.clone()),
        );
        let Change::Tree(paths) = change else {
            panic!("remove events must retain tree relevance");
        };
        assert!(paths.contains(&path));

        let modified = Change::from_event(
            &Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Any)))
                .add_path(root.join("other.rawr")),
        );
        assert!(matches!(modified, Change::Exact(_)));
    }

    #[test]
    fn canonical_regular_source_event_retains_one_identity() {
        let project = Project::new();
        project.write("main.rawr", "pub fn main() {}\n");
        let source = fs::canonicalize(project.root.join("main.rawr")).unwrap();
        let change = Change::from_event(
            &Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
                .add_path(source.clone()),
        );
        let Change::Exact(paths) = change else {
            panic!("a regular source edit must retain exact classification");
        };
        assert_eq!(paths.as_ref(), &[source]);
    }

    #[test]
    fn creating_an_ancestor_of_a_missing_imported_input_is_relevant() {
        let project = Project::new();
        project.write("rawr.toml", "[package]\nname = \"watch-input\"\n");
        project.write("src/main.rawr", "pub fn main() {}\n");
        let target = rawr::project::Target::discover(&project.root).unwrap();
        let graph = rawr::package::Graph::load(&rawr::Engine::new(), &target).unwrap();
        let source = rawr::reload::source::Graph::from_package(&graph);
        let root = fs::canonicalize(&project.root).unwrap();
        let missing = root.join("absent/config.toml");
        let inputs = rawr::reload::source::Inputs::from_graph(&source)
            .with_imported([rawr_import::input::proof::Watch::Exact(missing)]);

        let created = Change::from_event(
            &Event::new(EventKind::Create(CreateKind::Folder)).add_path(root.join("absent")),
        );
        let unrelated = Change::from_event(
            &Event::new(EventKind::Create(CreateKind::Folder)).add_path(root.join("other")),
        );

        assert!(created.relevant_inputs(&inputs));
        assert!(!unrelated.relevant_inputs(&inputs));
    }

    #[test]
    fn admitted_manifest_rollover_revokes_new_inputs_and_ignores_removed_inputs() {
        let project = Project::new();
        project.write("rawr.toml", manifest("old"));
        project.write("src/main.rawr", application());
        project.write("old/rawr.toml", "[package]\nname = \"old\"\n");
        project.write("old/src/lib.rawr", "pub fn value() -> i64 { 1 }\n");
        project.write("new/rawr.toml", "[package]\nname = \"new\"\n");
        project.write("new/src/lib.rawr", "pub fn value() -> i64 { 2 }\n");

        let mut engine = rawr::Engine::new();
        rawr_iced::install(&mut engine).unwrap();
        let engine = rawr::Shared::new(engine);
        let target = rawr::project::Target::discover(&project.root).unwrap();
        let first = engine.compile_package_generation(&target).unwrap();
        let publication = Arc::new(rawr::reload::scheduler::publication::Slot::new(
            rawr::reload::source::Inputs::from_graph(first.source()),
        ));
        let bootstrap = first.bind(&engine, registry()).unwrap();
        let mut slot = bootstrap.install(());

        project.write("rawr.toml", manifest("new"));
        let target = rawr::project::Target::discover(&project.root).unwrap();
        let candidate = engine.compile_package_generation(&target).unwrap();
        let snapshot = rawr::reload::source::Snapshot::from_graph(candidate.source());
        let gate = rawr::reload::scheduler::Gate::new(Duration::ZERO);
        let now = Instant::now();
        gate.observe(now).unwrap();
        let candidate = gate
            .start(now)
            .unwrap()
            .complete((candidate, snapshot))
            .stage_inputs(&publication)
            .try_map(|(candidate, staged)| {
                let plan = rawr::reload::compatibility::Plan::compare(
                    slot.generation(),
                    candidate,
                    Owners::one(Owner::Application),
                );
                let replacement = plan.bind(&engine, registry()).map_err(|_| ())?;
                let preparing = replacement.prepare(&slot).map_err(|_| ())?;
                Ok::<_, ()>((preparing.seal(()), staged))
            })
            .unwrap()
            .try_map(|(prepared, staged)| {
                staged.revalidate().map(|validated| (prepared, validated))
            })
            .unwrap();
        let Admission::Current(Ok((_evidence, retired, inputs))) =
            gate.admit_slot_and_publish(candidate, &mut slot, &publication)
        else {
            panic!("fresh candidate and exact publication must admit together");
        };
        let () = retired.release();
        inputs.release();

        let old = fs::canonicalize(project.root.join("old/src/lib.rawr")).unwrap();
        let new = fs::canonicalize(project.root.join("new/src/lib.rawr")).unwrap();
        let old = Change::from_event(&Event::new(EventKind::Modify(ModifyKind::Any)).add_path(old));
        let new = Change::from_event(&Event::new(EventKind::Modify(ModifyKind::Any)).add_path(new));
        let renamed_directory = Change::from_event(
            &Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::From)))
                .add_path(project.root.join("new")),
        );
        assert!(!old.relevant(&publication.read()));
        assert!(new.relevant(&publication.read()));
        assert!(renamed_directory.relevant(&publication.read()));
    }

    fn registry() -> rawr::host::runtime::Registry {
        let mut registry = rawr::host::runtime::Registry::new();
        for facade in rawr_iced::runtime_facades().unwrap() {
            registry.install(facade).unwrap();
        }
        registry
    }

    fn manifest(dependency: &str) -> String {
        format!(
            "[package]\nname = \"rollover\"\n\n[dependencies]\n{dependency} = {{ path = \"{dependency}\" }}\n"
        )
    }

    fn application() -> &'static str {
        r#"
use iced::Element;
use iced::widget::text;

#[derive(Default)] struct State;
#[derive(Debug, Clone, Copy)] enum Message { Tick }
impl State {
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<Message> { text("rollover") }
}
pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
    }

    struct Project {
        root: std::path::PathBuf,
    }

    impl Project {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "rawr-watch-rollover-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        fn write(&self, path: &str, text: impl AsRef<str>) {
            let path = self.root.join(path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, text.as_ref()).unwrap();
        }
    }

    impl Drop for Project {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).ok();
        }
    }
}
