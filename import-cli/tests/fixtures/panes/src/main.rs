// rawr
//! iced's pane_grid example keeps its panes in a generic `State<Pane>` and
//! reaches it through `use iced::widget::pane_grid;`. This crate does the
//! same with a small copy of that API, as the example writes it.

use natives::grid;

#[derive(Debug, Clone, Copy)]
struct Pane {
    id: usize,
    is_pinned: bool,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Split(grid::Axis, grid::Pane),
    Resized(grid::ResizeEvent),
    Dragged(grid::DragEvent),
    TogglePin(grid::Pane),
    Close(grid::Pane),
}

struct Example {
    panes: grid::State<Pane>,
    created: usize,
    focus: Option<grid::Pane>,
}

impl Example {
    fn new() -> Self {
        let (panes, _) = grid::State::new(Pane {
            id: 0,
            is_pinned: false,
        });
        Example {
            panes,
            created: 1,
            focus: None,
        }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Split(axis, pane) => {
                let result = self.panes.split(
                    axis,
                    pane,
                    Pane {
                        id: self.created,
                        is_pinned: false,
                    },
                );
                if let Some((pane, _)) = result {
                    self.focus = Some(pane);
                }
                self.created += 1;
            }
            Message::Resized(grid::ResizeEvent { split, ratio }) => {
                self.panes.resize(split, ratio);
            }
            Message::Dragged(grid::DragEvent::Dropped { pane, target }) => {
                println!("dropped {pane:?} on {target:?}");
            }
            Message::Dragged(_) => {}
            Message::TogglePin(pane) => {
                if let Some(Pane { is_pinned, .. }) = self.panes.get_mut(pane) {
                    *is_pinned = !*is_pinned;
                }
            }
            Message::Close(pane) => {
                if let Some(Pane { is_pinned, .. }) = self.panes.get(pane)
                    && !is_pinned
                    && let Some((closed, sibling)) = self.panes.close(pane)
                {
                    println!("closed {} for {sibling:?}", closed.id);
                    self.focus = Some(sibling);
                }
            }
        }
    }
}

fn main() {
    let mut example = Example::new();
    let first = example.panes.maximized().unwrap_or_else(|| {
        let (_, pane) = grid::State::new(0_u8);
        pane
    });
    example.update(Message::Split(grid::Axis::Vertical, first));
    println!("{} panes, focus {:?}", example.panes.len(), example.focus);
    let second = example.focus.unwrap();
    let (_, split) = example
        .panes
        .split(
            grid::Axis::Horizontal,
            second,
            Pane {
                id: 9,
                is_pinned: false,
            },
        )
        .unwrap();
    example.update(Message::Resized(grid::ResizeEvent { split, ratio: 0.25 }));
    println!("ratio {:?}", example.panes.ratio(split));
    example.update(Message::TogglePin(second));
    println!("{:?}", example.panes.get(second));
    example.update(Message::Close(second));
    println!("{} panes after a pinned close", example.panes.len());
    example.update(Message::TogglePin(second));
    example.update(Message::Close(second));
    println!("{} panes, focus {:?}", example.panes.len(), example.focus);
    example.update(Message::Dragged(grid::DragEvent::Dropped {
        pane: first,
        target: grid::Target::Pane(first),
    }));
    example.update(Message::Dragged(grid::DragEvent::Picked { pane: first }));
}
