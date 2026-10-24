//! Private Rust identities for exact compile-time and runtime facade types.

use rawr::{item, Dynamic};

use crate::widget;
use crate::widget::style::Source;
use iced::widget::{button, checkbox, radio, scrollable, slider, text_input, toggler};

pub(crate) mod text {
    /// One compiler-sealed identity for a pinned iced text style item.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum Style {
        Default,
        Base,
        Primary,
        Secondary,
        Success,
        Warning,
        Danger,
    }
}

pub(crate) mod container {
    /// A container's style: a pinned built-in, or a style function.
    #[derive(Clone)]
    pub(crate) enum Source {
        Builtin(Style),
        Function(rawr::item::Value),
    }

    /// One compiler-sealed identity for a pinned iced container style item.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum Style {
        Transparent,
        RoundedBox,
        BorderedBox,
        Dark,
        Primary,
        Secondary,
        Success,
        Warning,
        Danger,
    }
}

#[derive(Clone)]
pub(crate) struct Result;
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Alignment(pub(crate) iced::Alignment);
#[derive(Clone)]
pub(crate) struct Application;
/// iced's `Widget` trait, which source names only as `impl Widget<Message>`.
pub(crate) struct Widget;

/// iced's `table::column(header, view)`: a header and the function that
/// makes each row's cell.
#[derive(Clone)]
pub(crate) struct TableColumn {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) header: widget::Value,
    pub(crate) view: item::Value,
    pub(crate) width: Option<iced::Length>,
    pub(crate) align_x: Option<iced::alignment::Horizontal>,
    pub(crate) align_y: Option<iced::alignment::Vertical>,
}

/// iced's `table(columns, rows)`.
#[derive(Clone)]
pub(crate) struct Table {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) columns: Vec<TableColumn>,
    pub(crate) rows: Vec<Dynamic>,
    pub(crate) width: Option<iced::Length>,
    pub(crate) padding_x: Option<f32>,
    pub(crate) padding_y: Option<f32>,
    pub(crate) separator_x: Option<f32>,
    pub(crate) separator_y: Option<f32>,
}

/// iced's `Function` trait, whose `with` a `use iced::Function` brings to
/// every function of two or more inputs.
pub(crate) struct Function;

/// iced's `Program` trait, which source names only as `impl Program<..>`.
pub(crate) struct Program;

/// iced's `component::Component` trait, which script types implement.
pub(crate) struct ComponentTrait;

/// A script `component::Component`: its value, its impl's `update` and
/// `view`, and its State's default.
#[derive(Clone)]
pub(crate) struct Component {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) value: Dynamic,
    pub(crate) update: item::Value,
    pub(crate) view: item::Value,
    pub(crate) state: Dynamic,
}

/// iced's `Renderer`, which a component's `update` is lent.
#[derive(Clone)]
pub(crate) struct Renderer;

/// iced's `void()`, which containers drop.
#[derive(Clone)]
pub(crate) struct Void {
    pub(crate) identity: widget::Identity,
}

#[derive(Clone)]
pub(crate) struct Sensor {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) child: widget::Value,
    pub(crate) on_show: Option<item::Value>,
    pub(crate) on_resize: Option<item::Value>,
    pub(crate) on_hide: Option<Dynamic>,
}

#[derive(Clone)]
pub(crate) struct Column {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) children: Vec<widget::Value>,
    pub(crate) mode: widget::key::Mode,
    pub(crate) spacing: f32,
    pub(crate) padding: iced::Padding,
    pub(crate) width: iced::Length,
    pub(crate) height: Option<iced::Length>,
    pub(crate) align_x: iced::Alignment,
}

#[derive(Clone)]
pub(crate) struct Row {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) children: Vec<widget::Value>,
    pub(crate) spacing: f32,
    pub(crate) padding: iced::Padding,
    pub(crate) width: iced::Length,
    pub(crate) height: iced::Length,
    pub(crate) align_y: iced::Alignment,
}

#[derive(Clone)]
pub(crate) struct Container {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) child: widget::Value,
    pub(crate) padding: iced::Padding,
    pub(crate) width: iced::Length,
    pub(crate) height: iced::Length,
    pub(crate) align_x: iced::Alignment,
    pub(crate) align_y: iced::Alignment,
    pub(crate) style: Option<container::Source>,
    pub(crate) id: Option<iced::widget::Id>,
}

#[derive(Clone)]
pub(crate) struct Button {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) content: Content,
    pub(crate) on_press: Option<Dynamic>,
    pub(crate) padding: iced::Padding,
    pub(crate) width: Option<iced::Length>,
    pub(crate) height: Option<iced::Length>,
    pub(crate) style: Option<Source<button::Status, button::Style>>,
}

/// A button's content: a label, or any element as in iced.
#[derive(Clone)]
pub(crate) enum Content {
    Label(String),
    Element(widget::Value),
}

#[derive(Clone)]
pub(crate) struct Text {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) content: String,
    pub(crate) size: Option<f32>,
    pub(crate) width: iced::Length,
    pub(crate) height: Option<iced::Length>,
    pub(crate) align_x: iced::advanced::text::Alignment,
    pub(crate) align_y: Option<iced::alignment::Vertical>,
    pub(crate) style: Option<text::Style>,
    pub(crate) color: Option<iced::Color>,
    pub(crate) font: Option<iced::Font>,
    pub(crate) line_height: Option<f32>,
}

#[derive(Clone)]
pub(crate) struct Scrollable {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) child: widget::Value,
    pub(crate) width: Option<iced::Length>,
    pub(crate) height: Option<iced::Length>,
    pub(crate) style: Option<Source<scrollable::Status, scrollable::Style>>,
    pub(crate) id: Option<iced::widget::Id>,
    pub(crate) direction: Option<scrollable::Direction>,
    pub(crate) spacing: Option<f32>,
    pub(crate) auto_scroll: Option<bool>,
    pub(crate) on_scroll: Option<item::Value>,
}

/// iced's `scrollable::Direction::Both { horizontal, vertical }`, written
/// as a struct literal.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ScrollBoth {
    pub(crate) horizontal: scrollable::Scrollbar,
    pub(crate) vertical: scrollable::Scrollbar,
}

impl From<ScrollBoth> for scrollable::Direction {
    fn from(both: ScrollBoth) -> Self {
        Self::Both {
            horizontal: both.horizontal,
            vertical: both.vertical,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Space {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) width: iced::Length,
    pub(crate) height: iced::Length,
}

#[derive(Clone)]
pub(crate) struct TextInput {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) placeholder: String,
    pub(crate) value: String,
    pub(crate) on_input: Option<item::Value>,
    pub(crate) on_submit: Option<Dynamic>,
    pub(crate) padding: iced::Padding,
    pub(crate) size: Option<f32>,
    pub(crate) width: Option<iced::Length>,
    pub(crate) secure: bool,
    pub(crate) style: Option<Source<text_input::Status, text_input::Style>>,
    pub(crate) id: Option<iced::widget::Id>,
}

#[derive(Clone)]
pub(crate) struct Checkbox {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) checked: bool,
    pub(crate) label: Option<String>,
    pub(crate) on_toggle: Option<item::Value>,
    pub(crate) style: Option<Source<checkbox::Status, checkbox::Style>>,
    pub(crate) size: Option<f32>,
    pub(crate) width: Option<iced::Length>,
    pub(crate) spacing: Option<f32>,
    pub(crate) icon: Option<iced::widget::checkbox::Icon>,
}

#[derive(Clone)]
pub(crate) struct Toggler {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) toggled: bool,
    pub(crate) label: Option<String>,
    pub(crate) size: Option<f32>,
    pub(crate) on_toggle: Option<item::Value>,
    pub(crate) style: Option<Source<toggler::Status, toggler::Style>>,
    pub(crate) width: Option<iced::Length>,
    pub(crate) spacing: Option<f32>,
}

/// iced's `radio`: selected when `selected` is `Some` of `value`, compared
/// as a derived `PartialEq` does.
#[derive(Clone)]
pub(crate) struct Radio {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) label: String,
    pub(crate) value: Dynamic,
    pub(crate) selected: bool,
    pub(crate) on_click: item::Value,
    pub(crate) size: Option<f32>,
    pub(crate) spacing: Option<f32>,
    pub(crate) text_size: Option<f32>,
    pub(crate) width: Option<iced::Length>,
    pub(crate) style: Option<Source<radio::Status, radio::Style>>,
}

/// iced's `pick_list`: `options` are the script's values, labelled by
/// `to_string` when lowered.
#[derive(Clone)]
pub(crate) struct PickList {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) selected: Option<Dynamic>,
    pub(crate) options: Vec<Dynamic>,
    pub(crate) to_string: item::Value,
    pub(crate) on_select: Option<item::Value>,
    pub(crate) placeholder: Option<String>,
    pub(crate) width: Option<iced::Length>,
    pub(crate) padding: Option<iced::Padding>,
    pub(crate) text_size: Option<f32>,
}

/// iced's `combo_box::State<T>`: the script's options and the `to_string`
/// that labels them. The iced state it stands for (the typed text and the
/// options it filters) is made where it is first drawn and kept under `id`
/// (`widget::builtin::combo_box_state`), so it outlives each view as the
/// app-owned state does in iced.
#[derive(Clone)]
pub(crate) struct ComboBoxState {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) id: u64,
    pub(crate) options: Vec<Dynamic>,
    pub(crate) to_string: item::Value,
}

/// iced's `combo_box(state, placeholder, selection, on_selected)`.
#[derive(Clone)]
pub(crate) struct ComboBox {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) state: ComboBoxState,
    pub(crate) placeholder: String,
    pub(crate) selection: Option<Dynamic>,
    pub(crate) on_selected: item::Value,
    pub(crate) on_option_hovered: Option<item::Value>,
    pub(crate) on_open: Option<Dynamic>,
    pub(crate) on_close: Option<Dynamic>,
    pub(crate) width: Option<iced::Length>,
    pub(crate) size: Option<f32>,
}

/// The number a slider moves over.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SliderNumber {
    Integer,
    Float,
    Degrees,
    Radians,
}

/// iced's `Degrees::RANGE` or `Radians::RANGE`, a `RangeInclusive` of
/// an angle.
#[derive(Clone)]
pub(crate) enum AngleRange {
    Degrees(std::ops::RangeInclusive<iced::Degrees>),
    Radians(std::ops::RangeInclusive<iced::Radians>),
}

/// A slider over `i64`, `f64`, `Degrees` or `Radians`; its numbers are
/// kept as `f64` in every case.
#[derive(Clone)]
pub(crate) struct Slider {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) range: std::ops::RangeInclusive<f64>,
    pub(crate) value: f64,
    pub(crate) number: SliderNumber,
    pub(crate) on_change: item::Value,
    pub(crate) step: f64,
    pub(crate) default: Option<f64>,
    pub(crate) shift_step: Option<f64>,
    pub(crate) vertical: bool,
    pub(crate) style: Option<Source<slider::Status, slider::Style>>,
    /// iced's `width`/`height`: a `Length` along the rail, pixels across.
    pub(crate) width: Option<iced::Length>,
    pub(crate) height: Option<iced::Length>,
}

/// iced's logo, `iced(text_size)`.
#[derive(Clone)]
pub(crate) struct Logo {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) size: f32,
}

/// iced's `progress_bar(range, value)`.
#[derive(Clone)]
pub(crate) struct ProgressBar {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) range: std::ops::RangeInclusive<f32>,
    pub(crate) value: f32,
    pub(crate) vertical: bool,
    pub(crate) length: Option<iced::Length>,
    pub(crate) girth: Option<iced::Length>,
    pub(crate) style: Option<progress_bar::Source>,
}

pub(crate) mod progress_bar {
    use iced::widget::progress_bar::Style;

    /// A progress bar's style: a pinned iced function, or a Rawr function
    /// of the theme.
    #[derive(Clone)]
    pub(crate) enum Source {
        Builtin(fn(&iced::Theme) -> Style),
        Function(rawr::item::Value),
    }

    /// The value of a pinned iced style function, as
    /// `.style(progress_bar::success)`.
    #[derive(Clone, Copy)]
    pub(crate) struct Builtin(pub(crate) fn(&iced::Theme) -> Style);
}

/// iced's `transition(value, init, view)`: `view` drawn with the animation
/// settled at `value`.
#[derive(Clone)]
pub(crate) struct Transition {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) value: f64,
    pub(crate) view: item::Value,
}

/// iced's `Animation<T>` of a `bool` or an `f32`.
#[derive(Clone, Debug)]
pub(crate) enum Animation {
    Bool(iced::Animation<bool>),
    Float(iced::Animation<f32>),
}

impl PartialEq for Animation {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Bool(left), Self::Bool(right)) => left.value() == right.value(),
            (Self::Float(left), Self::Float(right)) => left.value() == right.value(),
            _ => false,
        }
    }
}

/// `animation::Easing`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Easing(pub(crate) iced::animation::Easing);

/// `time::Instant`: a moment, as `Instant::now()`, a frame or a tick reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Instant(pub(crate) iced::time::Instant);

impl Instant {
    /// The present, or the simulated time while a recording's clock is
    /// frozen.
    pub(crate) fn now() -> Self {
        Self(crate::snapshot::clock::now().unwrap_or_else(iced::time::Instant::now))
    }
}

/// iced's `tooltip(content, tooltip, position)`.
#[derive(Clone)]
pub(crate) struct Tooltip {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) content: widget::Value,
    pub(crate) tooltip: Content,
    pub(crate) position: iced::widget::tooltip::Position,
    pub(crate) gap: f32,
    pub(crate) delay: std::time::Duration,
    pub(crate) snap_within_viewport: bool,
    pub(crate) style: Option<container::Source>,
}

/// `alignment::Horizontal`, as the `Alignment` it converts into.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Horizontal(pub(crate) iced::Alignment);

/// `alignment::Vertical`, as the `Alignment` it converts into.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Vertical(pub(crate) iced::Alignment);

/// iced's `stack![..]`: children drawn on top of each other.
#[derive(Clone)]
pub(crate) struct Stack {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) children: Vec<widget::Value>,
    pub(crate) width: Option<iced::Length>,
    pub(crate) height: Option<iced::Length>,
}

/// A widget the facade lowers by one native function and no builders:
/// `rule::horizontal(1)`, `hover(base, top)`, `lazy(dependency, view)`.
#[derive(Clone)]
pub(crate) struct Native {
    pub(crate) identity: widget::Identity,
    pub(crate) lower: std::sync::Arc<
        dyn Fn(&widget::Context<'_>) -> iced::Element<'static, crate::runtime::Message>
            + Send
            + Sync,
    >,
}

/// iced's `pin(content)`: content at a fixed position.
#[derive(Clone)]
pub(crate) struct Pin {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) child: widget::Value,
    pub(crate) position: iced::Point,
    pub(crate) width: iced::Length,
    pub(crate) height: iced::Length,
}

/// iced's `float(content)`: content drawn scaled and moved above the rest.
#[derive(Clone)]
pub(crate) struct Float {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) child: widget::Value,
    pub(crate) scale: f32,
    /// A Rawr `Fn(Rectangle, Rectangle) -> Vector`.
    pub(crate) translate: Option<rawr::item::Value>,
    /// A Rawr `Fn(&Theme) -> float::Style`.
    pub(crate) style: Option<rawr::item::Value>,
}

/// iced's `mouse_area(content)`: the messages of each mouse event.
#[derive(Clone)]
pub(crate) struct MouseArea {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) child: widget::Value,
    /// One optional Message per `crate::MOUSE_AREA_MESSAGES` entry.
    pub(crate) messages: [Option<Dynamic>; 9],
    pub(crate) on_move: Option<item::Value>,
    pub(crate) interaction: Option<iced::mouse::Interaction>,
}

/// iced's `image(handle)`.
#[derive(Clone)]
pub(crate) struct Image {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) handle: iced::widget::image::Handle,
    pub(crate) width: Option<iced::Length>,
    pub(crate) height: Option<iced::Length>,
    pub(crate) content_fit: Option<iced::ContentFit>,
    pub(crate) opacity: Option<f32>,
    pub(crate) scale: Option<f32>,
    pub(crate) filter_method: Option<iced::widget::image::FilterMethod>,
    pub(crate) rotation: Option<iced::Rotation>,
}

/// iced's `span(text)`: a fragment of rich text, its link a Message-like
/// value that `on_link_click` maps.
#[derive(Clone)]
pub(crate) struct Span {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) text: String,
    pub(crate) size: Option<f32>,
    pub(crate) font: Option<iced::Font>,
    pub(crate) color: Option<iced::Color>,
    pub(crate) link: Option<Dynamic>,
    pub(crate) underline: bool,
    pub(crate) strikethrough: bool,
}

/// iced's `rich_text(spans)` and `rich_text![..]`.
#[derive(Clone)]
pub(crate) struct Rich {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) spans: Vec<Span>,
    pub(crate) on_link_click: Option<item::Value>,
    pub(crate) size: Option<f32>,
    pub(crate) font: Option<iced::Font>,
    pub(crate) color: Option<iced::Color>,
    pub(crate) width: Option<iced::Length>,
    pub(crate) height: Option<iced::Length>,
}

/// iced's `svg(handle)`.
#[derive(Clone)]
pub(crate) struct Svg {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) handle: iced::widget::svg::Handle,
    pub(crate) width: Option<iced::Length>,
    pub(crate) height: Option<iced::Length>,
    pub(crate) content_fit: Option<iced::ContentFit>,
    pub(crate) opacity: Option<f32>,
    pub(crate) style: Option<Source<iced::widget::svg::Status, iced::widget::svg::Style>>,
}

/// iced's `qr_code::Data`, as the text it encodes: iced's holds a drawing
/// cache that is neither `Clone` nor `Sync`, so each view encodes it again.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct QrData(pub(crate) std::sync::Arc<str>);

/// iced's `qr_code(&data)`.
#[derive(Clone)]
pub(crate) struct QrCode {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) data: QrData,
    pub(crate) cell_size: Option<f32>,
    pub(crate) total_size: Option<f32>,
}

/// iced's `canvas::Program` trait, which script types implement.
pub(crate) struct CanvasTrait;

/// A script `canvas::Program`: its value, its impl's `update`, `draw` and
/// `mouse_interaction`, and its State's default.
#[derive(Clone)]
pub(crate) struct CanvasProgram {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) value: Dynamic,
    pub(crate) update: item::Value,
    pub(crate) draw: item::Value,
    pub(crate) interaction: item::Value,
    pub(crate) state: Dynamic,
}

/// iced's `canvas(program)`.
#[derive(Clone)]
pub(crate) struct Canvas {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) identity: widget::Identity,
    pub(crate) program: CanvasProgram,
    pub(crate) width: iced::Length,
    pub(crate) height: iced::Length,
}

/// One drawing operation a script `Frame` records, replayed on iced's own
/// frame when the canvas draws.
#[derive(Clone, Debug)]
pub(crate) enum Draw {
    Fill(iced::widget::canvas::Path, iced::widget::canvas::Fill),
    FillRectangle(iced::Point, iced::Size, iced::widget::canvas::Fill),
    Stroke(iced::widget::canvas::Path, iced::widget::canvas::Stroke<'static>),
    StrokeRectangle(iced::Point, iced::Size, iced::widget::canvas::Stroke<'static>),
    FillText(iced::widget::canvas::Text),
    PushTransform,
    PopTransform,
    Translate(iced::Vector),
    Rotate(f32),
    Scale(f32),
    ScaleNonuniform(iced::Vector),
}

/// iced's `canvas::Frame`: the operations drawn on it so far. A copy draws
/// on the same frame, as the `&mut Frame` a closure takes does.
#[derive(Clone)]
pub(crate) struct Frame {
    pub(crate) size: iced::Size,
    pub(crate) draws: std::sync::Arc<std::sync::Mutex<Vec<Draw>>>,
}

/// iced's `canvas::Geometry`: a finished frame's operations and size.
#[derive(Clone, Debug)]
pub(crate) struct Geometry {
    pub(crate) size: iced::Size,
    pub(crate) draws: std::sync::Arc<[Draw]>,
}

/// iced's `canvas::Cache`: the geometry it last drew, and its size. Copies
/// share it, so `clear` through `&self` empties every copy.
#[derive(Clone, Default)]
pub(crate) struct Cache(pub(crate) std::sync::Arc<std::sync::Mutex<Option<Geometry>>>);

/// `canvas::Group`: caches iced draws together, which Rawr draws one by one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CacheGroup;

impl std::fmt::Debug for Cache {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Cache")
    }
}

/// iced's `canvas::path::Builder`, shared by the closure `Path::new` gives it.
#[derive(Clone)]
pub(crate) struct PathBuilder(
    pub(crate) std::sync::Arc<std::sync::Mutex<Option<iced::widget::canvas::path::Builder>>>,
);

/// iced's `canvas::Action<Message>`.
#[derive(Clone)]
pub(crate) struct CanvasAction {
    pub(crate) witness: rawr::host::runtime::Witness,
    pub(crate) message: Option<Dynamic>,
    pub(crate) redraw: bool,
    pub(crate) capture: bool,
}
