// rawr
// toast's boot schedules its first toast's expiry with `tokio::time::sleep`,
// which panics outside a Tokio runtime, so this test boots the example and
// runs its updates inside one. It replays toast.ice: the first toast shows,
// and its close button removes it.

#[test]
fn sim_toast() {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => panic!("a current-thread Tokio runtime builds: {error}"),
    };
    let _entered = runtime.enter();
    let (mut state, _boot) = App::new();
    {
        let mut ui = iced_test::Simulator::with_size(
            iced_test::core::Settings::default(),
            (1024.0, 768.0),
            App::view(&state),
        );
        for text in [
            "Example Toast",
            "Add more toasts in the form below!",
            "Add Toast",
        ] {
            if ui.find(text).is_err() {
                stop("sim_toast", &format!("expect {text:?}"));
            }
        }
    }
    let messages: Vec<_> = {
        let mut ui = iced_test::Simulator::with_size(
            iced_test::core::Settings::default(),
            (1024.0, 768.0),
            App::view(&state),
        );
        if ui.click("X").is_err() {
            stop("sim_toast", "click \"X\"");
        }
        ui.into_messages().collect()
    };
    for message in messages {
        let _ = App::update(&mut state, message);
    }
    let mut ui = iced_test::Simulator::with_size(
        iced_test::core::Settings::default(),
        (1024.0, 768.0),
        App::view(&state),
    );
    if ui.find("Title").is_err() {
        stop("sim_toast", "expect \"Title\"");
    }
    let frame = match ui.snapshot(&iced_test::core::Theme::Light) {
        Ok(frame) => frame,
        Err(error) => panic!("the headless renderer draws the final frame: {error}"),
    };
    if let Err(error) = frame.matches_hash("target/iced-parity/snapshots/sim_toast") {
        panic!("the example's target directory is writable: {error}");
    }
    println!("iced-parity sim_toast passed");
}
