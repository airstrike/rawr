//! A result that borrows its slice, of a trait with a type argument and a
//! supertrait, whose methods borrow what they are given for what they give
//! (#1997), as iced's `Widget<Message, Theme, Renderer>: Meta` has
//! `overlay<'a>(&'a mut self, tree: &'a mut Tree, ..) -> Vec<overlay::
//! Element<'a, ..>>`. A crate call takes the result by value.

use crate::shelf::Item;

/// What every rack can say of itself.
pub trait Labeled {
    fn label(&self) -> String {
        "rack".to_string()
    }
}

/// A rack of marks of type `T`, one for each item.
pub trait Rack<T>: Labeled {
    /// How many items it holds, each counted once more.
    fn count(&self) -> usize;

    /// Puts `mark` in `marks`.
    fn mark(&mut self, marks: &mut Vec<T>, mark: T);

    /// One note for each of `marks`, borrowing the rack and the marks.
    fn notes<'a>(&'a mut self, marks: &'a mut Vec<T>) -> Vec<Note<'a, T>>;

    /// The rack in a box, which only a sized rack is.
    fn boxed(self) -> Box<Self>
    where
        Self: Sized,
    {
        Box::new(self)
    }
}

/// A rack's title beside one of its marks.
pub struct Note<'a, T> {
    title: &'a str,
    mark: &'a mut T,
}

struct Shelved<'a> {
    items: &'a [Item],
    title: String,
    marked: usize,
}

impl Labeled for Shelved<'_> {
    fn label(&self) -> String {
        format!(
            "{} of {} marked {}",
            self.title,
            self.items.len(),
            self.marked
        )
    }
}

impl Rack<u32> for Shelved<'_> {
    fn count(&self) -> usize {
        self.items.iter().for_each(Item::count);
        self.items.len()
    }

    fn mark(&mut self, marks: &mut Vec<u32>, mark: u32) {
        self.marked += 1;
        marks.push(mark);
    }

    fn notes<'a>(&'a mut self, marks: &'a mut Vec<u32>) -> Vec<Note<'a, u32>> {
        let title = self.title.as_str();
        marks.iter_mut().map(|mark| Note { title, mark }).collect()
    }
}

/// The items as a rack, borrowed for as long as it lives.
pub fn rack<'a>(items: &'a [Item]) -> impl Rack<u32> + 'a {
    Shelved {
        items,
        title: "shelf".to_string(),
        marked: 0,
    }
}

/// What `rack` says once each item has a mark, which each note bumps:
/// it takes the rack by value, so what the rack borrows stays borrowed
/// for the call.
pub fn tally(mut rack: impl Rack<u32>) -> String {
    let mut marks = Vec::new();
    for at in 0..rack.count() {
        rack.mark(&mut marks, u32::try_from(at * 10).unwrap_or(u32::MAX));
    }
    let notes: Vec<String> = rack
        .notes(&mut marks)
        .into_iter()
        .map(|note| {
            *note.mark += 1;
            format!("{}:{}", note.title, note.mark)
        })
        .collect();
    format!("{} [{}]", rack.label(), notes.join(" "))
}
