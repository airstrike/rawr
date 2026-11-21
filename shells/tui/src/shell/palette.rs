//! The shell's colours: `rawr gui`'s dark theme, iced's Oxocarbon, in a
//! terminal. Each colour mirrors one of the window's in
//! `shells/gui/src/theme/colour.rs`, worked out from iced's Oxocarbon
//! palette: the window's colours need iced, which the terminal shell
//! doesn't take. A terminal that draws 24-bit colour gets them as they
//! are. One that draws 256 gets each one's nearest of the xterm cube and
//! greys, and never the sixteen named colours a terminal's own theme
//! repaints. Where the nearest would merge two colours (the surface and
//! the cursor's band, or the paused line's band and a selection), a
//! neighbour keeps them apart.

use ratatui::style::{Color, Modifier, Style};

use super::colours::Code;

/// The colours a terminal draws.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Depth {
    /// 24-bit colour.
    #[default]
    True,
    /// The xterm palette of 256.
    Indexed,
}

impl Depth {
    /// The depth of the terminal the shell runs in, as its `COLORTERM`
    /// says: `truecolor` or `24bit` for 24-bit colour, else 256.
    #[must_use]
    pub fn of_terminal() -> Self {
        match std::env::var("COLORTERM").as_deref() {
            Ok("truecolor" | "24bit") => Self::True,
            _ => Self::Indexed,
        }
    }
}

/// The colours, each named for what it draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Palette {
    /// The source's background: the window's surface.
    pub(super) surface: Color,
    /// The background of the panes beside and under the source, a shade
    /// under the surface.
    pub(super) side: Color,
    pub(super) text: Color,
    /// A gutter's numbers, a pane's title, marks and anything said aside.
    pub(super) quiet: Color,
    /// A pane's border.
    pub(super) rule: Color,
    /// What asks to be looked at: where the program paused.
    pub(super) accent: Color,
    /// The band under the line the program paused at: the accent, faint.
    pub(super) band: Color,
    /// A selected row: the accent, a little less faint.
    pub(super) selection: Color,
    /// The band under the source's cursor.
    pub(super) cursor: Color,
    pub(super) good: Color,
    /// What a run says on its standard error.
    pub(super) warning: Color,
    pub(super) bad: Color,
    /// A failure's own words.
    pub(super) error: Color,
    pub(super) keyword: Color,
    pub(super) function: Color,
    pub(super) kind: Color,
    pub(super) string: Color,
    pub(super) constant: Color,
}

/// One of the window's colours: its RGB and the xterm colour of the 256
/// that stands for it.
///
/// Each one below says how the window makes it from iced's Oxocarbon
/// palette, whose seed is a 0x232323 background, 0xd0d0d0 text, a 0x00b4ff
/// primary, a 0x00c15a success, a 0xbe95ff warning and a 0xf62d0f danger.
/// `calm` is a colour a quarter of the way to the text.
#[derive(Debug, Clone, Copy)]
struct Shade {
    rgb: [u8; 3],
    xterm: u8,
}

/// `background.base`.
const SURFACE: Shade = Shade {
    rgb: [0x23, 0x23, 0x23],
    xterm: 235,
};
/// `theme::side`: the background darkened by 0.025.
const SIDE: Shade = Shade {
    rgb: [0x1d, 0x1d, 0x1d],
    xterm: 234,
};
const TEXT: Shade = Shade {
    rgb: [0xd0, 0xd0, 0xd0],
    xterm: 252,
};
/// `theme::quiet`: `secondary.base`.
const QUIET: Shade = Shade {
    rgb: [0x7c, 0x7c, 0x7c],
    xterm: 244,
};
/// `theme::rule`: `background.strong`.
const RULE: Shade = Shade {
    rgb: [0x49, 0x49, 0x49],
    xterm: 238,
};
/// `primary.base`.
const ACCENT: Shade = Shade {
    rgb: [0x00, 0xb4, 0xff],
    xterm: 39,
};
/// `theme::marked`: the primary at 0.16 on the background. The nearest
/// xterm colour is a grey, so it takes the cube's darkest cyan.
const BAND: Shade = Shade {
    rgb: [0x1d, 0x3a, 0x46],
    xterm: 23,
};
/// `theme::item`'s selected row: the primary at 0.22 on the background,
/// and in 256 colours the cube's cyan a step bluer than the band's.
const SELECTION: Shade = Shade {
    rgb: [0x1b, 0x43, 0x53],
    xterm: 24,
};
/// `background.weakest`. The nearest xterm grey is the surface's, so it
/// takes the next.
const CURSOR: Shade = Shade {
    rgb: [0x2a, 0x2a, 0x2a],
    xterm: 236,
};
/// `theme::done`: the success, calm.
const GOOD: Shade = Shade {
    rgb: [0x6f, 0xc5, 0x85],
    xterm: 78,
};
/// The warning, calm.
const WARNING: Shade = Shade {
    rgb: [0xc3, 0xa6, 0xf4],
    xterm: 147,
};
/// `theme::breakpoint`: the danger, calm, and in 256 colours the red
/// a little further than the nearest orange.
const BAD: Shade = Shade {
    rgb: [0xed, 0x75, 0x70],
    xterm: 167,
};
/// `theme::error`: `danger.base`.
const ERROR: Shade = Shade {
    rgb: [0xf6, 0x2d, 0x0f],
    xterm: 196,
};
/// `theme::code`'s keyword: the warning, calm.
const KEYWORD: Shade = Shade {
    rgb: [0xc3, 0xa6, 0xf4],
    xterm: 147,
};
/// `theme::code`'s function and `theme::name`: the primary, calm.
const FUNCTION: Shade = Shade {
    rgb: [0x6f, 0xbc, 0xf4],
    xterm: 75,
};
/// `theme::code`'s type and path: the primary halfway to the text.
const KIND: Shade = Shade {
    rgb: [0x98, 0xc3, 0xe9],
    xterm: 110,
};
/// `theme::code`'s string: the success, calm.
const STRING: Shade = Shade {
    rgb: [0x6f, 0xc5, 0x85],
    xterm: 78,
};
/// `theme::code`'s constant: the danger 0.45 of the way to the text.
const CONSTANT: Shade = Shade {
    rgb: [0xe6, 0x94, 0x92],
    xterm: 174,
};

impl Palette {
    /// The window's colours as a terminal of `depth` draws them.
    pub(super) fn new(depth: Depth) -> Self {
        let colour = |shade: Shade| match depth {
            Depth::True => Color::Rgb(shade.rgb[0], shade.rgb[1], shade.rgb[2]),
            Depth::Indexed => Color::Indexed(shade.xterm),
        };
        Self {
            surface: colour(SURFACE),
            side: colour(SIDE),
            text: colour(TEXT),
            quiet: colour(QUIET),
            rule: colour(RULE),
            accent: colour(ACCENT),
            band: colour(BAND),
            selection: colour(SELECTION),
            cursor: colour(CURSOR),
            good: colour(GOOD),
            warning: colour(WARNING),
            bad: colour(BAD),
            error: colour(ERROR),
            keyword: colour(KEYWORD),
            function: colour(FUNCTION),
            kind: colour(KIND),
            string: colour(STRING),
            constant: colour(CONSTANT),
        }
    }

    /// How a piece of `code` is drawn, as `theme::code` draws it: each
    /// kind in a calm hue, names and anything plain in the text's colour,
    /// and comments in italics.
    pub(super) fn code(&self, code: Code) -> Style {
        let colour = match code {
            Code::Keyword | Code::Support => self.keyword,
            Code::Function => self.function,
            Code::Type | Code::Path => self.kind,
            Code::String => self.string,
            Code::Constant => self.constant,
            Code::Punctuation | Code::Comment => self.quiet,
            Code::Invalid => self.error,
            Code::Variable | Code::Other => self.text,
        };
        let style = Style::new().fg(colour);
        if code == Code::Comment {
            style.add_modifier(Modifier::ITALIC)
        } else {
            style
        }
    }

    /// How a value's text is drawn, as `theme::value` draws it: in the
    /// colour of the literal it starts with, a string or a character, a
    /// number, or neither.
    pub(super) fn value(&self, value: &str) -> Style {
        let code = match value.chars().next() {
            Some('"' | '\'') => Code::String,
            Some(first) if first.is_ascii_digit() || (first == '-' && value.len() > 1) => {
                Code::Constant
            }
            _ => Code::Other,
        };
        self.code(code)
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::{Depth, Palette};

    #[test]
    fn a_256_colour_terminal_keeps_every_band_apart_from_what_it_lies_on() {
        let palette = Palette::new(Depth::Indexed);
        assert_eq!(palette.accent, Color::Indexed(39));
        let grounds = [
            palette.surface,
            palette.side,
            palette.cursor,
            palette.band,
            palette.selection,
        ];
        for (index, ground) in grounds.iter().enumerate() {
            assert!(!grounds[index + 1..].contains(ground), "{grounds:?}");
        }
        assert_eq!(
            Palette::new(Depth::True).accent,
            Color::Rgb(0x00, 0xb4, 0xff)
        );
    }
}
