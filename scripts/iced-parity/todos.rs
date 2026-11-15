// rawr
// todos builds its program in `application()`, so the harness has no boot,
// update and view to simulate. This test does what upstream's
// `it_creates_a_new_task` does: it loads an empty list, adds two tasks through
// the input and completes one, checking the count each time.

#[test]
fn sim_todos() {
    let (mut todos, _boot) = Todos::new();
    let _ = todos.update(Message::Loaded(Err(LoadError::File)));

    let messages: Vec<_> = {
        let mut ui = iced_test::Simulator::with_size(
            iced_test::core::Settings::default(),
            (500.0, 800.0),
            todos.view(),
        );
        if ui.click(iced_test::selector::id("new-task")).is_err() {
            stop("sim_todos", "click #new-task");
        }
        let _ = ui.typewrite("Create the universe");
        let _ = ui.tap_key(iced_test::core::keyboard::key::Named::Enter);
        let _ = ui.typewrite("Make an apple pie");
        let _ = ui.tap_key(iced_test::core::keyboard::key::Named::Enter);
        ui.into_messages().collect()
    };
    for message in messages {
        let _ = todos.update(message);
    }
    {
        let mut ui = iced_test::Simulator::with_size(
            iced_test::core::Settings::default(),
            (500.0, 800.0),
            todos.view(),
        );
        if ui.find("2 tasks left").is_err() {
            stop("sim_todos", "expect \"2 tasks left\"");
        }
    }

    let messages: Vec<_> = {
        let mut ui = iced_test::Simulator::with_size(
            iced_test::core::Settings::default(),
            (500.0, 800.0),
            todos.view(),
        );
        if ui.click("Create the universe").is_err() {
            stop("sim_todos", "click \"Create the universe\"");
        }
        ui.into_messages().collect()
    };
    for message in messages {
        let _ = todos.update(message);
    }

    let mut ui = iced_test::Simulator::with_size(
        iced_test::core::Settings::default(),
        (500.0, 800.0),
        todos.view(),
    );
    if ui.find("1 task left").is_err() {
        stop("sim_todos", "expect \"1 task left\"");
    }
    let frame = match ui.snapshot(&iced_test::core::Theme::Light) {
        Ok(frame) => frame,
        Err(error) => panic!("the headless renderer draws the final frame: {error}"),
    };
    if let Err(error) = frame.matches_hash("parity-snapshots/sim_todos") {
        panic!("the copy's parity-snapshots directory is writable: {error}");
    }
    println!("iced-parity sim_todos passed");
}
