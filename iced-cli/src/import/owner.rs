//! rawr-iced's programs under the runner owner rawr-import provides: the
//! owner prepares each through the iced [`Iced`] profile. Snapshots are
//! rawr-iced's own.

use rawr::diagnostic;
use rawr_import_cli::owner::{fallback, framework, initial, same_build};

use super::Iced;

#[cfg(test)]
type Lane = rawr_import_cli::owner::Lane;

/// Check one imported iced program without opening a window: its Rust
/// imports are resolved and extracted and the Rawr source is checked
/// against them. No runner is built; `rawr run` builds it.
pub(crate) fn check(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
) -> Result<std::path::PathBuf, diagnostic::Report> {
    rawr_import_cli::owner::check(target, graph, &Iced)
}

/// Run one imported iced application or preview under the runner owner.
/// `Ok(false)` is a program that ended with a failing exit: it printed its
/// own error as it ran.
pub(crate) fn run(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    lifecycle: rawr_import::process::Lifecycle,
) -> Result<bool, diagnostic::Report> {
    rawr_import_cli::owner::run(target, graph, lifecycle, &Iced)
}

/// Execute one imported snapshot owner and decode its bounded authenticated
/// outcome into the command's ordinary public result.
pub(crate) fn snapshot(
    target: &rawr::project::Target,
    mode: crate::command::Mode,
) -> Result<rawr_iced::snapshot::Outcome, diagnostic::Report> {
    same_build(target)?;
    let lifecycle = match mode {
        crate::command::Mode::Check => rawr_import::process::Lifecycle::SnapshotCheck,
        crate::command::Mode::Update => rawr_import::process::Lifecycle::SnapshotUpdate,
    };
    let source = fallback(target)?;
    let (mut lane, _) = initial(target, None, lifecycle, false, &Iced)?;
    let outcome = lane
        .snapshot()
        .map_err(|error| framework(error, source.clone()));
    let shutdown = lane
        .shutdown()
        .map_err(|error| framework(error, source.clone()));
    let bytes = outcome?;
    shutdown?;
    rawr_iced::snapshot::wire::decode(&bytes).map_err(|error| framework(error, source))
}

#[cfg(test)]
mod tests {
    use super::{initial, Iced};
    use std::fs;

    use rawr_import::process::hot::Outcome;

    #[test]
    #[ignore = "production cutover gate starts one real run owner and admits watched recovery"]
    fn production_run_and_watch_preserve_one_real_generated_last_good_owner() {
        let project = crate::import::tests::real_iced_fixture();
        let root = project.path().to_path_buf();
        let source = root.join("src/main.rs");
        let original = fs::read_to_string(&source).expect("fixture source reads");
        let target = rawr::project::Target::discover(&root).expect("run target discovers");
        let (mut lane, _) = initial(
            &target,
            None,
            rawr_import::process::Lifecycle::Run,
            true,
            &Iced,
        )
        .unwrap_or_else(|error| panic!("production imported run rejects: {error}"));
        let initial = lane.current_key().expect("run owner is current");

        fs::write(
            &source,
            original.replace("text(\"generated\")", "text(\"reloaded\")"),
        )
        .expect("valid watched source writes");
        let reloaded = hot_reload(&mut lane, &root).expect("valid watched source admits");
        assert_ne!(reloaded, initial);
        assert_eq!(lane.current_key(), Some(reloaded));

        fs::write(&source, "pub fn main( {").expect("invalid watched source writes");
        let revision = lane.revision().expect("invalid revision allocates");
        let invalid = rawr::project::Target::discover(&root).expect("invalid target discovers");
        assert!(crate::import::prepare(&invalid, revision,).is_err());
        assert_eq!(lane.current_key(), Some(reloaded));
        lane.report("injected invalid watched generation")
            .expect("last-good owner accepts the bounded failure");

        fs::write(
            &source,
            original.replace("text(\"generated\")", "text(\"recovered\")"),
        )
        .expect("recovered watched source writes");
        let recovered = hot_reload(&mut lane, &root).expect("recovered watched source admits");
        assert_ne!(recovered, reloaded);
        assert_eq!(lane.current_key(), Some(recovered));
        lane.shutdown().expect("run owner exits and reaps");
        fs::remove_dir_all(root).ok();
    }

    #[test]
    #[ignore = "production cutover gate starts and retires one real preview owner"]
    fn production_preview_starts_and_retires_one_real_generated_owner() {
        let project = crate::import::tests::real_iced_fixture();
        let root = project.path().to_path_buf();
        let target = rawr::project::Target::discover(&root).expect("preview target discovers");
        let (mut preview, _) = initial(
            &target,
            None,
            rawr_import::process::Lifecycle::Preview,
            true,
            &Iced,
        )
        .unwrap_or_else(|error| panic!("production imported preview rejects: {error}"));
        assert!(preview.current_key().is_some());
        preview.shutdown().expect("preview owner exits and reaps");
        fs::remove_dir_all(root).ok();
    }

    fn hot_reload(
        lane: &mut super::Lane,
        root: &std::path::Path,
    ) -> Result<rawr_import::supervisor::GenerationKey, Box<dyn std::error::Error>> {
        let revision = lane.revision()?;
        let target = rawr::project::Target::discover(root)?;
        let prepared = crate::import::prepare(&target, revision)?;
        let (request, snapshot, _, _) = prepared.into_parts();
        match lane.compile(request, snapshot)? {
            Outcome::Hit(key) => Ok(key),
            Outcome::Miss(_) => {
                Err("same imported surface unexpectedly required a cold owner".into())
            }
        }
    }
}
