//! iced's `image`, `svg` and `qr_code` widgets, each when the project's
//! iced has it.
#![cfg_attr(
    not(all(feature = "image", feature = "svg", feature = "qr_code")),
    allow(unused_imports)
)]

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

#[cfg(any(feature = "image", feature = "svg"))]
/// Whether scripts in this process may read the files that image and svg
/// paths name (`rawr::resolve::deny_files`).
fn files() -> bool {
    rawr::resolve::files_denied().is_none()
}

#[cfg(feature = "image")]
/// `image::Handle::from_path`, unless files are denied.
fn image_from_path(path: &str) -> iced::widget::image::Handle {
    if files() {
        iced::widget::image::Handle::from_path(path)
    } else {
        blank()
    }
}

#[cfg(feature = "svg")]
/// `svg::Handle::from_path`, unless files are denied: then no bytes,
/// which draw as a missing file does.
fn svg_from_path(path: &str) -> iced::widget::svg::Handle {
    if files() {
        iced::widget::svg::Handle::from_path(path)
    } else {
        iced::widget::svg::Handle::from_memory(Vec::<u8>::new())
    }
}

#[cfg(feature = "image")]
/// `image::Handle::from_bytes`, or a blank pixel when `bytes` do not
/// start like a PNG or JPEG file. iced's tiny-skia renderer panics on an
/// image it cannot decode (see [`drawable`]), so this reads the header.
fn image_from_bytes(bytes: Vec<u8>) -> iced::widget::image::Handle {
    let header = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_dimensions().ok());
    match header {
        Some(_) => iced::widget::image::Handle::from_bytes(bytes),
        None => blank(),
    }
}

#[cfg(any(feature = "image", feature = "svg"))]
/// The bytes of a `Vec<u8>`, a `&[u8]` or a `&[u8; N]`: an array of
/// integers at runtime.
fn bytes(value: &mut Dynamic, operation: &str) -> Result<Vec<u8>, Box<EvalAltResult>> {
    let expected = || runtime_error(format!("{operation} expects bytes"));
    let array = value.take().into_array().map_err(|_| expected())?;
    array
        .iter()
        .map(|byte| {
            byte.as_int()
                .ok()
                .and_then(|byte| u8::try_from(byte).ok())
                .ok_or_else(expected)
        })
        .collect()
}

#[cfg(feature = "image")]
/// A `u32` operand of `operation`.
fn dimension(value: &Dynamic, operation: &str) -> Result<u32, Box<EvalAltResult>> {
    value
        .as_int()
        .ok()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| runtime_error(format!("{operation} expects a u32")))
}

#[cfg(feature = "image")]
/// A transparent pixel, one handle for the process.
fn blank() -> iced::widget::image::Handle {
    static BLANK: std::sync::OnceLock<iced::widget::image::Handle> = std::sync::OnceLock::new();
    BLANK
        .get_or_init(|| iced::widget::image::Handle::from_rgba(1, 1, vec![0; 4]))
        .clone()
}

pub(super) fn bind(builder: &mut host::runtime::Builder) {
    use crate::features::{on, Feature};
    #[cfg(feature = "image")]
    if on(Feature::Image) {
        bind_image(builder);
    }
    #[cfg(feature = "svg")]
    if on(Feature::Svg) {
        bind_svg(builder);
    }
    #[cfg(feature = "qr_code")]
    if on(Feature::QrCode) {
        bind_qr_code(builder);
    }
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
    #[cfg(feature = "image")]
    if on(Feature::Image) {
        for (name, method) in crate::IMAGE_FILTER_METHODS.into_iter().zip([
            iced::widget::image::FilterMethod::Linear,
            iced::widget::image::FilterMethod::Nearest,
        ]) {
            builder
                .bind(format!("iced::widget::image::FilterMethod::{name}"), move |_| {
                    Ok(Dynamic::from(method))
                })
                .expect("the iced facade must retain every image::FilterMethod");
        }
    }
}

#[cfg(any(feature = "image", feature = "svg", feature = "qr_code"))]
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

#[cfg(feature = "image")]
impl widget::Descriptor for marker::Image {
    fn lower(&self, _context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        let mut image = iced::widget::image(drawable(&self.handle));
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
        if let Some(method) = self.filter_method {
            image = image.filter_method(method);
        }
        if let Some(rotation) = self.rotation {
            image = image.rotation(rotation);
        }
        image.boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

#[cfg(feature = "image")]
/// `handle`, or a blank pixel when it names a file that does not exist or
/// holds too few pixels.
///
/// iced's wgpu renderer draws an image it cannot load as nothing, 1 pixel
/// square. iced's tiny-skia renderer panics instead: it measures the image
/// first, remembers the failed load, and expects an image when it draws.
/// A blank pixel draws the same in both.
fn drawable(handle: &iced::widget::image::Handle) -> iced::widget::image::Handle {
    match handle {
        iced::widget::image::Handle::Path(_, path) if !path.is_file() => blank(),
        // Fewer pixels than `width * height * 4` bytes do not load.
        iced::widget::image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        } if (pixels.len() as u64) < u64::from(*width) * u64::from(*height) * 4 => blank(),
        _ => handle.clone(),
    }
}

#[cfg(feature = "image")]
impl Witnessed for marker::Image {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

#[cfg(feature = "svg")]
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

#[cfg(feature = "svg")]
impl Witnessed for marker::Svg {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

#[cfg(any(feature = "image", feature = "svg", feature = "qr_code"))]
fn number(value: &Dynamic, operation: &str) -> Result<f32, Box<EvalAltResult>> {
    value
        .as_float()
        .map(|value| value as f32)
        .or_else(|_| value.as_int().map(|value| value as f32))
        .map_err(|_| runtime_error(format!("{operation} expects a number")))
}

#[cfg(feature = "image")]
fn bind_image(builder: &mut host::runtime::Builder) {
    attest::<marker::Image>(builder, "iced::widget::Image");
    builder
        .bind("iced::widget::image::Handle::from_path", |args| {
            let path = exact_string(
                args.first().ok_or_else(|| arity("image::Handle::from_path", 1, 0))?,
                "image::Handle::from_path",
            )?;
            Ok(Dynamic::from(image_from_path(&path)))
        })
        .expect("the iced facade must retain image::Handle::from_path");
    builder
        .bind("iced::widget::image::Handle::from_bytes", |args| {
            let [bytes_] = args else {
                return Err(arity("image::Handle::from_bytes", 1, args.len()));
            };
            Ok(Dynamic::from(image_from_bytes(bytes(bytes_, "image::Handle::from_bytes")?)))
        })
        .expect("the iced facade must retain image::Handle::from_bytes");
    builder
        .bind("iced::widget::image::Handle::from_rgba", |args| {
            let [width, height, pixels] = args else {
                return Err(arity("image::Handle::from_rgba", 3, args.len()));
            };
            let operation = "image::Handle::from_rgba";
            Ok(Dynamic::from(iced::widget::image::Handle::from_rgba(
                dimension(width, operation)?,
                dimension(height, operation)?,
                bytes(pixels, operation)?,
            )))
        })
        .expect("the iced facade must retain image::Handle::from_rgba");
    builder
        .bind_with_context("iced::widget::image", |call, args| {
            let [handle] = args else {
                return Err(arity("image", 1, args.len()));
            };
            let handle = if handle.is_string() {
                image_from_path(&exact_string(handle, "image")?)
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
                filter_method: None,
                rotation: None,
            }))
        })
        .expect("the iced facade must retain image");
    for name in [
        "width",
        "height",
        "content_fit",
        "opacity",
        "scale",
        "filter_method",
        "rotation",
    ] {
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
                    "rotation" => image.rotation = Some(super::angle::rotation(args[1])?),
                    "filter_method" => {
                        image.filter_method =
                            Some(take(args[1], "iced::widget::image::FilterMethod")?);
                    }
                    "opacity" => image.opacity = Some(number(args[1], &path)?),
                    _ => image.scale = Some(number(args[1], &path)?),
                }
                Ok(Dynamic::from(image))
            })
            .expect("the iced facade must retain every Image builder");
    }
}

#[cfg(feature = "svg")]
fn bind_svg(builder: &mut host::runtime::Builder) {
    attest::<marker::Svg>(builder, "iced::widget::Svg");
    builder
        .bind("iced::widget::svg::Handle::from_path", |args| {
            let path = exact_string(
                args.first().ok_or_else(|| arity("svg::Handle::from_path", 1, 0))?,
                "svg::Handle::from_path",
            )?;
            Ok(Dynamic::from(svg_from_path(&path)))
        })
        .expect("the iced facade must retain svg::Handle::from_path");
    builder
        .bind("iced::widget::svg::Handle::from_memory", |args| {
            let [memory] = args else {
                return Err(arity("svg::Handle::from_memory", 1, args.len()));
            };
            Ok(Dynamic::from(iced::widget::svg::Handle::from_memory(bytes(
                memory,
                "svg::Handle::from_memory",
            )?)))
        })
        .expect("the iced facade must retain svg::Handle::from_memory");
    builder
        .bind_with_context("iced::widget::svg", |call, args| {
            let [handle] = args else {
                return Err(arity("svg", 1, args.len()));
            };
            let handle = if handle.is_string() {
                svg_from_path(&exact_string(handle, "svg")?)
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

#[cfg(feature = "qr_code")]
/// iced's `QRCode` borrows its data; this owns it and builds the borrowing
/// widget for each call.
struct OwnedQrCode {
    data: iced::widget::qr_code::Data,
    cell_size: Option<f32>,
    total_size: Option<f32>,
}

#[cfg(feature = "qr_code")]
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

#[cfg(feature = "qr_code")]
impl iced::advanced::widget::Meta for OwnedQrCode {}

#[cfg(feature = "qr_code")]
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

#[cfg(feature = "qr_code")]
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

#[cfg(feature = "qr_code")]
impl Witnessed for marker::QrCode {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

#[cfg(feature = "qr_code")]
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
