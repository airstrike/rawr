//! Durable native implementations for the Phase 4 built-in widget facade.

use iced::widget::Widget as _;
use rawr::host;
use rawr::{Dynamic, EvalAltResult, Position};

use super::style;
use crate::{marker, runtime, widget};

pub(crate) fn bind(iced: &mut host::runtime::Builder, rawr: &mut host::runtime::Builder) {
    bind_attestations(iced);
    bind_items(iced, rawr);
    bind_projections(iced);
}

pub(super) fn bind_profile(iced: &mut host::runtime::Builder, rawr: &mut host::runtime::Builder) {
    bind_profile_attestations(iced);
    bind_items(iced, rawr);
    bind_projections(iced);
}

impl marker::text::Style {
    fn native(self) -> fn(&iced::Theme) -> iced::widget::text::Style {
        match self {
            Self::Default => iced::widget::text::default,
            Self::Base => iced::widget::text::base,
            Self::Primary => iced::widget::text::primary,
            Self::Secondary => iced::widget::text::secondary,
            Self::Success => iced::widget::text::success,
            Self::Warning => iced::widget::text::warning,
            Self::Danger => iced::widget::text::danger,
        }
    }
}

impl marker::container::Style {
    fn native(self) -> fn(&iced::Theme) -> iced::widget::container::Style {
        match self {
            Self::Transparent => iced::widget::container::transparent,
            Self::RoundedBox => iced::widget::container::rounded_box,
            Self::BorderedBox => iced::widget::container::bordered_box,
            Self::Dark => iced::widget::container::dark,
            Self::Primary => iced::widget::container::primary,
            Self::Secondary => iced::widget::container::secondary,
            Self::Success => iced::widget::container::success,
            Self::Warning => iced::widget::container::warning,
            Self::Danger => iced::widget::container::danger,
        }
    }
}

impl widget::Descriptor for marker::Text {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        let mut text = iced::widget::text(self.content.clone())
            .width(self.width)
            .align_x(self.align_x);
        if let Some(height) = self.height {
            text = text.height(height);
        }
        if let Some(align_y) = self.align_y {
            text = text.align_y(align_y);
        }
        match text_style(&self.style, context) {
            Ok(Some(style)) => text = text.style(style),
            Ok(None) => {}
            Err(diagnostic) => return diagnostic,
        }
        if let Some(color) = self.color {
            text = text.color(color);
        }
        if let Some(font) = self.font {
            text = text.font(font);
        }
        if let Some(line_height) = self.line_height {
            text = text.line_height(line_height);
        }
        match self.size {
            Some(size) => text.size(size).boxed(),
            None => text.boxed(),
        }
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl widget::Descriptor for marker::TextInput {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        let mut input = iced::widget::text_input(self.placeholder.clone(), self.value.clone())
            .padding(self.padding)
            .secure(self.secure);
        if let Some(size) = self.size {
            input = input.size(size);
        }
        if let Some(id) = &self.id {
            input = input.id(id.clone());
        }
        if let Some(width) = self.width {
            input = input.width(width);
        }
        if let Some(mapper) = &self.on_input {
            let mapper = context.message_mapper(mapper.clone());
            input = input.on_input(move |value| mapper.map(Dynamic::from(value)));
        }
        if let Some(payload) = &self.on_submit {
            input = input.on_submit(context.message(payload.clone()));
        }
        match style::lower(&self.style, context, &style::TEXT_INPUT, "text_input") {
            Ok(Some(style)) => input = input.style(style),
            Ok(None) => {}
            Err(diagnostic) => return diagnostic,
        }
        input.boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl widget::Descriptor for marker::Checkbox {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        let mut checkbox = iced::widget::checkbox(self.checked);
        if let Some(label) = &self.label {
            checkbox = checkbox.label(label.clone());
        }
        if let Some(mapper) = &self.on_toggle {
            let mapper = context.message_mapper(mapper.clone());
            checkbox = checkbox.on_toggle(move |checked| mapper.map(Dynamic::from(checked)));
        }
        if let Some(size) = self.size {
            checkbox = checkbox.size(size);
        }
        if let Some(text_size) = self.text_size {
            checkbox = checkbox.text_size(text_size);
        }
        if let Some(width) = self.width {
            checkbox = checkbox.width(width);
        }
        if let Some(spacing) = self.spacing {
            checkbox = checkbox.spacing(spacing);
        }
        if let Some(icon) = &self.icon {
            checkbox = checkbox.icon(icon.clone());
        }
        match style::lower(
            &self.style,
            context,
            &style::checkbox_statuses(),
            "checkbox",
        ) {
            Ok(Some(style)) => checkbox = checkbox.style(style),
            Ok(None) => {}
            Err(diagnostic) => return diagnostic,
        }
        checkbox.boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl widget::Descriptor for marker::Toggler {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        let mut toggler = iced::widget::toggler(self.toggled);
        if let Some(label) = &self.label {
            toggler = toggler.label(label.clone());
        }
        if let Some(size) = self.size {
            toggler = toggler.size(size);
        }
        if let Some(width) = self.width {
            toggler = toggler.width(width);
        }
        if let Some(spacing) = self.spacing {
            toggler = toggler.spacing(spacing);
        }
        if let Some(mapper) = &self.on_toggle {
            let mapper = context.message_mapper(mapper.clone());
            toggler = toggler.on_toggle(move |toggled| mapper.map(Dynamic::from(toggled)));
        }
        match style::lower(&self.style, context, &style::toggler_statuses(), "toggler") {
            Ok(Some(style)) => toggler = toggler.style(style),
            Ok(None) => {}
            Err(diagnostic) => return diagnostic,
        }
        toggler.boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl widget::Descriptor for marker::Slider {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        let style = match style::lower(&self.style, context, &style::SLIDER, "slider") {
            Ok(style) => style,
            Err(diagnostic) => return diagnostic,
        };
        let mapper = context.message_mapper(self.on_change.clone());
        // iced's slider over this slider's number: `i64`, or `f64`. An
        // angle's slider moves over its `f64` and hands back the angle.
        let number = self.number;
        macro_rules! native {
            ($slider:path, $number:ty, $along:ident, $across:ident) => {{
                let range = (*self.range.start() as $number)..=(*self.range.end() as $number);
                let on_change = move |value: $number| {
                    mapper.map(match number {
                        marker::SliderNumber::Degrees => Dynamic::from(iced::Degrees(value as f32)),
                        marker::SliderNumber::Radians => Dynamic::from(iced::Radians(value as f32)),
                        _ => Dynamic::from(value),
                    })
                };
                let mut slider = $slider(range, self.value as $number, on_change).step(self.step as $number);
                if let Some(default) = self.default {
                    slider = slider.default(default as $number);
                }
                if let Some(shift_step) = self.shift_step {
                    slider = slider.shift_step(shift_step as $number);
                }
                // Along the rail a `Length`, across it pixels.
                if let Some(along) = self.$along {
                    slider = slider.$along(along);
                }
                if let Some(iced::Length::Fixed(across)) = self.$across {
                    slider = slider.$across(across);
                }
                if let Some(style) = style {
                    slider = slider.style(style);
                }
                slider.boxed()
            }};
        }
        match (self.vertical, number != marker::SliderNumber::Integer) {
            (false, false) => native!(iced::widget::slider, rawr::INT, width, height),
            (false, true) => native!(iced::widget::slider, f64, width, height),
            (true, false) => native!(iced::widget::vertical_slider, rawr::INT, height, width),
            (true, true) => native!(iced::widget::vertical_slider, f64, height, width),
        }
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl widget::Descriptor for marker::ProgressBar {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        let mut bar = iced::widget::progress_bar(self.range.clone(), self.value).vertical(self.vertical);
        if let Some(length) = self.length {
            bar = bar.length(length);
        }
        if let Some(girth) = self.girth {
            bar = bar.girth(girth);
        }
        match style::progress_bar_style(&self.style, context) {
            Ok(Some(style)) => bar = bar.style(style),
            Ok(None) => {}
            Err(diagnostic) => return diagnostic,
        }
        bar.boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl widget::Descriptor for marker::Transition {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        let mut arguments = [
            Dynamic::from(marker::Animation::Float(iced::Animation::new(self.value as f32))),
            Dynamic::from(marker::Instant::now()),
        ];
        match context.invoke(&self.view, &mut arguments) {
            None => iced::widget::space().boxed(),
            Some(Ok(mut view)) => match take::<widget::Value>(&mut view, "iced::Element") {
                Ok(view) => view.lower(context),
                Err(error) => widget::diagnostic(format!("transition view failed: {error}")).lower(context),
            },
            Some(Err(error)) => {
                widget::diagnostic(format!("transition view failed: {error}")).lower(context)
            }
        }
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl widget::Descriptor for marker::Button {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        let content = match &self.content {
            marker::Content::Label(label) => iced::widget::text(label.clone()).boxed(),
            marker::Content::Element(element) => element.lower(context),
        };
        let mut button = iced::widget::button(content).padding(self.padding);
        if let Some(width) = self.width {
            button = button.width(width);
        }
        if let Some(height) = self.height {
            button = button.height(height);
        }
        match style::lower(&self.style, context, &style::BUTTON, "button") {
            Ok(Some(style)) => button = button.style(style),
            Ok(None) => {}
            Err(diagnostic) => return diagnostic,
        }
        match &self.on_press {
            Some(payload) => button.on_press(context.message(payload.clone())).boxed(),
            None => button.boxed(),
        }
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl widget::Descriptor for marker::Column {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        let children = self
            .children
            .iter()
            .map(|child| child.lower(context))
            .collect::<Vec<_>>();
        match &self.mode {
            widget::key::Mode::Positional => iced::widget::column(children)
                .spacing(self.spacing)
                .padding(self.padding)
                .width(self.width)
                .height(self.height.unwrap_or(iced::Length::Fit))
                .align_x(self.align_x)
                .boxed(),
            widget::key::Mode::Keyed(_) => super::column::Keyed::bind(
                self.mode.clone(),
                children,
                self.spacing,
                self.padding,
                self.width,
                self.height.unwrap_or(iced::Length::Fit),
                self.align_x,
            )
            .boxed(),
        }
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl widget::Descriptor for marker::Row {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        iced::widget::row(
            self.children
                .iter()
                .map(|child| child.lower(context))
                .collect::<Vec<_>>(),
        )
        .spacing(self.spacing)
        .padding(self.padding)
        .width(self.width)
        .height(self.height)
        .align_y(self.align_y)
        .boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl widget::Descriptor for marker::Wrapping {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        let row = &self.row;
        let mut wrapping = iced::widget::row(
            row.children
                .iter()
                .map(|child| child.lower(context))
                .collect::<Vec<_>>(),
        )
        .spacing(row.spacing)
        .padding(row.padding)
        .width(row.width)
        .height(row.height)
        .align_y(row.align_y)
        .wrap()
        .align_x(self.align_x);
        if let Some(spacing) = self.vertical_spacing {
            wrapping = wrapping.vertical_spacing(spacing);
        }
        wrapping.boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.row.identity.clone()
    }
}

impl widget::Descriptor for marker::Container {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        let mut container = iced::widget::container(self.child.lower(context))
            .padding(self.padding)
            .width(self.width)
            .height(self.height)
            .align_x(self.align_x)
            .align_y(self.align_y);
        if let Some(id) = &self.id {
            container = container.id(id.clone());
        }
        match container_style(&self.style, context, "container") {
            Ok(Some(style)) => container.style(style).boxed(),
            Ok(None) => container.boxed(),
            Err(diagnostic) => diagnostic,
        }
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

/// A container style, pinned or a Rawr function; a failing function is a
/// diagnostic in place of the widget.
#[allow(clippy::type_complexity)]
fn container_style(
    source: &Option<marker::container::Source>,
    context: &widget::Context<'_>,
    widget: &str,
) -> Result<
    Option<Box<dyn Fn(&iced::Theme) -> iced::widget::container::Style>>,
    iced::Element<'static, runtime::Message>,
> {
    let function = match source {
        None => return Ok(None),
        Some(marker::container::Source::Builtin(style)) => return Ok(Some(Box::new(style.native()))),
        Some(marker::container::Source::Function(function)) => function,
    };
    let style = style::cached(context, function).map(Ok).or_else(|| {
        let theme = Dynamic::from(context.theme().clone());
        context.invoke(function, &mut [theme]).map(|style| {
            style.and_then(|mut style| {
                take::<iced::widget::container::Style>(&mut style, "iced::widget::container::Style")
                    .map(|style| style::remember(context, function, style))
                    .map_err(|error| error.to_string())
            })
        })
    });
    match style {
        Some(Ok(style)) => Ok(Some(Box::new(move |_: &iced::Theme| *style))),
        Some(Err(error)) => {
            Err(widget::diagnostic(format!("{widget} style failed: {error}")).lower(context))
        }
        None => Ok(None),
    }
}

/// A text's style function: the pinned one, or a Rawr function of the
/// theme evaluated once per theme.
fn text_style(
    source: &Option<marker::text::Source>,
    context: &widget::Context<'_>,
) -> Result<
    Option<Box<dyn Fn(&iced::Theme) -> iced::widget::text::Style>>,
    iced::Element<'static, runtime::Message>,
> {
    let function = match source {
        None => return Ok(None),
        Some(marker::text::Source::Builtin(style)) => return Ok(Some(Box::new(style.native()))),
        Some(marker::text::Source::Function(function)) => function,
    };
    let style = style::cached(context, function).map(Ok).or_else(|| {
        let theme = Dynamic::from(context.theme().clone());
        context.invoke(function, &mut [theme]).map(|style| {
            style.and_then(|mut style| {
                take::<iced::widget::text::Style>(&mut style, "iced::widget::text::Style")
                    .map(|style| style::remember(context, function, style))
                    .map_err(|error| error.to_string())
            })
        })
    });
    match style {
        Some(Ok(style)) => Ok(Some(Box::new(move |_: &iced::Theme| *style))),
        Some(Err(error)) => {
            Err(widget::diagnostic(format!("text style failed: {error}")).lower(context))
        }
        None => Ok(None),
    }
}

impl widget::Descriptor for marker::Tooltip {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        let tip = match &self.tooltip {
            marker::Content::Label(label) => iced::widget::text(label.clone()).boxed(),
            marker::Content::Element(element) => element.lower(context),
        };
        let tooltip = iced::widget::tooltip(self.content.lower(context), tip, self.position)
        .gap(self.gap)
        .delay(self.delay)
        .snap_within_viewport(self.snap_within_viewport);
        match container_style(&self.style, context, "tooltip") {
            Ok(Some(style)) => tooltip.style(style).boxed(),
            Ok(None) => tooltip.boxed(),
            Err(diagnostic) => diagnostic,
        }
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl widget::Descriptor for marker::Scrollable {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        let mut scrollable = iced::widget::scrollable(self.child.lower(context));
        if let Some(id) = &self.id {
            scrollable = scrollable.id(id.clone());
        }
        if let Some(width) = self.width {
            scrollable = scrollable.width(width);
        }
        if let Some(height) = self.height {
            scrollable = scrollable.height(height);
        }
        if let Some(direction) = self.direction {
            scrollable = scrollable.direction(direction);
        }
        if let Some(spacing) = self.spacing {
            scrollable = scrollable.spacing(spacing);
        }
        if let Some(auto_scroll) = self.auto_scroll {
            scrollable = scrollable.auto_scroll(auto_scroll);
        }
        if let Some(mapper) = &self.on_scroll {
            let mapper = context.message_mapper(mapper.clone());
            scrollable = scrollable.on_scroll(move |scroll| mapper.map(Dynamic::from(scroll)));
        }
        match style::lower(
            &self.style,
            context,
            &style::scrollable_statuses(),
            "scrollable",
        ) {
            Ok(Some(style)) => scrollable = scrollable.style(style),
            Ok(None) => {}
            Err(diagnostic) => return diagnostic,
        }
        scrollable.boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl widget::Descriptor for marker::Logo {
    fn lower(&self, _context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        iced::widget::iced(self.size).boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl widget::Descriptor for marker::Void {
    fn lower(&self, _context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        iced::widget::void().boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl widget::Descriptor for marker::Sensor {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        let mut sensor = iced::widget::sensor(self.child.lower(context));
        if let Some(mapper) = &self.on_show {
            let mapper = context.message_mapper(mapper.clone());
            sensor = sensor.on_show(move |size| mapper.map(Dynamic::from(size)));
        }
        if let Some(mapper) = &self.on_resize {
            let mapper = context.message_mapper(mapper.clone());
            sensor = sensor.on_resize(move |size| mapper.map(Dynamic::from(size)));
        }
        if let Some(payload) = &self.on_hide {
            sensor = sensor.on_hide(context.message(payload.clone()));
        }
        sensor.boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl widget::Descriptor for marker::Space {
    fn lower(&self, _context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        iced::widget::space()
            .width(self.width)
            .height(self.height)
            .boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

fn bind_attestations(builder: &mut host::runtime::Builder) {
    builder
        .bind_type_attestation("iced::Element", |value, expected| {
            let value = value.read_lock::<widget::Value>()?;
            // A wrapping widget's forwarded call, whose Message is erased.
            if expected.argument_name(0).as_deref() == Some(super::custom::ERASED) {
                return expected.attest(value.witness().is_some());
            }
            (expected.argument_count() == 1)
                .then(|| expected.attest_witness(value.witness()))
                .flatten()
        })
        .expect("the iced facade must declare generic Element exactly once");
    bind_profile_attestations(builder);
}

impl widget::Descriptor for marker::Radio {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        // iced compares the value with the selection; the facade compared
        // them already, so iced's value is just whether it is selected.
        let mapper = context.message_mapper(self.on_click.clone());
        let value = self.value.clone();
        let mut radio = iced::widget::radio(
            self.label.clone(),
            true,
            self.selected.then_some(true),
            move |_| mapper.map(value),
        );
        if let Some(size) = self.size {
            radio = radio.size(size);
        }
        if let Some(spacing) = self.spacing {
            radio = radio.spacing(spacing);
        }
        if let Some(size) = self.text_size {
            radio = radio.text_size(size);
        }
        if let Some(width) = self.width {
            radio = radio.width(width);
        }
        match style::lower(&self.style, context, &style::RADIO, "radio") {
            Ok(Some(style)) => radio = radio.style(style),
            Ok(None) => {}
            Err(diagnostic) => return diagnostic,
        }
        radio.boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl widget::Descriptor for marker::PickList {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        // iced picks among indexes into the options, labelled by the
        // script's `to_string`; the selection is the option of its label.
        let label = |option: &Dynamic| match context.invoke(&self.to_string, &mut [option.clone()])
        {
            None => Ok(String::new()),
            Some(Ok(label)) => exact_string(&label, "pick_list to_string").map_err(|error| error.to_string()),
            Some(Err(error)) => Err(error),
        };
        let labels = match self.options.iter().map(label).collect::<Result<Vec<_>, _>>() {
            Ok(labels) => labels,
            Err(error) => {
                return widget::diagnostic(format!("pick_list to_string failed: {error}"))
                    .lower(context)
            }
        };
        let selected = match self.selected.as_ref().map(label).transpose() {
            Ok(selected) => {
                selected.and_then(|selected| labels.iter().position(|label| *label == selected))
            }
            Err(error) => {
                return widget::diagnostic(format!("pick_list to_string failed: {error}"))
                    .lower(context)
            }
        };
        let indexes = (0..labels.len()).collect::<Vec<_>>();
        let mut pick_list =
            iced::widget::pick_list(selected, indexes, move |index: &usize| labels[*index].clone());
        if let Some(mapper) = &self.on_select {
            let mapper = context.message_mapper(mapper.clone());
            let options = self.options.clone();
            pick_list = pick_list.on_select(move |index| mapper.map(options[index].clone()));
        }
        if let Some(placeholder) = &self.placeholder {
            pick_list = pick_list.placeholder(placeholder.clone());
        }
        if let Some(width) = self.width {
            pick_list = pick_list.width(width);
        }
        if let Some(padding) = self.padding {
            pick_list = pick_list.padding(padding);
        }
        if let Some(size) = self.text_size {
            pick_list = pick_list.text_size(size);
        }
        pick_list.boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

/// One option of a combo box: its index among the script's options, shown
/// by the script's label.
#[derive(Clone, Debug)]
struct Choice {
    index: usize,
    label: std::sync::Arc<str>,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.label)
    }
}

thread_local! {
    /// The iced state of each `combo_box::State` drawn on this thread, by
    /// id. iced's `ComboBox` borrows it for the element's whole life, so
    /// each is allocated once and kept (a few labels per `State::new`).
    static COMBO_BOX_STATES: std::cell::RefCell<
        std::collections::HashMap<u64, &'static iced::widget::combo_box::State<Choice>>,
    > = std::cell::RefCell::default();
}

/// The iced state of `state`, made from the script's labels the first
/// time it is drawn.
fn combo_box_state(
    state: &marker::ComboBoxState,
    context: &widget::Context<'_>,
) -> Result<&'static iced::widget::combo_box::State<Choice>, String> {
    if let Some(existing) = COMBO_BOX_STATES.with_borrow(|states| states.get(&state.id).copied()) {
        return Ok(existing);
    }
    let choices = state
        .options
        .iter()
        .enumerate()
        .map(|(index, option)| {
            let label = match context.invoke(&state.to_string, &mut [option.clone()]) {
                None => String::new(),
                Some(Ok(label)) => exact_string(&label, "combo_box to_string")
                    .map_err(|error| error.to_string())?,
                Some(Err(error)) => return Err(error),
            };
            Ok(Choice {
                index,
                label: label.into(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let created: &'static _ = Box::leak(Box::new(iced::widget::combo_box::State::new(choices)));
    COMBO_BOX_STATES.with_borrow_mut(|states| states.insert(state.id, created));
    Ok(created)
}

impl widget::Descriptor for marker::ComboBox {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        let state = match combo_box_state(&self.state, context) {
            Ok(state) => state,
            Err(error) => {
                return widget::diagnostic(format!("combo_box to_string failed: {error}"))
                    .lower(context)
            }
        };
        let selection = self.selection.as_ref().and_then(|selection| {
            let index = self
                .state
                .options
                .iter()
                .position(|option| host::value::structural_equal(option, selection))?;
            state.options().get(index).cloned()
        });
        let option = |mapper: &rawr::item::Value| {
            let mapper = context.message_mapper(mapper.clone());
            let options = self.state.options.clone();
            move |choice: Choice| mapper.map(options[choice.index].clone())
        };
        let mut combo_box = iced::widget::combo_box(
            state,
            self.placeholder.clone(),
            selection.as_ref(),
            option(&self.on_selected),
        );
        if let Some(mapper) = &self.on_option_hovered {
            combo_box = combo_box.on_option_hovered(option(mapper));
        }
        if let Some(payload) = &self.on_open {
            combo_box = combo_box.on_open(context.message(payload.clone()));
        }
        if let Some(payload) = &self.on_close {
            combo_box = combo_box.on_close(context.message(payload.clone()));
        }
        if let Some(width) = self.width {
            combo_box = combo_box.width(width);
        }
        if let Some(size) = self.size {
            combo_box = combo_box.size(size);
        }
        combo_box.boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

fn bind_profile_attestations(builder: &mut host::runtime::Builder) {
    builder
        .bind_type_attestation("iced::widget::TextInput", |value, expected| {
            let value = value.read_lock::<marker::TextInput>()?;
            (expected.argument_count() == 1
                && value
                    .on_submit
                    .as_ref()
                    .is_none_or(|payload| expected.argument_matches(0, payload)))
            .then(|| expected.attest_witness(Some(&value.witness)))
            .flatten()
        })
        .expect("the iced facade must declare generic widget::TextInput exactly once");
    builder
        .bind_type_attestation("iced::widget::Checkbox", |value, expected| {
            let value = value.read_lock::<marker::Checkbox>()?;
            (expected.argument_count() == 1)
                .then(|| expected.attest_witness(Some(&value.witness)))
                .flatten()
        })
        .expect("the iced facade must declare generic widget::Checkbox exactly once");
    builder
        .bind_type_attestation("iced::widget::Toggler", |value, expected| {
            let value = value.read_lock::<marker::Toggler>()?;
            (expected.argument_count() == 1)
                .then(|| expected.attest_witness(Some(&value.witness)))
                .flatten()
        })
        .expect("the iced facade must declare generic widget::Toggler exactly once");
    builder
        .bind_type_attestation("iced::widget::Slider", |value, expected| {
            let value = value.read_lock::<marker::Slider>()?;
            (expected.argument_count() == 2)
                .then(|| expected.attest_witness(Some(&value.witness)))
                .flatten()
        })
        .expect("the iced facade must declare specialized widget::Slider exactly once");
    builder
        .bind_type_attestation("iced::widget::Tooltip", |value, expected| {
            let value = value.read_lock::<marker::Tooltip>()?;
            (expected.argument_count() == 1)
                .then(|| expected.attest_witness(Some(&value.witness)))
                .flatten()
        })
        .expect("the iced facade must declare generic widget::Tooltip exactly once");
    builder
        .bind_type_attestation("iced::widget::Sensor", |value, expected| {
            let value = value.read_lock::<marker::Sensor>()?;
            (expected.argument_count() == 1)
                .then(|| expected.attest_witness(Some(&value.witness)))
                .flatten()
        })
        .expect("the iced facade must declare generic widget::Sensor exactly once");
    builder
        .bind_type_attestation("iced::widget::Transition", |value, expected| {
            let value = value.read_lock::<marker::Transition>()?;
            (expected.argument_count() == 1)
                .then(|| expected.attest_witness(Some(&value.witness)))
                .flatten()
        })
        .expect("the iced facade must declare generic widget::Transition exactly once");
    builder
        .bind_type_attestation("iced::widget::Radio", |value, expected| {
            let value = value.read_lock::<marker::Radio>()?;
            (expected.argument_count() == 1)
                .then(|| expected.attest_witness(Some(&value.witness)))
                .flatten()
        })
        .expect("the iced facade must declare generic widget::Radio exactly once");
    builder
        .bind_type_attestation("iced::widget::PickList", |value, expected| {
            let value = value.read_lock::<marker::PickList>()?;
            (expected.argument_count() == 2)
                .then(|| expected.attest_witness(Some(&value.witness)))
                .flatten()
        })
        .expect("the iced facade must declare generic widget::PickList exactly once");
    builder
        .bind_type_attestation("iced::widget::ComboBox", |value, expected| {
            let value = value.read_lock::<marker::ComboBox>()?;
            (expected.argument_count() == 2)
                .then(|| expected.attest_witness(Some(&value.witness)))
                .flatten()
        })
        .expect("the iced facade must declare generic widget::ComboBox exactly once");
    builder
        .bind_type_attestation("iced::widget::combo_box::State", |value, expected| {
            let value = value.read_lock::<marker::ComboBoxState>()?;
            (expected.argument_count() == 1)
                .then(|| expected.attest_witness(Some(&value.witness)))
                .flatten()
        })
        .expect("the iced facade must declare generic widget::combo_box::State exactly once");
    builder
        .bind_type_attestation("iced::widget::Button", |value, expected| {
            let button = value.read_lock::<marker::Button>()?;
            if expected.argument_count() != 1
                || !button
                    .on_press
                    .as_ref()
                    .is_none_or(|payload| expected.argument_matches(0, payload))
            {
                return None;
            }
            expected.attest_witness(Some(&button.witness))
        })
        .expect("the iced facade must declare generic widget::Button exactly once");
    builder
        .bind_type_attestation("iced::widget::Column", |value, expected| {
            let value = value.read_lock::<marker::Column>()?;
            if expected.argument_count() != 1
                || !value
                    .children
                    .iter()
                    .all(|child| expected.arguments_match_witness(child.witness()))
            {
                return None;
            }
            expected.attest_witness(Some(&value.witness))
        })
        .expect("the iced facade must declare generic widget::Column exactly once");
    builder
        .bind_type_attestation("iced::widget::Row", |value, expected| {
            let value = value.read_lock::<marker::Row>()?;
            if expected.argument_count() != 1
                || !value
                    .children
                    .iter()
                    .all(|child| expected.arguments_match_witness(child.witness()))
            {
                return None;
            }
            expected.attest_witness(Some(&value.witness))
        })
        .expect("the iced facade must declare generic widget::Row exactly once");
    builder
        .bind_type_attestation("iced::widget::row::Wrapping", |value, expected| {
            let value = value.read_lock::<marker::Wrapping>()?;
            if expected.argument_count() != 1
                || !value
                    .row
                    .children
                    .iter()
                    .all(|child| expected.arguments_match_witness(child.witness()))
            {
                return None;
            }
            expected.attest_witness(Some(&value.row.witness))
        })
        .expect("the iced facade must declare generic widget::row::Wrapping exactly once");
    builder
        .bind_type_attestation("iced::widget::Container", |value, expected| {
            let value = value.read_lock::<marker::Container>()?;
            if expected.argument_count() != 1
                || !expected.arguments_match_witness(value.child.witness())
            {
                return None;
            }
            expected.attest_witness(Some(&value.witness))
        })
        .expect("the iced facade must declare generic widget::Container exactly once");
    builder
        .bind_type_attestation("iced::widget::Scrollable", |value, expected| {
            let value = value.read_lock::<marker::Scrollable>()?;
            if expected.argument_count() != 1
                || !expected.arguments_match_witness(value.child.witness())
            {
                return None;
            }
            expected.attest_witness(Some(&value.witness))
        })
        .expect("the iced facade must declare generic widget::Scrollable exactly once");
}

fn bind_items(iced: &mut host::runtime::Builder, rawr: &mut host::runtime::Builder) {
    let builder = iced;
    super::helpers::bind(builder);
    bind_progress(builder);
    bind_tooltip(builder);
    bind_sensor(builder);
    builder
        .bind("iced::Center", |_| {
            Ok(Dynamic::from(marker::Alignment(iced::Alignment::Center)))
        })
        .expect("the iced facade must retain the exact Center value item");
    for (name, alignment) in [
        ("Start", iced::Alignment::Start),
        ("Center", iced::Alignment::Center),
        ("End", iced::Alignment::End),
    ] {
        builder
            .bind(format!("iced::Alignment::{name}"), move |_| {
                Ok(Dynamic::from(marker::Alignment(alignment)))
            })
            .expect("the iced facade must retain every Alignment value");
    }
    for (path, style) in [
        ("iced::widget::text::primary", marker::text::Style::Primary),
        (
            "iced::widget::text::secondary",
            marker::text::Style::Secondary,
        ),
        ("iced::widget::text::success", marker::text::Style::Success),
        ("iced::widget::text::warning", marker::text::Style::Warning),
        ("iced::widget::text::danger", marker::text::Style::Danger),
        ("iced::widget::text::base", marker::text::Style::Base),
        ("iced::widget::text::default", marker::text::Style::Default),
    ] {
        builder
            .bind(path, move |_| Ok(Dynamic::from(style)))
            .expect("the iced facade must retain every exact built-in text style item");
    }
    for (path, style) in [
        (
            "iced::widget::container::transparent",
            marker::container::Style::Transparent,
        ),
        (
            "iced::widget::container::rounded_box",
            marker::container::Style::RoundedBox,
        ),
        (
            "iced::widget::container::bordered_box",
            marker::container::Style::BorderedBox,
        ),
        (
            "iced::widget::container::dark",
            marker::container::Style::Dark,
        ),
        (
            "iced::widget::container::primary",
            marker::container::Style::Primary,
        ),
        (
            "iced::widget::container::secondary",
            marker::container::Style::Secondary,
        ),
        (
            "iced::widget::container::success",
            marker::container::Style::Success,
        ),
        (
            "iced::widget::container::warning",
            marker::container::Style::Warning,
        ),
        (
            "iced::widget::container::danger",
            marker::container::Style::Danger,
        ),
    ] {
        builder
            .bind(path, move |_| Ok(Dynamic::from(style)))
            .expect("the iced facade must retain every exact built-in container style item");
    }
    fn new_text(
        call: &host::runtime::Call<'_>,
        args: &mut [&mut Dynamic],
    ) -> Result<Dynamic, Box<rawr::EvalAltResult>> {
        let content = display_content(args.first().ok_or_else(|| arity("text", 1, 0))?)?;
        Ok(Dynamic::from(marker::Text {
            witness: call.output_witness().map_err(witness_error)?,
            identity: widget::Identity::unique(),
            content,
            size: None,
            width: iced::Length::Fit,
            height: None,
            align_x: iced::advanced::text::Alignment::Default,
            align_y: None,
            style: None,
            color: None,
            font: None,
            line_height: None,
        }))
    }
    builder
        .bind_with_context("iced::widget::text", |call, args| new_text(call, args))
        .expect("the iced facade must retain the exact text item");
    builder
        .bind_macro_with_context("iced::widget::text", |call, args| new_text(call, args))
        .expect("the iced facade must retain the exact text macro");
    builder
        .bind_with_context("iced::widget::button", |call, args| {
            let content = args.first_mut().ok_or_else(|| arity("button", 1, 0))?;
            let content = if content.is_string() {
                marker::Content::Label(string_content(content)?)
            } else {
                marker::Content::Element(take::<widget::Value>(content, "iced::Element")?)
            };
            Ok(Dynamic::from(marker::Button {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                content,
                on_press: None,
                padding: iced::widget::button::DEFAULT_PADDING,
                width: None,
                height: None,
                style: None,
            }))
        })
        .expect("the iced facade must retain the exact button item");
    builder
        .bind_macro_with_context("iced::widget::column", |call, args| {
            let children = args
                .iter_mut()
                .map(|value| take::<widget::Value>(value, "iced::Element"))
                .collect::<Result<Vec<_>, _>>()?;
            new_column(call, children)
        })
        .expect("the iced facade must retain the exact column macro");
    builder
        .bind_macro_with_context("iced::widget::row", |call, args| {
            let children = args
                .iter_mut()
                .map(|value| take::<widget::Value>(value, "iced::Element"))
                .collect::<Result<Vec<_>, _>>()?;
            new_row(call, children)
        })
        .expect("the iced facade must retain the exact row macro");
    for path in [
        "iced::widget::column",
        "iced::widget::Column::with_children",
        "iced::widget::Column::from_vec",
    ] {
        builder
            .bind_with_context(path, |call, args| {
                if args.len() != 1 {
                    return Err(arity("column", 1, args.len()));
                }
                new_column(call, take_children(args[0])?)
            })
            .expect("the iced facade must retain the exact column functions");
    }
    builder
        .bind_with_context("iced::widget::keyed_column", |call, args| {
            if args.len() != 1 {
                return Err(arity("keyed_column", 1, args.len()));
            }
            new_column(call, take_keyed_children(args[0])?)
        })
        .expect("the iced facade must retain the exact keyed_column function");
    for path in [
        "iced::widget::row",
        "iced::widget::Row::with_children",
        "iced::widget::Row::from_vec",
    ] {
        builder
            .bind_with_context(path, |call, args| {
                if args.len() != 1 {
                    return Err(arity("row", 1, args.len()));
                }
                new_row(call, take_children(args[0])?)
            })
            .expect("the iced facade must retain the exact row functions");
    }
    builder
        .bind_with_context("iced::widget::Column::new", |call, _| new_column(call, Vec::new()))
        .expect("the iced facade must retain the exact Column::new item");
    builder
        .bind_with_context("iced::widget::Row::new", |call, _| new_row(call, Vec::new()))
        .expect("the iced facade must retain the exact Row::new item");
    builder
        .bind_with_context("iced::widget::container", |call, args| {
            if args.len() != 1 {
                return Err(arity("container", 1, args.len()));
            }
            Ok(Dynamic::from(marker::Container {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                child: take::<widget::Value>(args[0], "iced::Element")?,
                padding: iced::Padding::ZERO,
                width: iced::Length::Fit,
                height: iced::Length::Fit,
                align_x: iced::Alignment::Start,
                align_y: iced::Alignment::Start,
                style: None,
                id: None,
            }))
        })
        .expect("the iced facade must retain the exact container item");
    for (name, x, y) in [
        ("iced::widget::center", true, true),
        ("iced::widget::center_x", true, false),
        ("iced::widget::center_y", false, true),
    ] {
        builder
            .bind_with_context(name, move |call, args| {
                if args.len() != 1 {
                    return Err(arity(name.rsplit("::").next().unwrap_or(name), 1, args.len()));
                }
                let (width, align_x) = if x {
                    (iced::Length::Fill, iced::Alignment::Center)
                } else {
                    (iced::Length::Fit, iced::Alignment::Start)
                };
                let (height, align_y) = if y {
                    (iced::Length::Fill, iced::Alignment::Center)
                } else {
                    (iced::Length::Fit, iced::Alignment::Start)
                };
                Ok(Dynamic::from(marker::Container {
                    witness: call.output_witness().map_err(witness_error)?,
                    identity: widget::Identity::unique(),
                    child: take::<widget::Value>(args[0], "iced::Element")?,
                    padding: iced::Padding::ZERO,
                    width,
                    height,
                    align_x,
                    align_y,
                    style: None,
                    id: None,
                }))
            })
            .expect("the iced facade must retain the exact center items");
    }
    builder
        .bind_with_context("iced::widget::scrollable", |call, args| {
            if args.len() != 1 {
                return Err(arity("scrollable", 1, args.len()));
            }
            Ok(Dynamic::from(marker::Scrollable {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                child: take::<widget::Value>(args[0], "iced::Element")?,
                width: None,
                height: None,
                style: None,
                id: None,
                direction: None,
                spacing: None,
                auto_scroll: None,
                on_scroll: None,
            }))
        })
        .expect("the iced facade must retain the exact scrollable item");
    builder
        .bind_with_context("iced::widget::space", |call, args| {
            if !args.is_empty() {
                return Err(arity("space", 0, args.len()));
            }
            Ok(Dynamic::from(marker::Space {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                width: iced::Length::Shrink,
                height: iced::Length::Shrink,
            }))
        })
        .expect("the iced facade must retain the exact space item");
    // iced master's `space::horizontal()` and `space::vertical()`: a space
    // that fills one axis.
    for (name, width, height) in [
        ("horizontal", iced::Length::Fill, iced::Length::Shrink),
        ("vertical", iced::Length::Shrink, iced::Length::Fill),
    ] {
        builder
            .bind_with_context(format!("iced::widget::space::{name}"), move |call, args| {
                if !args.is_empty() {
                    return Err(arity("space", 0, args.len()));
                }
                Ok(Dynamic::from(marker::Space {
                    witness: call.output_witness().map_err(witness_error)?,
                    identity: widget::Identity::unique(),
                    width,
                    height,
                }))
            })
            .expect("the iced facade must retain space::horizontal and space::vertical");
    }
    builder
        .bind_with_context("iced::widget::text_input", |call, args| {
            if args.len() != 2 {
                return Err(arity("text_input", 2, args.len()));
            }
            Ok(Dynamic::from(marker::TextInput {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                placeholder: exact_string(args[0], "text_input placeholder")?,
                value: exact_string(args[1], "text_input value")?,
                on_input: None,
                on_submit: None,
                padding: iced::widget::text_input::DEFAULT_PADDING,
                size: None,
                width: None,
                secure: false,
                style: None,
                id: None,
            }))
        })
        .expect("the iced facade must retain the exact text_input item");
    builder
        .bind_with_context("iced::widget::checkbox", |call, args| {
            if args.len() != 1 {
                return Err(arity("checkbox", 1, args.len()));
            }
            Ok(Dynamic::from(marker::Checkbox {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                checked: exact_bool(args[0], "checkbox")?,
                label: None,
                on_toggle: None,
                style: None,
                size: None,
                text_size: None,
                width: None,
                spacing: None,
                icon: None,
            }))
        })
        .expect("the iced facade must retain the exact checkbox item");
    builder
        .bind_with_context("iced::widget::toggler", |call, args| {
            if args.len() != 1 {
                return Err(arity("toggler", 1, args.len()));
            }
            Ok(Dynamic::from(marker::Toggler {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                toggled: exact_bool(args[0], "toggler")?,
                label: None,
                size: None,
                on_toggle: None,
                style: None,
                width: None,
                spacing: None,
            }))
        })
        .expect("the iced facade must retain the exact toggler item");
    for (path, vertical) in [
        ("iced::widget::slider", false),
        ("iced::widget::vertical_slider", true),
    ] {
        builder
            .bind_with_context(path, move |call, args| {
                if args.len() != 3 {
                    return Err(arity("slider", 3, args.len()));
                }
                let (number, range, value) = if args[0].is::<marker::AngleRange>() {
                    angle_slider(args)?
                } else {
                    let float = args[0].is::<std::ops::RangeInclusive<rawr::FLOAT>>();
                    (
                        if float { marker::SliderNumber::Float } else { marker::SliderNumber::Integer },
                        number_range(args[0], "slider")?,
                        number(args[1], "slider value")?,
                    )
                };
                Ok(Dynamic::from(marker::Slider {
                    witness: call.output_witness().map_err(witness_error)?,
                    identity: widget::Identity::unique(),
                    range,
                    value,
                    number,
                    on_change: take::<rawr::item::Value>(args[2], "symbolic message mapper")?,
                    step: 1.0,
                    default: None,
                    shift_step: None,
                    vertical,
                    style: None,
                    width: None,
                    height: None,
                }))
            })
            .expect("the iced facade must retain the exact slider items");
    }
    builder
        .bind_with_context("iced::widget::iced", |call, args| {
            if args.len() != 1 {
                return Err(arity("iced", 1, args.len()));
            }
            Ok(Dynamic::from(marker::Logo {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                size: pixels(args[0], "iced")?,
            }))
        })
        .expect("the iced facade must retain the exact iced logo items");
    builder
        .bind_with_context("iced::widget::pick_list", |call, args| {
            if args.len() != 3 {
                return Err(arity("pick_list", 3, args.len()));
            }
            let selected = optional_message(args[0], "pick_list")?;
            let options = take::<rawr::Array>(args[1], "&[T]")?;
            Ok(Dynamic::from(marker::PickList {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                selected,
                options: options.into_iter().collect(),
                to_string: take::<rawr::item::Value>(args[2], "symbolic `to_string`")?,
                on_select: None,
                placeholder: None,
                width: None,
                padding: None,
                text_size: None,
            }))
        })
        .expect("the iced facade must retain the exact pick_list item");
    builder
        .bind_with_context("iced::widget::radio", |call, args| {
            if args.len() != 4 {
                return Err(arity("radio", 4, args.len()));
            }
            let value = args[1].clone();
            let selected = optional_message(args[2], "radio")?
                .is_some_and(|selected| rawr::host::value::structural_equal(&value, &selected));
            Ok(Dynamic::from(marker::Radio {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                label: exact_string(args[0], "radio label")?,
                value,
                selected,
                on_click: take::<rawr::item::Value>(args[3], "symbolic message mapper")?,
                size: None,
                spacing: None,
                text_size: None,
                width: None,
                style: None,
            }))
        })
        .expect("the iced facade must retain the exact radio item");
    for (path, set) in [
        (
            "iced::widget::Radio::size",
            (|radio, value| {
                radio.size = Some(pixels(value, "Radio::size")?);
                Ok(())
            }) as fn(&mut marker::Radio, &mut Dynamic) -> Result<(), Box<EvalAltResult>>,
        ),
        ("iced::widget::Radio::spacing", |radio, value| {
            radio.spacing = Some(pixels(value, "Radio::spacing")?);
            Ok(())
        }),
        ("iced::widget::Radio::text_size", |radio, value| {
            radio.text_size = Some(pixels(value, "Radio::text_size")?);
            Ok(())
        }),
        ("iced::widget::Radio::width", |radio, value| {
            radio.width = Some(length(value)?);
            Ok(())
        }),
    ] {
        builder
            .bind(path, move |args| {
                if args.len() != 2 {
                    return Err(arity(path, 2, args.len()));
                }
                let mut radio = take::<marker::Radio>(args[0], "iced::widget::Radio")?;
                set(&mut radio, args[1])?;
                Ok(Dynamic::from(radio))
            })
            .expect("the iced facade must retain every Radio builder");
    }
    for (path, set) in [
        (
            "iced::widget::PickList::on_select",
            (|pick_list, value| {
                pick_list.on_select = Some(take::<rawr::item::Value>(
                    value,
                    "symbolic message mapper",
                )?);
                Ok(())
            })
                as fn(&mut marker::PickList, &mut Dynamic) -> Result<(), Box<EvalAltResult>>,
        ),
        ("iced::widget::PickList::placeholder", |pick_list, value| {
            pick_list.placeholder = Some(exact_string(value, "PickList::placeholder")?);
            Ok(())
        }),
        ("iced::widget::PickList::width", |pick_list, value| {
            pick_list.width = Some(length(value)?);
            Ok(())
        }),
        ("iced::widget::PickList::padding", |pick_list, value| {
            pick_list.padding = Some(padding(value)?);
            Ok(())
        }),
        ("iced::widget::PickList::text_size", |pick_list, value| {
            pick_list.text_size = Some(pixels(value, "PickList::text_size")?);
            Ok(())
        }),
    ] {
        builder
            .bind(path, move |args| {
                if args.len() != 2 {
                    return Err(arity(path, 2, args.len()));
                }
                let mut pick_list = take::<marker::PickList>(args[0], "iced::widget::PickList")?;
                set(&mut pick_list, args[1])?;
                Ok(Dynamic::from(pick_list))
            })
            .expect("the iced facade must retain every PickList builder");
    }
    bind_combo_box(builder);
    builder
        .bind("iced::widget::Button::on_press", |args| {
            if args.len() != 2 {
                return Err(arity("Button::on_press", 2, args.len()));
            }
            let mut button = take::<marker::Button>(args[0], "iced::widget::Button")?;
            button.on_press = Some(args[1].take());
            Ok(Dynamic::from(button))
        })
        .expect("the iced facade must retain the exact Button::on_press item");
    builder
        .bind("iced::widget::Button::on_press_maybe", |args| {
            if args.len() != 2 {
                return Err(arity("Button::on_press_maybe", 2, args.len()));
            }
            let mut button = take::<marker::Button>(args[0], "iced::widget::Button")?;
            button.on_press = optional_message(args[1], "Button::on_press_maybe")?;
            Ok(Dynamic::from(button))
        })
        .expect("the iced facade must retain the exact Button::on_press_maybe item");
    builder
        .bind("iced::widget::Button::padding", |args| {
            if args.len() != 2 {
                return Err(arity("Button::padding", 2, args.len()));
            }
            let mut button = take::<marker::Button>(args[0], "iced::widget::Button")?;
            button.padding = padding(args[1])?;
            Ok(Dynamic::from(button))
        })
        .expect("the iced facade must retain every exact Button::padding overload");
    for (path, set) in [
        (
            "iced::widget::Button::width",
            (|button, value| button.width = Some(value)) as fn(&mut marker::Button, iced::Length),
        ),
        ("iced::widget::Button::height", |button, value| {
            button.height = Some(value)
        }),
    ] {
        builder
            .bind(path, move |args| {
                if args.len() != 2 {
                    return Err(arity(path, 2, args.len()));
                }
                let mut button = take::<marker::Button>(args[0], "iced::widget::Button")?;
                set(&mut button, length(args[1])?);
                Ok(Dynamic::from(button))
            })
            .expect("the iced facade must retain every Button sizing item");
    }
    builder
        .bind("iced::widget::Text::size", |args| {
            if args.len() != 2 {
                return Err(arity("Text::size", 2, args.len()));
            }
            let mut text = take::<marker::Text>(args[0], "iced::widget::Text")?;
            text.size = Some(pixels(args[1], "Text::size")?);
            Ok(Dynamic::from(text))
        })
        .expect("the iced facade must retain the exact Text::size item");
    builder
        .bind("iced::widget::Text::width", |args| {
            if args.len() != 2 {
                return Err(arity("Text::width", 2, args.len()));
            }
            let mut text = take::<marker::Text>(args[0], "iced::widget::Text")?;
            text.width = length(args[1])?;
            Ok(Dynamic::from(text))
        })
        .expect("the iced facade must retain the exact Text::width item");
    builder
        .bind("iced::widget::Text::height", |args| {
            if args.len() != 2 {
                return Err(arity("Text::height", 2, args.len()));
            }
            let mut text = take::<marker::Text>(args[0], "iced::widget::Text")?;
            text.height = Some(length(args[1])?);
            Ok(Dynamic::from(text))
        })
        .expect("the iced facade must retain the exact Text::height item");
    builder
        .bind("iced::widget::Text::color", |args| {
            if args.len() != 2 {
                return Err(arity("Text::color", 2, args.len()));
            }
            let mut text = take::<marker::Text>(args[0], "iced::widget::Text")?;
            text.color = Some(take::<iced::Color>(args[1], "iced::Color")?);
            Ok(Dynamic::from(text))
        })
        .expect("the iced facade must retain the exact Text::color item");
    builder
        .bind("iced::widget::Text::font", |args| {
            if args.len() != 2 {
                return Err(arity("Text::font", 2, args.len()));
            }
            let mut text = take::<marker::Text>(args[0], "iced::widget::Text")?;
            text.font = Some(take::<iced::Font>(args[1], "iced::Font")?);
            Ok(Dynamic::from(text))
        })
        .expect("the iced facade must retain the exact Text::font item");
    builder
        .bind("iced::widget::Text::line_height", |args| {
            if args.len() != 2 {
                return Err(arity("Text::line_height", 2, args.len()));
            }
            let mut text = take::<marker::Text>(args[0], "iced::widget::Text")?;
            text.line_height = Some(pixels(args[1], "Text::line_height")?);
            Ok(Dynamic::from(text))
        })
        .expect("the iced facade must retain the exact Text::line_height item");
    builder
        .bind("iced::widget::Text::align_x", |args| {
            if args.len() != 2 {
                return Err(arity("Text::align_x", 2, args.len()));
            }
            let mut text = take::<marker::Text>(args[0], "iced::widget::Text")?;
            let alignment = alignment(args[1])?;
            text.align_x = iced::alignment::Horizontal::from(alignment).into();
            Ok(Dynamic::from(text))
        })
        .expect("the iced facade must retain the exact Text::align_x item");
    builder
        .bind("iced::widget::Text::align_y", |args| {
            if args.len() != 2 {
                return Err(arity("Text::align_y", 2, args.len()));
            }
            let mut text = take::<marker::Text>(args[0], "iced::widget::Text")?;
            let alignment = alignment(args[1])?;
            text.align_y = Some(alignment.into());
            Ok(Dynamic::from(text))
        })
        .expect("the iced facade must retain the exact Text::align_y item");
    builder
        .bind("iced::widget::Text::center", |args| {
            if args.len() != 1 {
                return Err(arity("Text::center", 1, args.len()));
            }
            let mut text = take::<marker::Text>(args[0], "iced::widget::Text")?;
            text.align_x = iced::alignment::Horizontal::Center.into();
            text.align_y = Some(iced::alignment::Vertical::Center);
            Ok(Dynamic::from(text))
        })
        .expect("the iced facade must retain the exact Text::center item");
    builder
        .bind("iced::widget::Text::color_maybe", |args| {
            if args.len() != 2 {
                return Err(arity("Text::color_maybe", 2, args.len()));
            }
            let mut text = take::<marker::Text>(args[0], "iced::widget::Text")?;
            text.color = super::rich::maybe::<iced::Color>(args[1], "iced::Color")?;
            Ok(Dynamic::from(text))
        })
        .expect("the iced facade must retain the exact Text::color_maybe item");
    builder
        .bind("iced::widget::Text::style", |args| {
            if args.len() != 2 {
                return Err(arity("Text::style", 2, args.len()));
            }
            let mut text = take::<marker::Text>(args[0], "iced::widget::Text")?;
            text.style = Some(if args[1].is::<marker::text::Style>() {
                marker::text::Source::Builtin(take(args[1], "text style")?)
            } else {
                marker::text::Source::Function(take(args[1], "text style function")?)
            });
            Ok(Dynamic::from(text))
        })
        .expect("the iced facade must retain the exact Text::style item");
    builder
        .bind("iced::widget::Column::spacing", |args| {
            if args.len() != 2 {
                return Err(arity("Column::spacing", 2, args.len()));
            }
            let mut column = take::<marker::Column>(args[0], "iced::widget::Column")?;
            column.spacing = spacing(args[1], "Column::spacing")?;
            Ok(Dynamic::from(column))
        })
        .expect("the iced facade must retain the exact Column::spacing item");
    builder
        .bind("iced::widget::Column::padding", |args| {
            if args.len() != 2 {
                return Err(arity("Column::padding", 2, args.len()));
            }
            let mut column = take::<marker::Column>(args[0], "iced::widget::Column")?;
            column.padding = padding(args[1])?;
            Ok(Dynamic::from(column))
        })
        .expect("the iced facade must retain the exact Column::padding item");
    builder
        .bind("iced::widget::Column::align_x", |args| {
            if args.len() != 2 {
                return Err(arity("Column::align_x", 2, args.len()));
            }
            let mut column = take::<marker::Column>(args[0], "iced::widget::Column")?;
            column.align_x = alignment(args[1])?;
            Ok(Dynamic::from(column))
        })
        .expect("the iced facade must retain the exact Column::align_x item");
    builder
        .bind("iced::widget::Column::width", |args| {
            if args.len() != 2 {
                return Err(arity("Column::width", 2, args.len()));
            }
            let mut column = take::<marker::Column>(args[0], "iced::widget::Column")?;
            column.width = length(args[1])?;
            Ok(Dynamic::from(column))
        })
        .expect("the iced facade must retain the exact Column::width item");
    builder
        .bind("iced::widget::Column::height", |args| {
            if args.len() != 2 {
                return Err(arity("Column::height", 2, args.len()));
            }
            let mut column = take::<marker::Column>(args[0], "iced::widget::Column")?;
            column.height = Some(length(args[1])?);
            Ok(Dynamic::from(column))
        })
        .expect("the iced facade must retain the exact Column::height item");
    builder
        .bind("iced::widget::Column::push", |args| {
            if args.len() != 2 {
                return Err(arity("Column::push", 2, args.len()));
            }
            let mut column = take::<marker::Column>(args[0], "iced::widget::Column")?;
            column
                .children
                .push(take::<widget::Value>(args[1], "iced::Element")?);
            column.mode = widget::key::Mode::bind(&column.children)
                .map_err(|error| runtime_error(error.to_string()))?;
            Ok(Dynamic::from(column))
        })
        .expect("the iced facade must retain the exact Column::push item");
    builder
        .bind("iced::widget::Column::extend", |args| {
            if args.len() != 2 {
                return Err(arity("Column::extend", 2, args.len()));
            }
            let mut column = take::<marker::Column>(args[0], "iced::widget::Column")?;
            column.children.extend(take_children(args[1])?);
            column.mode = widget::key::Mode::bind(&column.children)
                .map_err(|error| runtime_error(error.to_string()))?;
            Ok(Dynamic::from(column))
        })
        .expect("the iced facade must retain the exact Column::extend item");
    bind_row_builders(builder);
    bind_container_builders(builder);
    bind_space_builders(builder);
    bind_styles(builder);
    style::bind(builder);
    bind_intrinsic_control_builders(builder);
    builder
        .bind_with_context("iced::Element::map", |call, args| {
            if args.len() != 2 {
                return Err(arity("Element::map", 2, args.len()));
            }
            let child = take::<widget::Value>(args[0], "iced::Element")?;
            let mapper = take::<rawr::item::Value>(args[1], "symbolic callable")?;
            let witness = child.witness().ok_or_else(|| {
                runtime_error(
                    "native widget mapping received an unattested child value".to_string(),
                )
            })?;
            let key = child.key().cloned();
            let mut mapped = widget::Value::from_transform(
                super::mapped::Mapped::new(child.clone(), mapper),
                call,
                witness,
            )
            .map_err(witness_error)?;
            if let Some(key) = key {
                mapped = mapped.keyed(key);
            }
            Ok(Dynamic::from(mapped))
        })
        .expect("the iced facade must retain the exact Element::map item");
    builder
        .bind("iced::Element::boxed", |args| {
            if args.len() != 1 {
                return Err(arity("Element::boxed", 1, args.len()));
            }
            let element = take::<widget::Value>(args[0], "iced::Element")?;
            Ok(Dynamic::from(element))
        })
        .expect("the iced facade must retain the exact Element::boxed item");
    rawr.bind("rawr_iced::widget::key", |args| {
        if args.len() != 2 {
            return Err(arity("Element::key", 2, args.len()));
        }
        let value = take::<widget::Value>(args[0], "iced::Element")?;
        let key = if let Ok(key) = args[1].as_int() {
            widget::key::Value::integer(key)
        } else if let Ok(key) = args[1].as_immutable_string_ref() {
            widget::key::Value::string(key.clone())
        } else {
            return Err(runtime_error(format!(
                "native widget key binding received `{}`, expected `i64` or `String`",
                args[1].type_name()
            )));
        };
        Ok(Dynamic::from(value.keyed(key)))
    })
    .expect("the rawr_iced facade must retain both exact Element key overloads");
}

fn bind_projections(builder: &mut host::runtime::Builder) {
    builder
        .bind_projection_with_context("iced::widget::Tooltip", "iced::Element", |call, args| {
            project::<marker::Tooltip>(call, args, "iced::widget::Tooltip")
        })
        .expect("the iced facade must retain the exact Tooltip to Element projection");
    builder
        .bind_projection_with_context("iced::widget::ProgressBar", "iced::Element", |call, args| {
            project::<marker::ProgressBar>(call, args, "iced::widget::ProgressBar")
        })
        .expect("the iced facade must retain the exact ProgressBar to Element projection");
    builder
        .bind_projection_with_context("iced::widget::Transition", "iced::Element", |call, args| {
            project::<marker::Transition>(call, args, "iced::widget::Transition")
        })
        .expect("the iced facade must retain the exact Transition to Element projection");
    builder
        .bind_projection_with_context("iced::widget::Text", "iced::Element", |call, args| {
            project::<marker::Text>(call, args, "iced::widget::Text")
        })
        .expect("the iced facade must retain the exact Text to Element projection");
    builder
        .bind_projection_with_context("iced::widget::TextInput", "iced::Element", |call, args| {
            project::<marker::TextInput>(call, args, "iced::widget::TextInput")
        })
        .expect("the iced facade must retain the exact TextInput to Element projection");
    builder
        .bind_projection_with_context("iced::widget::Checkbox", "iced::Element", |call, args| {
            project::<marker::Checkbox>(call, args, "iced::widget::Checkbox")
        })
        .expect("the iced facade must retain the exact Checkbox to Element projection");
    builder
        .bind_projection_with_context("iced::widget::Toggler", "iced::Element", |call, args| {
            project::<marker::Toggler>(call, args, "iced::widget::Toggler")
        })
        .expect("the iced facade must retain the exact Toggler to Element projection");
    builder
        .bind_projection_with_context("iced::widget::Slider", "iced::Element", |call, args| {
            project::<marker::Slider>(call, args, "iced::widget::Slider")
        })
        .expect("the iced facade must retain the exact Slider to Element projection");
    builder
        .bind_projection_with_context("iced::widget::Logo", "iced::Element", |call, args| {
            project::<marker::Logo>(call, args, "iced::widget::Logo")
        })
        .expect("the iced facade must retain the exact Logo to Element projection");
    builder
        .bind_projection_with_context("iced::widget::Radio", "iced::Element", |call, args| {
            project::<marker::Radio>(call, args, "iced::widget::Radio")
        })
        .expect("the iced facade must retain the exact Radio to Element projection");
    builder
        .bind_projection_with_context("iced::widget::PickList", "iced::Element", |call, args| {
            project::<marker::PickList>(call, args, "iced::widget::PickList")
        })
        .expect("the iced facade must retain the exact PickList to Element projection");
    builder
        .bind_projection_with_context("iced::widget::ComboBox", "iced::Element", |call, args| {
            project::<marker::ComboBox>(call, args, "iced::widget::ComboBox")
        })
        .expect("the iced facade must retain the exact ComboBox to Element projection");
    builder
        .bind_projection_with_context("iced::widget::Button", "iced::Element", |call, args| {
            project::<marker::Button>(call, args, "iced::widget::Button")
        })
        .expect("the iced facade must retain the exact Button to Element projection");
    builder
        .bind_projection_with_context("iced::widget::Column", "iced::Element", |call, args| {
            project::<marker::Column>(call, args, "iced::widget::Column")
        })
        .expect("the iced facade must retain the exact Column to Element projection");
    builder
        .bind_projection_with_context("iced::widget::Row", "iced::Element", |call, args| {
            project::<marker::Row>(call, args, "iced::widget::Row")
        })
        .expect("the iced facade must retain the exact Row to Element projection");
    builder
        .bind_projection_with_context("iced::widget::row::Wrapping", "iced::Element", |call, args| {
            project::<marker::Wrapping>(call, args, "iced::widget::row::Wrapping")
        })
        .expect("the iced facade must retain the exact Wrapping to Element projection");
    builder
        .bind_projection_with_context("iced::widget::Container", "iced::Element", |call, args| {
            project::<marker::Container>(call, args, "iced::widget::Container")
        })
        .expect("the iced facade must retain the exact Container to Element projection");
    builder
        .bind_projection_with_context("iced::widget::Scrollable", "iced::Element", |call, args| {
            project::<marker::Scrollable>(call, args, "iced::widget::Scrollable")
        })
        .expect("the iced facade must retain the exact Scrollable to Element projection");
    builder
        .bind_projection_with_context("iced::widget::Sensor", "iced::Element", |call, args| {
            project::<marker::Sensor>(call, args, "iced::widget::Sensor")
        })
        .expect("the iced facade must retain the exact Sensor to Element projection");
    builder
        .bind_projection_with_context("iced::widget::Space", "iced::Element", |call, args| {
            project::<marker::Space>(call, args, "iced::widget::Space")
        })
        .expect("the iced facade must retain the exact Space to Element projection");
}

fn bind_row_builders(builder: &mut host::runtime::Builder) {
    builder
        .bind("iced::widget::Row::spacing", |args| {
            let mut row = take_row(args, "Row::spacing")?;
            row.spacing = spacing(args[1], "Row::spacing")?;
            Ok(Dynamic::from(row))
        })
        .expect("the iced facade must retain the exact Row::spacing item");
    builder
        .bind("iced::widget::Row::padding", |args| {
            let mut row = take_row(args, "Row::padding")?;
            row.padding = padding(args[1])?;
            Ok(Dynamic::from(row))
        })
        .expect("the iced facade must retain the exact Row::padding item");
    for (path, set) in [
        (
            "iced::widget::Row::width",
            set_row_width as fn(&mut marker::Row, iced::Length),
        ),
        ("iced::widget::Row::height", set_row_height),
    ] {
        builder
            .bind(path, move |args| {
                let mut row = take_row(args, path)?;
                set(&mut row, length(args[1])?);
                Ok(Dynamic::from(row))
            })
            .expect("the iced facade must retain every Row sizing item");
    }
    builder
        .bind("iced::widget::Row::push", |args| {
            let mut row = take_row(args, "Row::push")?;
            let child = take::<widget::Value>(args[1], "iced::Element")?;
            if child.key().is_some() {
                return Err(runtime_error(
                    "built-in Row requires positional children; keyed reconciliation is owned by Column"
                        .to_string(),
                ));
            }
            row.children.push(child);
            Ok(Dynamic::from(row))
        })
        .expect("the iced facade must retain the exact Row::push item");
    builder
        .bind("iced::widget::Row::extend", |args| {
            let mut row = take_row(args, "Row::extend")?;
            let children = take_children(args[1])?;
            if children.iter().any(|child| child.key().is_some()) {
                return Err(positional_row());
            }
            row.children.extend(children);
            Ok(Dynamic::from(row))
        })
        .expect("the iced facade must retain the exact Row::extend item");
    builder
        .bind("iced::widget::Row::align_y", |args| {
            let mut row = take_row(args, "Row::align_y")?;
            row.align_y = alignment(args[1])?;
            Ok(Dynamic::from(row))
        })
        .expect("the iced facade must retain the exact Row::align_y item");
    builder
        .bind_with_context("iced::widget::Row::wrap", |call, args| {
            let [row] = args else {
                return Err(arity("Row::wrap", 1, args.len()));
            };
            let mut row = take::<marker::Row>(row, "iced::widget::Row")?;
            row.witness = call.transform_witness(&row.witness).map_err(witness_error)?;
            Ok(Dynamic::from(marker::Wrapping {
                row,
                vertical_spacing: None,
                align_x: iced::Alignment::Start,
            }))
        })
        .expect("the iced facade must retain the exact Row::wrap item");
    for path in [
        "iced::widget::row::Wrapping::vertical_spacing",
        "iced::widget::row::Wrapping::align_x",
    ] {
        builder
            .bind(path, move |args| {
                if args.len() != 2 {
                    return Err(arity(path, 2, args.len()));
                }
                let mut wrapping = take::<marker::Wrapping>(args[0], "iced::widget::row::Wrapping")?;
                if path.ends_with("align_x") {
                    wrapping.align_x = alignment(args[1])?;
                } else {
                    wrapping.vertical_spacing = Some(spacing(args[1], path)?);
                }
                Ok(Dynamic::from(wrapping))
            })
            .expect("the iced facade must retain every Wrapping builder");
    }
}

fn bind_container_builders(builder: &mut host::runtime::Builder) {
    builder
        .bind("iced::widget::Container::padding", |args| {
            let mut container = take_container(args, "Container::padding")?;
            container.padding = padding(args[1])?;
            Ok(Dynamic::from(container))
        })
        .expect("the iced facade must retain the exact Container::padding item");

    builder
        .bind("iced::widget::Container::style", |args| {
            let mut container = take_container(args, "Container::style")?;
            container.style = Some(if args[1].is::<marker::container::Style>() {
                marker::container::Source::Builtin(take(args[1], "container style")?)
            } else {
                marker::container::Source::Function(take(args[1], "container style function")?)
            });
            Ok(Dynamic::from(container))
        })
        .expect("the iced facade must retain the exact Container::style item");
    for (path, set) in [
        (
            "iced::widget::Container::width",
            set_container_width as fn(&mut marker::Container, iced::Length),
        ),
        ("iced::widget::Container::height", set_container_height),
    ] {
        builder
            .bind(path, move |args| {
                let mut container = take_container(args, path)?;
                set(&mut container, length(args[1])?);
                Ok(Dynamic::from(container))
            })
            .expect("the iced facade must retain every Container sizing item");
    }
    for (path, set) in [
        (
            "iced::widget::Container::align_x",
            set_container_align_x as fn(&mut marker::Container, iced::Alignment),
        ),
        ("iced::widget::Container::align_y", set_container_align_y),
    ] {
        builder
            .bind(path, move |args| {
                let mut container = take_container(args, path)?;
                set(&mut container, alignment(args[1])?);
                Ok(Dynamic::from(container))
            })
            .expect("the iced facade must retain every Container alignment item");
    }
    for (path, set) in [
        (
            "iced::widget::Container::center_x",
            set_container_center_x as fn(&mut marker::Container, iced::Length),
        ),
        ("iced::widget::Container::center_y", set_container_center_y),
    ] {
        builder
            .bind(path, move |args| {
                let mut container = take_container(args, path)?;
                set(&mut container, length(args[1])?);
                Ok(Dynamic::from(container))
            })
            .expect("the iced facade must retain every Container centering item");
    }
}

fn bind_styles(builder: &mut host::runtime::Builder) {
    use iced::widget::container::Style;

    builder
        .bind("iced::widget::container::Style::default", |args| {
            if !args.is_empty() {
                return Err(arity("container::Style::default", 0, args.len()));
            }
            Ok(Dynamic::from(Style::default()))
        })
        .expect("the iced facade must retain container::Style::default");
    builder
        .bind("iced::Border::default", |args| {
            if !args.is_empty() {
                return Err(arity("Border::default", 0, args.len()));
            }
            Ok(Dynamic::from(iced::Border::default()))
        })
        .expect("the iced facade must retain Border::default");
    for (name, set) in [(
        "color",
        (|style, color| style.color(color)) as fn(Style, iced::Color) -> Style,
    )] {
        builder
            .bind(
                format!("iced::widget::container::Style::{name}"),
                move |args| {
                    if args.len() != 2 {
                        return Err(arity(name, 2, args.len()));
                    }
                    let style = take::<Style>(args[0], "iced::widget::container::Style")?;
                    Ok(Dynamic::from(set(style, take(args[1], "iced::Color")?)))
                },
            )
            .expect("the iced facade must retain every container::Style color builder");
    }
    builder
        .bind("iced::widget::container::Style::border", |args| {
            // A shared receiver is read in place, never taken.
            if let [style] = args {
                let style = style.read_lock::<Style>().ok_or_else(|| {
                    runtime_error("expected `iced::widget::container::Style`".to_string())
                })?;
                return Ok(Dynamic::from(style.border));
            }
            if args.len() != 2 {
                return Err(arity("container::Style::border", 2, args.len()));
            }
            let style = take::<Style>(args[0], "iced::widget::container::Style")?;
            Ok(Dynamic::from(
                style.border(take::<iced::Border>(args[1], "iced::Border")?),
            ))
        })
        .expect("the iced facade must retain container::Style::border");
    type Set = fn(iced::Border, &mut Dynamic) -> Result<iced::Border, Box<EvalAltResult>>;
    for (name, set) in [
        (
            "rounded",
            (|border, value| Ok(border.rounded(pixels(value, "border::rounded")?))) as Set,
        ),
        ("width", |border, value| {
            Ok(border.width(pixels(value, "border::width")?))
        }),
        ("color", |border, value| {
            Ok(border.color(take::<iced::Color>(value, "iced::Color")?))
        }),
    ] {
        builder
            .bind(format!("iced::border::{name}"), move |args| {
                if args.len() != 1 {
                    return Err(arity(name, 1, args.len()));
                }
                Ok(Dynamic::from(set(iced::Border::default(), args[0])?))
            })
            .expect("the iced facade must retain every border function");
        builder
            .bind(format!("iced::Border::{name}"), move |args| {
                // `border.width` and `border.color`, read as fields.
                if let [border] = args {
                    let border = border
                        .read_lock::<iced::Border>()
                        .ok_or_else(|| runtime_error("expected `iced::Border`".to_string()))?;
                    return Ok(match name {
                        "width" => Dynamic::from(f64::from(border.width)),
                        _ => Dynamic::from(border.color),
                    });
                }
                if args.len() != 2 {
                    return Err(arity(name, 2, args.len()));
                }
                let border = take::<iced::Border>(args[0], "iced::Border")?;
                Ok(Dynamic::from(set(border, args[1])?))
            })
            .expect("the iced facade must retain every Border builder");
    }
    builder
        .bind("iced::border::radius", |args| {
            let [radius] = args else {
                return Err(arity("border::radius", 1, args.len()));
            };
            Ok(Dynamic::from(iced::border::radius(pixels(radius, "border::radius")?)))
        })
        .expect("the iced facade must retain border::radius");
}

fn bind_space_builders(builder: &mut host::runtime::Builder) {
    for (path, set) in [
        (
            "iced::widget::Space::width",
            set_space_width as fn(&mut marker::Space, iced::Length),
        ),
        ("iced::widget::Space::height", set_space_height),
    ] {
        builder
            .bind(path, move |args| {
                if args.len() != 2 {
                    return Err(arity(path, 2, args.len()));
                }
                let mut space = take::<marker::Space>(args[0], "iced::widget::Space")?;
                set(&mut space, length(args[1])?);
                Ok(Dynamic::from(space))
            })
            .expect("the iced facade must retain every Space sizing item");
    }
    for (path, set) in [
        (
            "iced::widget::Scrollable::width",
            (|scrollable, value| scrollable.width = Some(value))
                as fn(&mut marker::Scrollable, iced::Length),
        ),
        ("iced::widget::Scrollable::height", |scrollable, value| {
            scrollable.height = Some(value)
        }),
    ] {
        builder
            .bind(path, move |args| {
                if args.len() != 2 {
                    return Err(arity(path, 2, args.len()));
                }
                let mut scrollable =
                    take::<marker::Scrollable>(args[0], "iced::widget::Scrollable")?;
                set(&mut scrollable, length(args[1])?);
                Ok(Dynamic::from(scrollable))
            })
            .expect("the iced facade must retain every Scrollable sizing item");
    }
}

/// The message of an `Option<Message>`, as iced's `_maybe` builders take.
fn optional_message(
    value: &mut Dynamic,
    item: &str,
) -> Result<Option<Dynamic>, Box<EvalAltResult>> {
    rawr::option::take(value).ok_or_else(|| {
        runtime_error(format!(
            "{item} expected an `Option`, found `{}`",
            value.type_name()
        ))
    })
}

/// The mapper of an `Option<impl Fn(T) -> Message>`, as iced's
/// `on_input_maybe` and `on_toggle_maybe` take.
fn optional_mapper(
    value: &mut Dynamic,
    item: &str,
) -> Result<Option<rawr::item::Value>, Box<EvalAltResult>> {
    optional_message(value, item)?
        .map(|mut mapper| take::<rawr::item::Value>(&mut mapper, "symbolic message mapper"))
        .transpose()
}

fn bind_intrinsic_control_builders(builder: &mut host::runtime::Builder) {
    builder
        .bind("iced::widget::TextInput::on_submit", |args| {
            let mut input = take_text_input(args, "TextInput::on_submit")?;
            input.on_submit = Some(args[1].take());
            Ok(Dynamic::from(input))
        })
        .expect("the iced facade must retain the exact TextInput::on_submit item");
    builder
        .bind("iced::widget::TextInput::on_submit_maybe", |args| {
            let mut input = take_text_input(args, "TextInput::on_submit_maybe")?;
            input.on_submit = optional_message(args[1], "TextInput::on_submit_maybe")?;
            Ok(Dynamic::from(input))
        })
        .expect("the iced facade must retain the exact TextInput::on_submit_maybe item");
    builder
        .bind("iced::widget::TextInput::on_input", |args| {
            let mut input = take_text_input(args, "TextInput::on_input")?;
            input.on_input = Some(take::<rawr::item::Value>(
                args[1],
                "symbolic message mapper",
            )?);
            Ok(Dynamic::from(input))
        })
        .expect("the iced facade must retain the exact TextInput::on_input item");
    builder
        .bind("iced::widget::TextInput::on_input_maybe", |args| {
            let mut input = take_text_input(args, "TextInput::on_input_maybe")?;
            input.on_input = optional_mapper(args[1], "TextInput::on_input_maybe")?;
            Ok(Dynamic::from(input))
        })
        .expect("the iced facade must retain the exact TextInput::on_input_maybe item");
    builder
        .bind("iced::widget::TextInput::padding", |args| {
            let mut input = take_text_input(args, "TextInput::padding")?;
            input.padding = padding(args[1])?;
            Ok(Dynamic::from(input))
        })
        .expect("the iced facade must retain every exact TextInput::padding overload");
    builder
        .bind("iced::widget::TextInput::width", |args| {
            let mut input = take_text_input(args, "TextInput::width")?;
            input.width = Some(length(args[1])?);
            Ok(Dynamic::from(input))
        })
        .expect("the iced facade must retain the exact TextInput::width item");
    builder
        .bind("iced::widget::TextInput::size", |args| {
            let mut input = take_text_input(args, "TextInput::size")?;
            input.size = Some(pixels(args[1], "TextInput::size")?);
            Ok(Dynamic::from(input))
        })
        .expect("the iced facade must retain the exact TextInput::size item");
    builder
        .bind("iced::widget::TextInput::secure", |args| {
            let mut input = take_text_input(args, "TextInput::secure")?;
            input.secure = exact_bool(args[1], "TextInput::secure")?;
            Ok(Dynamic::from(input))
        })
        .expect("the iced facade must retain the exact TextInput::secure item");
    builder
        .bind("iced::widget::Checkbox::label", |args| {
            let mut checkbox = take_checkbox(args, "Checkbox::label")?;
            checkbox.label = Some(exact_string(args[1], "Checkbox::label")?);
            Ok(Dynamic::from(checkbox))
        })
        .expect("the iced facade must retain the exact Checkbox::label item");
    builder
        .bind("iced::widget::Checkbox::on_toggle", |args| {
            let mut checkbox = take_checkbox(args, "Checkbox::on_toggle")?;
            checkbox.on_toggle = Some(take::<rawr::item::Value>(
                args[1],
                "symbolic message mapper",
            )?);
            Ok(Dynamic::from(checkbox))
        })
        .expect("the iced facade must retain the exact Checkbox::on_toggle item");
    builder
        .bind("iced::widget::Checkbox::on_toggle_maybe", |args| {
            let mut checkbox = take_checkbox(args, "Checkbox::on_toggle_maybe")?;
            checkbox.on_toggle = optional_mapper(args[1], "Checkbox::on_toggle_maybe")?;
            Ok(Dynamic::from(checkbox))
        })
        .expect("the iced facade must retain the exact Checkbox::on_toggle_maybe item");
    builder
        .bind("iced::widget::Toggler::label", |args| {
            let mut toggler = take_toggler(args, "Toggler::label")?;
            toggler.label = Some(exact_string(args[1], "Toggler::label")?);
            Ok(Dynamic::from(toggler))
        })
        .expect("the iced facade must retain the exact Toggler::label item");
    builder
        .bind("iced::widget::Toggler::size", |args| {
            let mut toggler = take_toggler(args, "Toggler::size")?;
            toggler.size = Some(pixels(args[1], "Toggler::size")?);
            Ok(Dynamic::from(toggler))
        })
        .expect("the iced facade must retain the exact Toggler::size item");
    builder
        .bind("iced::widget::Toggler::width", |args| {
            let mut toggler = take_toggler(args, "Toggler::width")?;
            toggler.width = Some(length(args[1])?);
            Ok(Dynamic::from(toggler))
        })
        .expect("the iced facade must retain the exact Toggler::width item");
    builder
        .bind("iced::widget::Toggler::spacing", |args| {
            let mut toggler = take_toggler(args, "Toggler::spacing")?;
            toggler.spacing = Some(pixels(args[1], "Toggler::spacing")?);
            Ok(Dynamic::from(toggler))
        })
        .expect("the iced facade must retain the exact Toggler::spacing item");
    builder
        .bind("iced::widget::Checkbox::size", |args| {
            let mut checkbox = take_checkbox(args, "Checkbox::size")?;
            checkbox.size = Some(pixels(args[1], "Checkbox::size")?);
            Ok(Dynamic::from(checkbox))
        })
        .expect("the iced facade must retain the exact Checkbox::size item");
    builder
        .bind("iced::widget::Checkbox::text_size", |args| {
            let mut checkbox = take_checkbox(args, "Checkbox::text_size")?;
            checkbox.text_size = Some(pixels(args[1], "Checkbox::text_size")?);
            Ok(Dynamic::from(checkbox))
        })
        .expect("the iced facade must retain the exact Checkbox::text_size item");
    builder
        .bind("iced::widget::Checkbox::width", |args| {
            let mut checkbox = take_checkbox(args, "Checkbox::width")?;
            checkbox.width = Some(length(args[1])?);
            Ok(Dynamic::from(checkbox))
        })
        .expect("the iced facade must retain the exact Checkbox::width item");
    builder
        .bind("iced::widget::Checkbox::spacing", |args| {
            let mut checkbox = take_checkbox(args, "Checkbox::spacing")?;
            checkbox.spacing = Some(pixels(args[1], "Checkbox::spacing")?);
            Ok(Dynamic::from(checkbox))
        })
        .expect("the iced facade must retain the exact Checkbox::spacing item");
    builder
        .bind("iced::widget::Checkbox::icon", |args| {
            let mut checkbox = take_checkbox(args, "Checkbox::icon")?;
            checkbox.icon = Some(take(args[1], "iced::widget::checkbox::Icon")?);
            Ok(Dynamic::from(checkbox))
        })
        .expect("the iced facade must retain the exact Checkbox::icon item");
    for (path, set) in [
        (
            "iced::widget::Slider::width",
            (|slider, value| slider.width = Some(value)) as fn(&mut marker::Slider, iced::Length),
        ),
        ("iced::widget::Slider::height", |slider, value| {
            slider.height = Some(value)
        }),
    ] {
        builder
            .bind(path, move |args| {
                if args.len() != 2 {
                    return Err(arity(path, 2, args.len()));
                }
                let mut slider = take::<marker::Slider>(args[0], "iced::widget::Slider")?;
                set(&mut slider, length(args[1])?);
                Ok(Dynamic::from(slider))
            })
            .expect("the iced facade must retain every Slider sizing item");
    }
    builder
        .bind("iced::widget::Toggler::on_toggle", |args| {
            let mut toggler = take_toggler(args, "Toggler::on_toggle")?;
            toggler.on_toggle = Some(take::<rawr::item::Value>(
                args[1],
                "symbolic message mapper",
            )?);
            Ok(Dynamic::from(toggler))
        })
        .expect("the iced facade must retain the exact Toggler::on_toggle item");
    builder
        .bind("iced::widget::Toggler::on_toggle_maybe", |args| {
            let mut toggler = take_toggler(args, "Toggler::on_toggle_maybe")?;
            toggler.on_toggle = optional_mapper(args[1], "Toggler::on_toggle_maybe")?;
            Ok(Dynamic::from(toggler))
        })
        .expect("the iced facade must retain the exact Toggler::on_toggle_maybe item");
    builder
        .bind("iced::widget::Slider::step", |args| {
            if args.len() != 2 {
                return Err(arity("Slider::step", 2, args.len()));
            }
            let mut slider = take::<marker::Slider>(args[0], "iced::widget::Slider")?;
            let step = number(args[1], "Slider::step")?;
            if step <= 0.0 {
                return Err(runtime_error(format!(
                    "Slider::step expects a positive value, found {step}"
                )));
            }
            slider.step = step;
            Ok(Dynamic::from(slider))
        })
        .expect("the iced facade must retain the exact Slider::step item");
    for (path, set) in [
        (
            "iced::widget::Slider::default",
            (|slider, value| slider.default = Some(value)) as fn(&mut marker::Slider, f64),
        ),
        ("iced::widget::Slider::shift_step", |slider, value| {
            slider.shift_step = Some(value)
        }),
    ] {
        builder
            .bind(path, move |args| {
                if args.len() != 2 {
                    return Err(arity(path, 2, args.len()));
                }
                let mut slider = take::<marker::Slider>(args[0], "iced::widget::Slider")?;
                let value = number(args[1], path)?;
                set(&mut slider, value);
                Ok(Dynamic::from(slider))
            })
            .expect("the iced facade must retain every Slider builder");
    }
}

fn take_text_input(
    args: &mut [&mut Dynamic],
    item: &str,
) -> Result<marker::TextInput, Box<EvalAltResult>> {
    if args.len() != 2 {
        return Err(arity(item, 2, args.len()));
    }
    take::<marker::TextInput>(args[0], "iced::widget::TextInput")
}

fn take_checkbox(
    args: &mut [&mut Dynamic],
    item: &str,
) -> Result<marker::Checkbox, Box<EvalAltResult>> {
    if args.len() != 2 {
        return Err(arity(item, 2, args.len()));
    }
    take::<marker::Checkbox>(args[0], "iced::widget::Checkbox")
}

fn take_toggler(
    args: &mut [&mut Dynamic],
    item: &str,
) -> Result<marker::Toggler, Box<EvalAltResult>> {
    if args.len() != 2 {
        return Err(arity(item, 2, args.len()));
    }
    take::<marker::Toggler>(args[0], "iced::widget::Toggler")
}

fn take_row(args: &mut [&mut Dynamic], item: &str) -> Result<marker::Row, Box<EvalAltResult>> {
    if args.len() != 2 {
        return Err(arity(item, 2, args.len()));
    }
    take::<marker::Row>(args[0], "iced::widget::Row")
}

pub(super) fn take_container(
    args: &mut [&mut Dynamic],
    item: &str,
) -> Result<marker::Container, Box<EvalAltResult>> {
    if args.len() != 2 {
        return Err(arity(item, 2, args.len()));
    }
    take::<marker::Container>(args[0], "iced::widget::Container")
}

fn set_row_width(row: &mut marker::Row, value: iced::Length) {
    row.width = value;
}

fn set_row_height(row: &mut marker::Row, value: iced::Length) {
    row.height = value;
}

fn set_container_width(container: &mut marker::Container, value: iced::Length) {
    container.width = value;
}

fn set_container_height(container: &mut marker::Container, value: iced::Length) {
    container.height = value;
}

fn set_container_align_x(container: &mut marker::Container, value: iced::Alignment) {
    container.align_x = value;
}

fn set_container_align_y(container: &mut marker::Container, value: iced::Alignment) {
    container.align_y = value;
}

fn set_container_center_x(container: &mut marker::Container, value: iced::Length) {
    container.width = value;
    container.align_x = iced::Alignment::Center;
}

fn set_container_center_y(container: &mut marker::Container, value: iced::Length) {
    container.height = value;
    container.align_y = iced::Alignment::Center;
}

fn set_space_width(space: &mut marker::Space, value: iced::Length) {
    space.width = value;
}

fn set_space_height(space: &mut marker::Space, value: iced::Length) {
    space.height = value;
}

/// The children of `column(..)`, `Column::extend(..)` and their kin: an
/// array of Elements.
pub(super) fn take_children(value: &mut Dynamic) -> Result<Vec<widget::Value>, Box<EvalAltResult>> {
    let children = value.take().into_array().map_err(|actual| {
        runtime_error(format!("expected a list of widgets, found `{actual}`"))
    })?;
    children
        .into_iter()
        .map(|mut child| take::<widget::Value>(&mut child, "iced::Element"))
        .collect()
}

/// The children of `keyed_column(..)`: a list of `(key, element)` pairs,
/// each element keyed by its pair's key.
fn take_keyed_children(value: &mut Dynamic) -> Result<Vec<widget::Value>, Box<EvalAltResult>> {
    let pairs = value.take().into_array().map_err(|actual| {
        runtime_error(format!("expected a list of keyed widgets, found `{actual}`"))
    })?;
    pairs
        .into_iter()
        .map(|pair| {
            let mut pair = pair.into_array().map_err(|actual| {
                runtime_error(format!("expected a `(key, widget)` pair, found `{actual}`"))
            })?;
            let [key, child] = pair.as_mut_slice() else {
                return Err(runtime_error(format!(
                    "expected a `(key, widget)` pair, found {} values",
                    pair.len()
                )));
            };
            let child = take::<widget::Value>(child, "iced::Element")?;
            Ok(child.keyed(key_value(key)))
        })
        .collect()
}

/// The reconciliation key of one Rawr value. An integer or a string keys
/// by itself. Any other value keys by its `Debug` text, which is the
/// canonical text of an imported value such as a `Uuid`.
fn key_value(key: &Dynamic) -> widget::key::Value {
    if let Ok(key) = key.as_int() {
        widget::key::Value::integer(key)
    } else if let Ok(key) = key.as_immutable_string_ref() {
        widget::key::Value::string(key.clone())
    } else {
        widget::key::Value::string(format!("{key:?}").into())
    }
}

fn new_column(
    call: &host::runtime::Call<'_>,
    children: Vec<widget::Value>,
) -> Result<Dynamic, Box<EvalAltResult>> {
    let mode =
        widget::key::Mode::bind(&children).map_err(|error| runtime_error(error.to_string()))?;
    Ok(Dynamic::from(marker::Column {
        witness: call.output_witness().map_err(witness_error)?,
        identity: widget::Identity::unique(),
        children,
        mode,
        spacing: 0.0,
        padding: iced::Padding::ZERO,
        width: iced::Length::Fit,
        height: None,
        align_x: iced::Alignment::Start,
    }))
}

fn new_row(
    call: &host::runtime::Call<'_>,
    children: Vec<widget::Value>,
) -> Result<Dynamic, Box<EvalAltResult>> {
    if !matches!(widget::key::Mode::bind(&children), Ok(widget::key::Mode::Positional)) {
        return Err(positional_row());
    }
    Ok(Dynamic::from(marker::Row {
        witness: call.output_witness().map_err(witness_error)?,
        identity: widget::Identity::unique(),
        children,
        spacing: 0.0,
        padding: iced::Padding::ZERO,
        width: iced::Length::Fit,
        height: iced::Length::Fit,
        align_y: iced::Alignment::Start,
    }))
}

pub(super) fn positional_row() -> Box<EvalAltResult> {
    runtime_error(
        "built-in Row requires positional children; keyed reconciliation is owned by Column"
            .to_string(),
    )
}

pub(super) fn project<T>(
    call: &host::runtime::Call<'_>,
    args: &mut [&mut Dynamic],
    expected: &str,
) -> Result<Dynamic, Box<EvalAltResult>>
where
    T: widget::Descriptor + Witnessed + Clone,
{
    if args.len() != 1 {
        return Err(arity("widget projection", 1, args.len()));
    }
    let descriptor = take::<T>(args[0], expected)?;
    let value = widget::Value::from_projection(descriptor.clone(), call, descriptor.witness())
        .map_err(witness_error)?;
    Ok(Dynamic::from(value))
}

pub(super) trait Witnessed {
    fn witness(&self) -> &host::runtime::Witness;
}

impl Witnessed for marker::ProgressBar {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

impl Witnessed for marker::Tooltip {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

/// `tooltip`, its builders, `tooltip::Position`, `alignment::Horizontal`
/// and `time::seconds`.
fn bind_sensor(builder: &mut host::runtime::Builder) {
    builder
        .bind_with_context("iced::widget::void", |call, args| {
            if !args.is_empty() {
                return Err(arity("void", 0, args.len()));
            }
            let void = marker::Void {
                identity: widget::Identity::unique(),
            };
            widget::Value::from_factory(void, call)
                .map(Dynamic::from)
                .map_err(witness_error)
        })
        .expect("the iced facade must retain the exact void item");
    builder
        .bind_with_context("iced::widget::sensor", |call, args| {
            if args.len() != 1 {
                return Err(arity("sensor", 1, args.len()));
            }
            Ok(Dynamic::from(marker::Sensor {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                child: take::<widget::Value>(args[0], "iced::Element")?,
                on_show: None,
                on_resize: None,
                on_hide: None,
            }))
        })
        .expect("the iced facade must retain the exact sensor item");
    for name in ["on_show", "on_resize", "on_hide"] {
        let path = format!("iced::widget::Sensor::{name}");
        builder
            .bind(path.clone(), move |args| {
                if args.len() != 2 {
                    return Err(arity(&path, 2, args.len()));
                }
                let mut sensor = take::<marker::Sensor>(args[0], "iced::widget::Sensor")?;
                match name {
                    "on_show" => sensor.on_show = Some(take(args[1], "symbolic message mapper")?),
                    "on_resize" => {
                        sensor.on_resize = Some(take(args[1], "symbolic message mapper")?);
                    }
                    _ => sensor.on_hide = Some(args[1].take()),
                }
                Ok(Dynamic::from(sensor))
            })
            .expect("the iced facade must retain every Sensor builder");
    }
}

fn bind_tooltip(builder: &mut host::runtime::Builder) {
    builder
        .bind_with_context("iced::widget::tooltip", |call, args| {
            if args.len() != 3 {
                return Err(arity("tooltip", 3, args.len()));
            }
            let content = take::<widget::Value>(args[0], "iced::Element")?;
            let tooltip = if args[1].is_string() {
                marker::Content::Label(string_content(args[1])?)
            } else {
                marker::Content::Element(take::<widget::Value>(args[1], "iced::Element")?)
            };
            Ok(Dynamic::from(marker::Tooltip {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                content,
                tooltip,
                position: take(args[2], "iced::widget::tooltip::Position")?,
                gap: 0.0,
                delay: std::time::Duration::ZERO,
                snap_within_viewport: true,
                style: None,
            }))
        })
        .expect("the iced facade must retain the exact tooltip items");
    builder
        .bind("iced::widget::Tooltip::gap", |args| {
            const PATH: &str = "iced::widget::Tooltip::gap";
            if args.len() != 2 {
                return Err(arity(PATH, 2, args.len()));
            }
            let mut tooltip = take::<marker::Tooltip>(args[0], "iced::widget::Tooltip")?;
            tooltip.gap = pixels(args[1], PATH)?;
            Ok(Dynamic::from(tooltip))
        })
        .expect("the iced facade must retain the Tooltip gap item");
    builder
        .bind("iced::widget::Tooltip::delay", |args| {
            if args.len() != 2 {
                return Err(arity("Tooltip::delay", 2, args.len()));
            }
            let mut tooltip = take::<marker::Tooltip>(args[0], "iced::widget::Tooltip")?;
            tooltip.delay = take(args[1], "iced::time::Duration")?;
            Ok(Dynamic::from(tooltip))
        })
        .expect("the iced facade must retain the exact Tooltip::delay item");
    builder
        .bind("iced::widget::Tooltip::snap_within_viewport", |args| {
            if args.len() != 2 {
                return Err(arity("Tooltip::snap_within_viewport", 2, args.len()));
            }
            let mut tooltip = take::<marker::Tooltip>(args[0], "iced::widget::Tooltip")?;
            tooltip.snap_within_viewport = exact_bool(args[1], "Tooltip::snap_within_viewport")?;
            Ok(Dynamic::from(tooltip))
        })
        .expect("the iced facade must retain the exact Tooltip::snap_within_viewport item");
    builder
        .bind("iced::widget::Tooltip::style", |args| {
            if args.len() != 2 {
                return Err(arity("Tooltip::style", 2, args.len()));
            }
            let mut tooltip = take::<marker::Tooltip>(args[0], "iced::widget::Tooltip")?;
            tooltip.style = Some(if args[1].is::<marker::container::Style>() {
                marker::container::Source::Builtin(take(args[1], "container style")?)
            } else {
                marker::container::Source::Function(take(args[1], "container style function")?)
            });
            Ok(Dynamic::from(tooltip))
        })
        .expect("the iced facade must retain the exact Tooltip::style item");
    for (name, position) in crate::TOOLTIP_POSITIONS.into_iter().zip([
        iced::widget::tooltip::Position::Top,
        iced::widget::tooltip::Position::Bottom,
        iced::widget::tooltip::Position::Left,
        iced::widget::tooltip::Position::Right,
        iced::widget::tooltip::Position::FollowCursor,
    ]) {
        builder
            .bind(format!("iced::widget::tooltip::Position::{name}"), move |_| {
                Ok(Dynamic::from(position))
            })
            .expect("the iced facade must retain every tooltip::Position variant");
    }
    for (name, alignment) in [
        ("Left", iced::Alignment::Start),
        ("Center", iced::Alignment::Center),
        ("Right", iced::Alignment::End),
    ] {
        builder
            .bind(format!("iced::alignment::Horizontal::{name}"), move |_| {
                Ok(Dynamic::from(marker::Horizontal(alignment)))
            })
            .expect("the iced facade must retain every alignment::Horizontal value");
    }
    for (name, unit) in [
        ("seconds", std::time::Duration::from_secs as fn(u64) -> std::time::Duration),
        ("milliseconds", std::time::Duration::from_millis),
    ] {
        builder
            .bind(format!("iced::time::{name}"), move |args| {
                let amount = args
                    .first()
                    .and_then(|value| value.as_int().ok())
                    .and_then(|value| u64::try_from(value).ok())
                    .ok_or_else(|| runtime_error(format!("time::{name} expects a `u64`")))?;
                Ok(Dynamic::from(unit(amount)))
            })
            .expect("the iced facade must retain every time unit");
    }
}

impl Witnessed for marker::Transition {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

/// `progress_bar`, `transition` and the settled `Animation` it draws with.
fn bind_progress(builder: &mut host::runtime::Builder) {
    builder
        .bind_with_context("iced::widget::progress_bar", |call, args| {
            if args.len() != 2 {
                return Err(arity("progress_bar", 2, args.len()));
            }
            let range = number_range(args[0], "progress_bar")?;
            Ok(Dynamic::from(marker::ProgressBar {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                range: (*range.start() as f32)..=(*range.end() as f32),
                value: number(args[1], "progress_bar value")? as f32,
                vertical: false,
                length: None,
                girth: None,
                style: None,
            }))
        })
        .expect("the iced facade must retain the exact progress_bar item");
    builder
        .bind("iced::widget::ProgressBar::vertical", |args| {
            if args.len() != 2 {
                return Err(arity("ProgressBar::vertical", 2, args.len()));
            }
            let mut bar = take::<marker::ProgressBar>(args[0], "iced::widget::ProgressBar")?;
            bar.vertical = exact_bool(args[1], "ProgressBar::vertical")?;
            Ok(Dynamic::from(bar))
        })
        .expect("the iced facade must retain the exact ProgressBar::vertical item");
    for (path, set) in [
        (
            "iced::widget::ProgressBar::length",
            (|bar, value| bar.length = Some(value)) as fn(&mut marker::ProgressBar, iced::Length),
        ),
        ("iced::widget::ProgressBar::girth", |bar, value| bar.girth = Some(value)),
    ] {
        builder
            .bind(path, move |args| {
                if args.len() != 2 {
                    return Err(arity(path, 2, args.len()));
                }
                let mut bar = take::<marker::ProgressBar>(args[0], "iced::widget::ProgressBar")?;
                set(&mut bar, length(args[1])?);
                Ok(Dynamic::from(bar))
            })
            .expect("the iced facade must retain every ProgressBar sizing item");
    }
    builder
        .bind_with_context("iced::widget::transition", |call, args| {
            if args.len() != 3 {
                return Err(arity("transition", 3, args.len()));
            }
            Ok(Dynamic::from(marker::Transition {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                value: number(args[0], "transition value")?,
                view: take::<rawr::item::Value>(args[2], "transition view")?,
            }))
        })
        .expect("the iced facade must retain the exact transition item");
    super::animation::bind(builder);
}

impl Witnessed for marker::Text {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

impl Witnessed for marker::TextInput {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

impl Witnessed for marker::Checkbox {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

impl Witnessed for marker::Toggler {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

impl Witnessed for marker::Slider {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

impl Witnessed for marker::Logo {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

impl Witnessed for marker::Radio {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

impl Witnessed for marker::ComboBox {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

/// `combo_box::State::new`, `combo_box` and the `ComboBox` builders.
fn bind_combo_box(builder: &mut host::runtime::Builder) {
    static NEXT_STATE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    builder
        .bind_with_context("iced::widget::combo_box::State::new", |call, args| {
            if args.len() != 2 {
                return Err(arity("combo_box::State::new", 2, args.len()));
            }
            Ok(Dynamic::from(marker::ComboBoxState {
                witness: call.output_witness().map_err(witness_error)?,
                id: NEXT_STATE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                options: take::<rawr::Array>(args[0], "Vec<T>")?.into_iter().collect(),
                to_string: take::<rawr::item::Value>(args[1], "symbolic `to_string`")?,
            }))
        })
        .expect("the iced facade must retain the exact combo_box::State::new item");
    builder
        .bind_with_context("iced::widget::combo_box", |call, args| {
            if args.len() != 4 {
                return Err(arity("combo_box", 4, args.len()));
            }
            let state = args[0]
                .read_lock::<marker::ComboBoxState>()
                .map(|state| state.clone())
                .ok_or_else(|| {
                    Box::new(EvalAltResult::ErrorRuntime(
                        "combo_box expects a combo_box::State".into(),
                        Position::NONE,
                    ))
                })?;
            Ok(Dynamic::from(marker::ComboBox {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                state,
                placeholder: exact_string(args[1], "combo_box placeholder")?,
                selection: optional_message(args[2], "combo_box")?,
                on_selected: take::<rawr::item::Value>(args[3], "symbolic message mapper")?,
                on_option_hovered: None,
                on_open: None,
                on_close: None,
                width: None,
                size: None,
            }))
        })
        .expect("the iced facade must retain the exact combo_box item");
    for (path, set) in [
        (
            "iced::widget::ComboBox::on_option_hovered",
            (|combo_box, value| {
                combo_box.on_option_hovered = Some(take::<rawr::item::Value>(
                    value,
                    "symbolic message mapper",
                )?);
                Ok(())
            }) as fn(&mut marker::ComboBox, &mut Dynamic) -> Result<(), Box<EvalAltResult>>,
        ),
        ("iced::widget::ComboBox::on_open", |combo_box, value| {
            combo_box.on_open = Some(value.take());
            Ok(())
        }),
        ("iced::widget::ComboBox::on_close", |combo_box, value| {
            combo_box.on_close = Some(value.take());
            Ok(())
        }),
        ("iced::widget::ComboBox::width", |combo_box, value| {
            combo_box.width = Some(length(value)?);
            Ok(())
        }),
        ("iced::widget::ComboBox::size", |combo_box, value| {
            combo_box.size = Some(pixels(value, "ComboBox::size")?);
            Ok(())
        }),
    ] {
        builder
            .bind(path, move |args| {
                if args.len() != 2 {
                    return Err(arity(path, 2, args.len()));
                }
                let mut combo_box = take::<marker::ComboBox>(args[0], "iced::widget::ComboBox")?;
                set(&mut combo_box, args[1])?;
                Ok(Dynamic::from(combo_box))
            })
            .expect("the iced facade must retain every ComboBox builder");
    }
}

impl Witnessed for marker::PickList {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

impl Witnessed for marker::Button {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

impl Witnessed for marker::Column {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

impl Witnessed for marker::Row {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

impl Witnessed for marker::Wrapping {
    fn witness(&self) -> &host::runtime::Witness {
        &self.row.witness
    }
}

impl Witnessed for marker::Container {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

impl Witnessed for marker::Scrollable {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

impl Witnessed for marker::Sensor {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

impl Witnessed for marker::Space {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

pub(super) fn take<T>(value: &mut Dynamic, expected: &str) -> Result<T, Box<EvalAltResult>>
where
    T: Clone + 'static,
{
    let actual = value.type_name();
    value.take().try_cast_result::<T>().map_err(|_| {
        runtime_error(format!(
            "native widget binding received `{actual}`, expected `{expected}`"
        ))
    })
}

pub(super) fn display_content(value: &Dynamic) -> Result<String, Box<EvalAltResult>> {
    if let Ok(content) = value.as_immutable_string_ref() {
        return Ok(content.to_string());
    }
    if let Ok(content) = value.as_int() {
        return Ok(content.to_string());
    }
    if let Ok(content) = value.as_bool() {
        return Ok(content.to_string());
    }
    if let Ok(content) = value.as_char() {
        return Ok(content.to_string());
    }
    if let Ok(content) = value.as_float() {
        return Ok(content.to_string());
    }
    Err(runtime_error(format!(
        "iced::widget::text cannot display `{}` yet",
        value.type_name()
    )))
}

pub(super) fn string_content(value: &Dynamic) -> Result<String, Box<EvalAltResult>> {
    value
        .as_immutable_string_ref()
        .map(|content| content.to_string())
        .map_err(|_| {
            runtime_error(format!(
                "iced::widget::button content must be `String`, found `{}`",
                value.type_name()
            ))
        })
}

pub(super) fn exact_string(value: &Dynamic, operation: &str) -> Result<String, Box<EvalAltResult>> {
    value
        .as_immutable_string_ref()
        .map(|content| content.to_string())
        .map_err(|_| {
            runtime_error(format!(
                "{operation} expects `String`, found `{}`",
                value.type_name()
            ))
        })
}

pub(super) fn exact_bool(value: &Dynamic, operation: &str) -> Result<bool, Box<EvalAltResult>> {
    value.as_bool().map_err(|_| {
        runtime_error(format!(
            "{operation} expects `bool`, found `{}`",
            value.type_name()
        ))
    })
}

/// Non-negative pixels from an integer or a float, as iced's `impl Into<Pixels>`.
pub(super) fn pixels(value: &Dynamic, operation: &str) -> Result<f32, Box<EvalAltResult>> {
    let pixels = match (value.as_int(), value.as_float()) {
        (Ok(value), _) => value as f32,
        (_, Ok(value)) => value as f32,
        _ => {
            return Err(runtime_error(format!(
                "{operation} expects pixels as `i64` or `f64`, found `{}`",
                value.type_name()
            )))
        }
    };
    if !pixels.is_finite() || pixels < 0.0 {
        return Err(runtime_error(format!(
            "{operation} expects non-negative pixels, found {pixels}"
        )));
    }
    Ok(pixels)
}

/// Spacing in pixels; iced lets it be negative, so children overlap.
pub(super) fn spacing(value: &Dynamic, operation: &str) -> Result<f32, Box<EvalAltResult>> {
    match (value.as_int(), value.as_float()) {
        (Ok(value), _) => Ok(value as f32),
        (_, Ok(value)) if value.is_finite() => Ok(value as f32),
        _ => Err(runtime_error(format!(
            "{operation} expects finite pixels as `i64` or `f64`, found `{}`",
            value.type_name()
        ))),
    }
}

/// An `Alignment`, or the `alignment::Horizontal` that converts into one.
pub(super) fn alignment(value: &mut Dynamic) -> Result<iced::Alignment, Box<EvalAltResult>> {
    if value.is::<marker::Horizontal>() {
        return Ok(take::<marker::Horizontal>(value, "iced::alignment::Horizontal")?.0);
    }
    if value.is::<marker::Vertical>() {
        return Ok(take::<marker::Vertical>(value, "iced::alignment::Vertical")?.0);
    }
    Ok(take::<marker::Alignment>(value, "iced::Alignment")?.0)
}

/// A slider's number, `i64` or `f64`.
pub(super) fn number(value: &Dynamic, operation: &str) -> Result<f64, Box<EvalAltResult>> {
    match (value.as_int(), value.as_float()) {
        (Ok(value), _) => Ok(value as f64),
        (_, Ok(value)) if value.is_finite() => Ok(value),
        _ => Err(runtime_error(format!(
            "{operation} expects a finite `i64` or `f64`, found `{}`",
            value.type_name()
        ))),
    }
}

/// `slider(Degrees::RANGE, degrees, on_change)`, or over `Radians`: the
/// angle's number, its range and its value in that unit.
fn angle_slider(
    args: &mut [&mut Dynamic],
) -> Result<(marker::SliderNumber, std::ops::RangeInclusive<f64>, f64), Box<EvalAltResult>> {
    let range = take::<marker::AngleRange>(args[0], "iced::RangeInclusive")?;
    Ok(match range {
        marker::AngleRange::Degrees(range) => (
            marker::SliderNumber::Degrees,
            f64::from(range.start().0)..=f64::from(range.end().0),
            f64::from(take::<iced::Degrees>(args[1], "iced::Degrees")?.0),
        ),
        marker::AngleRange::Radians(range) => (
            marker::SliderNumber::Radians,
            f64::from(range.start().0)..=f64::from(range.end().0),
            f64::from(take::<iced::Radians>(args[1], "iced::Radians")?.0),
        ),
    })
}

/// An ascending `RangeInclusive` of `i64` or `f64`.
fn number_range(
    value: &mut Dynamic,
    operation: &str,
) -> Result<std::ops::RangeInclusive<f64>, Box<EvalAltResult>> {
    let range = if value.is::<std::ops::RangeInclusive<rawr::INT>>() {
        let range = take::<std::ops::RangeInclusive<rawr::INT>>(value, "RangeInclusive<i64>")?;
        (*range.start() as f64)..=(*range.end() as f64)
    } else {
        take::<std::ops::RangeInclusive<rawr::FLOAT>>(value, "RangeInclusive<f64>")?
    };
    if !(range.start() <= range.end()) {
        return Err(runtime_error(format!(
            "{operation} expects an ascending inclusive range, found {}..={}",
            range.start(),
            range.end()
        )));
    }
    Ok(range)
}

/// iced's `impl Into<Length>`: a `Length`, or fixed pixels.
pub(super) fn length(value: &mut Dynamic) -> Result<iced::Length, Box<EvalAltResult>> {
    if value.is_int() || value.is_float() {
        return pixels(value, "Length").map(iced::Length::Fixed);
    }
    take::<iced::Length>(value, "iced::Length")
}

/// iced's `impl Into<Padding>`: a `Padding`, or uniform pixels.
/// `impl Into<Padding>`: uniform pixels, `[vertical, horizontal]`, or a
/// `Padding`, as iced's `From` impls.
fn padding(value: &mut Dynamic) -> Result<iced::Padding, Box<EvalAltResult>> {
    if value.is_int() || value.is_float() {
        return pixels(value, "Padding").map(iced::Padding::new);
    }
    if let Some(sides) = value.read_lock::<rawr::Array>() {
        let [vertical, horizontal] = sides.as_slice() else {
            return Err(runtime_error(format!(
                "Padding expects `[vertical, horizontal]`, found {} element(s)",
                sides.len()
            )));
        };
        let vertical = pixels(vertical, "Padding")?;
        let horizontal = pixels(horizontal, "Padding")?;
        return Ok(iced::Padding::from([vertical, horizontal]));
    }
    take::<iced::Padding>(value, "iced::Padding")
}

pub(super) fn arity(operation: &str, expected: usize, actual: usize) -> Box<EvalAltResult> {
    runtime_error(format!(
        "native widget binding `{operation}` received {actual} operand(s), expected {expected}"
    ))
}

pub(super) fn runtime_error(message: String) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into()
}

pub(super) fn witness_error(error: host::runtime::WitnessError) -> Box<EvalAltResult> {
    runtime_error(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ELEMENT: &str = "iced::Element";
    const SIBLING_ATTESTATIONS: [&str; 9] = [
        "iced::widget::TextInput",
        "iced::widget::Checkbox",
        "iced::widget::Toggler",
        "iced::widget::Slider",
        "iced::widget::Button",
        "iced::widget::Column",
        "iced::widget::Row",
        "iced::widget::Container",
        "iced::widget::Scrollable",
    ];

    #[test]
    fn profile_inventory_omits_only_the_replaced_element_attestor() {
        let definition = crate::facade().expect("the exact iced facade builds");
        let mut ordinary = host::runtime::Builder::new(definition.clone());
        bind_attestations(&mut ordinary);
        assert!(ordinary.has_type_attestation(ELEMENT));
        assert!(SIBLING_ATTESTATIONS
            .iter()
            .all(|path| ordinary.has_type_attestation(path)));

        let mut profile = host::runtime::Builder::new(definition);
        bind_profile_attestations(&mut profile);
        assert!(!profile.has_type_attestation(ELEMENT));
        assert!(SIBLING_ATTESTATIONS
            .iter()
            .all(|path| profile.has_type_attestation(path)));

        profile
            .bind_type_attestation(ELEMENT, crate::profile::runner::attest_support_element)
            .expect("the authenticated generated replacement occupies the sole Element slot");
        assert!(profile.has_type_attestation(ELEMENT));
        assert!(matches!(
            profile.bind_type_attestation(ELEMENT, |_, _| None),
            Err(host::runtime::Error::DuplicateTypeAttestation { .. })
        ));
    }

    #[test]
    fn sealed_style_identities_select_exact_pinned_native_functions() {
        let theme = iced::Theme::Dark;
        for (style, expected) in [
            (
                marker::text::Style::Primary,
                iced::widget::text::primary as fn(&iced::Theme) -> iced::widget::text::Style,
            ),
            (
                marker::text::Style::Secondary,
                iced::widget::text::secondary,
            ),
            (marker::text::Style::Success, iced::widget::text::success),
            (marker::text::Style::Warning, iced::widget::text::warning),
            (marker::text::Style::Danger, iced::widget::text::danger),
            (marker::text::Style::Default, iced::widget::text::default),
        ] {
            assert_eq!(style.native()(&theme), expected(&theme));
        }

        for (style, expected) in [
            (
                marker::container::Style::Transparent,
                iced::widget::container::transparent
                    as fn(&iced::Theme) -> iced::widget::container::Style,
            ),
            (
                marker::container::Style::RoundedBox,
                iced::widget::container::rounded_box,
            ),
            (
                marker::container::Style::BorderedBox,
                iced::widget::container::bordered_box,
            ),
            (
                marker::container::Style::Dark,
                iced::widget::container::dark,
            ),
            (
                marker::container::Style::Primary,
                iced::widget::container::primary,
            ),
            (
                marker::container::Style::Secondary,
                iced::widget::container::secondary,
            ),
            (
                marker::container::Style::Success,
                iced::widget::container::success,
            ),
            (
                marker::container::Style::Warning,
                iced::widget::container::warning,
            ),
            (
                marker::container::Style::Danger,
                iced::widget::container::danger,
            ),
        ] {
            for _ in 0..4 {
                assert_eq!(style.native()(&theme), expected(&theme));
            }
        }
    }
}
