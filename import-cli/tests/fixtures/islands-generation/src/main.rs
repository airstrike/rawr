//! Native `main` whose winit event loop goes idle and whose view is kept,
//! as an iced app's is: each frame draws the view it built last and makes
//! no call into marked code. The view is built again from the marked
//! module `scene` only when `rawr_hot` tells the program it took a new
//! generation, which it does under `rawr run` and never under cargo.
mod scene;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

/// The program's own event: rawr took a new generation.
struct Generation;

#[derive(Default)]
struct App {
    window: Option<Window>,
    view: Option<String>,
}

impl ApplicationHandler<Generation> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = WindowAttributes::default()
            .with_title("islands-generation")
            .with_inner_size(LogicalSize::new(160.0, 120.0))
            .with_active(false);
        self.window = Some(event_loop.create_window(attributes).expect("a window"));
    }

    fn user_event(&mut self, _: &ActiveEventLoop, _: Generation) {
        self.view = None;
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::RedrawRequested => {
                let view = self.view.get_or_insert_with(scene::view);
                println!("{view}");
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            _ => {}
        }
    }
}

fn main() {
    let event_loop = EventLoop::<Generation>::with_user_event()
        .build()
        .expect("an event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    rawr_hot::on_generation(move |_| {
        let _ = proxy.send_event(Generation);
    });
    event_loop
        .run_app(&mut App::default())
        .expect("the event loop runs");
}
