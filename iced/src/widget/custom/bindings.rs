//! The native side of the facade in `declare`.

use std::sync::{Arc, Mutex};

use iced::advanced::layout::{Layout, Limits};
use iced::advanced::widget::tree;
use iced::advanced::Widget as _;
use iced::widget::Widget as _;
use iced::{mouse, Length, Rectangle, Size};
use rawr::{host, Dynamic, EvalAltResult};

use super::lend::Frame;
use super::{record, ChildDraw, LengthSize, Shell, State, Tree};
use crate::renderer::Command;
use crate::widget::builtin::{arity, pixels, runtime_error, take};
use crate::widget as widgets;

pub(super) fn bind(builder: &mut host::runtime::Builder) {
    bind_tree(builder);
    bind_values(builder);
    bind_renderer(builder);
    bind_forwarding(builder);
}

/// A value of `T` read in place.
fn read<T: Clone + 'static>(value: &Dynamic, path: &str) -> Result<T, Box<EvalAltResult>> {
    value.read_lock::<T>().map(|value| value.clone()).ok_or_else(|| {
        runtime_error(format!(
            "`{path}` received `{}`, expected `{}`",
            value.type_name(),
            std::any::type_name::<T>()
        ))
    })
}

fn lent<T>(result: Result<T, super::lend::Error>) -> Result<T, Box<EvalAltResult>> {
    result.map_err(|error| runtime_error(error.to_string()))
}

fn bind_tree(builder: &mut host::runtime::Builder) {
    for name in ["size", "translation"] {
        let path = format!("iced::advanced::widget::Tree::{name}");
        builder
            .bind(path.clone(), move |args| {
                let Some((receiver, operands)) = args.split_first_mut() else {
                    return Err(arity(&path, 1, 0));
                };
                let handle = read::<Tree>(receiver, &path)?;
                let frame = handle.frame()?;
                match operands {
                    [] => lent(frame.tree(&handle.path, |tree| match name {
                        "size" => Dynamic::from(tree.size),
                        _ => Dynamic::from(tree.translation),
                    })),
                    [value] => {
                        match name {
                            "size" => {
                                let size = read::<Size>(value, &path)?;
                                lent(frame.tree_mut(&handle.path, |tree| tree.size = size))?;
                            }
                            _ => {
                                let translation = read::<iced::Vector>(value, &path)?;
                                lent(frame.tree_mut(&handle.path, |tree| tree.translation = translation))?;
                            }
                        }
                        Ok(Dynamic::from(handle))
                    }
                    _ => Err(arity(&path, 2, operands.len() + 1)),
                }
            })
            .expect("the iced facade must retain every Tree field");
    }
    builder
        .bind("iced::advanced::widget::tree::Tag::stateless", |_| {
            Ok(Dynamic::from(tree::Tag::stateless()))
        })
        .expect("the iced facade must retain Tag::stateless");
    builder
        .bind("iced::advanced::widget::tree::State::None", |_| {
            Ok(Dynamic::from(State(Arc::new(Mutex::new(Some(tree::State::None))))))
        })
        .expect("the iced facade must retain State::None");
}

fn bind_values(builder: &mut host::runtime::Builder) {
    for name in ["bounds", "position", "size"] {
        let path = format!("iced::advanced::layout::Layout::{name}");
        builder
            .bind(path.clone(), move |args| {
                let [layout] = args else {
                    return Err(arity(&path, 1, args.len()));
                };
                let layout = read::<Layout>(layout, &path)?;
                Ok(match name {
                    "bounds" => Dynamic::from(layout.bounds()),
                    "position" => Dynamic::from(layout.position()),
                    _ => Dynamic::from(layout.size()),
                })
            })
            .expect("the iced facade must retain every Layout method");
    }
    for name in ["min", "max", "loose", "shrink", "width", "height", "resolve"] {
        let path = format!("iced::advanced::layout::Limits::{name}");
        builder
            .bind(path.clone(), move |args| {
                let Some((receiver, operands)) = args.split_first_mut() else {
                    return Err(arity(&path, 1, 0));
                };
                let limits = read::<Limits>(receiver, &path)?;
                let count = operands.len();
                Ok(match (name, operands) {
                    ("min", []) => Dynamic::from(limits.min),
                    ("max", []) => Dynamic::from(limits.max),
                    ("loose", []) => Dynamic::from(limits.loose()),
                    ("shrink", [size]) => Dynamic::from(limits.shrink(read::<Size>(size, &path)?)),
                    ("width", [length]) => Dynamic::from(limits.width(read::<Length>(length, &path)?)),
                    ("height", [length]) => Dynamic::from(limits.height(read::<Length>(length, &path)?)),
                    ("resolve", [width, height, intrinsic]) => Dynamic::from(limits.resolve(
                        read::<Length>(width, &path)?,
                        read::<Length>(height, &path)?,
                        read::<Size>(intrinsic, &path)?,
                    )),
                    _ => return Err(arity(&path, 1, count + 1)),
                })
            })
            .expect("the iced facade must retain every Limits method");
    }
    builder
        .bind("iced::advanced::layout::Limits::new", |args| {
            let [min, max] = args else {
                return Err(arity("Limits::new", 2, args.len()));
            };
            Ok(Dynamic::from(Limits::new(
                read::<Size>(min, "Limits::new")?,
                read::<Size>(max, "Limits::new")?,
            )))
        })
        .expect("the iced facade must retain Limits::new");
    builder
        .bind("iced::advanced::renderer::Style::text_color", |args| {
            let [style] = args else {
                return Err(arity("Style::text_color", 1, args.len()));
            };
            Ok(Dynamic::from(
                read::<iced::advanced::renderer::Style>(style, "Style::text_color")?.text_color,
            ))
        })
        .expect("the iced facade must retain renderer::Style::text_color");

    builder
        .bind("iced::advanced::renderer::Quad::default", |_| {
            Ok(Dynamic::from(iced::advanced::renderer::Quad::default()))
        })
        .expect("the iced facade must retain Quad::default");
    for name in ["bounds", "border", "shadow", "snap"] {
        let path = format!("iced::advanced::renderer::Quad::{name}");
        builder
            .bind(path.clone(), move |args| {
                let Some((receiver, operands)) = args.split_first_mut() else {
                    return Err(arity(&path, 1, 0));
                };
                let mut quad = read::<iced::advanced::renderer::Quad>(receiver, &path)?;
                let [value] = operands else {
                    return Ok(match name {
                        "bounds" => Dynamic::from(quad.bounds),
                        "border" => Dynamic::from(quad.border),
                        "shadow" => Dynamic::from(quad.shadow),
                        _ => Dynamic::from(quad.snap),
                    });
                };
                match name {
                    "bounds" => quad.bounds = read(value, &path)?,
                    "border" => quad.border = read(value, &path)?,
                    "shadow" => quad.shadow = read(value, &path)?,
                    _ => {
                        quad.snap = value
                            .as_bool()
                            .map_err(|_| runtime_error(format!("`{path}` expects a `bool`")))?;
                    }
                }
                Ok(Dynamic::from(quad))
            })
            .expect("the iced facade must retain every Quad field");
    }

    builder
        .bind("iced::Size::default", |_| Ok(Dynamic::from(Size::<f32>::default())))
        .expect("the iced facade must retain Size::default");
    for name in ["width", "height"] {
        let path = format!("iced::advanced::layout::LengthSize::{name}");
        builder
            .bind(path.clone(), move |args| {
                let Some((receiver, operands)) = args.split_first_mut() else {
                    return Err(arity(&path, 1, 0));
                };
                let LengthSize(mut size) = read::<LengthSize>(receiver, &path)?;
                let [value] = operands else {
                    return Ok(Dynamic::from(if name == "width" { size.width } else { size.height }));
                };
                let length = read::<Length>(value, &path)?;
                if name == "width" {
                    size.width = length;
                } else {
                    size.height = length;
                }
                Ok(Dynamic::from(LengthSize(size)))
            })
            .expect("the iced facade must retain every Size<Length> field");
    }

    for name in ["request_redraw", "capture_event", "is_event_captured", "publish"] {
        let path = format!("iced::advanced::Shell::{name}");
        builder
            .bind(path.clone(), move |args| {
                let Some((receiver, operands)) = args.split_first_mut() else {
                    return Err(arity(&path, 1, 0));
                };
                let shell = read::<Shell>(receiver, &path)?;
                let frame = shell.frame()?;
                let count = operands.len();
                match (name, operands) {
                    ("request_redraw", []) => record(&path, |effects| effects.redraw = true)?,
                    ("capture_event", []) => record(&path, |effects| effects.capture = true)?,
                    ("is_event_captured", []) => {
                        let captured = record(&path, |effects| effects.capture)?;
                        return Ok(Dynamic::from(
                            captured || lent(frame.shell())?.is_event_captured(),
                        ));
                    }
                    ("publish", [message]) => {
                        let message = message.take();
                        record(&path, |effects| effects.messages.push(message))?;
                    }
                    _ => return Err(arity(&path, 1, count + 1)),
                }
                Ok(Dynamic::UNIT)
            })
            .expect("the iced facade must retain every Shell method");
    }

    builder
        .bind("iced::Transformation::IDENTITY", |_| {
            Ok(Dynamic::from(iced::Transformation::IDENTITY))
        })
        .expect("the iced facade must retain Transformation::IDENTITY");
    builder
        .bind("iced::Transformation::translate", |args| {
            let [x, y] = args else {
                return Err(arity("Transformation::translate", 2, args.len()));
            };
            Ok(Dynamic::from(iced::Transformation::translate(
                pixels(x, "Transformation::translate")?,
                pixels(y, "Transformation::translate")?,
            )))
        })
        .expect("the iced facade must retain Transformation::translate");
    builder
        .bind("iced::Transformation::scale", |args| {
            let [scale] = args else {
                return Err(arity("Transformation::scale", 1, args.len()));
            };
            Ok(Dynamic::from(iced::Transformation::scale(pixels(
                scale,
                "Transformation::scale",
            )?)))
        })
        .expect("the iced facade must retain Transformation::scale");
    builder
        .bind("iced::Transformation::mul", |args| {
            let [left, right] = args else {
                return Err(arity("Transformation::mul", 2, args.len()));
            };
            Ok(Dynamic::from(
                read::<iced::Transformation>(left, "Transformation::mul")?
                    * read::<iced::Transformation>(right, "Transformation::mul")?,
            ))
        })
        .expect("the iced facade must retain Transformation::mul");
    builder
        .bind("iced::Rectangle::INFINITE", |_| Ok(Dynamic::from(Rectangle::INFINITE)))
        .expect("the iced facade must retain Rectangle::INFINITE");
    builder
        .bind("iced::Rectangle::expand", |args| {
            let [bounds, padding] = args else {
                return Err(arity("Rectangle::expand", 2, args.len()));
            };
            let bounds = read::<Rectangle>(bounds, "Rectangle::expand")?;
            let padding = match padding.read_lock::<iced::Padding>() {
                Some(padding) => *padding,
                None => iced::Padding::from(pixels(padding, "Rectangle::expand")?),
            };
            Ok(Dynamic::from(bounds.expand(padding)))
        })
        .expect("the iced facade must retain Rectangle::expand");
}

fn bind_renderer(builder: &mut host::runtime::Builder) {
    builder
        .bind("iced::widget::Renderer::fill_quad", |args| {
            let [_, quad, background] = args else {
                return Err(arity("Renderer::fill_quad", 3, args.len()));
            };
            let quad = read::<iced::advanced::renderer::Quad>(quad, "Renderer::fill_quad")?;
            let background = match background.read_lock::<iced::Color>() {
                Some(color) => iced::Background::Color(*color),
                None => read::<iced::Background>(background, "Renderer::fill_quad")?,
            };
            record("Renderer::fill_quad", |effects| {
                effects.commands.push(Command::Quad(quad, background));
            })?;
            Ok(Dynamic::UNIT)
        })
        .expect("the iced facade must retain Renderer::fill_quad");
    for name in ["start_layer", "end_layer", "start_transformation", "end_transformation"] {
        let path = format!("iced::widget::Renderer::{name}");
        builder
            .bind(path.clone(), move |args| {
                let command = match (name, &*args) {
                    ("start_layer", [_, bounds]) => Command::StartLayer(read(bounds, &path)?),
                    ("start_transformation", [_, transformation]) => {
                        Command::StartTransformation(read(transformation, &path)?)
                    }
                    ("end_layer", [_]) => Command::EndLayer,
                    ("end_transformation", [_]) => Command::EndTransformation,
                    _ => return Err(arity(&path, 2, args.len())),
                };
                record(&path, |effects| effects.commands.push(command))?;
                Ok(Dynamic::UNIT)
            })
            .expect("the iced facade must retain every Renderer layer method");
    }
    for name in ["with_layer", "with_transformation", "with_translation"] {
        let path = format!("iced::widget::Renderer::{name}");
        builder
            .bind(path.clone(), move |_| {
                Err(runtime_error(format!(
                    "`{path}` runs its closure where it is written, so Rawr calls it there"
                )))
            })
            .expect("the iced facade must retain every closure-running Renderer method");
    }
}

/// The child an `Element` value names in the call running now.
fn child(element: &Dynamic, path: &'static str) -> Result<(Frame, usize), Box<EvalAltResult>> {
    let value = read::<widgets::Value>(element, path)?;
    let frame = lent(Frame::current(path))?;
    let index = lent(frame.child(&value))?;
    Ok((frame, index))
}

/// The frame a `Tree` handle names, which must be the call running now.
fn tree(handle: &Dynamic, path: &'static str) -> Result<Tree, Box<EvalAltResult>> {
    let tree = read::<Tree>(handle, path)?;
    let current = lent(Frame::current(path))?;
    if tree.frame()?.nonce() != current.nonce() {
        return Err(runtime_error(format!(
            "`{path}` received a `Tree` lent to another call"
        )));
    }
    Ok(tree)
}

fn busy() -> Box<EvalAltResult> {
    runtime_error("the wrapped widget is already in use by this call".into())
}

fn bind_forwarding(builder: &mut host::runtime::Builder) {
    builder
        .bind("iced::Element::size", |args| {
            let [element] = args else {
                return Err(arity("Element::size", 1, args.len()));
            };
            let (frame, index) = child(element, "Element::size")?;
            let size = frame
                .children()
                .with(index, |child| child.size())
                .ok_or_else(busy)?;
            Ok(Dynamic::from(LengthSize(size)))
        })
        .expect("the iced facade must retain Element::size");
    builder
        .bind("iced::Element::tag", |args| {
            let [element] = args else {
                return Err(arity("Element::tag", 1, args.len()));
            };
            let (frame, index) = child(element, "Element::tag")?;
            let tag = frame
                .children()
                .with(index, |child| child.tag())
                .ok_or_else(busy)?;
            Ok(Dynamic::from(tag))
        })
        .expect("the iced facade must retain Element::tag");
    builder
        .bind("iced::Element::state", |args| {
            let [element] = args else {
                return Err(arity("Element::state", 1, args.len()));
            };
            let (frame, index) = child(element, "Element::state")?;
            let state = frame
                .children()
                .with(index, |child| child.state())
                .ok_or_else(busy)?;
            Ok(Dynamic::from(State(Arc::new(Mutex::new(Some(state))))))
        })
        .expect("the iced facade must retain Element::state");
    builder
        .bind("iced::Element::diff", |args| {
            let [element, handle] = args else {
                return Err(arity("Element::diff", 2, args.len()));
            };
            let handle = tree(handle, "Element::diff")?;
            let (frame, index) = child(element, "Element::diff")?;
            lent(frame.tree_mut(&handle.path, |subtree| {
                frame
                    .children()
                    .with(index, |child| child.diff(subtree))
            }))?
            .ok_or_else(busy)?;
            Ok(Dynamic::UNIT)
        })
        .expect("the iced facade must retain Element::diff");
    builder
        .bind("iced::Element::layout", |args| {
            let [element, handle, _renderer, limits] = args else {
                return Err(arity("Element::layout", 4, args.len()));
            };
            let handle = tree(handle, "Element::layout")?;
            let limits = read::<Limits>(limits, "Element::layout")?;
            let (frame, index) = child(element, "Element::layout")?;
            let renderer = lent(frame.renderer())?;
            lent(frame.tree_mut(&handle.path, |subtree| {
                frame.children().with(index, |child| {
                    child.layout(subtree, renderer, &limits);
                })
            }))?
            .ok_or_else(busy)?;
            Ok(Dynamic::UNIT)
        })
        .expect("the iced facade must retain Element::layout");
    builder
        .bind("iced::Element::draw", |args| {
            let [element, handle, _renderer, _theme, style, layout, cursor, viewport] = args else {
                return Err(arity("Element::draw", 8, args.len()));
            };
            let path = "Element::draw";
            let handle = tree(handle, path)?;
            let (_, child) = child(element, path)?;
            let draw = ChildDraw {
                child,
                path: handle.path.clone(),
                style: read(style, path)?,
                layout: read(layout, path)?,
                cursor: read(cursor, path)?,
                viewport: read(viewport, path)?,
            };
            record(path, |effects| {
                effects.commands.push(Command::Child(effects.children.len()));
                effects.children.push(draw);
            })?;
            Ok(Dynamic::UNIT)
        })
        .expect("the iced facade must retain Element::draw");
    builder
        .bind("iced::Element::update", |args| {
            let [element, handle, event, layout, cursor, _renderer, _shell, viewport] = args else {
                return Err(arity("Element::update", 8, args.len()));
            };
            let path = "Element::update";
            let handle = tree(handle, path)?;
            let event = read::<iced::Event>(event, path)?;
            let layout = read::<Layout>(layout, path)?;
            let cursor = read::<mouse::Cursor>(cursor, path)?;
            let viewport = read::<Rectangle>(viewport, path)?;
            let (frame, index) = child(element, path)?;
            let renderer = lent(frame.renderer())?;
            let shell = lent(frame.shell())?;
            lent(frame.tree_mut(&handle.path, |subtree| {
                frame.children().with(index, |child| {
                    child
                        
                        .update(subtree, &event, layout, cursor, renderer, shell, &viewport);
                })
            }))?
            .ok_or_else(busy)?;
            Ok(Dynamic::UNIT)
        })
        .expect("the iced facade must retain Element::update");
    builder
        .bind("iced::Element::mouse_interaction", |args| {
            let [element, handle, layout, cursor, viewport, _renderer] = args else {
                return Err(arity("Element::mouse_interaction", 6, args.len()));
            };
            let path = "Element::mouse_interaction";
            let handle = tree(handle, path)?;
            let layout = read::<Layout>(layout, path)?;
            let cursor = read::<mouse::Cursor>(cursor, path)?;
            let viewport = read::<Rectangle>(viewport, path)?;
            let (frame, index) = child(element, path)?;
            let renderer = lent(frame.renderer())?;
            let interaction = lent(frame.tree(&handle.path, |subtree| {
                frame.children().with(index, |child| {
                    child
                        
                        .mouse_interaction(subtree, layout, cursor, &viewport, renderer)
                })
            }))?
            .ok_or_else(busy)?;
            Ok(Dynamic::from(interaction))
        })
        .expect("the iced facade must retain Element::mouse_interaction");
    builder
        .bind_with_context("iced::Element::explain", |call, args| {
            let [element, color] = args else {
                return Err(arity("Element::explain", 2, args.len()));
            };
            let child = take::<widgets::Value>(element, "iced::Element")?;
            let color = read::<iced::Color>(color, "Element::explain")?;
            let witness = child.witness().ok_or_else(|| {
                runtime_error("`explain` received an unattested element".into())
            })?;
            let explained = widgets::Value::from_transform(
                Explain {
                    identity: widgets::Identity::unique(),
                    child: child.clone(),
                    color,
                },
                call,
                witness,
            )
            .map_err(crate::widget::builtin::witness_error)?;
            Ok(Dynamic::from(explained))
        })
        .expect("the iced facade must retain Element::explain");
}

/// iced's `Element::explain`: the child with its layout outlined.
struct Explain {
    identity: widgets::Identity,
    child: widgets::Value,
    color: iced::Color,
}

impl widgets::Descriptor for Explain {
    fn lower(&self, context: &widgets::Context<'_>) -> iced::Element<'static, crate::runtime::Message> {
        iced::widget::Widget::explain(self.child.lower(context), self.color).boxed()
    }

    fn identity(&self) -> widgets::Identity {
        self.identity.clone()
    }
}
