//! `iced::backend`: iced's `Backend` values, `backend::Settings` built with
//! a functional update, and `backend::configure` as iced's own task.

use super::Adapter;

/// iced's solar_system's backend picker, with what it shows as text.
const PICKER: &str = r#"
use iced::backend;
use iced::widget::{column, pick_list, text};
use iced::{Backend, Element, Task};

struct Picker {
    backend: Backend,
    settings: backend::Settings,
    configured: u32,
    error: Option<backend::Error>,
}

#[derive(Debug, Clone)]
enum Message {
    Selected(Backend),
    Configured(Backend, Result<(), backend::Error>),
}

fn software() -> Message {
    Message::Selected(Backend::Software)
}

impl Picker {
    fn new() -> Self {
        Picker {
            backend: Backend::default(),
            settings: backend::Settings::default(),
            configured: 0,
            error: None,
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Selected(backend) => {
                self.settings = backend::Settings {
                    backend: backend.clone(),
                    ..backend::Settings::default()
                };
                backend::configure(self.settings.clone())
                    .map(move |result| Message::Configured(backend.clone(), result))
            }
            Message::Configured(backend, Ok(())) => {
                self.backend = backend;
                self.configured += 1;
                Task::none()
            }
            Message::Configured(_backend, Err(error)) => {
                self.error = Some(error);
                self.configured += 1;
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        column![
            text(format!("{} of {}", self.backend, Backend::ALL.len())),
            text(format!("asked {} vsync {}", self.settings.backend, self.settings.vsync)),
            text(format!("configured {}", self.configured)),
            pick_list(Some(&self.backend), Backend::ALL, Backend::to_string)
                .on_select(Message::Selected),
        ]
    }
}

pub fn main() -> iced::Result {
    iced::application(Picker::new, Picker::update, Picker::view).run()
}
"#;

fn shows(adapter: &Adapter, line: &str) -> bool {
    iced_test::simulator(adapter.view()).find(line).is_ok()
}

#[test]
fn the_backend_picker_checks_runs_and_asks_iced_to_configure() {
    let mut adapter = super::carry_tests::boot(PICKER);
    // `Backend::default()` is iced's own: `Best` without `ICED_BACKEND`.
    let expected = iced::Backend::default().to_string();
    assert!(shows(&adapter, &format!("{expected} of 8")));
    assert!(shows(&adapter, "asked Best Backend vsync true"));
    let message = adapter.message_of("software").unwrap().unwrap();
    adapter.deliver([message]);
    // The functional update keeps the default's other fields.
    assert!(shows(&adapter, "asked Software Backend vsync true"));
    // iced's `configure` reaches the shell that owns a compositor. A
    // headless adapter has none, so its sender drops and the task ends
    // with no result, as in iced's test emulator and on the web.
    assert!(shows(&adapter, "configured 0"));
    assert!(shows(&adapter, &format!("{expected} of 8")));
}
