//! Which snippets `?play` and the playground can't run yet.
//!
//! Both run plain Rust only. An iced program needs generated bindings, and
//! none reach the bot or the browser yet. An async program needs an async
//! runtime such as tokio, which needs them too. Such a snippet gets one
//! plain message instead of a run.
//!
//! A snippet is iced's when it names the `iced` crate, or when it uses a
//! name from [`ICED_PRELUDE`] that it neither declares nor imports. That
//! list is routing data: the names a snippet could once use without a
//! `use`. It binds nothing.

use rawr::layer::Facts;

/// Why a snippet doesn't run here yet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unavailable {
    /// It uses iced.
    Iced,
    /// Its `main` is `async`, or it names tokio.
    Async,
}

impl Unavailable {
    /// The one message a snippet of this kind gets.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::Iced => ICED,
            Self::Async => ASYNC,
        }
    }
}

/// The message an iced snippet gets.
pub const ICED: &str = "iced programs don't run here yet. \
They will once generated bindings reach ?play and the playground.";

/// The message an async snippet gets.
pub const ASYNC: &str = "async programs (`async fn main`, `#[tokio::main]`) don't run here yet. \
They will once generated bindings reach ?play and the playground.";

/// Why the snippet `source`, whose parse gave `facts`, doesn't run here:
/// `None` for a plain program.
#[must_use]
pub fn unavailable(source: &str, facts: &Facts) -> Option<Unavailable> {
    let prelude = ICED_PRELUDE.iter().any(|path| {
        let name = path.rsplit("::").next().unwrap_or(path);
        facts.uses_free(name)
    });
    if facts.names_crate("iced") || prelude {
        Some(Unavailable::Iced)
    } else if facts.names_crate("tokio")
        || crate::plain::has_async_main(&crate::without_tests(source))
    {
        Some(Unavailable::Async)
    } else {
        None
    }
}

/// The iced paths a snippet could use without a `use`. A snippet that uses
/// one of these names, and neither declares nor imports it, is iced's.
pub const ICED_PRELUDE: &[&str] = &[
    "iced::Alignment",
    "iced::Background",
    "iced::Border",
    "iced::Bottom",
    "iced::Center",
    "iced::Color",
    "iced::ContentFit",
    "iced::Degrees",
    "iced::Element",
    "iced::Fill",
    "iced::FillPortion",
    "iced::Font",
    "iced::Function",
    "iced::Left",
    "iced::Length",
    "iced::Padding",
    "iced::Point",
    "iced::Radians",
    "iced::Rectangle",
    "iced::Renderer",
    "iced::Right",
    "iced::Shadow",
    "iced::Shrink",
    "iced::Size",
    "iced::Subscription",
    "iced::Task",
    "iced::Theme",
    "iced::Top",
    "iced::Vector",
    "iced::Widget",
    "iced::alignment",
    "iced::border",
    "iced::color",
    "iced::font",
    "iced::gradient",
    "iced::mouse",
    "iced::padding",
    "iced::theme",
    "iced::widget::Button",
    "iced::widget::Canvas",
    "iced::widget::Checkbox",
    "iced::widget::Column",
    "iced::widget::ComboBox",
    "iced::widget::Container",
    "iced::widget::Float",
    "iced::widget::Id",
    "iced::widget::Image",
    "iced::widget::Logo",
    "iced::widget::MouseArea",
    "iced::widget::PickList",
    "iced::widget::Pin",
    "iced::widget::ProgressBar",
    "iced::widget::QRCode",
    "iced::widget::Radio",
    "iced::widget::Responsive",
    "iced::widget::Row",
    "iced::widget::Scrollable",
    "iced::widget::Sensor",
    "iced::widget::Slider",
    "iced::widget::Space",
    "iced::widget::Stack",
    "iced::widget::Svg",
    "iced::widget::Table",
    "iced::widget::Text",
    "iced::widget::TextEditor",
    "iced::widget::TextInput",
    "iced::widget::Toggler",
    "iced::widget::Tooltip",
    "iced::widget::Transition",
    "iced::widget::bottom",
    "iced::widget::button",
    "iced::widget::canvas",
    "iced::widget::center",
    "iced::widget::center_x",
    "iced::widget::center_y",
    "iced::widget::checkbox",
    "iced::widget::column",
    "iced::widget::combo_box",
    "iced::widget::component",
    "iced::widget::container",
    "iced::widget::float",
    "iced::widget::hover",
    "iced::widget::image",
    "iced::widget::keyed_column",
    "iced::widget::lazy",
    "iced::widget::left",
    "iced::widget::mouse_area",
    "iced::widget::opaque",
    "iced::widget::pick_list",
    "iced::widget::pin",
    "iced::widget::progress_bar",
    "iced::widget::qr_code",
    "iced::widget::radio",
    "iced::widget::responsive",
    "iced::widget::rich_text",
    "iced::widget::right",
    "iced::widget::row",
    "iced::widget::rule",
    "iced::widget::scrollable",
    "iced::widget::sensor",
    "iced::widget::slider",
    "iced::widget::space",
    "iced::widget::span",
    "iced::widget::stack",
    "iced::widget::svg",
    "iced::widget::table",
    "iced::widget::text",
    "iced::widget::text_editor",
    "iced::widget::text_input",
    "iced::widget::toggler",
    "iced::widget::tooltip",
    "iced::widget::top",
    "iced::widget::transition",
    "iced::widget::vertical_slider",
    "iced::widget::void",
];

#[cfg(test)]
mod tests {
    use super::{unavailable, Unavailable};

    fn route(source: &str) -> Option<Unavailable> {
        let program = crate::plain::program(source);
        let unit = rawr::Engine::new().parse_unit(&program).unwrap();
        unavailable(source, &rawr::layer::Facts::of_unit(&unit))
    }

    #[test]
    fn iced_and_async_snippets_are_routed_and_plain_ones_run() {
        // The `iced` crate, or a prelude name used without a `use`.
        assert_eq!(
            route("use iced::widget::text;\nfn main() {}\n"),
            Some(Unavailable::Iced)
        );
        assert_eq!(route("column![text(\"hi\")]"), Some(Unavailable::Iced));
        assert_eq!(
            route("fn view() -> impl Widget<()> {\n    text(\"x\")\n}\n"),
            Some(Unavailable::Iced)
        );
        // A name the snippet declares itself is its own.
        assert_eq!(
            route("fn text(s: &str) -> String { s.to_owned() }\nfn main() { println!(\"{}\", text(\"x\")); }\n"),
            None
        );
        assert_eq!(
            route("#[tokio::main]\nasync fn main() {\n    println!(\"hi\");\n}\n"),
            Some(Unavailable::Async)
        );
        assert_eq!(
            route("async fn main() {\n    println!(\"hi\");\n}\n"),
            Some(Unavailable::Async)
        );
        assert_eq!(route("1 + 2"), None);
        assert_eq!(route("fn main() {\n    println!(\"hi\");\n}\n"), None);
    }
}
