//! iced's `image`, `svg` and `qr_code` widgets.

use std::sync::Arc;

use iced::advanced::widget::{tree, Tree};
use iced::advanced::{layout, mouse, renderer};
use iced::widget::Widget as _;
use rawr::host;
use rawr::{Dynamic, EvalAltResult};

use super::builtin::{
    arity, exact_string, length, pixels, project, runtime_error, take, witness_error, Witnessed,
};
use crate::{marker, runtime, widget};

pub(super) fn bind(builder: &mut host::runtime::Builder) {
    bind_image(builder);
    bind_svg(builder);
    bind_qr_code(builder);
    for (name, fit) in crate::CONTENT_FITS.into_iter().zip([
        iced::ContentFit::Contain,
        iced::ContentFit::Cover,
        iced::ContentFit::Fill,
        iced::ContentFit::None,
        iced::ContentFit::ScaleDown,
    ]) {
        builder
            .bind(format!("iced::ContentFit::{name}"), move |_| Ok(Dynamic::from(fit)))
            .expect("the iced facade must retain every ContentFit");
    }
}

fn attest<T>(builder: &mut host::runtime::Builder, path: &'static str)
where
    T: widget::Descriptor + Witnessed + Clone,
{
    builder
        .bind_projection_with_context(path, "iced::Element", move |call, args| {
            project::<T>(call, args, path)
        })
        .expect("the iced facade must retain each widget's projection into Element");
}

impl widget::Descriptor for marker::Image {
    fn lower(&self, _context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        let mut image = iced::widget::image(self.handle.clone());
        if let Some(width) = self.width {
            image = image.width(width);
        }
        if let Some(height) = self.height {
            image = image.height(height);
        }
        if let Some(fit) = self.content_fit {
            image = image.content_fit(fit);
        }
        if let Some(opacity) = self.opacity {
            image = image.opacity(opacity);
        }
        if let Some(scale) = self.scale {
            image = image.scale(scale);
        }
        image.boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl Witnessed for marker::Image {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

impl widget::Descriptor for marker::Svg {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        let mut svg = iced::widget::svg(self.handle.clone());
        if let Some(width) = self.width {
            svg = svg.width(width);
        }
        if let Some(height) = self.height {
            svg = svg.height(height);
        }
        if let Some(fit) = self.content_fit {
            svg = svg.content_fit(fit);
        }
        if let Some(opacity) = self.opacity {
            svg = svg.opacity(opacity);
        }
        match super::style::lower(&self.style, context, &super::style::SVG, "svg") {
            Ok(Some(style)) => svg = svg.style(style),
            Ok(None) => {}
            Err(diagnostic) => return diagnostic,
        }
        svg.boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl Witnessed for marker::Svg {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

fn number(value: &Dynamic, operation: &str) -> Result<f32, Box<EvalAltResult>> {
    value
        .as_float()
        .map(|value| value as f32)
        .or_else(|_| value.as_int().map(|value| value as f32))
        .map_err(|_| runtime_error(format!("{operation} expects a number")))
}

fn bind_image(builder: &mut host::runtime::Builder) {
    attest::<marker::Image>(builder, "iced::widget::Image");
    builder
        .bind("iced::widget::image::Handle::from_path", |args| {
            let path = exact_string(
                args.first().ok_or_else(|| arity("image::Handle::from_path", 1, 0))?,
                "image::Handle::from_path",
            )?;
            Ok(Dynamic::from(iced::widget::image::Handle::from_path(path)))
        })
        .expect("the iced facade must retain image::Handle::from_path");
    builder
        .bind_with_context("iced::widget::image", |call, args| {
            let [handle] = args else {
                return Err(arity("image", 1, args.len()));
            };
            let handle = if handle.is_string() {
                iced::widget::image::Handle::from_path(exact_string(handle, "image")?)
            } else {
                take::<iced::widget::image::Handle>(handle, "iced::widget::image::Handle")?
            };
            Ok(Dynamic::from(marker::Image {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                handle,
                width: None,
                height: None,
                content_fit: None,
                opacity: None,
                scale: None,
            }))
        })
        .expect("the iced facade must retain image");
    for name in ["width", "height", "content_fit", "opacity", "scale"] {
        let path = format!("iced::widget::Image::{name}");
        builder
            .bind(path.clone(), move |args| {
                if args.len() != 2 {
                    return Err(arity(&path, 2, args.len()));
                }
                let mut image = take::<marker::Image>(args[0], "iced::widget::Image")?;
                match name {
                    "width" => image.width = Some(length(args[1])?),
                    "height" => image.height = Some(length(args[1])?),
                    "content_fit" => {
                        image.content_fit = Some(take(args[1], "iced::ContentFit")?);
                    }
                    "opacity" => image.opacity = Some(number(args[1], &path)?),
                    _ => image.scale = Some(number(args[1], &path)?),
                }
                Ok(Dynamic::from(image))
            })
            .expect("the iced facade must retain every Image builder");
    }
}

fn bind_svg(builder: &mut host::runtime::Builder) {
    attest::<marker::Svg>(builder, "iced::widget::Svg");
    builder
        .bind("iced::widget::svg::Handle::from_path", |args| {
            let path = exact_string(
                args.first().ok_or_else(|| arity("svg::Handle::from_path", 1, 0))?,
                "svg::Handle::from_path",
            )?;
            Ok(Dynamic::from(iced::widget::svg::Handle::from_path(path)))
        })
        .expect("the iced facade must retain svg::Handle::from_path");
    builder
        .bind_with_context("iced::widget::svg", |call, args| {
            let [handle] = args else {
                return Err(arity("svg", 1, args.len()));
            };
            let handle = if handle.is_string() {
                iced::widget::svg::Handle::from_path(exact_string(handle, "svg")?)
            } else {
                take::<iced::widget::svg::Handle>(handle, "iced::widget::svg::Handle")?
            };
            Ok(Dynamic::from(marker::Svg {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                handle,
                width: None,
                height: None,
                content_fit: None,
                opacity: None,
                style: None,
            }))
        })
        .expect("the iced facade must retain svg");
    for name in ["width", "height", "content_fit", "opacity"] {
        let path = format!("iced::widget::Svg::{name}");
        builder
            .bind(path.clone(), move |args| {
                if args.len() != 2 {
                    return Err(arity(&path, 2, args.len()));
                }
                let mut svg = take::<marker::Svg>(args[0], "iced::widget::Svg")?;
                match name {
                    "width" => svg.width = Some(length(args[1])?),
                    "height" => svg.height = Some(length(args[1])?),
                    "content_fit" => svg.content_fit = Some(take(args[1], "iced::ContentFit")?),
                    _ => svg.opacity = Some(number(args[1], &path)?),
                }
                Ok(Dynamic::from(svg))
            })
            .expect("the iced facade must retain every Svg builder");
    }
}

/// iced's `QRCode` borrows its data; this owns it and builds the borrowing
/// widget for each call.
struct OwnedQrCode {
    data: iced::widget::qr_code::Data,
    cell_size: Option<f32>,
    total_size: Option<f32>,
}

impl OwnedQrCode {
    fn widget(&self) -> iced::widget::QRCode<'_, iced::Theme> {
        let mut code = iced::widget::QRCode::new(&self.data);
        if let Some(size) = self.cell_size {
            code = code.cell_size(size);
        }
        if let Some(size) = self.total_size {
            code = code.total_size(size);
        }
        code
    }
}

impl iced::advanced::widget::Meta for OwnedQrCode {}

impl iced::advanced::Widget<runtime::Message, iced::Theme, iced::Renderer> for OwnedQrCode {
    fn tag(&self) -> tree::Tag {
        iced::advanced::Widget::<runtime::Message, iced::Theme, iced::Renderer>::tag(&self.widget())
    }

    fn state(&self) -> tree::State {
        iced::advanced::Widget::<runtime::Message, iced::Theme, iced::Renderer>::state(
            &self.widget(),
        )
    }

    fn size(&self) -> iced::Size<iced::Length> {
        iced::advanced::Widget::<runtime::Message, iced::Theme, iced::Renderer>::size(
            &self.widget(),
        )
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) {
        iced::advanced::Widget::<runtime::Message, iced::Theme, iced::Renderer>::layout(
            &mut self.widget(),
            tree,
            renderer,
            limits,
        );
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: iced::advanced::Layout,
        cursor: mouse::Cursor,
        viewport: &iced::Rectangle,
    ) {
        iced::advanced::Widget::<runtime::Message, iced::Theme, iced::Renderer>::draw(
            &self.widget(),
            tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
}

impl widget::Descriptor for marker::QrCode {
    fn lower(&self, _context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        // The text was encoded when the data was made, so it encodes again.
        match iced::widget::qr_code::Data::new(self.data.0.as_bytes()) {
            Ok(data) => OwnedQrCode {
                data,
                cell_size: self.cell_size,
                total_size: self.total_size,
            }
            .boxed(),
            Err(error) => iced::widget::text(error.to_string()).boxed(),
        }
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl Witnessed for marker::QrCode {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

fn bind_qr_code(builder: &mut host::runtime::Builder) {
    attest::<marker::QrCode>(builder, "iced::widget::QRCode");
    builder
        .bind("iced::widget::qr_code::Data::new", |args| {
            let text = exact_string(
                args.first().ok_or_else(|| arity("qr_code::Data::new", 1, 0))?,
                "qr_code::Data::new",
            )?;
            Ok(rawr::result::from(
                iced::widget::qr_code::Data::new(text.as_bytes())
                    .map(|_| marker::QrData(Arc::from(text.as_str()))),
            ))
        })
        .expect("the iced facade must retain qr_code::Data::new");
    builder
        .bind_with_context("iced::widget::qr_code", |call, args| {
            let [data] = args else {
                return Err(arity("qr_code", 1, args.len()));
            };
            Ok(Dynamic::from(marker::QrCode {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                data: take::<marker::QrData>(data, "iced::widget::qr_code::Data")?,
                cell_size: None,
                total_size: None,
            }))
        })
        .expect("the iced facade must retain qr_code");
    for name in ["cell_size", "total_size"] {
        let path = format!("iced::widget::QRCode::{name}");
        builder
            .bind(path.clone(), move |args| {
                if args.len() != 2 {
                    return Err(arity(&path, 2, args.len()));
                }
                let mut code = take::<marker::QrCode>(args[0], "iced::widget::QRCode")?;
                let size = pixels(args[1], &path)?;
                if name == "cell_size" {
                    code.cell_size = Some(size);
                    code.total_size = None;
                } else {
                    code.total_size = Some(size);
                    code.cell_size = None;
                }
                Ok(Dynamic::from(code))
            })
            .expect("the iced facade must retain every QRCode builder");
    }
}
