// rawr
// iced's pane_grid (#1191): `Panes::new(&self.board, |index, tile, last|
// ..)` calls its closure while the view's region is open, and the closure
// gives a pane at the panes' own lifetime, `Pane<'a, ..>`, invariant in it
// through its title bar's container. Rust makes the panes at the view's
// lifetime, so the closure's pane is made in a region of the call's own.

use events::widgets::{button, container, pane, text, Board, Panes, Widget};

#[derive(Clone, Copy)]
struct Tile {
    id: usize,
    pinned: bool,
}

struct Grid {
    board: Board<Tile>,
    made: usize,
    focus: usize,
}

impl Default for Grid {
    fn default() -> Self {
        Grid {
            board: Board::new(Tile {
                id: 0,
                pinned: true,
            }),
            made: 1,
            focus: 0,
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
                self.board.split(Tile {
                    id: self.made,
                    pinned: false,
                });
                self.focus = self.made;
                self.made += 1;
            }
            Message::Decrement => self.focus = 0,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let focus = self.focus;
        let total = self.board.len();
        Panes::new(&self.board, |index, tile, last| {
            let title = container(text(tile.id)).padding(if index == focus { 1 } else { 0 });
            let body = if last {
                button("Increment").on_press(Message::Increment)
            } else {
                button(if tile.pinned { "Decrement" } else { "Pane" })
                    .on_press(Message::Decrement)
            };
            pane(body).title_bar(title.padding(total as u16))
        })
        .spacing(2)
        .boxed()
    }
}

fn main() {
    events::widgets::run(Grid::update, Grid::view);
}
