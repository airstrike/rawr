//! How the shell draws: the phase on top, the source beside the frames,
//! bindings and breakpoints, the console under them, and the prompt or the
//! keys at the bottom. Its colours are `rawr gui`'s (see `palette`): the
//! source on the window's surface, the panes around it a shade under, quiet
//! borders, and the line the program paused at on a faint band of the
//! accent.

use std::path::Path;

use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line as Text, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};
use ratatui::Frame as Screen;

use crate::home;
use crate::phase::Phase;

use super::update::file_name;
use super::{colours, Breakpoint, Focus, Kind, Purpose, Shell};

/// Each key and what it does, as the bottom row lists them.
const KEYS: [(&str, &str); 10] = [
    ("b", "break"),
    ("B", "break at"),
    ("p", "pause"),
    ("s", "step"),
    ("i", "into"),
    ("o", "out"),
    ("c", "continue"),
    ("e", "eval"),
    ("Tab", "pane"),
    ("q", "quit"),
];

/// A status's tone, as `rawr gui`'s lamp takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    /// What asks to be looked at: a pause.
    Accent,
    Good,
    Quiet,
    Bad,
}

impl Shell {
    /// Draw the shell on `screen`.
    pub fn draw(&self, screen: &mut Screen) {
        let palette = &self.palette;
        screen.render_widget(
            Block::new().style(Style::new().bg(palette.side).fg(palette.text)),
            screen.area(),
        );
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

        self.draw_status(screen, top);
        self.draw_source(screen, source);
        self.draw_frames(screen, frames);
        self.draw_bindings(screen, bindings);
        self.draw_breakpoints(screen, marks);
        self.draw_console(screen, console);
        self.draw_bottom(screen, bottom);
    }

    /// Where the program is, as `rawr gui`'s status says it: a dot in the
    /// phase's tone, the phase, and where or how far, quiet. The program's
    /// root comes last, so a narrow terminal cuts it first.
    fn draw_status(&self, screen: &mut Screen, area: Rect) {
        let palette = &self.palette;
        let (tone, word, detail) = match &self.phase {
            Phase::Starting => (Tone::Quiet, "Starting", String::new()),
            Phase::Building { stage, percent } => {
                (Tone::Quiet, "Building", format!("{stage} {percent}%"))
            }
            Phase::Running => (Tone::Good, "Running", String::new()),
            Phase::Paused(stop) => {
                let mut place = format!("{}:{}", file_name(stop.file()), stop.line());
                if let Some(function) = stop.function() {
                    place = format!("{place} in {function}");
                }
                if let Some(reason) = stop.reason() {
                    place = format!("{place} · {reason}");
                }
                (Tone::Accent, "Paused", place)
            }
            Phase::Restarting => (Tone::Quiet, "Restarting", String::new()),
            Phase::Failed => (
                Tone::Bad,
                "Failed",
                "a save that fixes it starts it".to_owned(),
            ),
            Phase::Ended(status) => (Tone::Quiet, "Ended", format!("status {status}")),
            Phase::Gone => (Tone::Bad, "Exited", String::new()),
        };
        let lamp = match tone {
            Tone::Accent => palette.accent,
            Tone::Good => palette.good,
            Tone::Quiet => palette.quiet,
            Tone::Bad => palette.bad,
        };
        let quiet = Style::new().fg(palette.quiet);
        let mut spans = vec![
            Span::styled(" ● ", Style::new().fg(lamp)),
            Span::styled(word, Style::new().add_modifier(Modifier::BOLD)),
        ];
        if !detail.is_empty() {
            spans.push(Span::styled(format!("  {detail}"), quiet));
        }
        if !self.root.is_empty() {
            spans.push(Span::styled(
                format!("  ·  {}", home::shorten(&self.root)),
                quiet,
            ));
        }
        screen.render_widget(Paragraph::new(Text::from(spans)), area);
    }

    /// A pane titled `title` on `background`: its border quiet, and a
    /// little less so, with its title in the accent, while `focus` is
    /// where the keys go.
    fn pane(&self, title: String, focus: Option<Focus>, background: Background) -> Block<'static> {
        let palette = &self.palette;
        let focused = focus.is_some_and(|focus| focus == self.focus);
        let (border, title_colour) = if focused {
            (palette.quiet, palette.accent)
        } else {
            (palette.rule, palette.quiet)
        };
        let background = match background {
            Background::Surface => palette.surface,
            Background::Side => palette.side,
        };
        Block::bordered()
            .title(Span::styled(title, Style::new().fg(title_colour)))
            .border_style(Style::new().fg(border))
            .style(Style::new().bg(background))
    }

    fn draw_source(&self, screen: &mut Screen, area: Rect) {
        let palette = &self.palette;
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
        let block = self.pane(title, Some(Focus::Source), Background::Surface);
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
        let focused = self.focus == Focus::Source;

        // The bands first, across the pane's whole width: the paused line's,
        // which the cursor starts on and leaves as it is, and the cursor's.
        for (row, index) in (top..count.min(top + height)).enumerate() {
            let here = stopped == Some(index + 1);
            let cursor = focused && index == self.cursor;
            let band = match (here, cursor) {
                (true, _) => palette.band,
                (false, true) => palette.cursor,
                (false, false) => continue,
            };
            let y = inner.y + u16::try_from(row).unwrap_or(u16::MAX);
            screen
                .buffer_mut()
                .set_style(Rect::new(inner.x, y, inner.width, 1), Style::new().bg(band));
        }

        let shown = |pieces: &[colours::Pieces]| -> Vec<Text<'_>> {
            self.source
                .lines
                .iter()
                .enumerate()
                .skip(top)
                .take(height)
                .map(|(index, text)| {
                    let number = index + 1;
                    let marked = path.is_some_and(|path| self.has_breakpoint(path, number));
                    let here = stopped == Some(number);
                    let gutter = if here {
                        Style::new().fg(palette.accent).add_modifier(Modifier::BOLD)
                    } else if index == self.cursor {
                        Style::new().fg(palette.text)
                    } else {
                        Style::new().fg(palette.quiet)
                    };
                    let mut spans = vec![
                        Span::styled(if marked { "●" } else { " " }, Style::new().fg(palette.bad)),
                        Span::styled(format!("{number:>width$}"), gutter),
                        Span::styled(
                            if here { " ▶ " } else { "   " },
                            Style::new().fg(palette.accent),
                        ),
                    ];
                    match pieces.get(index) {
                        Some(pieces) => spans.extend(pieces.iter().map(|(bytes, code)| {
                            Span::styled(&text[bytes.clone()], palette.code(*code))
                        })),
                        None => spans.push(Span::styled(
                            text.as_str(),
                            palette.code(colours::Code::Other),
                        )),
                    }
                    Text::from(spans)
                })
                .collect()
        };
        let lines = match path {
            Some(path) => self
                .source
                .colours
                .with(path, &self.source.lines, top + height, shown),
            None => shown(&[]),
        };
        screen.render_widget(Paragraph::new(lines), inner);
    }

    fn has_breakpoint(&self, path: &Path, line: usize) -> bool {
        self.breakpoints.iter().any(|breakpoint| match breakpoint {
            Breakpoint::Line { file, line: at } => *at == line && Path::new(file) == path,
            Breakpoint::Function(_) => false,
        })
    }

    fn draw_frames(&self, screen: &mut Screen, area: Rect) {
        let palette = &self.palette;
        let items: Vec<ListItem> = self
            .frames
            .iter()
            .enumerate()
            .map(|(index, frame)| {
                let mut spans = vec![
                    Span::styled(format!("#{index} "), Style::new().fg(palette.quiet)),
                    Span::raw(frame.function.as_str()),
                ];
                if let (Some(file), Some(line)) = (&frame.file, frame.line) {
                    spans.push(Span::styled(
                        format!(" {}:{line}", file_name(file)),
                        Style::new().fg(palette.quiet),
                    ));
                }
                ListItem::new(Text::from(spans))
            })
            .collect();
        let mut state =
            ListState::default().with_selected((!items.is_empty()).then_some(self.frame));
        screen.render_stateful_widget(
            List::new(items)
                .block(self.pane(" frames ".to_owned(), Some(Focus::Frames), Background::Side))
                .highlight_symbol(Span::styled("> ", Style::new().fg(palette.accent)))
                .highlight_style(Style::new().bg(palette.selection)),
            area,
            &mut state,
        );
    }

    fn draw_bindings(&self, screen: &mut Screen, area: Rect) {
        let palette = &self.palette;
        let quiet = Style::new().fg(palette.quiet);
        let rows = self.rows();
        let mut items: Vec<ListItem> = rows
            .iter()
            .map(|row| {
                let mark = match (row.expandable, row.expanded) {
                    (true, true) => "▾ ",
                    (true, false) => "▸ ",
                    (false, _) => "  ",
                };
                let mut spans = vec![
                    Span::styled(format!("{}{mark}", "  ".repeat(row.depth)), quiet),
                    Span::styled(row.name.as_str(), Style::new().fg(palette.function)),
                ];
                match &row.value {
                    Ok(value) => spans.extend([
                        Span::styled(" = ", quiet),
                        Span::styled(value.as_str(), palette.value(value)),
                    ]),
                    Err(why) => spans.push(Span::styled(format!(": {why}"), quiet)),
                }
                ListItem::new(Text::from(spans))
            })
            .collect();
        if let Some(why) = &self.unread {
            items.push(ListItem::new(Span::styled(
                format!("no bindings to read: {why}"),
                quiet,
            )));
        }
        let selected = (self.focus == Focus::Bindings && !rows.is_empty()).then_some(self.row);
        let mut state = ListState::default().with_selected(selected);
        screen.render_stateful_widget(
            List::new(items)
                .block(self.pane(
                    " bindings ".to_owned(),
                    Some(Focus::Bindings),
                    Background::Side,
                ))
                .highlight_style(Style::new().bg(palette.selection)),
            area,
            &mut state,
        );
    }

    fn draw_breakpoints(&self, screen: &mut Screen, area: Rect) {
        let palette = &self.palette;
        let dot = Span::styled("● ", Style::new().fg(palette.bad));
        let items: Vec<ListItem> = self
            .breakpoints
            .iter()
            .map(|breakpoint| {
                let spans = match breakpoint {
                    Breakpoint::Line { file, line } => {
                        vec![
                            dot.clone(),
                            Span::raw(format!("{}:{line}", file_name(file))),
                        ]
                    }
                    Breakpoint::Function(name) => vec![
                        dot.clone(),
                        Span::styled("fn ", palette.code(colours::Code::Keyword)),
                        Span::styled(name.as_str(), palette.code(colours::Code::Function)),
                    ],
                };
                ListItem::new(Text::from(spans))
            })
            .collect();
        screen.render_widget(
            List::new(items).block(self.pane(" breakpoints ".to_owned(), None, Background::Side)),
            area,
        );
    }

    fn draw_console(&self, screen: &mut Screen, area: Rect) {
        let palette = &self.palette;
        let quiet = Style::new().fg(palette.quiet);
        let block = self.pane(" console ".to_owned(), None, Background::Side);
        let inner = block.inner(area);
        screen.render_widget(block, area);
        let height = usize::from(inner.height);
        let from = self.console.len().saturating_sub(height);
        let lines: Vec<Text> = self.console[from..]
            .iter()
            .map(|line| {
                let text = line.text.as_str();
                match line.kind {
                    Kind::Stdout => Text::raw(text),
                    Kind::Stderr => Text::styled(text, Style::new().fg(palette.warning)),
                    Kind::Eval => Text::from(vec![Span::styled("> ", quiet), Span::raw(text)]),
                    Kind::Value => Text::from(vec![
                        Span::styled("= ", quiet),
                        Span::styled(text, palette.value(text)),
                    ]),
                    Kind::Error => Text::styled(text, Style::new().fg(palette.error)),
                    Kind::Note => Text::styled(format!("-- {text}"), quiet),
                }
            })
            .collect();
        screen.render_widget(Paragraph::new(lines), inner);
    }

    fn draw_bottom(&self, screen: &mut Screen, area: Rect) {
        let palette = &self.palette;
        let quiet = Style::new().fg(palette.quiet);
        if let Some(prompt) = &self.prompt {
            let label = match prompt.purpose {
                Purpose::Eval => "eval> ",
                Purpose::Break => "break at (file:line, line or function)> ",
            };
            let line = Text::from(vec![
                Span::styled(label, Style::new().fg(palette.accent)),
                Span::raw(prompt.text.as_str()),
            ]);
            screen.render_widget(Paragraph::new(line), area);
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
        let line = match &self.status {
            Some(status) => Text::styled(status.as_str(), Style::new().fg(palette.bad)),
            None => {
                let mut spans = Vec::new();
                for (index, (key, does)) in KEYS.iter().enumerate() {
                    if index > 0 {
                        spans.push(Span::styled(" · ", Style::new().fg(palette.rule)));
                    }
                    spans.push(Span::raw(*key));
                    spans.push(Span::styled(format!(" {does}"), quiet));
                }
                Text::from(spans)
            }
        };
        screen.render_widget(Paragraph::new(line), area);
    }
}

/// What a pane is drawn on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Background {
    /// The source's: the window's surface.
    Surface,
    /// Every other pane's: a shade under the surface.
    Side,
}
