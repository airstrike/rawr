//! A native `main` keeps a ratatui `ListState` and lends it, with the
//! frame, to a marked `ui::draw`, which passes it on to ratatui's
//! `render_stateful_widget`. Each frame is drawn on a `TestBackend` and
//! printed with the state the draw left, and `main` moves the selection
//! down between frames.

mod ui;

use ratatui::backend::TestBackend;
use ratatui::widgets::ListState;
use ratatui::Terminal;

fn main() {
    let mut terminal = Terminal::new(TestBackend::new(16, 5)).expect("a test backend starts");
    let mut state = ListState::default();
    for _ in 0..4 {
        terminal
            .draw(|frame| ui::draw(frame, &mut state))
            .expect("a test backend draws");
        let buffer = terminal.backend().buffer().clone();
        for y in 0..buffer.area.height {
            let row: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            println!("|{row}|");
        }
        println!("selected {:?} offset {}", state.selected(), state.offset());
        state.select_next();
    }
}
