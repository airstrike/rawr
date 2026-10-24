//! iced master's small helpers: alignment shorthands, `left`/`right`/`top`/
//! `bottom`, `rule`, `stack`, `color!`, `time::hours`/`minutes` and `font`.

use iced::widget::Widget as _;
use rawr::host;
use rawr::{Dynamic, EvalAltResult};

use super::builtin::{
    arity, length, positional_row, project, runtime_error, take, take_children,
    take_container, witness_error, Witnessed,
};
use crate::{marker, runtime, widget};

pub(super) fn bind(builder: &mut host::runtime::Builder) {
    bind_alignment(builder);
    bind_containers(builder);
    bind_rule(builder);
    bind_stack(builder);
    bind_values(builder);
    bind_font(builder);
    bind_displays(builder);
    bind_fields(builder);
    bind_wrappers(builder);
    super::media::bind(builder);
    super::icon::bind(builder);
    super::angle::bind(builder);
    super::scroll::bind(builder);
    super::table::bind(builder);
    super::rich::bind(builder);
    bind_operations(builder);
    bind_keyboard(builder);
    bind_events(builder);
}

/// The readers that match `iced::Event`, `window::Event`, `mouse::Event`,
/// `touch::Event` and `Key`'s tuple variants as facade enums.
fn bind_events(builder: &mut host::runtime::Builder) {
    use iced::{keyboard, mouse, touch, window, Event};

    fn read<T: Clone + Send + Sync + 'static>(
        builder: &mut host::runtime::Builder,
        path: &'static str,
        read: fn(&T) -> Dynamic,
    ) {
        builder
            .bind(path, move |args| read_value(path, args, read))
            .expect("the iced facade must retain every event reader");
    }
    fn read_value<T: Clone + Send + Sync + 'static>(
        path: &'static str,
        args: &mut [&mut Dynamic],
        read: fn(&T) -> Dynamic,
    ) -> Result<Dynamic, Box<EvalAltResult>> {
        let [value] = args else {
            return Err(arity(path, 1, args.len()));
        };
        let value = value
            .read_lock::<T>()
            .ok_or_else(|| runtime_error(format!("`{path}` read a value of another type")))?;
        Ok(read(&value))
    }
    read::<Event>(builder, "iced::Event::Keyboard", |event| match event {
        Event::Keyboard(event) => rawr::option::some(Dynamic::from(event.clone())),
        _ => rawr::option::none(),
    });
    read::<Event>(builder, "iced::Event::Mouse", |event| match event {
        Event::Mouse(event) => rawr::option::some(Dynamic::from(*event)),
        _ => rawr::option::none(),
    });
    read::<Event>(builder, "iced::Event::Window", |event| match event {
        Event::Window(event) => rawr::option::some(Dynamic::from(event.clone())),
        _ => rawr::option::none(),
    });
    read::<Event>(builder, "iced::Event::Touch", |event| match event {
        Event::Touch(event) => rawr::option::some(Dynamic::from(*event)),
        _ => rawr::option::none(),
    });
    read::<Event>(builder, "iced::Event::InputMethod", |event| {
        Dynamic::from(matches!(event, Event::InputMethod(_)))
    });
    read::<Event>(builder, "iced::Event::Clipboard", |event| {
        Dynamic::from(matches!(event, Event::Clipboard(_)))
    });
    read::<Event>(builder, "iced::Event::Waken", |event| {
        Dynamic::from(matches!(event, Event::Waken))
    });

    macro_rules! test {
        ($ty:ty, $path:literal, $pattern:pat) => {
            read::<$ty>(builder, $path, |event| Dynamic::from(matches!(event, $pattern)));
        };
    }
    test!(window::Event, "iced::window::Event::Opened", window::Event::Opened { .. });
    test!(window::Event, "iced::window::Event::Closed", window::Event::Closed);
    test!(window::Event, "iced::window::Event::CloseRequested", window::Event::CloseRequested);
    test!(window::Event, "iced::window::Event::Focused", window::Event::Focused);
    test!(window::Event, "iced::window::Event::Unfocused", window::Event::Unfocused);
    test!(window::Event, "iced::window::Event::FileHovered", window::Event::FileHovered(_));
    test!(window::Event, "iced::window::Event::FileDropped", window::Event::FileDropped(_));
    test!(window::Event, "iced::window::Event::FilesHoveredLeft", window::Event::FilesHoveredLeft);
    read::<window::Event>(builder, "iced::window::Event::Moved", |event| match event {
        window::Event::Moved(point) => rawr::option::some(Dynamic::from(*point)),
        _ => rawr::option::none(),
    });
    read::<window::Event>(builder, "iced::window::Event::Resized", |event| match event {
        window::Event::Resized(size) => rawr::option::some(Dynamic::from(*size)),
        _ => rawr::option::none(),
    });
    read::<window::Event>(builder, "iced::window::Event::Rescaled", |event| match event {
        window::Event::Rescaled(scale) => rawr::option::some(Dynamic::from(f64::from(*scale))),
        _ => rawr::option::none(),
    });
    read::<window::Event>(builder, "iced::window::Event::RedrawRequested", |event| match event {
        window::Event::RedrawRequested(now) => rawr::option::some(Dynamic::from(marker::Instant(*now))),
        _ => rawr::option::none(),
    });
    read::<window::Event>(builder, "iced::window::Event::position", |event| match event {
        window::Event::Opened { position, .. } => {
            position.map_or_else(rawr::option::none, |point| rawr::option::some(Dynamic::from(point)))
        }
        _ => rawr::option::none(),
    });
    read::<window::Event>(builder, "iced::window::Event::size", |event| match event {
        window::Event::Opened { size, .. } => Dynamic::from(*size),
        _ => Dynamic::from(iced::Size::ZERO),
    });
    read::<window::Event>(builder, "iced::window::Event::scale_factor", |event| match event {
        window::Event::Opened { scale_factor, .. } => Dynamic::from(f64::from(*scale_factor)),
        _ => Dynamic::from(1.0_f64),
    });

    test!(mouse::Event, "iced::mouse::Event::CursorEntered", mouse::Event::CursorEntered);
    test!(mouse::Event, "iced::mouse::Event::CursorLeft", mouse::Event::CursorLeft);
    test!(mouse::Event, "iced::mouse::Event::CursorMoved", mouse::Event::CursorMoved { .. });
    test!(mouse::Event, "iced::mouse::Event::WheelScrolled", mouse::Event::WheelScrolled { .. });
    read::<mouse::Event>(builder, "iced::mouse::Event::ButtonPressed", |event| match event {
        mouse::Event::ButtonPressed(button) => rawr::option::some(Dynamic::from(*button)),
        _ => rawr::option::none(),
    });
    read::<mouse::Event>(builder, "iced::mouse::Event::ButtonReleased", |event| match event {
        mouse::Event::ButtonReleased(button) => rawr::option::some(Dynamic::from(*button)),
        _ => rawr::option::none(),
    });
    read::<mouse::Event>(builder, "iced::mouse::Event::position", |event| match event {
        mouse::Event::CursorMoved { position } => Dynamic::from(*position),
        _ => Dynamic::from(iced::Point::ORIGIN),
    });
    for (name, button) in [
        ("Left", mouse::Button::Left),
        ("Right", mouse::Button::Right),
        ("Middle", mouse::Button::Middle),
        ("Back", mouse::Button::Back),
        ("Forward", mouse::Button::Forward),
    ] {
        builder
            .bind(format!("iced::mouse::Button::{name}"), move |_| Ok(Dynamic::from(button)))
            .expect("the iced facade must retain every mouse::Button");
    }

    test!(touch::Event, "iced::touch::Event::FingerPressed", touch::Event::FingerPressed { .. });
    test!(touch::Event, "iced::touch::Event::FingerMoved", touch::Event::FingerMoved { .. });
    test!(touch::Event, "iced::touch::Event::FingerLifted", touch::Event::FingerLifted { .. });
    test!(touch::Event, "iced::touch::Event::FingerLost", touch::Event::FingerLost { .. });
    read::<touch::Event>(builder, "iced::touch::Event::position", |event| match event {
        touch::Event::FingerPressed { position, .. }
        | touch::Event::FingerMoved { position, .. }
        | touch::Event::FingerLifted { position, .. }
        | touch::Event::FingerLost { position, .. } => Dynamic::from(*position),
    });

    builder
        .bind_variant_reader("iced::keyboard::Key::Named", |args| {
            read_value::<keyboard::Key>("iced::keyboard::Key::Named", args, |key| match key {
                keyboard::Key::Named(named) => rawr::option::some(Dynamic::from(*named)),
                _ => rawr::option::none(),
            })
        })
        .expect("the iced facade must retain Key::Named's reader");
    builder
        .bind_variant_reader("iced::keyboard::Key::Character", |args| {
            read_value::<keyboard::Key>("iced::keyboard::Key::Character", args, |key| match key {
                keyboard::Key::Character(text) => {
                    rawr::option::some(Dynamic::from(text.to_string()))
                }
                _ => rawr::option::none(),
            })
        })
        .expect("the iced facade must retain Key::Character's reader");
}

/// `keyboard::Event`'s variants and fields, `Key`, `key::Named` and
/// `Modifiers`.
fn bind_keyboard(builder: &mut host::runtime::Builder) {
    use iced::keyboard::{key::Named, Event, Key, Modifiers};

    fn read<T: Clone + Send + Sync + 'static>(
        builder: &mut host::runtime::Builder,
        path: &'static str,
        read: fn(&T) -> Dynamic,
    ) {
        builder
            .bind(path, move |args| {
                let [value] = args else {
                    return Err(arity(path, 1, args.len()));
                };
                let value = value
                    .read_lock::<T>()
                    .ok_or_else(|| runtime_error(format!("`{path}` read a value of another type")))?;
                Ok(read(&value))
            })
            .expect("the iced facade must retain every keyboard reader");
    }
    read::<Event>(builder, "iced::keyboard::Event::KeyPressed", |event| {
        Dynamic::from(matches!(event, Event::KeyPressed { .. }))
    });
    read::<Event>(builder, "iced::keyboard::Event::KeyReleased", |event| {
        Dynamic::from(matches!(event, Event::KeyReleased { .. }))
    });
    read::<Event>(builder, "iced::keyboard::Event::ModifiersChanged", |event| {
        Dynamic::from(matches!(event, Event::ModifiersChanged(_)))
    });
    read::<Event>(builder, "iced::keyboard::Event::key", |event| match event {
        Event::KeyPressed { key, .. } | Event::KeyReleased { key, .. } => Dynamic::from(key.clone()),
        Event::ModifiersChanged(_) => Dynamic::from(<Key>::Unidentified),
    });
    read::<Event>(builder, "iced::keyboard::Event::modified_key", |event| match event {
        Event::KeyPressed { modified_key, .. } | Event::KeyReleased { modified_key, .. } => {
            Dynamic::from(modified_key.clone())
        }
        Event::ModifiersChanged(_) => Dynamic::from(<Key>::Unidentified),
    });
    read::<Event>(builder, "iced::keyboard::Event::modifiers", |event| match event {
        Event::KeyPressed { modifiers, .. }
        | Event::KeyReleased { modifiers, .. }
        | Event::ModifiersChanged(modifiers) => Dynamic::from(*modifiers),
    });
    read::<Event>(builder, "iced::keyboard::Event::repeat", |event| {
        Dynamic::from(matches!(event, Event::KeyPressed { repeat: true, .. }))
    });
    read::<Key>(builder, "iced::keyboard::Key::as_ref", |key| Dynamic::from(key.clone()));
    for (name, read_modifier) in [
        ("shift", Modifiers::shift as fn(Modifiers) -> bool),
        ("control", Modifiers::control),
        ("alt", Modifiers::alt),
        ("logo", Modifiers::logo),
        ("command", Modifiers::command),
        ("macos_command", Modifiers::macos_command),
        ("jump", Modifiers::jump),
    ] {
        let path = format!("iced::keyboard::Modifiers::{name}");
        builder
            .bind(path.clone(), move |args| {
                let [value] = args else {
                    return Err(arity(&path, 1, args.len()));
                };
                let modifiers = value
                    .read_lock::<Modifiers>()
                    .map(|modifiers| *modifiers)
                    .ok_or_else(|| runtime_error(format!("`{path}` read a value of another type")))?;
                Ok(Dynamic::from(read_modifier(modifiers)))
            })
            .expect("the iced facade must retain every Modifiers reader");
    }
    builder
        .bind("iced::keyboard::Key::Named", |args| {
            let named = take::<Named>(
                args.first_mut().ok_or_else(|| arity("Key::Named", 1, 0))?,
                "iced::keyboard::key::Named",
            )?;
            Ok(Dynamic::from(<Key>::Named(named)))
        })
        .expect("the iced facade must retain Key::Named");
    builder
        .bind("iced::keyboard::Key::Character", |args| {
            let character = super::builtin::exact_string(
                args.first().ok_or_else(|| arity("Key::Character", 1, 0))?,
                "Key::Character",
            )?;
            Ok(Dynamic::from(<Key>::Character(character.into())))
        })
        .expect("the iced facade must retain Key::Character");
    builder
        .bind("iced::keyboard::Key::Unidentified", |_| Ok(Dynamic::from(<Key>::Unidentified)))
        .expect("the iced facade must retain Key::Unidentified");
    for (name, named) in crate::KEY_NAMES.into_iter().zip([
        Named::Alt,
        Named::Control,
        Named::Shift,
        Named::Super,
        Named::Meta,
        Named::Enter,
        Named::Tab,
        Named::Space,
        Named::ArrowDown,
        Named::ArrowLeft,
        Named::ArrowRight,
        Named::ArrowUp,
        Named::End,
        Named::Home,
        Named::PageDown,
        Named::PageUp,
        Named::Backspace,
        Named::Delete,
        Named::Insert,
        Named::Escape,
        Named::CapsLock,
        Named::F1,
        Named::F2,
        Named::F3,
        Named::F4,
        Named::F5,
        Named::F6,
        Named::F7,
        Named::F8,
        Named::F9,
        Named::F10,
        Named::F11,
        Named::F12,
        Named::Copy,
        Named::Cut,
        Named::Paste,
        Named::Undo,
        Named::Redo,
        Named::ContextMenu,
        Named::PrintScreen,
    ]) {
        builder
            .bind(format!("iced::keyboard::key::Named::{name}"), move |_| {
                Ok(Dynamic::from(named))
            })
            .expect("the iced facade must retain every key::Named");
    }
}

/// `widget::Id`, the `id` builders, `operation::Animation` and the
/// scrollable offsets.
fn bind_operations(builder: &mut host::runtime::Builder) {
    use iced::widget::scrollable::{AbsoluteOffset, RelativeOffset};

    builder
        .bind("iced::widget::Id::new", |args| {
            let name = super::builtin::exact_string(
                args.first().ok_or_else(|| arity("Id::new", 1, 0))?,
                "Id::new",
            )?;
            Ok(Dynamic::from(iced::widget::Id::from(name)))
        })
        .expect("the iced facade must retain Id::new");
    builder
        .bind("iced::widget::Id::unique", |_| Ok(Dynamic::from(iced::widget::Id::unique())))
        .expect("the iced facade must retain Id::unique");
    builder
        .bind("iced::widget::Container::id", |args| {
            if args.len() != 2 {
                return Err(arity("Container::id", 2, args.len()));
            }
            let mut container = take::<marker::Container>(args[0], "iced::widget::Container")?;
            container.id = Some(crate::task::take_id(args[1])?);
            Ok(Dynamic::from(container))
        })
        .expect("the iced facade must retain Container::id");
    builder
        .bind("iced::widget::Scrollable::id", |args| {
            if args.len() != 2 {
                return Err(arity("Scrollable::id", 2, args.len()));
            }
            let mut scrollable = take::<marker::Scrollable>(args[0], "iced::widget::Scrollable")?;
            scrollable.id = Some(crate::task::take_id(args[1])?);
            Ok(Dynamic::from(scrollable))
        })
        .expect("the iced facade must retain Scrollable::id");
    builder
        .bind("iced::widget::TextInput::id", |args| {
            if args.len() != 2 {
                return Err(arity("TextInput::id", 2, args.len()));
            }
            let mut input = take::<marker::TextInput>(args[0], "iced::widget::TextInput")?;
            input.id = Some(crate::task::take_id(args[1])?);
            Ok(Dynamic::from(input))
        })
        .expect("the iced facade must retain TextInput::id");
    for (name, animation) in crate::OPERATION_ANIMATIONS.into_iter().zip([
        iced::widget::operation::Animation::Auto,
        iced::widget::operation::Animation::Instant,
        iced::widget::operation::Animation::Smooth,
    ]) {
        builder
            .bind(format!("iced::widget::operation::Animation::{name}"), move |_| {
                Ok(Dynamic::from(animation))
            })
            .expect("the iced facade must retain every operation::Animation");
    }
    for (name, offset) in [("START", RelativeOffset::START), ("END", RelativeOffset::END)] {
        builder
            .bind(format!("iced::widget::scrollable::RelativeOffset::{name}"), move |_| {
                Ok(Dynamic::from(offset))
            })
            .expect("the iced facade must retain every RelativeOffset constant");
    }
    builder
        .bind("iced::widget::scrollable::RelativeOffset::default", |_| {
            Ok(Dynamic::from(RelativeOffset::<f32>::default()))
        })
        .expect("the iced facade must retain RelativeOffset::default");
    builder
        .bind("iced::widget::scrollable::AbsoluteOffset::default", |_| {
            Ok(Dynamic::from(AbsoluteOffset::<f32>::default()))
        })
        .expect("the iced facade must retain AbsoluteOffset::default");
    fn field<T: Clone + Send + Sync + 'static>(
        builder: &mut host::runtime::Builder,
        path: &'static str,
        read: fn(&T) -> f32,
        write: fn(&mut T, f32),
    ) {
        builder
            .bind(path, move |args| match args {
                [value] => {
                    let value = value.read_lock::<T>().ok_or_else(|| {
                        runtime_error(format!("`{path}` read a value of another type"))
                    })?;
                    Ok(Dynamic::from(f64::from(read(&value))))
                }
                [value, field] => {
                    let mut value = take::<T>(value, path)?;
                    write(&mut value, super::builtin::pixels(field, path)?);
                    Ok(Dynamic::from(value))
                }
                _ => Err(arity(path, 2, args.len())),
            })
            .expect("the iced facade must retain every offset field");
    }
    field::<RelativeOffset>(
        builder,
        "iced::widget::scrollable::RelativeOffset::x",
        |offset| offset.x,
        |offset, x| offset.x = x,
    );
    field::<RelativeOffset>(
        builder,
        "iced::widget::scrollable::RelativeOffset::y",
        |offset| offset.y,
        |offset, y| offset.y = y,
    );
    field::<AbsoluteOffset>(
        builder,
        "iced::widget::scrollable::AbsoluteOffset::x",
        |offset| offset.x,
        |offset, x| offset.x = x,
    );
    field::<AbsoluteOffset>(
        builder,
        "iced::widget::scrollable::AbsoluteOffset::y",
        |offset| offset.y,
        |offset, y| offset.y = y,
    );
}

fn bind_alignment(builder: &mut host::runtime::Builder) {
    for (name, alignment) in [
        ("Top", iced::Alignment::Start),
        ("Center", iced::Alignment::Center),
        ("Bottom", iced::Alignment::End),
    ] {
        builder
            .bind(format!("iced::alignment::Vertical::{name}"), move |_| {
                Ok(Dynamic::from(marker::Vertical(alignment)))
            })
            .expect("the iced facade must retain every alignment::Vertical value");
    }
    for (name, alignment) in [("Left", iced::Alignment::Start), ("Right", iced::Alignment::End)] {
        builder
            .bind(format!("iced::{name}"), move |_| {
                Ok(Dynamic::from(marker::Horizontal(alignment)))
            })
            .expect("the iced facade must retain every horizontal shorthand");
    }
    for (name, alignment) in [("Top", iced::Alignment::Start), ("Bottom", iced::Alignment::End)] {
        builder
            .bind(format!("iced::{name}"), move |_| {
                Ok(Dynamic::from(marker::Vertical(alignment)))
            })
            .expect("the iced facade must retain every vertical shorthand");
    }
}

/// `left(content)` and kin: a container filling one axis, its content
/// aligned to that side, as iced's `container(content).align_right(Fill)`.
fn bind_containers(builder: &mut host::runtime::Builder) {
    for (name, x, y) in [
        ("left", Some(iced::Alignment::Start), None),
        ("right", Some(iced::Alignment::End), None),
        ("top", None, Some(iced::Alignment::Start)),
        ("bottom", None, Some(iced::Alignment::End)),
    ] {
        builder
            .bind_with_context(format!("iced::widget::{name}"), move |call, args| {
                if args.len() != 1 {
                    return Err(arity(name, 1, args.len()));
                }
                let mut container = marker::Container {
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
                };
                if let Some(x) = x {
                    container.width = iced::Length::Fill;
                    container.align_x = x;
                }
                if let Some(y) = y {
                    container.height = iced::Length::Fill;
                    container.align_y = y;
                }
                Ok(Dynamic::from(container))
            })
            .expect("the iced facade must retain every side container");
    }
    type Set = fn(&mut marker::Container, iced::Length);
    for (name, set) in [
        (
            "center",
            (|container, length| {
                container.width = length;
                container.height = length;
                container.align_x = iced::Alignment::Center;
                container.align_y = iced::Alignment::Center;
            }) as Set,
        ),
        ("align_left", |container, length| {
            container.width = length;
            container.align_x = iced::Alignment::Start;
        }),
        ("align_right", |container, length| {
            container.width = length;
            container.align_x = iced::Alignment::End;
        }),
        ("align_top", |container, length| {
            container.height = length;
            container.align_y = iced::Alignment::Start;
        }),
        ("align_bottom", |container, length| {
            container.height = length;
            container.align_y = iced::Alignment::End;
        }),
    ] {
        let path = format!("iced::widget::Container::{name}");
        builder
            .bind(path.clone(), move |args| {
                let mut container = take_container(args, &path)?;
                set(&mut container, length(args[1])?);
                Ok(Dynamic::from(container))
            })
            .expect("the iced facade must retain every Container side builder");
    }
}

impl widget::Descriptor for marker::Native {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        (self.lower)(context)
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

/// An Element lowered by `lower`, as a facade function's output.
pub(super) fn native(
    call: &host::runtime::Call<'_>,
    lower: impl Fn(&widget::Context<'_>) -> iced::Element<'static, runtime::Message>
        + Send
        + Sync
        + 'static,
) -> Result<Dynamic, Box<EvalAltResult>> {
    let native = marker::Native {
        identity: widget::Identity::unique(),
        lower: std::sync::Arc::new(lower),
    };
    widget::Value::from_factory(native, call)
        .map(Dynamic::from)
        .map_err(witness_error)
}

/// `rule::horizontal(height)` and `rule::vertical(width)`.
fn bind_rule(builder: &mut host::runtime::Builder) {
    for (name, horizontal) in [("horizontal", true), ("vertical", false)] {
        builder
            .bind_with_context(format!("iced::widget::rule::{name}"), move |call, args| {
                if args.len() != 1 {
                    return Err(arity(name, 1, args.len()));
                }
                let thickness = super::builtin::pixels(args[0], name)?;
                native(call, move |_| {
                    if horizontal {
                        iced::widget::rule::horizontal(thickness).boxed()
                    } else {
                        iced::widget::rule::vertical(thickness).boxed()
                    }
                })
            })
            .expect("the iced facade must retain every rule");
    }
}

impl widget::Descriptor for marker::Stack {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        // Unset, iced's stack takes the size of its first child.
        let mut stack =
            iced::widget::Stack::with_children(self.children.iter().map(|child| child.lower(context)));
        if let Some(width) = self.width {
            stack = stack.width(width);
        }
        if let Some(height) = self.height {
            stack = stack.height(height);
        }
        stack.boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl Witnessed for marker::Stack {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

fn new_stack(
    call: &host::runtime::Call<'_>,
    children: Vec<widget::Value>,
) -> Result<Dynamic, Box<EvalAltResult>> {
    if children.iter().any(|child| child.key().is_some()) {
        return Err(positional_row());
    }
    Ok(Dynamic::from(marker::Stack {
        witness: call.output_witness().map_err(witness_error)?,
        identity: widget::Identity::unique(),
        children,
        width: None,
        height: None,
    }))
}

fn bind_stack(builder: &mut host::runtime::Builder) {
    builder
        .bind_type_attestation("iced::widget::Stack", |value, expected| {
            let value = value.read_lock::<marker::Stack>()?;
            (expected.argument_count() == 1)
                .then(|| expected.attest_witness(Some(&value.witness)))
                .flatten()
        })
        .expect("the iced facade must declare generic widget::Stack exactly once");
    builder
        .bind_projection_with_context("iced::widget::Stack", "iced::Element", |call, args| {
            project::<marker::Stack>(call, args, "iced::widget::Stack")
        })
        .expect("the iced facade must retain the exact Stack to Element projection");
    builder
        .bind_macro_with_context("iced::widget::stack", |call, args| {
            let children = args
                .iter_mut()
                .map(|value| take::<widget::Value>(value, "iced::Element"))
                .collect::<Result<Vec<_>, _>>()?;
            new_stack(call, children)
        })
        .expect("the iced facade must retain the exact stack macro");
    for path in [
        "iced::widget::stack",
        "iced::widget::Stack::with_children",
        "iced::widget::Stack::from_vec",
    ] {
        builder
            .bind_with_context(path, |call, args| {
                if args.len() != 1 {
                    return Err(arity("stack", 1, args.len()));
                }
                new_stack(call, take_children(args[0])?)
            })
            .expect("the iced facade must retain the exact stack functions");
    }
    builder
        .bind_with_context("iced::widget::Stack::new", |call, _| new_stack(call, Vec::new()))
        .expect("the iced facade must retain the exact Stack::new item");
    for (name, height) in [("width", false), ("height", true)] {
        let path = format!("iced::widget::Stack::{name}");
        builder
            .bind(path.clone(), move |args| {
                if args.len() != 2 {
                    return Err(arity(&path, 2, args.len()));
                }
                let mut stack = take::<marker::Stack>(args[0], "iced::widget::Stack")?;
                let value = length(args[1])?;
                if height {
                    stack.height = Some(value);
                } else {
                    stack.width = Some(value);
                }
                Ok(Dynamic::from(stack))
            })
            .expect("the iced facade must retain every Stack sizing item");
    }
    builder
        .bind("iced::widget::Stack::push", |args| {
            if args.len() != 2 {
                return Err(arity("Stack::push", 2, args.len()));
            }
            let mut stack = take::<marker::Stack>(args[0], "iced::widget::Stack")?;
            let child = take::<widget::Value>(args[1], "iced::Element")?;
            if child.key().is_some() {
                return Err(positional_row());
            }
            stack.children.push(child);
            Ok(Dynamic::from(stack))
        })
        .expect("the iced facade must retain the exact Stack::push item");
    builder
        .bind("iced::widget::Stack::extend", |args| {
            if args.len() != 2 {
                return Err(arity("Stack::extend", 2, args.len()));
            }
            let mut stack = take::<marker::Stack>(args[0], "iced::widget::Stack")?;
            let children = take_children(args[1])?;
            if children.iter().any(|child| child.key().is_some()) {
                return Err(positional_row());
            }
            stack.children.extend(children);
            Ok(Dynamic::from(stack))
        })
        .expect("the iced facade must retain the exact Stack::extend item");
}

/// `time::hours`/`minutes` and `color!(0xrrggbb)` / `color!(r, g, b)`.
fn bind_values(builder: &mut host::runtime::Builder) {
    for (name, seconds) in [("hours", 3600_u64), ("minutes", 60)] {
        builder
            .bind(format!("iced::time::{name}"), move |args| {
                let amount = args
                    .first()
                    .and_then(|value| value.as_int().ok())
                    .and_then(|value| u64::try_from(value).ok())
                    .ok_or_else(|| runtime_error(format!("time::{name} expects a `u64`")))?;
                Ok(Dynamic::from(std::time::Duration::from_secs(
                    amount.saturating_mul(seconds),
                )))
            })
            .expect("the iced facade must retain every time unit");
    }
    use std::time::Duration;
    for (name, unit) in [
        ("from_secs", Duration::from_secs as fn(u64) -> Duration),
        ("from_millis", Duration::from_millis),
        ("from_micros", Duration::from_micros),
        ("from_nanos", Duration::from_nanos),
    ] {
        builder
            .bind(format!("iced::time::Duration::{name}"), move |args| {
                let amount = args
                    .first()
                    .and_then(|value| value.as_int().ok())
                    .and_then(|value| u64::try_from(value).ok())
                    .ok_or_else(|| runtime_error(format!("Duration::{name} expects a `u64`")))?;
                Ok(Dynamic::from(unit(amount)))
            })
            .expect("the iced facade must retain every Duration constructor");
    }
    for name in ["from_secs_f64", "from_secs_f32"] {
        builder
            .bind(format!("iced::time::Duration::{name}"), move |args| {
                let seconds = args
                    .first()
                    .and_then(|value| value.as_float().ok())
                    .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
                    .ok_or_else(|| {
                        runtime_error(format!("Duration::{name} expects a finite, nonnegative float"))
                    })?;
                Ok(Dynamic::from(Duration::from_secs_f64(seconds)))
            })
            .expect("the iced facade must retain every Duration constructor");
    }
    for (name, read) in [
        ("as_secs", (|d: &Duration| Dynamic::from(d.as_secs() as i64)) as fn(&Duration) -> Dynamic),
        ("as_millis", |d| Dynamic::from(d.as_millis() as i64)),
        ("as_micros", |d| Dynamic::from(d.as_micros() as i64)),
        ("as_nanos", |d| Dynamic::from(d.as_nanos() as i64)),
        ("as_secs_f64", |d| Dynamic::from(d.as_secs_f64())),
        ("as_secs_f32", |d| Dynamic::from(d.as_secs_f64())),
    ] {
        let path = format!("iced::time::Duration::{name}");
        builder
            .bind(path.clone(), move |args| {
                let [value] = args else {
                    return Err(arity(&path, 1, args.len()));
                };
                let duration = value
                    .read_lock::<Duration>()
                    .ok_or_else(|| runtime_error(format!("`{path}` read a value of another type")))?;
                Ok(read(&duration))
            })
            .expect("the iced facade must retain every Duration reader");
    }
    // `Duration`'s operators and sub-second readers, and `Instant`'s.
    fn pair<A: Clone + Send + Sync + 'static, B: Clone + Send + Sync + 'static>(
        path: &str,
        args: &mut [&mut Dynamic],
    ) -> Result<Option<(A, B)>, Box<EvalAltResult>> {
        let [lhs, rhs] = args else {
            return Err(arity(path, 2, args.len()));
        };
        Ok(lhs
            .read_lock::<A>()
            .map(|lhs| lhs.clone())
            .zip(rhs.read_lock::<B>().map(|rhs| rhs.clone())))
    }
    for (name, apply) in [
        ("add", Duration::checked_add as fn(Duration, Duration) -> Option<Duration>),
        ("sub", Duration::checked_sub),
    ] {
        let path = format!("iced::time::Duration::{name}");
        builder
            .bind(path.clone(), move |args| {
                let (lhs, rhs) = pair::<Duration, Duration>(&path, args)?
                    .ok_or_else(|| runtime_error(format!("`{path}` read a value of another type")))?;
                apply(lhs, rhs).map(Dynamic::from).ok_or_else(|| {
                    runtime_error(format!("overflow when computing `Duration::{name}`"))
                })
            })
            .expect("the iced facade must retain every Duration operator");
    }
    for (name, apply) in [
        ("mul", Duration::checked_mul as fn(Duration, u32) -> Option<Duration>),
        ("div", Duration::checked_div),
    ] {
        let path = format!("iced::time::Duration::{name}");
        builder
            .bind(path.clone(), move |args| {
                let [lhs, rhs] = args else {
                    return Err(arity(&path, 2, args.len()));
                };
                let duration = lhs
                    .read_lock::<Duration>()
                    .map(|duration| *duration)
                    .ok_or_else(|| runtime_error(format!("`{path}` read a value of another type")))?;
                let factor = rhs
                    .as_int()
                    .ok()
                    .and_then(|factor| u32::try_from(factor).ok())
                    .ok_or_else(|| runtime_error(format!("`Duration::{name}` expects a `u32`")))?;
                apply(duration, factor).map(Dynamic::from).ok_or_else(|| {
                    runtime_error(format!("overflow or division by zero in `Duration::{name}`"))
                })
            })
            .expect("the iced facade must retain every Duration operator");
    }
    for (name, read) in [
        ("subsec_millis", (|d: &Duration| Dynamic::from(i64::from(d.subsec_millis()))) as fn(&Duration) -> Dynamic),
        ("subsec_micros", |d| Dynamic::from(i64::from(d.subsec_micros()))),
        ("subsec_nanos", |d| Dynamic::from(i64::from(d.subsec_nanos()))),
        ("is_zero", |d| Dynamic::from(d.is_zero())),
    ] {
        let path = format!("iced::time::Duration::{name}");
        builder
            .bind(path.clone(), move |args| {
                let [value] = args else {
                    return Err(arity(&path, 1, args.len()));
                };
                let duration = value
                    .read_lock::<Duration>()
                    .ok_or_else(|| runtime_error(format!("`{path}` read a value of another type")))?;
                Ok(read(&duration))
            })
            .expect("the iced facade must retain every Duration reader");
    }
    builder
        .bind("iced::time::Duration::ZERO", |_| Ok(Dynamic::from(Duration::ZERO)))
        .expect("the iced facade must retain Duration::ZERO");
    builder
        .bind("iced::time::Instant::now", |_| Ok(Dynamic::from(marker::Instant::now())))
        .expect("the iced facade must retain Instant::now");
    builder
        .bind("iced::time::Instant::elapsed", |args| {
            let [value] = args else {
                return Err(arity("Instant::elapsed", 1, args.len()));
            };
            let instant = value
                .read_lock::<marker::Instant>()
                .map(|instant| instant.0)
                .ok_or_else(|| runtime_error("`Instant::elapsed` read a value of another type".to_string()))?;
            Ok(Dynamic::from(marker::Instant::now().0.saturating_duration_since(instant)))
        })
        .expect("the iced facade must retain Instant::elapsed");
    builder
        .bind("iced::time::Instant::duration_since", |args| {
            let (lhs, rhs) = pair::<marker::Instant, marker::Instant>("Instant::duration_since", args)?
                .ok_or_else(|| runtime_error("`Instant::duration_since` read a value of another type".to_string()))?;
            Ok(Dynamic::from(lhs.0.saturating_duration_since(rhs.0)))
        })
        .expect("the iced facade must retain Instant::duration_since");
    // `a - b` of two instants is a `Duration`; `instant ± duration` an
    // `Instant`.
    builder
        .bind("iced::time::Instant::sub", |args| {
            if let Some((lhs, rhs)) = pair::<marker::Instant, marker::Instant>("Instant::sub", args)? {
                return Ok(Dynamic::from(lhs.0.saturating_duration_since(rhs.0)));
            }
            let (lhs, rhs) = pair::<marker::Instant, Duration>("Instant::sub", args)?
                .ok_or_else(|| runtime_error("`Instant::sub` read a value of another type".to_string()))?;
            lhs.0
                .checked_sub(rhs)
                .map(|instant| Dynamic::from(marker::Instant(instant)))
                .ok_or_else(|| runtime_error("overflow when subtracting a duration from an instant".to_string()))
        })
        .expect("the iced facade must retain Instant::sub");
    builder
        .bind("iced::time::Instant::add", |args| {
            let (lhs, rhs) = pair::<marker::Instant, Duration>("Instant::add", args)?
                .ok_or_else(|| runtime_error("`Instant::add` read a value of another type".to_string()))?;
            lhs.0
                .checked_add(rhs)
                .map(|instant| Dynamic::from(marker::Instant(instant)))
                .ok_or_else(|| runtime_error("overflow when adding a duration to an instant".to_string()))
        })
        .expect("the iced facade must retain Instant::add");
    builder
        .bind_macro("iced::color", |args| {
            let channels = args
                .iter()
                .map(|value| value.as_int())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|actual| {
                    runtime_error(format!("color! expects integers, found `{actual}`"))
                })?;
            let channel = |value: i64| {
                u8::try_from(value)
                    .map_err(|_| runtime_error(format!("color! channel {value} is not a `u8`")))
            };
            match channels.as_slice() {
                [hex] => {
                    let hex = u32::try_from(*hex).ok().filter(|hex| *hex <= 0x00ff_ffff).ok_or_else(
                        || runtime_error(format!("color! expects `0xrrggbb`, found {hex:#x}")),
                    )?;
                    Ok(Dynamic::from(iced::Color::from_rgb8(
                        (hex >> 16) as u8,
                        (hex >> 8) as u8,
                        hex as u8,
                    )))
                }
                [r, g, b] => Ok(Dynamic::from(iced::Color::from_rgb8(
                    channel(*r)?,
                    channel(*g)?,
                    channel(*b)?,
                ))),
                _ => Err(runtime_error(format!(
                    "color! takes `0xrrggbb` or `r, g, b`, found {} values",
                    channels.len()
                ))),
            }
        })
        .expect("the iced facade must retain the exact color macro");
}

fn bind_fields(builder: &mut host::runtime::Builder) {
    fn reader<T: Clone + 'static>(
        builder: &mut host::runtime::Builder,
        path: &'static str,
        read: fn(&T) -> f32,
    ) {
        builder
            .bind(path, move |args| {
                let [value] = args else {
                    return Err(arity(path, 1, args.len()));
                };
                let value = value
                    .read_lock::<T>()
                    .ok_or_else(|| runtime_error(format!("`{path}` read a value of another type")))?;
                Ok(Dynamic::from(f64::from(read(&value))))
            })
            .expect("the iced facade must retain every field reader");
    }
    // `size.width`, and `Size { width: Length::Fill, .. }`, which makes
    // iced's `Size<Length>`.
    for name in ["width", "height"] {
        let path = format!("iced::Size::{name}");
        builder
            .bind(path.clone(), move |args| match args {
                [size] => {
                    let size = size
                        .read_lock::<iced::Size>()
                        .ok_or_else(|| runtime_error(format!("`{path}` read a value of another type")))?;
                    Ok(Dynamic::from(f64::from(if name == "width" { size.width } else { size.height })))
                }
                [size, number] if number.as_float().is_ok() || number.as_int().is_ok() => {
                    let mut size = super::builtin::take::<iced::Size>(size, "iced::Size")?;
                    let number = super::builtin::pixels(number, &path)?;
                    if name == "width" {
                        size.width = number;
                    } else {
                        size.height = number;
                    }
                    Ok(Dynamic::from(size))
                }
                [_, length] => {
                    let length = super::builtin::length(length)?;
                    let mut size = iced::Size::new(iced::Length::Shrink, iced::Length::Shrink);
                    if name == "width" {
                        size.width = length;
                    } else {
                        size.height = length;
                    }
                    Ok(Dynamic::from(super::custom::LengthSize(size)))
                }
                _ => Err(arity(&path, 1, args.len())),
            })
            .expect("the iced facade must retain every Size field");
    }
    // `point.x`, read, or set as a place (`offset.x = 2.0`).
    fn coordinate<T: Clone + Send + Sync + 'static>(
        builder: &mut host::runtime::Builder,
        path: &'static str,
        field: fn(&mut T) -> &mut f32,
    ) {
        builder
            .bind(path, move |args| match args {
                [value] => {
                    let mut value = value
                        .read_lock::<T>()
                        .ok_or_else(|| runtime_error(format!("`{path}` read a value of another type")))?
                        .clone();
                    Ok(Dynamic::from(f64::from(*field(&mut value))))
                }
                [value, number] => {
                    let mut value = super::builtin::take::<T>(value, path)?;
                    *field(&mut value) = super::builtin::pixels(number, path)?;
                    Ok(Dynamic::from(value))
                }
                _ => Err(arity(path, 1, args.len())),
            })
            .expect("the iced facade must retain every coordinate");
    }
    coordinate::<iced::Point>(builder, "iced::Point::x", |point| &mut point.x);
    coordinate::<iced::Point>(builder, "iced::Point::y", |point| &mut point.y);
    coordinate::<iced::Vector>(builder, "iced::Vector::x", |vector| &mut vector.x);
    coordinate::<iced::Vector>(builder, "iced::Vector::y", |vector| &mut vector.y);
    reader::<iced::Rectangle>(builder, "iced::Rectangle::x", |bounds| bounds.x);
    reader::<iced::Rectangle>(builder, "iced::Rectangle::y", |bounds| bounds.y);
    reader::<iced::Rectangle>(builder, "iced::Rectangle::width", |bounds| bounds.width);
    reader::<iced::Rectangle>(builder, "iced::Rectangle::height", |bounds| bounds.height);
    // A channel read (`color.a`), or set by a literal (`Color { a: 0.8, ..base }`).
    fn channel<'a>(color: &'a mut iced::Color, name: &str) -> &'a mut f32 {
        match name {
            "r" => &mut color.r,
            "g" => &mut color.g,
            "b" => &mut color.b,
            _ => &mut color.a,
        }
    }
    for name in ["r", "g", "b", "a"] {
        let path = format!("iced::Color::{name}");
        builder
            .bind(path.clone(), move |args| {
                let mut color = args
                    .first()
                    .and_then(|color| color.read_lock::<iced::Color>().map(|color| *color))
                    .ok_or_else(|| runtime_error(format!("`{path}` read a value of another type")))?;
                match args {
                    [_] => Ok(Dynamic::from(f64::from(*channel(&mut color, name)))),
                    [_, value] => {
                        let value = value.as_float().map_err(|actual| {
                            runtime_error(format!("`{path}` expects a float, found `{actual}`"))
                        })?;
                        *channel(&mut color, name) = value as f32;
                        Ok(Dynamic::from(color))
                    }
                    _ => Err(arity(&path, 2, args.len())),
                }
            })
            .expect("the iced facade must retain every Color channel");
    }
    builder
        .bind("iced::Color::scale_alpha", |args| {
            let [color, factor] = args else {
                return Err(arity("Color::scale_alpha", 2, args.len()));
            };
            let color = color
                .read_lock::<iced::Color>()
                .map(|color| *color)
                .ok_or_else(|| runtime_error("`Color::scale_alpha` read a value of another type".into()))?;
            let factor = factor
                .as_float()
                .map_err(|actual| runtime_error(format!("`Color::scale_alpha` expects a float, found `{actual}`")))?;
            Ok(Dynamic::from(color.scale_alpha(factor as f32)))
        })
        .expect("the iced facade must retain Color::scale_alpha");
    builder
        .bind("iced::Color::inverse", |args| {
            let [color] = args else {
                return Err(arity("Color::inverse", 1, args.len()));
            };
            let color = color
                .read_lock::<iced::Color>()
                .map(|color| *color)
                .ok_or_else(|| runtime_error("`Color::inverse` read a value of another type".into()))?;
            Ok(Dynamic::from(color.inverse()))
        })
        .expect("the iced facade must retain Color::inverse");
    builder
        .bind("iced::time::Duration::default", |args| {
            if !args.is_empty() {
                return Err(arity("Duration::default", 0, args.len()));
            }
            Ok(Dynamic::from(std::time::Duration::default()))
        })
        .expect("the iced facade must retain Duration::default");
}

fn bind_font(builder: &mut host::runtime::Builder) {
    use iced::font::{Family, Stretch, Style, Weight};

    for (name, value) in crate::FONT_WEIGHTS.into_iter().zip([
        Weight::Thin,
        Weight::ExtraLight,
        Weight::Light,
        Weight::Normal,
        Weight::Medium,
        Weight::Semibold,
        Weight::Bold,
        Weight::ExtraBold,
        Weight::Black,
    ]) {
        builder
            .bind(format!("iced::font::Weight::{name}"), move |_| Ok(Dynamic::from(value)))
            .expect("the iced facade must retain every font::Weight");
    }
    for (name, value) in crate::FONT_STRETCHES.into_iter().zip([
        Stretch::UltraCondensed,
        Stretch::ExtraCondensed,
        Stretch::Condensed,
        Stretch::SemiCondensed,
        Stretch::Normal,
        Stretch::SemiExpanded,
        Stretch::Expanded,
        Stretch::ExtraExpanded,
        Stretch::UltraExpanded,
    ]) {
        builder
            .bind(format!("iced::font::Stretch::{name}"), move |_| Ok(Dynamic::from(value)))
            .expect("the iced facade must retain every font::Stretch");
    }
    for (name, value) in crate::FONT_STYLES.into_iter().zip([
        Style::Normal,
        Style::Italic,
        Style::Oblique,
    ]) {
        builder
            .bind(format!("iced::font::Style::{name}"), move |_| Ok(Dynamic::from(value)))
            .expect("the iced facade must retain every font::Style");
    }
    for (name, value) in crate::FONT_FAMILIES.into_iter().zip([
        Family::Serif,
        Family::SansSerif,
        Family::Cursive,
        Family::Fantasy,
        Family::Monospace,
    ]) {
        builder
            .bind(format!("iced::font::Family::{name}"), move |_| Ok(Dynamic::from(value)))
            .expect("the iced facade must retain every font::Family");
    }
    builder
        .bind("iced::font::Family::VARIANTS", |_| {
            Ok(Dynamic::from_array(
                Family::VARIANTS.iter().copied().map(Dynamic::from).collect(),
            ))
        })
        .expect("the iced facade must retain font::Family::VARIANTS");
    builder
        .bind("iced::font::Family::name", |args| {
            let name = super::builtin::exact_string(
                args.first().ok_or_else(|| arity("Family::name", 1, 0))?,
                "Family::name",
            )?;
            Ok(Dynamic::from(Family::name(&name)))
        })
        .expect("the iced facade must retain font::Family::name");
    builder
        .bind("iced::Font::new", |args| {
            let name = super::builtin::exact_string(
                args.first().ok_or_else(|| arity("Font::new", 1, 0))?,
                "Font::new",
            )?;
            Ok(Dynamic::from(iced::Font::with_family(Family::name(&name))))
        })
        .expect("the iced facade must retain Font::new");
    builder
        .bind("iced::Font::with_family", |args| {
            let family = take::<Family>(
                args.first_mut().ok_or_else(|| arity("Font::with_family", 1, 0))?,
                "iced::font::Family",
            )?;
            Ok(Dynamic::from(iced::Font::with_family(family)))
        })
        .expect("the iced facade must retain Font::with_family");
    builder
        .bind("iced::Font::weight", |args| {
            if let [font] = args {
                return read_font(font, |font| Dynamic::from(font.weight));
            }
            let (font, value) = font_and::<Weight>(args, "Font::weight", "iced::font::Weight")?;
            Ok(Dynamic::from(font.weight(value)))
        })
        .expect("the iced facade must retain Font::weight");
    builder
        .bind("iced::Font::stretch", |args| {
            if let [font] = args {
                return read_font(font, |font| Dynamic::from(font.stretch));
            }
            let (font, value) = font_and::<Stretch>(args, "Font::stretch", "iced::font::Stretch")?;
            Ok(Dynamic::from(font.stretch(value)))
        })
        .expect("the iced facade must retain Font::stretch");
    builder
        .bind("iced::Font::style", |args| {
            if let [font] = args {
                return read_font(font, |font| Dynamic::from(font.style));
            }
            let (font, value) = font_and::<Style>(args, "Font::style", "iced::font::Style")?;
            Ok(Dynamic::from(font.style(value)))
        })
        .expect("the iced facade must retain Font::style");
    builder
        .bind("iced::Font::family", |args| {
            if let [font] = args {
                return read_font(font, |font| Dynamic::from(font.family));
            }
            let (mut font, value) = font_and::<Family>(args, "Font::family", "iced::font::Family")?;
            font.family = value;
            Ok(Dynamic::from(font))
        })
        .expect("the iced facade must retain Font::family");
}

fn bind_displays(builder: &mut host::runtime::Builder) {
    fn display<T: std::fmt::Display + Clone + Send + Sync + 'static>(
        builder: &mut host::runtime::Builder,
        path: &'static str,
    ) {
        builder
            .bind(path, move |args| {
                let [value] = args else {
                    return Err(arity(path, 1, args.len()));
                };
                value
                    .read_lock::<T>()
                    .map(|value| Dynamic::from(value.to_string()))
                    .ok_or_else(|| runtime_error(format!("`{path}` read a value of another type")))
            })
            .expect("the iced facade must retain every to_string");
    }
    display::<iced::font::Family>(builder, "iced::font::Family::to_string");
    display::<iced::ContentFit>(builder, "iced::ContentFit::to_string");
}

/// `font.weight` and kin, read as fields.
fn read_font(
    font: &Dynamic,
    read: fn(&iced::Font) -> Dynamic,
) -> Result<Dynamic, Box<EvalAltResult>> {
    font.read_lock::<iced::Font>()
        .map(|font| read(&font))
        .ok_or_else(|| runtime_error("expected an `iced::Font`".into()))
}

fn font_and<T: Clone + 'static>(
    args: &mut [&mut Dynamic],
    operation: &str,
    expected: &str,
) -> Result<(iced::Font, T), Box<EvalAltResult>> {
    if args.len() != 2 {
        return Err(arity(operation, 2, args.len()));
    }
    let font = take::<iced::Font>(args[0], "iced::Font")?;
    let value = take::<T>(args[1], expected)?;
    Ok((font, value))
}

/// The Element `function` returns for `arguments`, lowered; a diagnostic
/// when it fails.
fn invoke_view(
    context: &widget::Context<'_>,
    function: &rawr::item::Value,
    arguments: &mut [Dynamic],
    widget: &str,
) -> iced::Element<'static, runtime::Message> {
    match context.invoke(function, arguments) {
        None => iced::widget::space().boxed(),
        Some(Ok(view)) => match into_element(view) {
            Ok(view) => view.lower(context),
            Err(error) => {
                widget::diagnostic(format!("{widget} view failed: {error}")).lower(context)
            }
        },
        Some(Err(error)) => {
            widget::diagnostic(format!("{widget} view failed: {error}")).lower(context)
        }
    }
}

/// A view function's widget as an Element: a closure's body is the widget
/// itself (`column![..]`), which iced converts with `Into<Element>`.
fn into_element(view: Dynamic) -> Result<widget::Value, String> {
    macro_rules! widgets {
        ($($marker:ident),*) => {
            $(if view.is::<marker::$marker>() {
                return Ok(widget::Value::new(view.cast::<marker::$marker>()));
            })*
        };
    }
    if view.is::<widget::Value>() {
        return Ok(view.cast::<widget::Value>());
    }
    widgets!(
        Text, TextInput, Checkbox, Toggler, Slider, ProgressBar, Transition, Button, Column, Row,
        Container, Tooltip, Scrollable, Logo, Sensor, Space, Radio, PickList, ComboBox, Stack,
        Pin, MouseArea, Image, Svg, QrCode, Rich
    );
    Err(format!("expected a widget, found `{}`", view.type_name()))
}

fn take_function(
    value: &mut Dynamic,
    operation: &str,
) -> Result<rawr::item::Value, Box<EvalAltResult>> {
    let actual = value.type_name();
    value.take().try_cast_result::<rawr::item::Value>().map_err(|_| {
        runtime_error(format!(
            "{operation} received `{actual}`, expected an exact symbolic function"
        ))
    })
}

impl widget::Descriptor for marker::Pin {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        iced::widget::pin(self.child.lower(context))
            .position(self.position)
            .width(self.width)
            .height(self.height)
            .boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl Witnessed for marker::Pin {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

impl widget::Descriptor for marker::MouseArea {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        type Set = fn(
            iced::widget::MouseArea<'static, runtime::Message>,
            runtime::Message,
        ) -> iced::widget::MouseArea<'static, runtime::Message>;
        let setters: [Set; 9] = [
            |area, message| area.on_press(message),
            |area, message| area.on_release(message),
            |area, message| area.on_double_click(message),
            |area, message| area.on_right_press(message),
            |area, message| area.on_right_release(message),
            |area, message| area.on_middle_press(message),
            |area, message| area.on_middle_release(message),
            |area, message| area.on_enter(message),
            |area, message| area.on_exit(message),
        ];
        let mut area = iced::widget::mouse_area(self.child.lower(context));
        for (message, set) in self.messages.iter().zip(setters) {
            if let Some(payload) = message {
                area = set(area, context.message(payload.clone()));
            }
        }
        if let Some(mapper) = &self.on_move {
            let mapper = context.message_mapper(mapper.clone());
            area = area.on_move(move |point| mapper.map(Dynamic::from(point)));
        }
        if let Some(interaction) = self.interaction {
            area = area.interaction(interaction);
        }
        area.boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl Witnessed for marker::MouseArea {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

fn bind_wrappers(builder: &mut host::runtime::Builder) {
    builder
        .bind_with_context("iced::widget::lazy", |call, args| {
            if args.len() != 2 {
                return Err(arity("lazy", 2, args.len()));
            }
            let dependency = args[0].take();
            let view = take_function(args[1], "lazy")?;
            native(call, move |context| {
                invoke_view(context, &view, &mut [dependency.clone()], "lazy")
            })
        })
        .expect("the iced facade must retain lazy");
    builder
        .bind_with_context("iced::widget::hover", |call, args| {
            if args.len() != 2 {
                return Err(arity("hover", 2, args.len()));
            }
            let base = take::<widget::Value>(args[0], "iced::Element")?;
            let top = take::<widget::Value>(args[1], "iced::Element")?;
            native(call, move |context| {
                iced::widget::hover(base.lower(context), top.lower(context)).boxed()
            })
        })
        .expect("the iced facade must retain hover");
    builder
        .bind_with_context("iced::widget::opaque", |call, args| {
            if args.len() != 1 {
                return Err(arity("opaque", 1, args.len()));
            }
            let content = take::<widget::Value>(args[0], "iced::Element")?;
            native(call, move |context| iced::widget::opaque(content.lower(context)).boxed())
        })
        .expect("the iced facade must retain opaque");
    builder
        .bind_with_context("iced::widget::responsive", |call, args| {
            if args.len() != 1 {
                return Err(arity("responsive", 1, args.len()));
            }
            let view = take_function(args[0], "responsive")?;
            native(call, move |context| {
                let owned = context.owned();
                let view = view.clone();
                iced::widget::responsive(move |size| {
                    invoke_view(&owned.context(), &view, &mut [Dynamic::from(size)], "responsive")
                })
                .boxed()
            })
        })
        .expect("the iced facade must retain responsive");

    bind_widget_type::<marker::Pin>(builder, "iced::widget::Pin");
    builder
        .bind_with_context("iced::widget::pin", |call, args| {
            if args.len() != 1 {
                return Err(arity("pin", 1, args.len()));
            }
            Ok(Dynamic::from(marker::Pin {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                child: take::<widget::Value>(args[0], "iced::Element")?,
                position: iced::Point::ORIGIN,
                width: iced::Length::Fill,
                height: iced::Length::Fill,
            }))
        })
        .expect("the iced facade must retain pin");
    for name in ["x", "y", "position", "width", "height"] {
        let path = format!("iced::widget::Pin::{name}");
        builder
            .bind(path.clone(), move |args| {
                if args.len() != 2 {
                    return Err(arity(&path, 2, args.len()));
                }
                let mut pin = take::<marker::Pin>(args[0], "iced::widget::Pin")?;
                match name {
                    "x" => pin.position.x = super::builtin::pixels(args[1], &path)?,
                    "y" => pin.position.y = super::builtin::pixels(args[1], &path)?,
                    "position" => pin.position = take::<iced::Point>(args[1], "iced::Point")?,
                    "width" => pin.width = length(args[1])?,
                    _ => pin.height = length(args[1])?,
                }
                Ok(Dynamic::from(pin))
            })
            .expect("the iced facade must retain every Pin builder");
    }

    bind_widget_type::<marker::MouseArea>(builder, "iced::widget::MouseArea");
    builder
        .bind_with_context("iced::widget::mouse_area", |call, args| {
            if args.len() != 1 {
                return Err(arity("mouse_area", 1, args.len()));
            }
            Ok(Dynamic::from(marker::MouseArea {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                child: take::<widget::Value>(args[0], "iced::Element")?,
                messages: Default::default(),
                on_move: None,
                interaction: None,
            }))
        })
        .expect("the iced facade must retain mouse_area");
    for (index, name) in crate::MOUSE_AREA_MESSAGES.into_iter().enumerate() {
        let path = format!("iced::widget::MouseArea::{name}");
        builder
            .bind(path.clone(), move |args| {
                if args.len() != 2 {
                    return Err(arity(&path, 2, args.len()));
                }
                let mut area = take::<marker::MouseArea>(args[0], "iced::widget::MouseArea")?;
                area.messages[index] = Some(args[1].take());
                Ok(Dynamic::from(area))
            })
            .expect("the iced facade must retain every MouseArea message builder");
    }
    builder
        .bind("iced::widget::MouseArea::on_move", |args| {
            if args.len() != 2 {
                return Err(arity("MouseArea::on_move", 2, args.len()));
            }
            let mut area = take::<marker::MouseArea>(args[0], "iced::widget::MouseArea")?;
            area.on_move = Some(take_function(args[1], "MouseArea::on_move")?);
            Ok(Dynamic::from(area))
        })
        .expect("the iced facade must retain MouseArea::on_move");
    builder
        .bind("iced::widget::MouseArea::interaction", |args| {
            if args.len() != 2 {
                return Err(arity("MouseArea::interaction", 2, args.len()));
            }
            let mut area = take::<marker::MouseArea>(args[0], "iced::widget::MouseArea")?;
            area.interaction = Some(take::<iced::mouse::Interaction>(
                args[1],
                "iced::mouse::Interaction",
            )?);
            Ok(Dynamic::from(area))
        })
        .expect("the iced facade must retain MouseArea::interaction");
}

/// A generic widget type's attestation and its projection into Element.
pub(super) fn bind_widget_type<T>(builder: &mut host::runtime::Builder, path: &'static str)
where
    T: widget::Descriptor + Witnessed + Clone,
{
    builder
        .bind_type_attestation(path, |value, expected| {
            let value = value.read_lock::<T>()?;
            (expected.argument_count() == 1)
                .then(|| expected.attest_witness(Some(value.witness())))
                .flatten()
        })
        .expect("the iced facade must declare each generic widget type exactly once");
    builder
        .bind_projection_with_context(path, "iced::Element", move |call, args| {
            project::<T>(call, args, path)
        })
        .expect("the iced facade must retain each widget's projection into Element");
}
