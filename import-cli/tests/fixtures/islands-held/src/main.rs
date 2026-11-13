//! Native Rust: the workspace holds the marked home screen by value, as
//! inboard's `Workspace` holds `screen::home::State`, makes it with
//! `State::new()` and calls its methods, never reading its fields.
mod editor;
mod home;

/// The app's state, native, holding the marked screen by name.
struct Workspace {
    home: home::State,
    registry: editor::Registry,
}

fn main() {
    let mut workspace = Workspace {
        home: home::State::new(),
        registry: editor::Registry::new(&["mail", "notes"]),
    };
    workspace.home.type_text("hello");
    println!("{}", workspace.home.title());
    // The title and the sidebar both borrow the screen, and the sidebar
    // the registry too, read after their calls.
    let title = workspace.home.title();
    let sidebar = workspace.home.sidebar(&workspace.registry);
    println!("{} {title}", sidebar.draw());
    println!("{:?}", workspace.home);
    workspace.home.type_text(" world");
    println!("{} {}", workspace.home.title(), workspace.home.view());
    // A screen moved to another thread and back, as it is `Send`.
    let spare = home::State::new();
    let spare = std::thread::spawn(move || spare).join().unwrap();
    println!("{}", spare.view());
    // Run on, as a running program does, till told to stop: the workspace
    // ticks its screen and views it each tick.
    if let Some(stop) = std::env::var_os("ISLANDS_STOP") {
        let mut tick = 0;
        while !std::path::Path::new(&stop).exists() {
            tick += 1;
            workspace.home.tick();
            println!("tick {tick} {}", workspace.home.view());
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}
