//! iced's `text_editor(&content)` over a `text_editor::Content` the app
//! owns (D546).
//!
//! The `Content` lives in the application's native home and the script's
//! value holds it by handle ([`marker::EditorContent`]). A transition reaches
//! it mutably (`self.content.perform(action)` in `update`), and the editor a
//! view lowers borrows it for iced's view lifetime, as
//! `text_editor(&self.content)` does in Rust. So the text, the cursor, the
//! selection and the undo history are the `Content`'s own, kept across views
//! and across an edit that keeps the State.

use iced::widget::Widget as _;
use rawr::host;
use rawr::{Dynamic, EvalAltResult, Position};

use super::builtin::{
    arity, exact_string, length, padding, pixels, take, witness_error, Witnessed,
};
use super::helpers::bind_widget_type;
use crate::{home, marker, runtime, widget};

type Content = iced::widget::text_editor::Content<iced::Renderer>;

pub(super) fn bind(builder: &mut host::runtime::Builder) {
    bind_content(builder);
    bind_values(builder);
    bind_editor(builder);
}

impl widget::Descriptor for marker::TextEditor {
    fn lower<'a>(&self, context: &widget::Context<'a>) -> iced::Element<'a, runtime::Message> {
        let Some(home) = context.home() else {
            return widget::failure(widget::refuse_borrow("text_editor", "text_editor::Content"));
        };
        let content = match home.get::<Content>(&self.content) {
            Ok(content) => content,
            Err(error) => return widget::failure(format!("text_editor: {error}")),
        };
        let mut editor = iced::widget::text_editor(content);
        if let Some(mapper) = &self.on_action {
            let mapper = context.message_mapper(mapper.clone());
            editor = editor.on_action(move |action| mapper.map(Dynamic::from(action)));
        }
        if let Some(placeholder) = &self.placeholder {
            editor = editor.placeholder(placeholder.clone());
        }
        if let Some(width) = self.width {
            editor = editor.width(width);
        }
        if let Some(height) = self.height {
            editor = editor.height(height);
        }
        if let Some(padding) = self.padding {
            editor = editor.padding(padding);
        }
        if let Some(size) = self.size {
            editor = editor.size(size);
        }
        if let Some(font) = self.font {
            editor = editor.font(font);
        }
        if let Some(wrapping) = self.wrapping {
            editor = editor.wrapping(wrapping);
        }
        if let Some(id) = &self.id {
            editor = editor.id(id.clone());
        }
        editor.boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl Witnessed for marker::TextEditor {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

/// A native home error as the script's runtime error.
fn home_error(error: home::Error) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorRuntime(
        error.to_string().into(),
        Position::NONE,
    ))
}

fn handle(value: &Dynamic, operation: &str) -> Result<home::Handle, Box<EvalAltResult>> {
    value
        .read_lock::<marker::EditorContent>()
        .map(|content| content.native.clone())
        .ok_or_else(|| {
            Box::new(EvalAltResult::ErrorRuntime(
                format!("{operation} expects a `text_editor::Content`").into(),
                Position::NONE,
            ))
        })
}

/// A new `Content` in the home of the transition running now.
fn insert(content: Content) -> Result<Dynamic, Box<EvalAltResult>> {
    home::insert(content)
        .map(|native| Dynamic::from(marker::EditorContent { native }))
        .map_err(home_error)
}

/// Read the `Content` the receiver names.
fn read<R>(
    args: &[&mut Dynamic],
    operation: &str,
    f: impl FnOnce(&Content) -> R,
) -> Result<R, Box<EvalAltResult>> {
    let [content] = args else {
        return Err(arity(operation, 1, args.len()));
    };
    home::get(&handle(content, operation)?, f).map_err(home_error)
}

/// A copy of the value a `&self` method's receiver holds, which stays.
fn peek<T: Clone + 'static>(value: &Dynamic, expected: &str) -> Result<T, Box<EvalAltResult>> {
    value
        .read_lock::<T>()
        .map(|value| value.clone())
        .ok_or_else(|| {
            Box::new(EvalAltResult::ErrorRuntime(
                format!("expected `{expected}`, found `{}`", value.type_name()).into(),
                Position::NONE,
            ))
        })
}

fn usize_value(value: usize) -> Dynamic {
    Dynamic::from(rawr::INT::try_from(value).unwrap_or(rawr::INT::MAX))
}

/// `Content::new`, `with_text`, `perform` and the readers.
fn bind_content(builder: &mut host::runtime::Builder) {
    for name in ["new", "default"] {
        builder
            .bind(
                format!("iced::widget::text_editor::Content::{name}"),
                |_| insert(Content::new()),
            )
            .expect("the iced facade must retain Content::new and Content::default");
    }
    builder
        .bind("iced::widget::text_editor::Content::with_text", |args| {
            let [text] = args else {
                return Err(arity("Content::with_text", 1, args.len()));
            };
            insert(Content::with_text(&exact_string(
                text,
                "Content::with_text",
            )?))
        })
        .expect("the iced facade must retain Content::with_text");
    builder
        .bind("iced::widget::text_editor::Content::perform", |args| {
            let [content, action] = args else {
                return Err(arity("Content::perform", 2, args.len()));
            };
            let action = take::<iced::widget::text_editor::Action>(
                action,
                "iced::widget::text_editor::Action",
            )?;
            home::with_mut::<Content, _>(&handle(content, "Content::perform")?, |content| {
                content.perform(action);
            })
            .map_err(home_error)?;
            Ok(Dynamic::UNIT)
        })
        .expect("the iced facade must retain Content::perform");
    builder
        .bind("iced::widget::text_editor::Content::move_to", |args| {
            let [content, cursor] = args else {
                return Err(arity("Content::move_to", 2, args.len()));
            };
            let cursor = take::<iced::widget::text_editor::Cursor>(
                cursor,
                "iced::widget::text_editor::Cursor",
            )?;
            home::with_mut::<Content, _>(&handle(content, "Content::move_to")?, |content| {
                content.move_to(cursor);
            })
            .map_err(home_error)?;
            Ok(Dynamic::UNIT)
        })
        .expect("the iced facade must retain Content::move_to");
    builder
        .bind("iced::widget::text_editor::Content::text", |args| {
            read(args, "Content::text", |content| {
                Dynamic::from(content.text())
            })
        })
        .expect("the iced facade must retain Content::text");
    builder
        .bind("iced::widget::text_editor::Content::line_count", |args| {
            read(args, "Content::line_count", |content| {
                usize_value(content.line_count())
            })
        })
        .expect("the iced facade must retain Content::line_count");
    builder
        .bind("iced::widget::text_editor::Content::cursor", |args| {
            read(args, "Content::cursor", |content| {
                Dynamic::from(content.cursor())
            })
        })
        .expect("the iced facade must retain Content::cursor");
    builder
        .bind("iced::widget::text_editor::Content::selection", |args| {
            read(args, "Content::selection", |content| {
                content.selection().map_or_else(rawr::option::none, |text| {
                    rawr::option::some(Dynamic::from(text))
                })
            })
        })
        .expect("the iced facade must retain Content::selection");
    builder
        .bind("iced::widget::text_editor::Content::is_empty", |args| {
            read(args, "Content::is_empty", |content| {
                Dynamic::from(content.is_empty())
            })
        })
        .expect("the iced facade must retain Content::is_empty");
    builder
        .bind("iced::widget::text_editor::Content::line_ending", |args| {
            read(args, "Content::line_ending", |content| {
                content
                    .line_ending()
                    .map_or_else(rawr::option::none, |ending| {
                        rawr::option::some(Dynamic::from(ending))
                    })
            })
        })
        .expect("the iced facade must retain Content::line_ending");
}

/// `Action::is_edit`, the `Cursor` and `Position` fields,
/// `LineEnding::as_str` and `text::Wrapping`'s variants.
fn bind_values(builder: &mut host::runtime::Builder) {
    use iced::advanced::text::Position as TextPosition;
    use iced::widget::text_editor::{Action, Cursor, LineEnding};

    builder
        .bind("iced::widget::text_editor::Action::is_edit", |args| {
            let [action] = args else {
                return Err(arity("Action::is_edit", 1, args.len()));
            };
            let action = peek::<Action>(action, "iced::widget::text_editor::Action")?;
            Ok(Dynamic::from(action.is_edit()))
        })
        .expect("the iced facade must retain Action::is_edit");
    builder
        .bind("iced::widget::text_editor::Cursor::position", |args| {
            let [cursor] = args else {
                return Err(arity("Cursor::position", 1, args.len()));
            };
            let cursor = peek::<Cursor>(cursor, "iced::widget::text_editor::Cursor")?;
            Ok(Dynamic::from(cursor.position))
        })
        .expect("the iced facade must retain Cursor::position");
    builder
        .bind("iced::widget::text_editor::Cursor::selection", |args| {
            let [cursor] = args else {
                return Err(arity("Cursor::selection", 1, args.len()));
            };
            let cursor = peek::<Cursor>(cursor, "iced::widget::text_editor::Cursor")?;
            Ok(rawr::option::from(cursor.selection))
        })
        .expect("the iced facade must retain Cursor::selection");
    for (name, field) in [
        (
            "line",
            (|position: TextPosition| position.line) as fn(TextPosition) -> usize,
        ),
        ("index", |position| position.index),
    ] {
        builder
            .bind(
                format!("iced::advanced::text::Position::{name}"),
                move |args| {
                    let [position] = args else {
                        return Err(arity("Position", 1, args.len()));
                    };
                    let position =
                        peek::<TextPosition>(position, "iced::advanced::text::Position")?;
                    Ok(usize_value(field(position)))
                },
            )
            .expect("the iced facade must retain each Position field");
    }
    builder
        .bind("iced::widget::text_editor::LineEnding::as_str", |args| {
            let [ending] = args else {
                return Err(arity("LineEnding::as_str", 1, args.len()));
            };
            let ending = take::<LineEnding>(ending, "iced::widget::text_editor::LineEnding")?;
            Ok(Dynamic::from(ending.as_str().to_owned()))
        })
        .expect("the iced facade must retain LineEnding::as_str");
    for (name, wrapping) in crate::TEXT_WRAPPINGS.into_iter().zip([
        iced::widget::text::Wrapping::None,
        iced::widget::text::Wrapping::Word,
        iced::widget::text::Wrapping::Glyph,
        iced::widget::text::Wrapping::WordOrGlyph,
    ]) {
        builder
            .bind(format!("iced::widget::text::Wrapping::{name}"), move |_| {
                Ok(Dynamic::from(wrapping))
            })
            .expect("the iced facade must retain every text::Wrapping variant");
    }
}

fn take_editor(
    args: &mut [&mut Dynamic],
    operation: &str,
) -> Result<marker::TextEditor, Box<EvalAltResult>> {
    if args.len() != 2 {
        return Err(arity(operation, 2, args.len()));
    }
    take::<marker::TextEditor>(args[0], "iced::widget::TextEditor")
}

/// `text_editor(&content)` and the `TextEditor` builders.
fn bind_editor(builder: &mut host::runtime::Builder) {
    bind_widget_type::<marker::TextEditor>(builder, "iced::widget::TextEditor");
    builder
        .bind_with_context("iced::widget::text_editor", |call, args| {
            let [content] = args else {
                return Err(arity("text_editor", 1, args.len()));
            };
            Ok(Dynamic::from(marker::TextEditor {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                content: handle(content, "text_editor")?,
                on_action: None,
                placeholder: None,
                width: None,
                height: None,
                padding: None,
                size: None,
                font: None,
                wrapping: None,
                id: None,
            }))
        })
        .expect("the iced facade must retain text_editor");
    type Set = fn(&mut marker::TextEditor, &mut Dynamic) -> Result<(), Box<EvalAltResult>>;
    let setters: [(&str, Set); 9] = [
        ("on_action", |editor, value| {
            editor.on_action = Some(take::<rawr::item::Value>(value, "symbolic message mapper")?);
            Ok(())
        }),
        ("placeholder", |editor, value| {
            editor.placeholder = Some(exact_string(value, "TextEditor::placeholder")?);
            Ok(())
        }),
        ("width", |editor, value| {
            editor.width = Some(pixels(value, "TextEditor::width")?);
            Ok(())
        }),
        ("height", |editor, value| {
            editor.height = Some(length(value)?);
            Ok(())
        }),
        ("padding", |editor, value| {
            editor.padding = Some(padding(value)?);
            Ok(())
        }),
        ("size", |editor, value| {
            editor.size = Some(pixels(value, "TextEditor::size")?);
            Ok(())
        }),
        ("font", |editor, value| {
            editor.font = Some(take::<iced::Font>(value, "iced::Font")?);
            Ok(())
        }),
        ("wrapping", |editor, value| {
            editor.wrapping = Some(take::<iced::widget::text::Wrapping>(
                value,
                "iced::widget::text::Wrapping",
            )?);
            Ok(())
        }),
        ("id", |editor, value| {
            editor.id = Some(crate::task::take_id(value)?);
            Ok(())
        }),
    ];
    for (name, set) in setters {
        let operation = format!("TextEditor::{name}");
        builder
            .bind(format!("iced::widget::TextEditor::{name}"), move |args| {
                let mut editor = take_editor(args, &operation)?;
                set(&mut editor, args[1])?;
                Ok(Dynamic::from(editor))
            })
            .expect("the iced facade must retain every TextEditor builder");
    }
}
