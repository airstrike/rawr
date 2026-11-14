//! AppKit's side of rawr's menus. The program's name is set as the runner
//! starts, before its app finishes launching, as its process name, which
//! winit's app menu reads for "Quit counter". rawr's menus are added to
//! the app's main menu on the main queue, which runs once the app's event
//! loop runs, after the app finished launching and made its own menus.
//! The display name LaunchServices gives the menu bar, the Dock and app
//! switching is set then too. Each item's target is an object of a class of
//! rawr's own, whose action runs the item on the main thread.

use std::ffi::{c_char, c_void, CStr};
use std::sync::OnceLock;

use serde_json::{json, Value};

use super::{Action, Entry, Menu, MENUS};
use crate::inspect::input::appkit::{self, msg, Id, Runtime, Sel};
use crate::inspect::input::Error;

/// The identifier each of rawr's menus' items in the menu bar carries, so
/// rawr finds them again.
const ROOT: &CStr = c"rawr.menu";

/// The selector of the action each of rawr's items sends.
const CHOSEN: &str = "rawrChosen:\0";

unsafe extern "C" {
    fn dispatch_async_f(
        queue: *const c_void,
        context: *mut c_void,
        work: unsafe extern "C" fn(*mut c_void),
    );
    static _dispatch_main_q: u8;
}

/// The Objective-C runtime's functions that make a class.
struct Classes {
    allocate: unsafe extern "C" fn(Id, *const c_char, usize) -> Id,
    method: unsafe extern "C" fn(Id, Sel, *const c_void, *const c_char) -> bool,
    register: unsafe extern "C" fn(Id),
}

/// What a chosen item runs: [`perform`] in a program, a recorder in a test.
static PERFORM: OnceLock<fn(Action)> = OnceLock::new();

pub(super) fn install() {
    let Some(runtime) = Runtime::get() else {
        return;
    };
    if PERFORM.set(perform).is_err() {
        return;
    }
    if let Some(name) = super::name() {
        rename(runtime, &name);
    }
    unsafe extern "C" fn attach(_: *mut c_void) {
        if let Some(runtime) = Runtime::get() {
            add(runtime);
        }
    }
    // SAFETY: the main queue runs `attach` once, which takes no context.
    unsafe {
        dispatch_async_f(
            (&raw const _dispatch_main_q).cast(),
            std::ptr::null_mut(),
            attach,
        );
    }
}

/// Give the process `name`: the process name AppKit's menus read, and the
/// display name LaunchServices shows.
fn rename(runtime: &Runtime, name: &str) {
    // SAFETY: the pool is pushed and popped on this thread around the
    // objects this call makes.
    let pool = unsafe { (runtime.push)() };
    let info = msg!(runtime, Id; runtime.class(c"NSProcessInfo"), "processInfo");
    msg!(runtime, (); info, "setProcessName:", runtime.string(name) => Id);
    // SAFETY: each type is the C signature of the LaunchServices function
    // its symbol names; the key is a constant `CFStringRef`.
    let found = unsafe {
        (
            appkit::function::<unsafe extern "C" fn() -> *const c_void>(
                c"_LSGetCurrentApplicationASN",
            ),
            appkit::function::<unsafe extern "C" fn(i32, *const c_void, Id, Id, *mut c_void) -> i32>(
                c"_LSSetApplicationInformationItem",
            ),
        )
    };
    let key = appkit::symbol(c"_kLSDisplayNameKey").cast::<Id>();
    if let ((Some(current), Some(set)), false) = (found, key.is_null()) {
        // The session every app of the person's login runs in.
        const SESSION: i32 = -2;
        // SAFETY: the current app's serial number and a constant key, set
        // to a string of this pool's.
        unsafe {
            let serial = current();
            if !serial.is_null() {
                set(
                    SESSION,
                    serial,
                    key.read(),
                    runtime.string(name),
                    std::ptr::null_mut(),
                );
            }
        }
    }
    // SAFETY: `pool` was pushed above on this thread.
    unsafe { (runtime.pop)(pool) };
}

/// Add rawr's menus after the app's own, once: on the main thread, in the
/// main menu the app made, or in a new one when it made none.
fn add(runtime: &Runtime) {
    // LaunchServices takes the display name once the app checked in with
    // it, which it does as it finishes launching: a name set before then
    // is lost.
    if let Some(name) = super::name() {
        rename(runtime, &name);
    }
    let application = runtime.application();
    let Some(target) = target(runtime) else {
        return;
    };
    if application.is_null() {
        return;
    }
    // SAFETY: the pool is pushed and popped on this thread around the
    // objects this call makes.
    let pool = unsafe { (runtime.push)() };
    let mut main = msg!(runtime, Id; application, "mainMenu");
    if main.is_null() {
        main = menu(runtime, "");
        // The first menu is the app's own, whose title the app's name is.
        msg!(runtime, (); main, "addItem:", root(runtime, menu(runtime, "")) => Id);
        msg!(runtime, (); application, "setMainMenu:", main => Id);
        msg!(runtime, (); main, "release");
    }
    if !ours(runtime, main).is_empty() {
        // SAFETY: `pool` was pushed above on this thread.
        unsafe { (runtime.pop)(pool) };
        return;
    }
    for menu in MENUS {
        let item = build(runtime, menu, target);
        msg!(runtime, (); main, "addItem:", item => Id);
        msg!(runtime, (); item, "release");
    }
    // SAFETY: `pool` was pushed above on this thread.
    unsafe { (runtime.pop)(pool) };
}

/// A new menu titled `title`, which the caller releases.
fn menu(runtime: &Runtime, title: &str) -> Id {
    let made = msg!(runtime, Id; runtime.class(c"NSMenu"), "alloc");
    msg!(runtime, Id; made, "initWithTitle:", runtime.string(title) => Id)
}

/// A menu bar item showing `submenu`, which it takes, autoreleased.
fn root(runtime: &Runtime, submenu: Id) -> Id {
    let item = item(runtime, "", None);
    msg!(runtime, (); item, "setSubmenu:", submenu => Id);
    msg!(runtime, (); submenu, "release");
    msg!(runtime, Id; item, "autorelease")
}

/// A new item titled `title` that sends [`CHOSEN`] when `action` says so,
/// which the caller releases.
fn item(runtime: &Runtime, title: &str, action: Option<Sel>) -> Id {
    let made = msg!(runtime, Id; runtime.class(c"NSMenuItem"), "alloc");
    msg!(runtime, Id; made, "initWithTitle:action:keyEquivalent:",
        runtime.string(title) => Id,
        action.unwrap_or(std::ptr::null()) => Sel,
        runtime.string("") => Id,
    )
}

/// The menu bar item of `menu`, its items sending their action to
/// `target`, which the caller releases.
fn build(runtime: &Runtime, menu: &Menu, target: Id) -> Id {
    let submenu = self::menu(runtime, menu.title);
    let chosen = runtime.selector(CHOSEN);
    for entry in menu.entries {
        match entry {
            Entry::Separator => {
                let separator = msg!(runtime, Id; runtime.class(c"NSMenuItem"), "separatorItem");
                msg!(runtime, (); submenu, "addItem:", separator => Id);
            }
            Entry::Item(action) => {
                let line = item(runtime, action.title(), Some(chosen));
                msg!(runtime, (); line, "setTarget:", target => Id);
                msg!(runtime, (); line, "setTag:", action.tag() => isize);
                msg!(runtime, (); submenu, "addItem:", line => Id);
                msg!(runtime, (); line, "release");
            }
        }
    }
    let root = item(runtime, menu.title, None);
    msg!(runtime, (); root, "setSubmenu:", submenu => Id);
    msg!(runtime, (); submenu, "release");
    msg!(runtime, (); root, "setIdentifier:", runtime.string(&ROOT.to_string_lossy()) => Id);
    root
}

/// The menus of rawr's in `main`: each one's index there.
fn ours(runtime: &Runtime, main: Id) -> Vec<isize> {
    let identifier = runtime.string(&ROOT.to_string_lossy());
    let count = msg!(runtime, isize; main, "numberOfItems");
    (0..count)
        .filter(|&index| {
            let item = msg!(runtime, Id; main, "itemAtIndex:", index => isize);
            let named = msg!(runtime, Id; item, "identifier");
            !named.is_null() && msg!(runtime, u8; named, "isEqualToString:", identifier => Id) != 0
        })
        .collect()
}

/// The object every item of rawr's sends its action to, made once, with
/// its class: it lives as long as the process.
fn target(runtime: &Runtime) -> Option<Id> {
    static TARGET: OnceLock<usize> = OnceLock::new();
    if let Some(target) = TARGET.get() {
        return Some(*target as Id);
    }
    // SAFETY: each field's type is the C signature of the function its
    // symbol names.
    let classes = unsafe {
        Classes {
            allocate: appkit::function(c"objc_allocateClassPair")?,
            method: appkit::function(c"class_addMethod")?,
            register: appkit::function(c"objc_registerClassPair")?,
        }
    };
    unsafe extern "C" fn chosen(_this: Id, _selector: Sel, item: Id) {
        let Some(runtime) = Runtime::get() else {
            return;
        };
        let tag = msg!(runtime, isize; item, "tag");
        if let (Some(action), Some(perform)) = (Action::from_tag(tag), PERFORM.get()) {
            perform(action);
        }
    }
    let name = c"RawrMenuTarget";
    let mut class = runtime.class(name);
    if class.is_null() {
        // SAFETY: a new subclass of `NSObject` with one method, of the
        // signature `v@:@` says (it returns nothing and takes the item),
        // registered before any object of it is made.
        unsafe {
            class = (classes.allocate)(runtime.class(c"NSObject"), name.as_ptr(), 0);
            if class.is_null() {
                return None;
            }
            let method: unsafe extern "C" fn(Id, Sel, Id) = chosen;
            (classes.method)(
                class,
                runtime.selector(CHOSEN),
                method as *const c_void,
                c"v@:@".as_ptr(),
            );
            (classes.register)(class);
        }
    }
    let made = msg!(runtime, Id; msg!(runtime, Id; class, "alloc"), "init");
    Some(*TARGET.get_or_init(|| made as usize) as Id)
}

/// Run `action`, chosen from rawr's menus, on the main thread.
fn perform(action: Action) {
    let Some(runtime) = Runtime::get() else {
        return;
    };
    match action {
        Action::Restart => super::restart(),
        Action::Exit => super::exit(),
        Action::About => about(runtime),
        Action::Screenshot => match shown(runtime) {
            Some(window) => super::screenshot(runtime.text(msg!(runtime, Id; window, "title"))),
            None => eprintln!("rawr: the program shows no window to take a screenshot of"),
        },
    }
}

/// The program's window a screenshot shows: its main window, else its
/// front one. A panel, such as About's, is never the main window.
fn shown(runtime: &Runtime) -> Option<Id> {
    let main = msg!(runtime, Id; runtime.application(), "mainWindow");
    if main.is_null() {
        appkit::window(runtime, None)
    } else {
        Some(main)
    }
}

/// Show AppKit's About panel for rawr: its name and version.
fn about(runtime: &Runtime) {
    // SAFETY: the pool is pushed and popped on this thread around the
    // objects this call makes.
    let pool = unsafe { (runtime.push)() };
    let keys = [
        runtime.string("ApplicationName"),
        runtime.string("ApplicationVersion"),
        runtime.string("Version"),
    ];
    let values = [
        runtime.string("rawr"),
        runtime.string(env!("CARGO_PKG_VERSION")),
        runtime.string(""),
    ];
    let options = msg!(runtime, Id; runtime.class(c"NSDictionary"),
        "dictionaryWithObjects:forKeys:count:",
        values.as_ptr() => *const Id,
        keys.as_ptr() => *const Id,
        keys.len() => usize,
    );
    let application = runtime.application();
    msg!(runtime, (); application, "orderFrontStandardAboutPanelWithOptions:", options => Id);
    // SAFETY: `pool` was pushed above on this thread.
    unsafe { (runtime.pop)(pool) };
}

/// The program's menu bar, and `action` chosen from it, on the main
/// thread; `None` once `interrupted` gave it up.
pub(super) fn answer(action: Option<Action>, interrupted: &dyn Fn() -> bool) -> Option<Value> {
    let Some(runtime) = Runtime::get() else {
        return Some(json!({ "error": "the program shows no menu bar" }));
    };
    let read = appkit::main(interrupted, move || {
        let application = runtime.application();
        if application.is_null() {
            return json!({ "error": "the program shows no menu bar" });
        }
        // SAFETY: the pool is pushed and popped on this thread around the
        // objects this call makes.
        let pool = unsafe { (runtime.push)() };
        let main = msg!(runtime, Id; application, "mainMenu");
        let answer = match action {
            Some(action) if choose(runtime, main, action) => json!({ "chosen": action.title() }),
            Some(_) => json!({ "error": "rawr's menus aren't in the menu bar yet" }),
            None => bar(runtime, main),
        };
        // SAFETY: `pool` was pushed above on this thread.
        unsafe { (runtime.pop)(pool) };
        answer
    });
    match read {
        Ok(answer) => Some(answer),
        Err(Error::Interrupted) => None,
        Err(error) => Some(json!({ "error": error.to_string() })),
    }
}

/// The app's name and every menu of `main`, its items' titles in order.
fn bar(runtime: &Runtime, main: Id) -> Value {
    let info = msg!(runtime, Id; runtime.class(c"NSProcessInfo"), "processInfo");
    let current = msg!(runtime, Id; runtime.class(c"NSRunningApplication"), "currentApplication");
    let count = if main.is_null() {
        0
    } else {
        msg!(runtime, isize; main, "numberOfItems")
    };
    let menus: Vec<Value> = (0..count)
        .map(|index| {
            let item = msg!(runtime, Id; main, "itemAtIndex:", index => isize);
            let submenu = msg!(runtime, Id; item, "submenu");
            let lines = if submenu.is_null() {
                0
            } else {
                msg!(runtime, isize; submenu, "numberOfItems")
            };
            let items: Vec<String> = (0..lines)
                .map(|line| {
                    let line = msg!(runtime, Id; submenu, "itemAtIndex:", line => isize);
                    if msg!(runtime, u8; line, "isSeparatorItem") != 0 {
                        "-".to_owned()
                    } else {
                        runtime.text(msg!(runtime, Id; line, "title"))
                    }
                })
                .collect();
            json!({ "title": runtime.text(msg!(runtime, Id; item, "title")), "items": items })
        })
        .collect();
    json!({
        "name": runtime.text(msg!(runtime, Id; current, "localizedName")),
        "process": runtime.text(msg!(runtime, Id; info, "processName")),
        "menus": menus,
    })
}

/// Choose `action`'s item in rawr's menus in `main`: its action is sent
/// to its target, as a click sends it. `false` when they aren't there.
fn choose(runtime: &Runtime, main: Id, action: Action) -> bool {
    if main.is_null() {
        return false;
    }
    for index in ours(runtime, main) {
        let item = msg!(runtime, Id; main, "itemAtIndex:", index => isize);
        let submenu = msg!(runtime, Id; item, "submenu");
        let line = msg!(runtime, isize; submenu, "indexOfItemWithTag:", action.tag() => isize);
        if line >= 0 {
            let line = msg!(runtime, Id; submenu, "itemAtIndex:", line => isize);
            let target = msg!(runtime, Id; line, "target");
            let action = msg!(runtime, Sel; line, "action");
            // SAFETY: the item's action is `CHOSEN`, a method of its target
            // that takes the item and returns nothing, as AppKit sends it
            // when the item is clicked.
            unsafe {
                let function: unsafe extern "C" fn(Id, Sel, Id) = std::mem::transmute(runtime.send);
                function(target, action, line);
            }
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    unsafe extern "C" {
        fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
    }

    /// The actions chosen in this process's test.
    static CHOSEN_ACTIONS: Mutex<Vec<Action>> = Mutex::new(Vec::new());

    fn record(action: Action) {
        CHOSEN_ACTIONS.lock().unwrap().push(action);
    }

    /// rawr's menu, built as the menu bar gets it, sends each item's own
    /// action to rawr's target when chosen, with no window system needed.
    #[test]
    fn each_of_rawrs_items_runs_its_own_action_when_chosen() {
        // SAFETY: loading a system framework by its path.
        let handle = unsafe {
            dlopen(
                c"/System/Library/Frameworks/AppKit.framework/AppKit".as_ptr(),
                1,
            )
        };
        assert!(!handle.is_null());
        let runtime = Runtime::get().expect("AppKit is loaded");
        assert!(
            PERFORM.set(record).is_ok(),
            "only this test sets what an item runs"
        );
        // SAFETY: the pool is pushed and popped on this thread.
        let pool = unsafe { (runtime.push)() };
        let target = target(runtime).expect("rawr's target class is made");
        let main = menu(runtime, "");
        msg!(runtime, (); main, "addItem:", root(runtime, menu(runtime, "counter")) => Id);
        assert!(ours(runtime, main).is_empty());
        let built = build(runtime, &MENUS[0], target);
        msg!(runtime, (); main, "addItem:", built => Id);
        msg!(runtime, (); built, "release");
        // rawr's menu follows the program's own, which stays first.
        assert_eq!(ours(runtime, main), [1]);
        let menus = bar(runtime, main)["menus"].clone();
        assert_eq!(
            menus,
            json!([
                { "title": "", "items": [] },
                {
                    "title": "rawr",
                    "items": ["Restart", "Take Screenshot", "-", "About rawr", "-", "Exit"],
                },
            ])
        );
        for action in super::super::ACTIONS {
            assert!(choose(runtime, main, action));
        }
        assert_eq!(*CHOSEN_ACTIONS.lock().unwrap(), super::super::ACTIONS);
        msg!(runtime, (); main, "release");
        // SAFETY: `pool` was pushed above on this thread.
        unsafe { (runtime.pop)(pool) };
    }
}
