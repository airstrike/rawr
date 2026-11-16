// rawr
use ratatui::widgets::{Block, Paragraph};
use ratatui::{Frame, Terminal};
use ratzilla::event::{KeyCode, KeyEvent};
use ratzilla::{DomBackend, WebEventHandler};
use tokio::sync::mpsc::{self, UnboundedReceiver};

struct Counter {
    value: i64,
}

impl Counter {
    fn press(&mut self, key: KeyCode) {
        match key {
            KeyCode::Up | KeyCode::Char('+') => self.value += 1,
            KeyCode::Down | KeyCode::Char('-') => self.value -= 1,
            _ => {}
        }
    }

    fn draw(&self, frame: &mut Frame) {
        let text = format!("Count: {}", self.value);
        frame.render_widget(
            Paragraph::new(text).block(Block::bordered().title(" Counter ")),
            frame.area(),
        );
    }
}

fn main() -> std::io::Result<()> {
    let mut backend = DomBackend::new()?;
    let (keys, presses) = mpsc::unbounded_channel();
    backend.on_key_event(move |key| {
        let _ = keys.send(key);
    })?;
    let terminal = Terminal::new(backend)?;
    wasm_bindgen_futures::spawn_local(async move {
        run(terminal, presses).await.expect("a drawn frame");
    });
    Ok(())
}

async fn run(
    mut terminal: Terminal<DomBackend>,
    mut presses: UnboundedReceiver<KeyEvent>,
) -> std::io::Result<()> {
    let mut counter = Counter { value: 0 };
    loop {
        terminal.draw(|frame| counter.draw(frame))?;
        let Some(key) = presses.recv().await else {
            return Ok(());
        };
        counter.press(key.code);
    }
}
