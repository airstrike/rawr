//! rawr's own menu bar items in a program `rawr run` runs (#1990): a
//! `rawr` menu beside the program's own, with Restart, Take Screenshot,
//! About rawr and Exit. The program's menus stay as its iced made them,
//! upstream's or a fork's: rawr adds its menus after them and never takes
//! one of theirs away. A program `cargo run` runs has none of this.
//!
//! The program keeps its own name: the menu bar, the Dock and the
//! program's own app menu say the name its binary has under `cargo run`,
//! not the runner's.
//!
//! The socket also reads the menu bar, with `{"op": "menu"}`: `{"name":
//! .., "menus": [{"title": .., "items": [..]}]}`, every menu the program
//! shows and rawr's after them, a separator as `"-"`. With `"choose":
//! "Take Screenshot"`, it chooses that item of rawr's menus as a click
//! would, answered once it ran: `{"chosen": ..}`. Restart and Exit end the
//! runner, so their answer is the socket closing.
//!
//! Each system shows menus its own way; macOS's is the only one so far.

#[cfg(target_os = "macos")]
mod appkit;

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// What one of rawr's items does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    /// Start the program again, as a save it can't take in place does.
    Restart,
    /// Save a capture of the program's window under `target/rawr`.
    Screenshot,
    /// Show which rawr runs the program.
    About,
    /// End the run, as the program ending does.
    Exit,
}

/// One line of a menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Entry {
    Item(Action),
    Separator,
}

/// One of rawr's menus: its title in the menu bar and its lines.
#[derive(Debug)]
struct Menu {
    title: &'static str,
    entries: &'static [Entry],
}

/// rawr's menus, in the order they follow the program's. A debugger's
/// menu goes here when it comes.
const MENUS: &[Menu] = &[Menu {
    title: "rawr",
    entries: &[
        Entry::Item(Action::Restart),
        Entry::Item(Action::Screenshot),
        Entry::Separator,
        Entry::Item(Action::About),
        Entry::Separator,
        Entry::Item(Action::Exit),
    ],
}];

const ACTIONS: [Action; 4] = [
    Action::Restart,
    Action::Screenshot,
    Action::About,
    Action::Exit,
];

impl Action {
    /// The item's title.
    const fn title(self) -> &'static str {
        match self {
            Self::Restart => "Restart",
            Self::Screenshot => "Take Screenshot",
            Self::About => "About rawr",
            Self::Exit => "Exit",
        }
    }

    /// The number the item carries, which says what it does when chosen.
    fn tag(self) -> isize {
        ACTIONS
            .iter()
            .position(|action| *action == self)
            .map_or(0, |index| index as isize + 1)
    }

    fn from_tag(tag: isize) -> Option<Self> {
        let index = usize::try_from(tag.checked_sub(1)?).ok()?;
        ACTIONS.get(index).copied()
    }

    /// The action of the item of rawr's menus titled `title`.
    fn titled(title: &str) -> Option<Self> {
        ACTIONS.into_iter().find(|action| action.title() == title)
    }
}

/// Give the program's menu bar rawr's menus once its app has finished
/// launching, and the program its own name. The runner calls it first
/// thing under `rawr run`. A program that loads no window system gets
/// nothing.
pub fn install() {
    #[cfg(target_os = "macos")]
    appkit::install();
}

#[cfg(target_os = "macos")]
/// The name the program goes by: its binary's, as `rawr run` starts it
/// with its `cargo run` path.
fn name() -> Option<String> {
    let argument = std::env::args_os().next()?;
    let name = Path::new(&argument).file_name()?.to_str()?.to_owned();
    (!name.is_empty()).then_some(name)
}

#[cfg(target_os = "macos")]
/// Start the program again: the runner ends with the status `rawr run`
/// restarts on.
fn restart() -> ! {
    eprintln!("rawr: restarting the program, as its menu asked");
    crate::process::leave(crate::process::RESTART)
}

#[cfg(target_os = "macos")]
/// End the run.
fn exit() -> ! {
    crate::process::leave(0)
}

/// Where a capture taken at `millis` since the epoch goes, in the
/// project's `directory` (`target/rawr`).
fn screenshot_path(directory: &Path, name: &str, millis: u128) -> PathBuf {
    directory
        .join("screenshots")
        .join(format!("{name}-{millis}.png"))
}

/// Capture the window titled `title` into `screenshots` in the project's
/// `target/rawr`, as `rawr mcp`'s screenshot captures it: the window draws
/// the capture itself (#1989). The capture waits on the main thread, which
/// runs this, so it runs on a thread of its own. One line says where it
/// went.
#[cfg(target_os = "macos")]
fn screenshot(title: String) {
    let directory = std::env::var_os("RAWR_DIRECTORY")
        .filter(|directory| !directory.is_empty())
        .map_or_else(|| PathBuf::from("target/rawr"), PathBuf::from);
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    let path = screenshot_path(&directory, &name().unwrap_or_default(), millis);
    let spawned = std::thread::Builder::new()
        .name("rawr-screenshot".into())
        .spawn(move || {
            let captured = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .map_err(|error| error.to_string())
                .and_then(|()| {
                    super::screenshot::capture(Some(&title), &path, &|| false)
                        .map_err(|error| error.to_string())
                });
            match captured {
                Ok(_) => eprintln!(
                    "rawr: saved a screenshot of the window to {}",
                    path.display()
                ),
                Err(error) => eprintln!("rawr: the screenshot failed: {error}"),
            }
        });
    if let Err(error) = spawned {
        eprintln!("rawr: the screenshot failed: {error}");
    }
}

/// Answer a `menu` request, or `None` once `interrupted` gave it up.
pub(super) fn answer(request: &Value, interrupted: &dyn Fn() -> bool) -> Option<Value> {
    let choose = request.get("choose").and_then(Value::as_str);
    let action = match choose.map(|title| (title, Action::titled(title))) {
        Some((title, None)) => {
            return Some(json!({ "error": format!("rawr's menus have no item `{title}`") }));
        }
        Some((_, action)) => action,
        None => None,
    };
    #[cfg(target_os = "macos")]
    {
        appkit::answer(action, interrupted)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (action, interrupted);
        Some(json!({ "error": "rawr's menus need macOS here" }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_item_carries_a_tag_that_names_it_again() {
        for action in ACTIONS {
            assert_eq!(Action::from_tag(action.tag()), Some(action));
            assert_eq!(Action::titled(action.title()), Some(action));
        }
        // An item without a tag, as AppKit makes them, is none of rawr's.
        assert_eq!(Action::from_tag(0), None);
        assert_eq!(Action::from_tag(5), None);
    }

    #[test]
    fn rawrs_menu_has_every_item_once() {
        let items: Vec<Action> = MENUS
            .iter()
            .flat_map(|menu| menu.entries)
            .filter_map(|entry| match entry {
                Entry::Item(action) => Some(*action),
                Entry::Separator => None,
            })
            .collect();
        assert_eq!(items, ACTIONS);
        assert_eq!(MENUS[0].title, "rawr");
    }

    #[test]
    fn a_screenshot_goes_under_target_rawr_by_the_programs_name() {
        assert_eq!(
            screenshot_path(Path::new("/p/target/rawr"), "counter", 17),
            Path::new("/p/target/rawr/screenshots/counter-17.png")
        );
    }

    #[test]
    fn choosing_an_item_rawr_has_none_of_says_so() {
        let answer = answer(&json!({ "op": "menu", "choose": "Debug" }), &|| true).unwrap();
        assert_eq!(answer["error"], "rawr's menus have no item `Debug`");
    }
}
