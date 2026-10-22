//! Inspect and operate a prepared preview host without opening a native window.

use rawr_iced::{control, preview};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut host = preview::Compiler::new()?
        .check_named_source("control-example.rawr", source())?
        .prepare()?
        .into_control();

    show("initial", &host);
    for preview in host.previews() {
        println!(
            "preview: {} / {} ({}x{}, snapshot={})",
            preview.group(),
            preview.name(),
            preview.width(),
            preview.height(),
            preview.snapshot()
        );
    }

    let key = host.previews()[0].key().clone();
    let _ = host.select(&key)?;
    show("selected", &host);
    let _ = host.reset()?;
    show("reset", &host);
    let _ = host.close();
    show("closed", &host);
    Ok(())
}

fn show(label: &str, host: &control::Host) {
    let status = host.status();
    let selection = match status.selection() {
        control::Selection::Application => "Application".to_owned(),
        control::Selection::Preview(key) => host
            .previews()
            .iter()
            .find(|preview| preview.key() == key)
            .map_or_else(|| key.to_string(), |preview| preview.name().to_owned()),
    };
    println!(
        "{label}: selection={selection}, active={}, theme={}, diagnostics={}",
        status.is_active(),
        status.theme().unwrap_or("none"),
        status.diagnostics()
    );
}

fn source() -> &'static str {
    r#"
use iced::Element;
use iced::widget::text;

#[derive(Default)] struct State;
#[derive(Debug, Clone, Copy)] enum Message { Tick }

impl State {
    fn update(&mut self, message: Message) {}

    #[preview(name = "Counter", group = "example", size = (320, 180))]
    fn view(&self) -> Element<'_, Message> { text("controlled preview") }
}

pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
}
