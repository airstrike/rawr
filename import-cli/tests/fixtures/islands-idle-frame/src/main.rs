//! Native `main` whose winit event loop goes idle, as an iced app's does,
//! and whose frames make no call into marked code on the main thread: each
//! frame asks another thread for what the marked module `scene` gives, as
//! an app's async work runs on a runtime's workers, and prints it. Nothing
//! else calls marked code.
mod scene;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

#[derive(Default)]
struct App {
    window: Option<Window>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = WindowAttributes::default()
            .with_title("islands-idle-frame")
            .with_inner_size(LogicalSize::new(160.0, 120.0))
            .with_active(false);
        self.window = Some(event_loop.create_window(attributes).expect("a window"));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::RedrawRequested => {
                let frame = std::thread::spawn(|| scene::frame())
                    .join()
                    .expect("the frame's thread ends");
                println!("{frame}");
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            _ => {}
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("an event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop
        .run_app(&mut App::default())
        .expect("the event loop runs");
}
