// Generated automatically by iced_lucide at build time.
// Do not edit manually.
// 90ce28d4079990f28085ec1b953b279637b1d9eaf2cbec838433f00497710b48
use iced::widget::text::{self, Text};

pub const FONT: &[u8] = include_bytes!("../fonts/lucide.ttf");

/// All icons as `(name, codepoint_str)` pairs.
/// Use this to populate an icon-picker widget.
#[allow(dead_code)]
pub const ALL_ICONS: &[(&str, &str)] = &[
    ("chevron_down", "\u{E06D}"),
    ("chevron_right", "\u{E06F}"),
    ("pause", "\u{E12E}"),
    ("refresh", "\u{E145}"),
    ("resume", "\u{E13C}"),
    ("step_into", "\u{E44D}"),
    ("step_out", "\u{E44E}"),
    ("step_over", "\u{E450}"),
];

pub fn chevron_down<'a, Theme>() -> Text<'a, Theme>
where
    Theme: text::Catalog + 'a,
{
    icon("\u{E06D}")
}

pub fn chevron_right<'a, Theme>() -> Text<'a, Theme>
where
    Theme: text::Catalog + 'a,
{
    icon("\u{E06F}")
}

pub fn pause<'a, Theme>() -> Text<'a, Theme>
where
    Theme: text::Catalog + 'a,
{
    icon("\u{E12E}")
}

pub fn refresh<'a, Theme>() -> Text<'a, Theme>
where
    Theme: text::Catalog + 'a,
{
    icon("\u{E145}")
}

pub fn resume<'a, Theme>() -> Text<'a, Theme>
where
    Theme: text::Catalog + 'a,
{
    icon("\u{E13C}")
}

pub fn step_into<'a, Theme>() -> Text<'a, Theme>
where
    Theme: text::Catalog + 'a,
{
    icon("\u{E44D}")
}

pub fn step_out<'a, Theme>() -> Text<'a, Theme>
where
    Theme: text::Catalog + 'a,
{
    icon("\u{E44E}")
}

pub fn step_over<'a, Theme>() -> Text<'a, Theme>
where
    Theme: text::Catalog + 'a,
{
    icon("\u{E450}")
}

/// Render any Lucide icon by its codepoint string.
/// Use this together with [`ALL_ICONS`] to display icons dynamically:
/// ```ignore
/// for (name, cp) in ALL_ICONS {
///     button(render(cp)).on_press(Msg::Pick(name.to_string()))
/// }
/// ```
pub fn render<'a, Theme>(codepoint: &'a str) -> Text<'a, Theme>
where
    Theme: text::Catalog + 'a,
{
    Text::new(codepoint).font("lucide")
}

fn icon<'a, Theme>(codepoint: &'a str) -> Text<'a, Theme>
where
    Theme: text::Catalog + 'a,
{
    render(codepoint)
}
