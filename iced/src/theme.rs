//! Inert application themes and the Rawr facade bindings that create them.

mod palette;

pub use palette::Palette;

use rawr::host;
use rawr::{Dynamic, EvalAltResult, Position};

/// One fully materialized native iced theme.
#[derive(Clone, Debug, PartialEq)]
pub struct Value {
    native: iced::Theme,
}

/// A Rawr theme callback returned a value outside the inert theme boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error {
    actual: &'static str,
}

impl Value {
    /// Build a custom native iced theme from inert Rawr-authored data.
    #[must_use]
    pub fn custom(name: impl Into<String>, palette: Palette) -> Self {
        Self {
            native: iced::Theme::custom(name.into(), palette.seed()),
        }
    }

    /// Return the materialized iced theme.
    #[must_use]
    pub const fn native(&self) -> &iced::Theme {
        &self.native
    }

    /// Promote an opaque Rawr value only when it contains this exact theme type.
    pub fn from_dynamic(value: Dynamic) -> Result<Self, Error> {
        value.try_cast_result::<Self>().map_err(|actual| Error {
            actual: actual.type_name(),
        })
    }

    fn built_in(native: iced::Theme) -> Self {
        Self { native }
    }
}

impl Default for Value {
    fn default() -> Self {
        Self::built_in(iced::Theme::Light)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Rawr theme callback returned `{}`, expected `iced::Theme`",
            self.actual
        )
    }
}

impl std::error::Error for Error {}

pub(crate) fn bind(iced: &mut host::runtime::Builder, rawr: &mut host::runtime::Builder) {
    bind_values(iced);
    bind_constructors(rawr);
}

fn bind_values(iced: &mut host::runtime::Builder) {
    bind_palette(iced);
    for (path, theme) in built_in_themes() {
        iced.bind(path, move |_| {
            Ok(Dynamic::from(Value::built_in(theme.clone())))
        })
        .expect("the iced facade must retain every built-in Theme value");
    }
}

fn bind_palette(iced: &mut host::runtime::Builder) {
    use iced::theme::palette::{Background, Pair, Seed, Swatch};
    use iced::theme::Palette;

    iced.bind("iced::Theme::palette", |args| {
        require_arity("Theme::palette", args, 1)?;
        let theme = take::<Value>(args[0], "iced::Theme")?;
        Ok(Dynamic::from(*theme.native.palette()))
    })
    .expect("the iced facade must retain Theme::palette");
    iced.bind("iced::Theme::seed", |args| {
        require_arity("Theme::seed", args, 1)?;
        let theme = take::<Value>(args[0], "iced::Theme")?;
        Ok(Dynamic::from(theme.native.seed()))
    })
    .expect("the iced facade must retain Theme::seed");

    /// Bind one field reader of `T`.
    fn field<T: Clone + 'static, F: Clone + Send + Sync + 'static>(
        iced: &mut host::runtime::Builder,
        path: impl Into<String>,
        read: fn(&T) -> F,
    ) {
        let path = path.into();
        let owner = path.clone();
        iced.bind(path, move |args| {
            require_arity(&owner, args, 1)?;
            // A shared receiver is read in place, never taken.
            let value = args[0].read_lock::<T>().ok_or_else(|| {
                runtime_error(format!(
                    "native theme binding received `{}`, expected `{owner}`",
                    args[0].type_name()
                ))
            })?;
            Ok(Dynamic::from(read(&value)))
        })
        .expect("the iced facade must retain every palette field");
    }
    field::<Palette, _>(iced, "iced::theme::Palette::background", |p| p.background);
    field::<Palette, _>(iced, "iced::theme::Palette::primary", |p| p.primary);
    field::<Palette, _>(iced, "iced::theme::Palette::secondary", |p| p.secondary);
    field::<Palette, _>(iced, "iced::theme::Palette::success", |p| p.success);
    field::<Palette, _>(iced, "iced::theme::Palette::warning", |p| p.warning);
    field::<Palette, _>(iced, "iced::theme::Palette::danger", |p| p.danger);
    field::<Palette, _>(iced, "iced::theme::Palette::is_dark", |p| p.is_dark);
    let background = "iced::theme::palette::Background";
    for (name, read) in [
        ("base", (|b| b.base) as fn(&Background) -> Pair),
        ("weakest", |b| b.weakest),
        ("weaker", |b| b.weaker),
        ("weak", |b| b.weak),
        ("neutral", |b| b.neutral),
        ("strong", |b| b.strong),
        ("stronger", |b| b.stronger),
        ("strongest", |b| b.strongest),
    ] {
        field(iced, format!("{background}::{name}"), read);
    }
    for (name, read) in [
        ("base", (|s| s.base) as fn(&Swatch) -> Pair),
        ("weak", |s| s.weak),
        ("strong", |s| s.strong),
    ] {
        field(iced, format!("iced::theme::palette::Swatch::{name}"), read);
    }
    field::<Pair, _>(iced, "iced::theme::palette::Pair::color", |p| p.color);
    field::<Pair, _>(iced, "iced::theme::palette::Pair::text", |p| p.text);
    // A `Seed` field is read (`seed.text`) or set by a struct literal's
    // builder (`Seed { text, .. }`), which starts from `Seed::default()`.
    type SeedField = (&'static str, fn(&Seed) -> iced::Color, fn(&mut Seed, iced::Color));
    let fields: [SeedField; 6] = [
        ("background", |s| s.background, |s, c| s.background = c),
        ("text", |s| s.text, |s, c| s.text = c),
        ("primary", |s| s.primary, |s, c| s.primary = c),
        ("success", |s| s.success, |s, c| s.success = c),
        ("warning", |s| s.warning, |s, c| s.warning = c),
        ("danger", |s| s.danger, |s, c| s.danger = c),
    ];
    for (name, read, write) in fields {
        let path = format!("iced::theme::palette::Seed::{name}");
        let owner = path.clone();
        iced.bind(path, move |args| {
            // A shared receiver is read in place, never taken.
            if args.len() == 1 {
                let seed = args[0].read_lock::<Seed>().ok_or_else(|| {
                    runtime_error(format!(
                        "native theme binding received `{}`, expected `{owner}`",
                        args[0].type_name()
                    ))
                })?;
                return Ok(Dynamic::from(read(&seed)));
            }
            require_arity(&owner, args, 2)?;
            let color = take::<iced::Color>(args[1], "iced::Color")?;
            let mut seed = take::<Seed>(args[0], "iced::theme::palette::Seed")?;
            write(&mut seed, color);
            Ok(Dynamic::from(seed))
        })
        .expect("the iced facade must retain every seed field");
    }
    iced.bind("iced::theme::palette::Seed::default", |args| {
        require_arity("Seed::default", args, 0)?;
        Ok(Dynamic::from(Seed::LIGHT))
    })
    .expect("the iced facade must retain Seed::default");
    iced.bind("iced::Theme::custom", |args| {
        require_arity("Theme::custom", args, 2)?;
        let name = string(args[0], "Theme::custom")?;
        let seed = take::<Seed>(args[1], "iced::theme::palette::Seed")?;
        Ok(Dynamic::from(Value {
            native: iced::Theme::custom(name, seed),
        }))
    })
    .expect("the iced facade must retain Theme::custom");
}

fn bind_constructors(builder: &mut host::runtime::Builder) {
    builder
        .bind("rawr_iced::theme::Palette::new", |args| {
            require_arity("Palette::new", args, 6)?;
            let colors = args
                .iter()
                .map(|value| string(value, "Palette::new"))
                .collect::<Result<Vec<_>, _>>()?;
            Palette::from_hex(
                &colors[0], &colors[1], &colors[2], &colors[3], &colors[4], &colors[5],
            )
            .map(Dynamic::from)
            .map_err(|error| runtime_error(error.to_string()))
        })
        .expect("the rawr_iced facade must retain theme::Palette::new");
    builder
        .bind("rawr_iced::theme::custom", |args| {
            require_arity("rawr_iced::theme::custom", args, 2)?;
            let name = string(args[0], "rawr_iced::theme::custom")?;
            let palette = take::<Palette>(args[1], "rawr_iced::theme::Palette")?;
            Ok(Dynamic::from(Value::custom(name, palette)))
        })
        .expect("the rawr_iced facade must retain theme::custom");
}

fn built_in_themes() -> [(&'static str, iced::Theme); 22] {
    [
        ("iced::Theme::Light", iced::Theme::Light),
        ("iced::Theme::Dark", iced::Theme::Dark),
        ("iced::Theme::Dracula", iced::Theme::Dracula),
        ("iced::Theme::Nord", iced::Theme::Nord),
        ("iced::Theme::SolarizedLight", iced::Theme::SolarizedLight),
        ("iced::Theme::SolarizedDark", iced::Theme::SolarizedDark),
        ("iced::Theme::GruvboxLight", iced::Theme::GruvboxLight),
        ("iced::Theme::GruvboxDark", iced::Theme::GruvboxDark),
        ("iced::Theme::CatppuccinLatte", iced::Theme::CatppuccinLatte),
        (
            "iced::Theme::CatppuccinFrappe",
            iced::Theme::CatppuccinFrappe,
        ),
        (
            "iced::Theme::CatppuccinMacchiato",
            iced::Theme::CatppuccinMacchiato,
        ),
        ("iced::Theme::CatppuccinMocha", iced::Theme::CatppuccinMocha),
        ("iced::Theme::TokyoNight", iced::Theme::TokyoNight),
        ("iced::Theme::TokyoNightStorm", iced::Theme::TokyoNightStorm),
        ("iced::Theme::TokyoNightLight", iced::Theme::TokyoNightLight),
        ("iced::Theme::KanagawaWave", iced::Theme::KanagawaWave),
        ("iced::Theme::KanagawaDragon", iced::Theme::KanagawaDragon),
        ("iced::Theme::KanagawaLotus", iced::Theme::KanagawaLotus),
        ("iced::Theme::Moonfly", iced::Theme::Moonfly),
        ("iced::Theme::Nightfly", iced::Theme::Nightfly),
        ("iced::Theme::Oxocarbon", iced::Theme::Oxocarbon),
        ("iced::Theme::Ferra", iced::Theme::Ferra),
    ]
}

fn require_arity(
    operation: &str,
    args: &[&mut Dynamic],
    expected: usize,
) -> Result<(), Box<EvalAltResult>> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(runtime_error(format!(
            "native theme binding `{operation}` received {} operand(s), expected {expected}",
            args.len()
        )))
    }
}

fn string(value: &Dynamic, operation: &str) -> Result<String, Box<EvalAltResult>> {
    value
        .as_immutable_string_ref()
        .map(|value| value.to_string())
        .map_err(|_| {
            runtime_error(format!(
                "{operation} expects `String`, found `{}`",
                value.type_name()
            ))
        })
}

fn take<T>(value: &mut Dynamic, expected: &str) -> Result<T, Box<EvalAltResult>>
where
    T: Clone + 'static,
{
    let actual = value.type_name();
    value.take().try_cast_result::<T>().map_err(|_| {
        runtime_error(format!(
            "native theme binding received `{actual}`, expected `{expected}`"
        ))
    })
}

fn runtime_error(message: String) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into()
}

/// A `Color` in State outlives a `.rs` edit: its four channels.
pub(crate) struct CarryColor;

impl host::value::Carry for CarryColor {
    type Value = iced::Color;

    fn encode(color: &iced::Color, out: &mut Vec<u8>) {
        for channel in [color.r, color.g, color.b, color.a] {
            out.extend_from_slice(&channel.to_le_bytes());
        }
    }

    fn decode(bytes: &[u8]) -> Option<iced::Color> {
        let channel = |index: usize| {
            Some(f32::from_le_bytes(
                bytes.get(index * 4..index * 4 + 4)?.try_into().ok()?,
            ))
        };
        (bytes.len() == 16).then_some(())?;
        Some(iced::Color::from_rgba(
            channel(0)?,
            channel(1)?,
            channel(2)?,
            channel(3)?,
        ))
    }
}

/// A `Theme` in State outlives a `.rs` edit: a built-in one by its place in
/// `Theme::ALL`, a custom one by its seed colors and name.
pub(crate) struct CarryTheme;

impl host::value::Carry for CarryTheme {
    type Value = Value;

    fn encode(theme: &Value, out: &mut Vec<u8>) {
        if let Some(index) = iced::Theme::ALL.iter().position(|t| *t == theme.native) {
            out.extend_from_slice(&(index as u32).to_le_bytes());
            return;
        }
        out.extend_from_slice(&u32::MAX.to_le_bytes());
        let seed = theme.native.seed();
        for color in [
            seed.background,
            seed.text,
            seed.primary,
            seed.success,
            seed.warning,
            seed.danger,
        ] {
            CarryColor::encode(&color, out);
        }
        out.extend_from_slice(theme.native.to_string().as_bytes());
    }

    fn decode(bytes: &[u8]) -> Option<Value> {
        let index = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?);
        if index != u32::MAX {
            return (bytes.len() == 4)
                .then(|| iced::Theme::ALL.get(index as usize).cloned())
                .flatten()
                .map(Value::built_in);
        }
        let color = |index: usize| CarryColor::decode(bytes.get(4 + index * 16..4 + index * 16 + 16)?);
        let seed = iced::theme::palette::Seed {
            background: color(0)?,
            text: color(1)?,
            primary: color(2)?,
            success: color(3)?,
            warning: color(4)?,
            danger: color(5)?,
        };
        let name = std::str::from_utf8(bytes.get(4 + 6 * 16..)?).ok()?;
        Some(Value::built_in(iced::Theme::custom(name.to_owned(), seed)))
    }
}
