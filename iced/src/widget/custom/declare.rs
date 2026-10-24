//! The facade of iced's `advanced` module that a script widget uses: the
//! `Widget` and `Meta` traits, `Tree`, `Layout`, `Limits`, `Shell`,
//! `renderer::{Quad, Style, Renderer}` and the mesh API.

use rawr::host::{self, facade, item, signature, type_ref};
use rawr::{capability, receiver};

use super::{LengthSize, MeshRendererTrait, MetaTrait, RendererTrait, Shell, State, Tree, WidgetTrait};

/// iced's `advanced::Widget` as script types implement it. `operate` and
/// `overlay` are not here yet. A method the impl leaves out runs iced's
/// default natively, so these default bodies only type-check.
const WIDGET: &str = "pub trait Widget<Message, Theme, Renderer> {
    fn size(&self) -> iced::Size<iced::Length>;
    fn tag(&self) -> iced::advanced::widget::tree::Tag { iced::advanced::widget::tree::Tag::stateless() }
    fn state(&self) -> iced::advanced::widget::tree::State { iced::advanced::widget::tree::State::None }
    fn diff(&mut self, tree: &mut iced::advanced::widget::Tree) {}
    fn layout(&mut self, tree: &mut iced::advanced::widget::Tree, renderer: &Renderer, limits: &iced::advanced::layout::Limits);
    fn draw(&self, tree: &iced::advanced::widget::Tree, renderer: &mut Renderer, theme: &Theme, style: &iced::advanced::renderer::Style, layout: iced::advanced::layout::Layout, cursor: iced::mouse::Cursor, viewport: &iced::Rectangle);
    fn update(&mut self, tree: &mut iced::advanced::widget::Tree, event: &iced::Event, layout: iced::advanced::layout::Layout, cursor: iced::mouse::Cursor, renderer: &Renderer, shell: &mut iced::advanced::Shell<'_, Message>, viewport: &iced::Rectangle) {}
    fn mouse_interaction(&self, tree: &iced::advanced::widget::Tree, layout: iced::advanced::layout::Layout, cursor: iced::mouse::Cursor, viewport: &iced::Rectangle, renderer: &Renderer) -> iced::mouse::Interaction { iced::mouse::Interaction::None }
}";

const META: &str = "pub trait Meta {
    fn is_void(&self) -> bool { false }
}";

/// iced's `advanced::Renderer`, as a generic `Renderer` bound reads it. Its
/// one implementor is iced's `Renderer`.
const RENDERER: &str = "pub trait Renderer {
    fn fill_quad(&mut self, quad: iced::advanced::renderer::Quad, background: iced::Color);
    fn start_layer(&mut self, bounds: iced::Rectangle);
    fn end_layer(&mut self);
    fn start_transformation(&mut self, transformation: iced::Transformation);
    fn end_transformation(&mut self);
}";


pub(crate) fn declare(builder: &mut facade::Builder) -> Result<(), host::Error> {
    for module in [
        "advanced::layout",
        "advanced::renderer",
        "advanced::widget",
        "advanced::widget::tree",
    ] {
        builder.module(module)?;
    }
    let copied = crate::OWNED_VALUE_CAPABILITIES;
    let cloned = &[capability::Kind::Clone, capability::Kind::Send, capability::Kind::Static];
    builder.value_type::<iced::advanced::Layout>("advanced::layout::Layout", &[], copied)?;
    builder.alias("advanced::Layout", "advanced::layout::Layout")?;
    builder.value_type::<iced::advanced::layout::Limits>("advanced::layout::Limits", &[], copied)?;
    builder.private_type::<LengthSize>("advanced::layout::LengthSize", &[], copied)?;
    builder.type_::<Tree>("advanced::widget::Tree", &[], cloned)?;
    builder.alias("advanced::widget::tree::Tree", "advanced::widget::Tree")?;
    builder.value_type::<iced::advanced::widget::tree::Tag>(
        "advanced::widget::tree::Tag",
        &[],
        crate::OWNED_EQ_VALUE_CAPABILITIES,
    )?;
    builder.type_::<State>("advanced::widget::tree::State", &[], cloned)?;
    builder.value_type::<iced::advanced::renderer::Style>("advanced::renderer::Style", &[], copied)?;
    builder.default_value_type::<iced::advanced::renderer::Quad>(
        "advanced::renderer::Quad",
        &[],
        crate::OWNED_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.type_::<Shell>("advanced::Shell", &["Message"], cloned)?;
    builder.alias("advanced::shell::Shell", "advanced::Shell")?;
    builder.value_type::<iced::Transformation>("Transformation", &[], copied)?;

    builder.implementable_trait::<WidgetTrait>(
        "advanced::widget::Widget",
        &["Message", "Theme", "Renderer"],
        WIDGET,
    )?;
    builder.alias("advanced::Widget", "advanced::widget::Widget")?;
    builder.implementable_trait::<MetaTrait>("advanced::widget::Meta", &[], META)?;
    builder.implementable_trait::<RendererTrait>("advanced::renderer::Renderer", &[], RENDERER)?;
    builder.alias("advanced::Renderer", "advanced::renderer::Renderer")?;
    builder.private_type::<super::Instance>("advanced::widget::Instance", &["Message"], &[])?;
    builder.project("advanced::widget::Widget", "advanced::widget::Instance", &["Message"])?;
    builder.project("advanced::widget::Instance", "Element", &["Message"])?;

    declare_instance(builder)?;
    declare_values(builder)?;
    declare_renderer(builder)?;
    declare_forwarding(builder)?;
    declare_mesh(builder)?;
    Ok(())
}

fn ty(path: &str) -> Result<type_ref::Type, host::Error> {
    type_ref::Type::registered(path)
}

fn f64_() -> type_ref::Type {
    type_ref::Type::builtin(type_ref::Builtin::F64)
}

fn unit() -> type_ref::Type {
    type_ref::Type::unit()
}

fn signature(inputs: Vec<type_ref::Type>, output: type_ref::Type) -> signature::Definition {
    let mut signature = signature::Builder::new();
    for input in inputs {
        signature.parameter(input);
    }
    signature.output(output);
    signature.finish()
}

fn method(
    builder: &mut facade::Builder,
    path: &str,
    mode: receiver::Mode,
    inputs: Vec<type_ref::Type>,
    output: type_ref::Type,
) -> Result<(), host::Error> {
    builder.item(item::Definition::method(path, mode, signature(inputs, output))?)?;
    Ok(())
}

fn associated(
    builder: &mut facade::Builder,
    path: &str,
    inputs: Vec<type_ref::Type>,
    output: type_ref::Type,
) -> Result<(), host::Error> {
    builder.item(item::Definition::associated_function(path, signature(inputs, output))?)?;
    Ok(())
}

/// `Instance::new(value, size, tag, state, diff, layout, draw, update,
/// mouse_interaction, written)`, which a script widget converts by.
fn declare_instance(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let mut new = signature::Builder::new();
    for name in ["C", "Message"] {
        new.type_parameter(name, &[])?;
    }
    let parameter = |name: &str| type_ref::Type::parameter(name);
    let tuple = |members: Vec<type_ref::Type>| type_ref::Type::tuple(members);
    let tree = || ty("advanced::widget::Tree");
    let layout = || ty("advanced::layout::Layout");
    let cursor = || ty("mouse::Cursor");
    let rectangle = || ty("Rectangle");
    new.parameter(parameter("C")?);
    // size, tag, state
    new.parameter(type_ref::Type::callable(parameter("C")?, ty("advanced::layout::LengthSize")?));
    new.parameter(type_ref::Type::callable(parameter("C")?, ty("advanced::widget::tree::Tag")?));
    new.parameter(type_ref::Type::callable(parameter("C")?, ty("advanced::widget::tree::State")?));
    // diff
    new.parameter(type_ref::Type::callable(tuple(vec![parameter("C")?, tree()?])?, unit()));
    // layout
    new.parameter(type_ref::Type::callable(
        tuple(vec![parameter("C")?, tree()?, ty("widget::Renderer")?, ty("advanced::layout::Limits")?])?,
        unit(),
    ));
    // draw
    new.parameter(type_ref::Type::callable(
        tuple(vec![
            parameter("C")?,
            tree()?,
            ty("widget::Renderer")?,
            ty("Theme")?,
            ty("advanced::renderer::Style")?,
            layout()?,
            cursor()?,
            rectangle()?,
        ])?,
        unit(),
    ));
    // update
    new.parameter(type_ref::Type::callable(
        tuple(vec![
            parameter("C")?,
            tree()?,
            ty("Event")?,
            layout()?,
            cursor()?,
            ty("widget::Renderer")?,
            type_ref::Type::applied("advanced::Shell", [parameter("Message")?])?,
            rectangle()?,
        ])?,
        unit(),
    ));
    // mouse_interaction
    new.parameter(type_ref::Type::callable(
        tuple(vec![
            parameter("C")?,
            tree()?,
            layout()?,
            cursor()?,
            rectangle()?,
            ty("widget::Renderer")?,
        ])?,
        ty("mouse::Interaction")?,
    ));
    new.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
    new.output(type_ref::Type::applied(
        "advanced::widget::Instance",
        [parameter("Message")?],
    )?);
    builder.item(item::Definition::associated_function(
        "advanced::widget::Instance::new",
        new.finish(),
    )?)?;
    Ok(())
}

/// `Tree`, `Layout`, `Limits`, `Quad`, `Style`, `Shell`, `Tag`, `State`,
/// `Size<Length>` and `Transformation`.
fn declare_values(builder: &mut facade::Builder) -> Result<(), host::Error> {
    use receiver::Mode::{Owned, Shared};

    // `tree.size` and `tree.translation`, read and set.
    for (name, field) in [("size", ty("Size")?), ("translation", ty("Vector")?)] {
        let path = format!("advanced::widget::Tree::{name}");
        method(builder, &path, Shared, Vec::new(), field.clone())?;
        method(builder, &path, Owned, vec![field], ty("advanced::widget::Tree")?)?;
    }
    associated(builder, "advanced::widget::tree::Tag::stateless", Vec::new(), ty("advanced::widget::tree::Tag")?)?;
    builder.item(item::Definition::associated_value(
        "advanced::widget::tree::State::None",
        ty("advanced::widget::tree::State")?,
    )?)?;

    for (name, output) in [
        ("bounds", ty("Rectangle")?),
        ("position", ty("Point")?),
        ("size", ty("Size")?),
    ] {
        method(builder, &format!("advanced::layout::Layout::{name}"), Shared, Vec::new(), output)?;
    }

    let limits = || ty("advanced::layout::Limits");
    for name in ["min", "max"] {
        method(builder, &format!("advanced::layout::Limits::{name}"), Shared, Vec::new(), ty("Size")?)?;
    }
    associated(builder, "advanced::layout::Limits::new", vec![ty("Size")?, ty("Size")?], limits()?)?;
    method(builder, "advanced::layout::Limits::loose", Shared, Vec::new(), limits()?)?;
    method(builder, "advanced::layout::Limits::shrink", Shared, vec![ty("Size")?], limits()?)?;
    for name in ["width", "height"] {
        method(builder, &format!("advanced::layout::Limits::{name}"), Owned, vec![ty("Length")?], limits()?)?;
    }
    method(
        builder,
        "advanced::layout::Limits::resolve",
        Shared,
        vec![ty("Length")?, ty("Length")?, ty("Size")?],
        ty("Size")?,
    )?;

    method(builder, "advanced::renderer::Style::text_color", Shared, Vec::new(), ty("Color")?)?;

    // `renderer::Quad { bounds, border, shadow, snap }`.
    let quad = || ty("advanced::renderer::Quad");
    associated(builder, "advanced::renderer::Quad::default", Vec::new(), quad()?)?;
    for (name, field) in [
        ("bounds", ty("Rectangle")?),
        ("border", ty("Border")?),
        ("shadow", ty("Shadow")?),
        ("snap", type_ref::Type::builtin(type_ref::Builtin::Bool)),
    ] {
        let path = format!("advanced::renderer::Quad::{name}");
        method(builder, &path, Shared, Vec::new(), field.clone())?;
        method(builder, &path, Owned, vec![field], quad()?)?;
    }

    // `Size { width: Length::Shrink, height: Length::Fill }`, iced's
    // `Size<Length>`.
    let lengths = || ty("advanced::layout::LengthSize");
    associated(builder, "Size::default", Vec::new(), ty("Size")?)?;
    for name in ["width", "height"] {
        method(builder, &format!("Size::{name}"), Owned, vec![ty("Length")?], lengths()?)?;
        let path = format!("advanced::layout::LengthSize::{name}");
        method(builder, &path, Shared, Vec::new(), ty("Length")?)?;
        method(builder, &path, Owned, vec![ty("Length")?], lengths()?)?;
    }

    // `shell.request_redraw()` and kin.
    let shell = || type_ref::Type::applied("advanced::Shell", [type_ref::Type::parameter("Message")?]);
    let generic = |inputs: Vec<type_ref::Type>, output: type_ref::Type| -> Result<_, host::Error> {
        let mut signature = signature::Builder::new();
        signature.type_parameter("Message", &[])?;
        for input in inputs {
            signature.parameter(input);
        }
        signature.output(output);
        Ok(signature.finish())
    };
    let _ = shell;
    for (name, inputs, output) in [
        ("request_redraw", Vec::new(), unit()),
        ("capture_event", Vec::new(), unit()),
        ("is_event_captured", Vec::new(), type_ref::Type::builtin(type_ref::Builtin::Bool)),
        ("publish", vec![type_ref::Type::parameter("Message")?], unit()),
    ] {
        builder.item(item::Definition::method(
            format!("advanced::Shell::{name}"),
            Shared,
            generic(inputs, output)?,
        )?)?;
    }

    // `Transformation::translate(x, y) * Transformation::scale(s)`.
    let transformation = || ty("Transformation");
    builder.item(item::Definition::associated_value("Transformation::IDENTITY", transformation()?)?)?;
    associated(builder, "Transformation::translate", vec![f64_(), f64_()], transformation()?)?;
    associated(builder, "Transformation::scale", vec![f64_()], transformation()?)?;
    method(builder, "Transformation::mul", Owned, vec![transformation()?], transformation()?)?;
    builder.item(item::Definition::associated_value("Rectangle::INFINITE", ty("Rectangle")?)?)?;
    for input in crate::PIXELS {
        method(builder, "Rectangle::expand", Owned, vec![type_ref::Type::builtin(input)], ty("Rectangle")?)?;
    }
    method(builder, "Rectangle::expand", Owned, vec![ty("Padding")?], ty("Rectangle")?)?;
    Ok(())
}

/// iced's `Renderer` as a script widget draws on it.
fn declare_renderer(builder: &mut facade::Builder) -> Result<(), host::Error> {
    use receiver::Mode::Shared;

    let renderer = || ty("widget::Renderer");
    for background in [ty("Color")?, ty("Background")?] {
        method(
            builder,
            "widget::Renderer::fill_quad",
            Shared,
            vec![ty("advanced::renderer::Quad")?, background],
            unit(),
        )?;
    }
    method(builder, "widget::Renderer::start_layer", Shared, vec![ty("Rectangle")?], unit())?;
    method(builder, "widget::Renderer::end_layer", Shared, Vec::new(), unit())?;
    method(builder, "widget::Renderer::start_transformation", Shared, vec![ty("Transformation")?], unit())?;
    method(builder, "widget::Renderer::end_transformation", Shared, Vec::new(), unit())?;
    // `with_layer`, `with_transformation` and `with_translation` run their
    // closure where they are written, between a start and an end.
    for (name, input) in [
        ("with_layer", ty("Rectangle")?),
        ("with_transformation", ty("Transformation")?),
        ("with_translation", ty("Vector")?),
    ] {
        method(
            builder,
            &format!("widget::Renderer::{name}"),
            Shared,
            vec![input, type_ref::Type::callable(renderer()?, unit())],
            unit(),
        )?;
    }
    Ok(())
}

/// An `Element`'s own `advanced::Widget` methods, which a wrapping widget
/// forwards to, and `explain`.
fn declare_forwarding(builder: &mut facade::Builder) -> Result<(), host::Error> {
    use receiver::Mode::Shared;

    let generic = |inputs: Vec<type_ref::Type>, output: type_ref::Type| -> Result<_, host::Error> {
        let mut signature = signature::Builder::new();
        signature.type_parameter("Message", &[])?;
        for name in ["T", "R"] {
            signature.type_parameter(name, &[])?;
        }
        for input in inputs {
            signature.parameter(input);
        }
        signature.output(output);
        Ok(signature.finish())
    };
    let tree = || ty("advanced::widget::Tree");
    let layout = || ty("advanced::layout::Layout");
    let cursor = || ty("mouse::Cursor");
    let rectangle = || ty("Rectangle");
    let r = || type_ref::Type::parameter("R");
    let shell = || type_ref::Type::applied("advanced::Shell", [type_ref::Type::parameter("Message")?]);
    for (name, inputs, output) in [
        ("size", Vec::new(), ty("advanced::layout::LengthSize")?),
        ("tag", Vec::new(), ty("advanced::widget::tree::Tag")?),
        ("state", Vec::new(), ty("advanced::widget::tree::State")?),
        ("diff", vec![tree()?], unit()),
        ("layout", vec![tree()?, r()?, ty("advanced::layout::Limits")?], unit()),
        (
            "draw",
            vec![
                tree()?,
                r()?,
                type_ref::Type::parameter("T")?,
                ty("advanced::renderer::Style")?,
                layout()?,
                cursor()?,
                rectangle()?,
            ],
            unit(),
        ),
        (
            "update",
            vec![tree()?, ty("Event")?, layout()?, cursor()?, r()?, shell()?, rectangle()?],
            unit(),
        ),
        (
            "mouse_interaction",
            vec![tree()?, layout()?, cursor()?, rectangle()?, r()?],
            ty("mouse::Interaction")?,
        ),
    ] {
        builder.item(item::Definition::method(
            format!("Element::{name}"),
            Shared,
            generic(inputs, output)?,
        )?)?;
    }
    let mut explain = signature::Builder::new();
    explain.type_parameter("Message", &[])?;
    explain.parameter(ty("Color")?);
    explain.output(type_ref::Type::applied("Element", [type_ref::Type::parameter("Message")?])?);
    builder.item(item::Definition::method(
        "Element::explain",
        receiver::Mode::Owned,
        explain.finish(),
    )?)?;
    Ok(())
}

/// `graphics::mesh::{Mesh, Indexed, SolidVertex2D}`, `graphics::color::pack`
/// and `renderer.draw_mesh(mesh)`.
fn declare_mesh(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let _ = (builder, MeshRendererTrait);
    Ok(())
}
