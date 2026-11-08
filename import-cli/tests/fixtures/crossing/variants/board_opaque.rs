// rawr
// iced's pane_grid (#1191, #1190): as `board.rs`, but each pane's body is
// an opaque widget, `Pane<'a, T, impl Widget>`, so the region the closure
// gives it from holds the pane boxed, and native code takes the pane
// itself, at the panes' lifetime.

use events::widgets::{button, column, container, opaque, pane, text, Board, Panes, Widget};

#[derive(Clone, Copy)]
struct Tile {
    id: usize,
}

struct Grid {
    board: Board<Tile>,
    made: usize,
}

impl Default for Grid {
    fn default() -> Self {
        Grid {
            board: Board::new(Tile { id: 0 }),
            made: 1,
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    Increment,
    Decrement,
}

impl Grid {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => {
                self.board.split(Tile { id: self.made });
                self.made += 1;
            }
            Message::Decrement => self.made -= 1,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let total = self.board.len();
        Panes::new(&self.board, |_, tile, last| {
            let title = container(text(tile.id)).padding(total as u16);
            let body = opaque(column![
                button(if last { "Increment" } else { "Decrement" }).on_press(if last {
                    Message::Increment
                } else {
                    Message::Decrement
                }),
                text(tile.id * 10),
            ]);
            pane(body).title_bar(title)
        })
        .boxed()
    }
}

fn main() {
    events::widgets::run(Grid::update, Grid::view);
}
