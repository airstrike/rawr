//! Conversions at the crate boundary, in the shape of iced's exit, modal
//! and table examples: a number or an array of two into a `Padding`
//! through the `From` impls it has, a crate type into another inside
//! `Some(..)` through `From`, and rows a crate takes by `IntoIterator`.

use crate::widgets::Widget;

/// Space around content, as iced's `Padding`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Padding {
    /// Above and below.
    pub vertical: f32,
    /// Left and right.
    pub horizontal: f32,
}

impl From<u16> for Padding {
    fn from(padding: u16) -> Self {
        Padding {
            vertical: f32::from(padding),
            horizontal: f32::from(padding),
        }
    }
}

impl From<f32> for Padding {
    fn from(padding: f32) -> Self {
        Padding {
            vertical: padding,
            horizontal: padding,
        }
    }
}

impl From<[u16; 2]> for Padding {
    fn from([vertical, horizontal]: [u16; 2]) -> Self {
        Padding {
            vertical: f32::from(vertical),
            horizontal: f32::from(horizontal),
        }
    }
}

impl From<[f32; 2]> for Padding {
    fn from([vertical, horizontal]: [f32; 2]) -> Self {
        Padding {
            vertical,
            horizontal,
        }
    }
}

/// What a frame is filled with, as iced's `Background`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Background {
    /// One color.
    Color(crate::Color),
}

impl From<crate::Color> for Background {
    fn from(color: crate::Color) -> Self {
        Background::Color(color)
    }
}

/// A frame's style, as iced's `container::Style`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Style {
    /// What it is filled with, if anything.
    pub background: Option<Background>,
    /// Its tone.
    pub tone: u8,
}

/// A labelled frame, as iced's `button` and `container`.
pub struct Frame<'a, Message, Theme = crate::Theme> {
    label: String,
    padding: Padding,
    style: Box<dyn Fn(&Theme) -> Style + 'a>,
    on_press: Option<Message>,
}

/// A frame around `label`.
pub fn frame<'a, Message, Theme>(label: impl ToString) -> Frame<'a, Message, Theme> {
    Frame {
        label: label.to_string(),
        padding: Padding::from(0u16),
        style: Box::new(|_| Style::default()),
        on_press: None,
    }
}

impl<'a, Message, Theme> Frame<'a, Message, Theme> {
    /// The space around the label, as iced's `Button::padding`.
    pub fn padding<P: Into<Padding>>(mut self, padding: P) -> Self {
        self.padding = padding.into();
        self
    }

    /// The frame's style.
    pub fn style(mut self, style: impl Fn(&Theme) -> Style + 'a) -> Self {
        self.style = Box::new(style);
        self
    }

    /// The message a press sends.
    pub fn on_press(mut self, message: Message) -> Self {
        self.on_press = Some(message);
        self
    }
}

impl<Message: Clone, Theme: Default, Renderer> Widget<Message, Theme, Renderer>
    for Frame<'_, Message, Theme>
{
    fn labels(&self, labels: &mut Vec<String>) {
        let style = (self.style)(&Theme::default());
        labels.push(format!(
            "[{} {}x{} {:?}/{}]",
            self.label,
            self.padding.vertical,
            self.padding.horizontal,
            style.background,
            style.tone
        ));
    }

    fn press(&self, label: &str) -> Option<Message> {
        (self.label == label)
            .then(|| self.on_press.clone())
            .flatten()
    }
}

/// A column of a table: its header and what it shows of a row, as iced's
/// `table::Column`.
pub struct Column<'a, T> {
    header: String,
    view: Box<dyn Fn(T) -> String + 'a>,
}

/// A column headed `header` that shows `view` of each row, as iced's
/// `table::column`.
pub fn column<'a, T>(header: impl ToString, view: impl Fn(T) -> String + 'a) -> Column<'a, T> {
    Column {
        header: header.to_string(),
        view: Box::new(view),
    }
}

/// Rows shown by columns, as iced's `Table`.
pub struct Table {
    cells: Vec<String>,
}

/// The table of `rows` by `columns`, as iced's `table(columns, rows)`,
/// whose rows are any `IntoIterator`, as a `&Vec<T>` is, of items of the
/// type the columns show.
pub fn table<'a, T>(
    columns: impl IntoIterator<Item = Column<'a, T>>,
    rows: impl IntoIterator<Item = T>,
) -> Table
where
    T: Clone,
{
    let columns = columns.into_iter().collect::<Vec<_>>();
    let mut cells = columns
        .iter()
        .map(|column| column.header.clone())
        .collect::<Vec<_>>();
    for row in rows {
        for column in &columns {
            cells.push((column.view)(row.clone()));
        }
    }
    Table { cells }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Table {
    fn labels(&self, labels: &mut Vec<String>) {
        labels.push(format!("|{}|", self.cells.join("|")));
    }

    fn press(&self, _label: &str) -> Option<Message> {
        None
    }
}
