//! A value as a tree of its parts, drawn as a compact table, one row per
//! value, as DevTools' scope pane draws one. The parts are the inspector's
//! (`"children"` in its answer): each field, item or map entry, named by its
//! field, its index or its key, so a map opens to a row per key. A row
//! with parts opens and folds, and what is open stays open by its place,
//! the names from its root down. A long value elides to one line, with its
//! whole text in a tooltip.

use std::collections::HashSet;

use iced::widget::text::{Ellipsis, Wrapping};
use iced::widget::{button, row, space, table, text, tooltip};
use iced::{Element, Fill, Font, Widget};
use serde_json::Value;

use crate::icon;
use crate::theme;

/// A value longer than this many characters carries its whole text in a
/// tooltip, as the row may elide it.
const LONG: usize = 40;

/// How far each level of parts is indented.
const INDENT: f32 = 12.0;

/// One part of a value: a field, an item or a map's entry, its name, its
/// value's text, and its own parts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    name: String,
    value: String,
    parts: Vec<Part>,
}

/// A tree's top row: a binding, or an answer in the console.
#[derive(Clone, Copy, Debug)]
pub struct Root<'a> {
    /// What keeps it and its parts open across answers.
    pub place: &'a str,
    pub name: &'a str,
    /// Its value's text, or why it has none.
    pub value: Result<&'a str, &'a str>,
    pub parts: &'a [Part],
}

/// One row shown.
#[derive(Clone, Debug)]
struct Row<'a> {
    place: String,
    depth: u16,
    name: &'a str,
    value: Result<&'a str, &'a str>,
    parts: &'a [Part],
    open: bool,
}

impl Part {
    /// The parts in the inspector's `children`: each `{"name", "value"}`,
    /// with `children` of its own when it has parts.
    #[must_use]
    pub fn all(children: &Value) -> Vec<Self> {
        children
            .as_array()
            .into_iter()
            .flatten()
            .map(|part| Self {
                name: part["name"].as_str().unwrap_or_default().to_owned(),
                value: part["value"].as_str().unwrap_or_default().to_owned(),
                parts: Self::all(&part["children"]),
            })
            .collect()
    }
}

/// The rows of `roots`, and under each open one its parts' rows, depth
/// first, drawn as a table of names and values. A click on a row with parts
/// gives `toggle` its place.
pub fn view<'a, Message: Clone + 'a>(
    roots: impl IntoIterator<Item = Root<'a>>,
    open: &HashSet<String>,
    toggle: impl Fn(String) -> Message + Clone + 'a,
) -> Element<'a, Message> {
    let mut rows = Vec::new();
    for root in roots {
        walk(
            &mut rows,
            open,
            Row {
                place: root.place.to_owned(),
                depth: 0,
                name: root.name,
                value: root.value,
                parts: root.parts,
                open: false,
            },
        );
    }
    let names = table::column(space(), move |row: Row<'a>| name(&row, toggle.clone()));
    let values = table::column(space(), |row: Row<'a>| value(row.value)).width(Fill);
    table::table([names, values], rows)
        .width(Fill)
        .padding_x(4)
        .padding_y(0)
        .separator(0)
        .boxed()
}

/// `row`, and its parts' rows when it is open.
fn walk<'a>(rows: &mut Vec<Row<'a>>, open: &HashSet<String>, mut row: Row<'a>) {
    row.open = !row.parts.is_empty() && open.contains(&row.place);
    let (place, depth, parts, shown) = (row.place.clone(), row.depth, row.parts, row.open);
    rows.push(row);
    if !shown {
        return;
    }
    for part in parts {
        walk(
            rows,
            open,
            Row {
                place: format!("{place}/{}", part.name),
                depth: depth + 1,
                name: &part.name,
                value: Ok(&part.value),
                parts: &part.parts,
                open: false,
            },
        );
    }
}

/// A row's name, indented by its depth, after the triangle that opens or
/// folds a row with parts.
fn name<'a, Message: Clone + 'a>(
    row: &Row<'a>,
    toggle: impl Fn(String) -> Message,
) -> Element<'a, Message> {
    let twisty = match (row.parts.is_empty(), row.open) {
        (true, _) => space().width(INDENT).boxed(),
        (false, true) => icon::chevron_down().size(theme::SIZE).boxed(),
        (false, false) => icon::chevron_right().size(theme::SIZE).boxed(),
    };
    let label = row![
        space().width(f32::from(row.depth) * INDENT),
        twisty,
        text(row.name)
            .font(Font::MONOSPACE)
            .size(theme::SIZE)
            .style(theme::name),
    ]
    .align_y(iced::Center);
    button(label)
        .padding(0)
        .style(|theme, status| theme::item(theme, status, false))
        .on_press_maybe((!row.parts.is_empty()).then(|| toggle(row.place.clone())))
        .boxed()
}

/// A row's value on one line, elided at its end when the pane is narrower;
/// a long one carries its whole text in a tooltip.
fn value<'a, Message: 'a>(value: Result<&'a str, &'a str>) -> Element<'a, Message> {
    let shown = value.unwrap_or_else(|error| error);
    let line = text(shown)
        .font(Font::MONOSPACE)
        .size(theme::SIZE)
        .wrapping(Wrapping::None)
        .ellipsis(Ellipsis::End)
        .width(Fill)
        .style(move |theme| theme::value(theme, value));
    if shown.chars().count() > LONG {
        theme::tipped(line, shown, tooltip::Position::Bottom)
    } else {
        line.boxed()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::Part;

    #[test]
    fn a_map_s_parts_are_its_entries_by_key_down_to_their_own_parts() {
        let parts = Part::all(&json!([
            { "name": "\"a\"", "value": "1" },
            { "name": "\"b\"", "value": "Point(1, 2)", "children": [
                { "name": "0", "value": "1" },
                { "name": "1", "value": "2" },
            ] },
        ]));
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].name, "\"a\"");
        assert!(parts[0].parts.is_empty(), "a leaf has no parts");
        assert_eq!(parts[1].parts[1].value, "2");
        assert!(
            Part::all(&json!(null)).is_empty(),
            "a value without parts has none"
        );
    }
}
