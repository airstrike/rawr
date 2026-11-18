use std::cell::RefCell;
use std::rc::Rc;

use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;
use ratatui::Terminal;
use ratzilla::event::KeyCode;
use ratzilla::{DomBackend, WebEventHandler, WebRenderer};

struct Todos {
    items: Vec<(String, bool)>,
    selected: usize,
    draft: String,
}

impl Todos {
    fn new() -> Todos {
        let items = ["Buy milk", "Walk the dog", "Try rawr"];
        Todos {
            items: items.iter().map(|item| (item.to_string(), false)).collect(),
            selected: 0,
            draft: String::new(),
        }
    }

    fn press(&mut self, key: KeyCode) {
        let last = self.items.len().saturating_sub(1);
        match key {
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => self.selected = (self.selected + 1).min(last),
            KeyCode::Char(' ') if self.draft.is_empty() => {
                if let Some(item) = self.items.get_mut(self.selected) {
                    item.1 = !item.1;
                }
            }
            KeyCode::Char(c) => self.draft.push(c),
            KeyCode::Backspace => {
                self.draft.pop();
            }
            KeyCode::Enter if !self.draft.is_empty() => {
                self.items.push((std::mem::take(&mut self.draft), false));
                self.selected = self.items.len() - 1;
            }
            _ => {}
        }
    }

    fn draw(&self, frame: &mut Frame) {
        let areas = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());
        let mut list = String::new();
        let mut left = 0;
        for (index, (text, done)) in self.items.iter().enumerate() {
            let cursor = if index == self.selected { '>' } else { ' ' };
            let mark = if *done { '✓' } else { '☐' };
            list.push_str(&format!("{cursor} {mark} {text}\n"));
            if !done {
                left += 1;
            }
        }
        let draft = format!("{}_", self.draft);
        frame.render_widget(
            Paragraph::new(draft).block(Block::bordered().title(" New to-do ")),
            areas[0],
        );
        frame.render_widget(
            Paragraph::new(list).block(Block::bordered().title(format!(" To do: {left} left "))),
            areas[1],
        );
        frame.render_widget(
            Paragraph::new("Type and Enter adds · ↑↓ · Space checks"),
            areas[2],
        );
    }
}

pub fn main() -> std::io::Result<()> {
    let todos = Rc::new(RefCell::new(Todos::new()));
    let mut backend = DomBackend::new()?;
    backend.on_key_event({
        let todos = Rc::clone(&todos);
        move |key| todos.borrow_mut().press(key.code)
    })?;
    let terminal = Terminal::new(backend)?;
    terminal.draw_web(move |frame| todos.borrow().draw(frame));
    Ok(())
}
