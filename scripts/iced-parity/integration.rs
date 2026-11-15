// rawr
// iced-parity: backend wgpu
//
// integration drives winit and wgpu itself, so it has no iced program for the
// emulator, and its controls draw only with iced_wgpu's renderer. This test
// simulates the controls headless on the GPU (no window opens) and looks only
// for their first view's text.

#[test]
fn sim_integration() {
    let controls = Controls::new();
    let mut ui = iced_test::Simulator::with_size(
        iced_test::core::Settings::default(),
        (1024.0, 768.0),
        controls.view(),
    );
    if ui.find("Background color").is_err() {
        stop("sim_integration", "expect \"Background color\"");
    }
    if ui.find("Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }").is_err() {
        stop(
            "sim_integration",
            "expect \"Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }\"",
        );
    }
    println!("iced-parity sim_integration passed");
}
