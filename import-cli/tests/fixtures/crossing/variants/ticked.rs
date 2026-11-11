// rawr
// iced's game_of_life (#1659): `Task::perform(task, Message::Grid.with(version))`,
// where `task` is the `impl Future + use<>` a program method gives from an
// async block, and `.with(version)` a crate callable value. A capturing
// closure in its place takes the future's output too, as rustc accepts.

use events::{Function, Task};

#[derive(Debug)]
enum Message {
    Grid(u64, Ticked),
    Ticked(u64, usize),
}

#[derive(Debug)]
struct Ticked {
    cells: usize,
}

struct Grid {
    cells: usize,
    queued: usize,
}

impl Grid {
    fn tick(&mut self, amount: usize) -> Option<impl Future<Output = Ticked> + use<>> {
        if amount == 0 {
            return None;
        }
        self.queued = amount;
        let cells = self.cells;
        Some(async move {
            Ticked {
                cells: cells * amount,
            }
        })
    }
}

fn next(grid: &mut Grid, version: u64) -> Option<Task<Message>> {
    let task = grid.tick(3)?;
    Some(Task::perform(task, Message::Grid.with(version)))
}

fn counted(grid: &mut Grid, version: u64) -> Option<Task<Message>> {
    let task = grid.tick(2)?;
    Some(Task::perform(task, move |ticked| {
        Message::Ticked(version, ticked.cells)
    }))
}

fn main() {
    let mut grid = Grid {
        cells: 5,
        queued: 0,
    };
    println!("{:?}", next(&mut grid, 1).and_then(|task| task.value()));
    println!("{:?}", counted(&mut grid, 2).and_then(|task| task.value()));
    println!("queued {}", grid.queued);
}
