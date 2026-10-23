//! iced's `span`, `rich_text` and `rich_text!`.

use iced::widget::Widget as _;
use rawr::host;
use rawr::{Dynamic, EvalAltResult};

use super::builtin::{
    arity, exact_string, length, pixels, project, runtime_error, take, witness_error, Witnessed,
};
use crate::{marker, runtime, widget};

pub(super) fn bind(builder: &mut host::runtime::Builder) {
    builder
        .bind_type_attestation("iced::widget::text::Span", |value, expected| {
            let value = value.read_lock::<marker::Span>()?;
            (expected.argument_count() == 1)
                .then(|| expected.attest_witness(Some(&value.witness)))
                .flatten()
        })
        .expect("the iced facade must declare generic widget::text::Span exactly once");
    builder
        .bind_type_attestation("iced::widget::text::Rich", |value, expected| {
            let value = value.read_lock::<marker::Rich>()?;
            (expected.argument_count() == 2)
                .then(|| expected.attest_witness(Some(&value.witness)))
                .flatten()
        })
        .expect("the iced facade must declare generic widget::text::Rich exactly once");
    builder
        .bind_projection_with_context("iced::widget::text::Rich", "iced::Element", |call, args| {
            project::<marker::Rich>(call, args, "iced::widget::text::Rich")
        })
        .expect("the iced facade must retain rich text's projection into Element");
    builder
        .bind_with_context("iced::widget::span", |call, args| {
            let [text] = args else {
                return Err(arity("span", 1, args.len()));
            };
            Ok(Dynamic::from(marker::Span {
                witness: call.output_witness().map_err(witness_error)?,
                text: exact_string(text, "span")?,
                size: None,
                font: None,
                color: None,
                link: None,
                underline: false,
                strikethrough: false,
            }))
        })
        .expect("the iced facade must retain span");
    for name in [
        "size",
        "font",
        "font_maybe",
        "color",
        "color_maybe",
        "link",
        "link_maybe",
        "underline",
        "strikethrough",
    ] {
        let path = format!("iced::widget::text::Span::{name}");
        builder
            .bind(path.clone(), move |args| {
                if args.len() != 2 {
                    return Err(arity(&path, 2, args.len()));
                }
                let mut span = take::<marker::Span>(args[0], "iced::widget::text::Span")?;
                match name {
                    "size" => span.size = Some(pixels(args[1], &path)?),
                    "font" => span.font = Some(take(args[1], "iced::Font")?),
                    "font_maybe" => span.font = maybe(args[1], "iced::Font")?,
                    "color" => span.color = Some(take(args[1], "iced::Color")?),
                    "color_maybe" => span.color = maybe(args[1], "iced::Color")?,
                    "link" => span.link = Some(args[1].take()),
                    "link_maybe" => span.link = rawr::option::take(args[1]).flatten(),
                    "underline" => span.underline = flag(args[1], &path)?,
                    _ => span.strikethrough = flag(args[1], &path)?,
                }
                Ok(Dynamic::from(span))
            })
            .expect("the iced facade must retain every Span builder");
    }
    builder
        .bind_with_context("iced::widget::rich_text", |call, args| {
            let [spans] = args else {
                return Err(arity("rich_text", 1, args.len()));
            };
            let spans = spans
                .take()
                .into_typed_array::<Dynamic>()
                .map_err(|_| runtime_error("rich_text expects a slice of spans".into()))?
                .into_iter()
                .map(|mut span| take::<marker::Span>(&mut span, "iced::widget::text::Span"))
                .collect::<Result<Vec<_>, _>>()?;
            new_rich(call, spans)
        })
        .expect("the iced facade must retain rich_text");
    builder
        .bind_macro_with_context("iced::widget::rich_text", |call, args| {
            let spans = args
                .iter_mut()
                .map(|span| take::<marker::Span>(span, "iced::widget::text::Span"))
                .collect::<Result<Vec<_>, _>>()?;
            new_rich(call, spans)
        })
        .expect("the iced facade must retain the rich_text macro");
    for name in ["size", "font", "color", "color_maybe", "width", "height", "on_link_click"] {
        let path = format!("iced::widget::text::Rich::{name}");
        builder
            .bind(path.clone(), move |args| {
                if args.len() != 2 {
                    return Err(arity(&path, 2, args.len()));
                }
                let mut rich = take::<marker::Rich>(args[0], "iced::widget::text::Rich")?;
                match name {
                    "size" => rich.size = Some(pixels(args[1], &path)?),
                    "font" => rich.font = Some(take(args[1], "iced::Font")?),
                    "color" => rich.color = Some(take(args[1], "iced::Color")?),
                    "color_maybe" => rich.color = maybe(args[1], "iced::Color")?,
                    "width" => rich.width = Some(length(args[1])?),
                    "height" => rich.height = Some(length(args[1])?),
                    _ => rich.on_link_click = Some(take(args[1], "on_link_click function")?),
                }
                Ok(Dynamic::from(rich))
            })
            .expect("the iced facade must retain every Rich builder");
    }
}

fn new_rich(
    call: &host::runtime::Call<'_>,
    spans: Vec<marker::Span>,
) -> Result<Dynamic, Box<EvalAltResult>> {
    Ok(Dynamic::from(marker::Rich {
        witness: call.output_witness().map_err(witness_error)?,
        identity: widget::Identity::unique(),
        spans,
        on_link_click: None,
        size: None,
        font: None,
        color: None,
        width: None,
        height: None,
    }))
}

fn maybe<T: Clone + Send + Sync + 'static>(
    value: &mut Dynamic,
    expected: &str,
) -> Result<Option<T>, Box<EvalAltResult>> {
    match rawr::option::take(value) {
        Some(Some(mut inner)) => take(&mut inner, expected).map(Some),
        Some(None) => Ok(None),
        None => take(value, expected).map(Some),
    }
}

fn flag(value: &Dynamic, operation: &str) -> Result<bool, Box<EvalAltResult>> {
    value
        .as_bool()
        .map_err(|_| runtime_error(format!("{operation} expects a `bool`")))
}

impl widget::Descriptor for marker::Rich {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        let spans = self
            .spans
            .iter()
            .map(|span| {
                let mut native = iced::widget::span::<Dynamic>(span.text.clone())
                    .color_maybe(span.color)
                    .font_maybe(span.font)
                    .link_maybe(span.link.clone())
                    .underline(span.underline)
                    .strikethrough(span.strikethrough);
                if let Some(size) = span.size {
                    native = native.size(size);
                }
                native
            })
            .collect::<Vec<_>>();
        let mut rich =
            iced::widget::rich_text::<Dynamic, runtime::Message, iced::Theme>(spans)
                .color_maybe(self.color);
        if let Some(size) = self.size {
            rich = rich.size(size);
        }
        if let Some(font) = self.font {
            rich = rich.font(font);
        }
        if let Some(width) = self.width {
            rich = rich.width(width);
        }
        if let Some(height) = self.height {
            rich = rich.height(height);
        }
        if let Some(mapper) = &self.on_link_click {
            let mapper = context.message_mapper(mapper.clone());
            rich = rich.on_link_click(move |link| mapper.map(link));
        }
        rich.boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl Witnessed for marker::Rich {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}
