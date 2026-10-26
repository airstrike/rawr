// rawr
//! Draws a list whose state native code keeps and lends `draw` for the
//! call: `draw` reads and changes it, and ratatui's `render_stateful_widget`
//! takes it as it is, keeping the selection in view.

use ratatui::widgets::{Block, List, ListState};
use ratatui::Frame;

pub fn draw(frame: &mut Frame, state: &mut ListState) {
    if state.selected().is_none() {
        state.select(Some(1));
    }
    let list = List::new(["alpha", "beta", "gamma", "delta", "epsilon"])
        .block(Block::bordered().title("list"))
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, frame.area(), state);
}
