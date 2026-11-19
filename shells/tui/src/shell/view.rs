//! How the shell draws: the phase on top, the source beside the frames,
//! bindings and breakpoints, the console under them, and the prompt or the
//! keys at the bottom.

use std::path::Path;

use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line as Text, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};
use ratatui::Frame as Screen;

use super::update::file_name;
use super::{Breakpoint, Focus, Kind, Purpose, Shell};

const KEYS: &str =
    "b break · B break at · p pause · s step · i into · o out · c continue · e eval · Tab pane · q quit";

impl Shell {
    /// Draw the shell on `screen`.
    pub fn draw(&self, screen: &mut Screen) {
        let [top, middle, console, bottom] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(6),
            Constraint::Length(8),
            Constraint::Length(1),
        ])
        .areas(screen.area());
        let [source, side] =
            Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
                .areas(middle);
        let breakpoints = u16::try_from(self.breakpoints.len().clamp(1, 4)).unwrap_or(4) + 2;
        let [frames, bindings, marks] = Layout::vertical([
            Constraint::Percentage(35),
            Constraint::Min(3),
            Constraint::Length(breakpoints),
        ])
        .areas(side);

        // The phase first, so a long root is what a narrow terminal cuts.
        let header = format!(" rawr tui · {} · {}", self.phase, self.root);
        screen.render_widget(
            Paragraph::new(header).style(Style::new().add_modifier(Modifier::BOLD)),
            top,
        );
        self.draw_source(screen, source);
        self.draw_frames(screen, frames);
        self.draw_bindings(screen, bindings);
        self.draw_breakpoints(screen, marks);
        self.draw_console(screen, console);
        self.draw_bottom(screen, bottom);
    }

    fn pane(&self, title: String, focus: Option<Focus>) -> Block<'static> {
        let block = Block::bordered().title(title);
        if focus.is_some_and(|focus| focus == self.focus) {
            block.border_style(Style::new().fg(Color::Cyan))
        } else {
            block
        }
    }

    fn draw_source(&self, screen: &mut Screen, area: Rect) {
        let path = self.source.path.as_deref();
        // A file of the program's package is named from the package's root;
        // a single-file program by its name.
        let title = path.map_or_else(String::new, |path| {
            let shown = path
                .strip_prefix(&self.root)
                .ok()
                .filter(|shown| !shown.as_os_str().is_empty())
                .map_or_else(
                    || file_name(&path.to_string_lossy()),
                    |shown| shown.display().to_string(),
                );
            format!(" {shown} ")
        });
        let block = self.pane(title, Some(Focus::Source));
        let inner = block.inner(area);
        screen.render_widget(block, area);
        let height = usize::from(inner.height);
        let count = self.source.lines.len();
        let top = self
            .cursor
            .saturating_sub(height / 2)
            .min(count.saturating_sub(height));
        let width = count.max(1).to_string().len();
        let stopped = self
            .phase
            .stop()
            .filter(|stop| path.is_some_and(|path| path == Path::new(stop.file())))
            .map(|stop| stop.line());
        let lines: Vec<Text> = self
            .source
            .lines
            .iter()
            .enumerate()
            .skip(top)
            .take(height)
            .map(|(index, text)| {
                let number = index + 1;
                let marked = path.is_some_and(|path| self.has_breakpoint(path, number));
                let here = stopped == Some(number);
                let mut style = Style::new();
                if here {
                    style = style.fg(Color::Yellow);
                }
                if index == self.cursor {
                    style = style.add_modifier(if self.focus == Focus::Source {
                        Modifier::REVERSED
                    } else {
                        Modifier::UNDERLINED
                    });
                }
                Text::from(vec![
                    Span::styled(if marked { "●" } else { " " }, Style::new().fg(Color::Red)),
                    Span::raw(format!("{number:>width$}")),
                    Span::raw(if here { " ▶ " } else { "   " }),
                    Span::raw(text.clone()),
                ])
                .style(style)
            })
            .collect();
        screen.render_widget(Paragraph::new(lines), inner);
    }

    fn has_breakpoint(&self, path: &Path, line: usize) -> bool {
        self.breakpoints.iter().any(|breakpoint| match breakpoint {
            Breakpoint::Line { file, line: at } => *at == line && Path::new(file) == path,
            Breakpoint::Function(_) => false,
        })
    }

    fn draw_frames(&self, screen: &mut Screen, area: Rect) {
        let items: Vec<ListItem> = self
            .frames
            .iter()
            .enumerate()
            .map(|(index, frame)| {
                let at = match (&frame.file, frame.line) {
                    (Some(file), Some(line)) => format!(" {}:{line}", file_name(file)),
                    _ => String::new(),
                };
                ListItem::new(format!("#{index} {}{at}", frame.function))
            })
            .collect();
        let mut state =
            ListState::default().with_selected((!items.is_empty()).then_some(self.frame));
        screen.render_stateful_widget(
            List::new(items)
                .block(self.pane(" frames ".to_owned(), Some(Focus::Frames)))
                .highlight_symbol("> ")
                .highlight_style(Style::new().add_modifier(Modifier::BOLD)),
            area,
            &mut state,
        );
    }

    fn draw_bindings(&self, screen: &mut Screen, area: Rect) {
        let rows = self.rows();
        let mut items: Vec<ListItem> = rows
            .iter()
            .map(|row| {
                let mark = match (row.expandable, row.expanded) {
                    (true, true) => "▾ ",
                    (true, false) => "▸ ",
                    (false, _) => "  ",
                };
                let item = ListItem::new(format!("{}{mark}{}", "  ".repeat(row.depth), row.text));
                if row.unreadable {
                    item.style(Style::new().add_modifier(Modifier::DIM))
                } else {
                    item
                }
            })
            .collect();
        if let Some(why) = &self.unread {
            items.push(ListItem::new(format!("no bindings to read: {why}")));
        }
        let selected = (self.focus == Focus::Bindings && !rows.is_empty()).then_some(self.row);
        let mut state = ListState::default().with_selected(selected);
        screen.render_stateful_widget(
            List::new(items)
                .block(self.pane(" bindings ".to_owned(), Some(Focus::Bindings)))
                .highlight_style(Style::new().add_modifier(Modifier::REVERSED)),
            area,
            &mut state,
        );
    }

    fn draw_breakpoints(&self, screen: &mut Screen, area: Rect) {
        let items: Vec<ListItem> = self
            .breakpoints
            .iter()
            .map(|breakpoint| match breakpoint {
                Breakpoint::Line { file, line } => {
                    ListItem::new(format!("● {}:{line}", file_name(file)))
                }
                Breakpoint::Function(name) => ListItem::new(format!("● fn {name}")),
            })
            .collect();
        screen.render_widget(
            List::new(items).block(self.pane(" breakpoints ".to_owned(), None)),
            area,
        );
    }

    fn draw_console(&self, screen: &mut Screen, area: Rect) {
        let block = self.pane(" console ".to_owned(), None);
        let inner = block.inner(area);
        screen.render_widget(block, area);
        let height = usize::from(inner.height);
        let from = self.console.len().saturating_sub(height);
        let lines: Vec<Text> = self.console[from..]
            .iter()
            .map(|line| match line.kind {
                Kind::Stdout => Text::raw(line.text.clone()),
                Kind::Stderr => Text::styled(line.text.clone(), Style::new().fg(Color::Yellow)),
                Kind::Eval => Text::styled(
                    format!("> {}", line.text),
                    Style::new().add_modifier(Modifier::BOLD),
                ),
                Kind::Value => {
                    Text::styled(format!("= {}", line.text), Style::new().fg(Color::Green))
                }
                Kind::Error => Text::styled(line.text.clone(), Style::new().fg(Color::Red)),
                Kind::Note => Text::styled(
                    format!("-- {}", line.text),
                    Style::new().add_modifier(Modifier::DIM),
                ),
            })
            .collect();
        screen.render_widget(Paragraph::new(lines), inner);
    }

    fn draw_bottom(&self, screen: &mut Screen, area: Rect) {
        if let Some(prompt) = &self.prompt {
            let label = match prompt.purpose {
                Purpose::Eval => "eval> ",
                Purpose::Break => "break at (file:line, line or function)> ",
            };
            screen.render_widget(Paragraph::new(format!("{label}{}", prompt.text)), area);
            let column = label.chars().count() + prompt.text.chars().count();
            let column = u16::try_from(column).unwrap_or(u16::MAX);
            screen.set_cursor_position(Position::new(
                area.x
                    .saturating_add(column)
                    .min(area.right().saturating_sub(1)),
                area.y,
            ));
            return;
        }
        let (text, style) = match &self.status {
            Some(status) => (status.as_str(), Style::new().fg(Color::Red)),
            None => (KEYS, Style::new().add_modifier(Modifier::DIM)),
        };
        screen.render_widget(Paragraph::new(text).style(style), area);
    }
}
