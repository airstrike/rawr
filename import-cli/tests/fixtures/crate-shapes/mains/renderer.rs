// rawr
use std::cell::RefCell;
use std::rc::Rc;

use natives::web::{DomBackend, Key, WebRenderer};
use parts::terminal::Terminal;

fn main() {
    let typed = Rc::new(RefCell::new(String::new()));
    let backend = DomBackend::new_by_id("page").expect("an element");
    let mut terminal = Terminal::new(backend).expect("a terminal");
    terminal
        .on_key_event({
            let typed = Rc::clone(&typed);
            move |key| match key {
                Key::Char(letter) => typed.borrow_mut().push(letter),
                Key::Enter => typed.borrow_mut().push('!'),
            }
        })
        .expect("keys from the page");
    terminal.draw_web(move |frame| frame.write(&typed.borrow()));
}
