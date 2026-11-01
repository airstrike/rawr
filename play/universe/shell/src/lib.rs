//! ?play's iced shell (#434). iced hands a program to its shell's `run`,
//! and in ?play's universe this crate is that shell in place of
//! iced_winit. It opens no window. It runs the program in iced_test's
//! emulator, waits until the boot and the tasks it starts have finished,
//! and writes that first settled frame as a PNG to the file [`FRAME`]
//! names. Then `run` returns, as iced's does when its window closes.
//!
//! Its public items are the ones iced takes from iced_winit.

pub use iced_debug as debug;
pub use iced_program as program;
pub use iced_runtime as runtime;
pub use program::core;
pub use program::graphics;
pub use runtime::futures;

use crate::core::backend;
use crate::core::theme;
use crate::futures::futures::channel::mpsc;
use crate::futures::futures::executor;
use crate::futures::futures::StreamExt;
use crate::program::Program;

use iced_test::emulator::{self, Emulator};

/// The variable naming the file the settled frame is written to.
pub const FRAME: &str = "PLAY_FRAME";

/// An error that stopped a program, as iced_winit names them.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The futures executor could not be created.
    #[error("the futures executor could not be created")]
    ExecutorCreationFailed(futures::futures::io::Error),

    /// The application window could not be created: here, its frame could
    /// not be written.
    #[error("the application window could not be created")]
    WindowCreationFailed(std::io::Error),

    /// The application graphics context could not be created.
    #[error("the application graphics context could not be created")]
    GraphicsCreationFailed(backend::Error),
}

impl From<backend::Error> for Error {
    fn from(error: backend::Error) -> Error {
        Error::GraphicsCreationFailed(error)
    }
}

/// Runs a [`Program`] until its boot settles, and writes that frame.
pub fn run<P>(program: P) -> Result<(), Error>
where
    P: Program + 'static,
    P::Theme: theme::Base,
{
    let Some(path) = std::env::var_os(FRAME) else {
        return Err(Error::WindowCreationFailed(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no display",
        )));
    };
    let size = program.window().unwrap_or_default().size;
    let (sender, mut receiver) = mpsc::channel(100);
    let mut emulator = Emulator::new(sender, &program, emulator::Mode::Zen, size);
    loop {
        match executor::block_on(receiver.next()) {
            Some(emulator::Event::Action(action)) => emulator.perform(&program, action),
            Some(emulator::Event::Ready) | None => break,
            Some(emulator::Event::Failed(_)) => {
                unreachable!("the shell runs no instruction")
            }
        }
    }
    let theme = emulator
        .theme(&program)
        .unwrap_or_else(|| <P::Theme as theme::Base>::default(theme::Mode::None));
    let screenshot = emulator.screenshot(&program, &theme, 1.0);
    write_png(&path, &screenshot).map_err(Error::WindowCreationFailed)
}

fn write_png(path: &std::ffi::OsStr, screenshot: &core::window::Screenshot) -> std::io::Result<()> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, screenshot.size.width, screenshot.size.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(std::io::Error::other)?;
    writer
        .write_image_data(&screenshot.rgba)
        .map_err(std::io::Error::other)?;
    writer.finish().map_err(std::io::Error::other)
}
