//! The window on a real run's session, driven as a person drives it: it
//! sets a breakpoint in the gutter, waits for the program to stop there,
//! shows the stop's frames and bindings, steps and continues. It needs a
//! `rawr run` going, so it is ignored unless asked for:
//!
//! - `RAWR_GUI_SOCKET`: the run's session socket;
//! - `RAWR_GUI_LINE`: the line of the program's entry file to stop at;
//! - `RAWR_GUI_BINDING`: a binding the stop shows, which the console
//!   evaluates there.
//!
//! The program reaches the line by itself, or by input another shell
//! sends it. Snapshots of the window go to `target/tmp/rawr-gui`.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use iced::futures::executor::block_on;
use iced::futures::{Stream, StreamExt as _};
use iced::keyboard::key::Named;
use iced::Theme;
use iced_test::Simulator;
use rawr_gui::app::Message;
use rawr_gui::session::{self, Event, Phase};
use rawr_gui::{console, App};

/// The window and its connection, with the work its updates started.
struct Window {
    app: App,
    session: Pin<Box<dyn Stream<Item = Event>>>,
}

impl Window {
    /// Hand the window `message`, and run the work that starts to its end.
    fn update(&mut self, message: Message) {
        let mut tasks = vec![self.app.update(message)];
        while let Some(task) = tasks.pop() {
            let Some(mut stream) = iced_runtime::task::into_stream(task) else {
                continue;
            };
            while let Some(action) = block_on(stream.next()) {
                if let iced_runtime::Action::Output(message) = action {
                    tasks.push(self.app.update(message));
                }
            }
        }
    }

    /// Take what the session tells until `done` holds.
    fn until(&mut self, what: &str, done: impl Fn(&App) -> bool) {
        while !done(&self.app) {
            let event = block_on(self.session.next()).expect("the connection's stream goes on");
            if let Event::Closed(why) = &event {
                panic!("the session closed waiting for {what}: {why}");
            }
            self.update(Message::Session(event));
        }
    }

    fn interact(&mut self, act: impl FnOnce(&mut Simulator<'_, Message>)) {
        let messages: Vec<Message> = {
            let mut ui = tall(&self.app);
            act(&mut ui);
            ui.into_messages().collect()
        };
        for message in messages {
            self.update(message);
        }
    }

    fn snapshot(&self, name: &str) {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join("rawr-gui");
        std::fs::create_dir_all(&directory).unwrap();
        for entry in std::fs::read_dir(&directory).unwrap().flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with(name.trim_end_matches(".png"))
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        let shot = tall(&self.app).snapshot(&Theme::Dark).unwrap();
        assert!(shot.matches_image(directory.join(name)).unwrap());
    }
}

fn shows(app: &App, text: &str) -> bool {
    tall(app).find(text).is_ok()
}

/// The window at a height that shows a whole short file, whose lines the
/// test clicks without scrolling.
fn tall(app: &App) -> Simulator<'_, Message> {
    Simulator::with_size(iced::Settings::default(), (1200.0, 1500.0), app.view())
}

fn stopped_line(app: &App) -> Option<u64> {
    app.phase().stop().and_then(|stop| stop.line)
}

#[test]
#[ignore = "needs a `rawr run` going: RAWR_GUI_SOCKET, RAWR_GUI_LINE and RAWR_GUI_BINDING"]
fn the_window_attaches_to_a_run_stops_at_its_breakpoint_steps_and_continues() {
    let socket = PathBuf::from(std::env::var("RAWR_GUI_SOCKET").expect("RAWR_GUI_SOCKET"));
    let line: u64 = std::env::var("RAWR_GUI_LINE")
        .expect("RAWR_GUI_LINE")
        .parse()
        .expect("RAWR_GUI_LINE is a line number");
    let binding = std::env::var("RAWR_GUI_BINDING").expect("RAWR_GUI_BINDING");
    let mut window = Window {
        app: App::new(socket.clone()),
        session: Box::pin(session::connect(&socket)),
    };
    let gutter = format!("{line:>4}");
    window.until("the entry file", |app| shows(app, &gutter));
    window.interact(|ui| {
        let _ = ui.click(gutter.as_str()).unwrap();
    });
    window.until("the breakpoint", |app| shows(app, "●"));
    window.snapshot("attached.png");

    window.until("the stop", |app| {
        stopped_line(app) == Some(line) && shows(app, &binding)
    });
    window.snapshot("attached-stop.png");

    // The console evaluates the binding where the program stopped.
    window.update(Message::Console(console::Message::Input(binding.clone())));
    window.interact(|ui| {
        let _ = ui.click(iced::widget::Id::new(console::INPUT)).unwrap();
        let _ = ui.tap_key(Named::Enter);
    });
    window.until("the eval's answer", |app| {
        shows(app, &format!("> {binding}")) && !shows(app, "…")
    });

    // The breakpoint comes off while its line is shown, so the program
    // runs on after the step.
    window.interact(|ui| {
        let _ = ui.click(gutter.as_str()).unwrap();
    });
    window.until("the breakpoint to clear", |app| !shows(app, "●"));

    // The step pauses again, wherever the program goes next: a step out
    // of a function native code called pauses in the program's next call
    // from it, where the stop's bindings may be out of scope.
    let stop = window.app.phase().stop().cloned();
    let went_on = Cell::new(false);
    window.interact(|ui| {
        let _ = ui.click("Step over").unwrap();
    });
    window.until("the step", |app| {
        went_on.set(went_on.get() || *app.phase() == Phase::Running);
        app.phase()
            .stop()
            .is_some_and(|now| went_on.get() || Some(now) != stop.as_ref())
    });
    window.snapshot("attached-step.png");

    window.interact(|ui| {
        let _ = ui.click("Continue").unwrap();
    });
    window.until("the program to run on", |app| {
        *app.phase() == Phase::Running
    });

    // Pause stops it at the next statement it runs.
    window.interact(|ui| {
        let _ = ui.click("Pause").unwrap();
    });
    window.until(
        "the pause",
        |app| matches!(app.phase().stop(), Some(stop) if stop.reason == "pause"),
    );
    window.snapshot("attached-pause.png");
    window.interact(|ui| {
        let _ = ui.click("Continue").unwrap();
    });
    window.until("the program to run on", |app| {
        *app.phase() == Phase::Running
    });
}
