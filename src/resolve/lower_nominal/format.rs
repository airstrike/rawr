//! Format strings and their specs: `format!`'s arguments, `Debug` text,
//! and the panic and assert macros that format.

use super::lvalue::Lvalue;
use super::models::{Models, Nominal};
use super::op::PreludeOp;
use super::ty::{Float, Int, Ty};
use super::{
    cell, counted, cow, origin, point_span, span, split_arguments, text_end, thread, Binding,
    BindingKind, ParseError, ParsedExpr, Parser,
};
use crate::resolve::{type_ref, Error};
use crate::tokenizer::Token;
use crate::types::dynamic::Union;
use crate::{Dynamic, ImmutableString, Position, Span, INT};
use std::collections::BTreeSet;
use std::fmt::Write;

/// A `.into()` whose `From` impl is chosen when its target is known.
/// A `format!` piece whose value's type was not known yet where it was
/// written: a closure parameter typed by a call later in the body. It is
/// written over its placeholder once the callable's types are settled.
pub(super) struct Pending {
    pub(super) placeholder: String,
    pub(super) ty: Ty,
    pub(super) name: String,
    pub(super) debug: bool,
    pub(super) alternate: bool,
    pub(super) precision: Option<String>,
    pub(super) span: Span,
}

/// The width of a pending `format!` piece's placeholder: room for the
/// piece once its type is known.
pub(super) const PENDING_FORMAT_WIDTH: usize = 160;

pub(super) fn format_placeholder(index: usize) -> String {
    let head = format!("__rawr_nominal_format_{index:010}");
    format!("{head:_<PENDING_FORMAT_WIDTH$}")
}

/// An argument a format string names: `{0}`, `{name}`, `1$`, `name$`.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Argument {
    Index(usize),
    Name(String),
}

/// A width or precision as written: `5`, `1$`, `name$` or `.*`.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum CountRef {
    Literal(usize),
    Argument(Argument),
    Star,
}

/// A width or precision: a literal or the argument holding it.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Count {
    Literal(usize),
    Argument(usize),
}

/// The trait a placeholder formats with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Kind {
    Display,
    /// `?`.
    Debug,
    /// `x`, `X`, `o`, `b`: `16`, `-16`, `8`, `2`.
    Radix(i8),
    /// `x?`, `X?`: `Debug` with hexadecimal integers.
    HexDebug(i8),
    /// `e`, `E`.
    Exp {
        upper: bool,
    },
}

/// std::fmt's `format_spec`, before its arguments are resolved:
/// `[[fill]align][sign]['#']['0'][width]['.' precision]type`.
pub(super) struct RawSpec {
    pub(super) fill: char,
    pub(super) align: Option<char>,
    pub(super) sign: bool,
    pub(super) alternate: bool,
    pub(super) zero: bool,
    pub(super) width: Option<CountRef>,
    pub(super) precision: Option<CountRef>,
    pub(super) kind: Kind,
}

/// Why a placeholder's `format_spec` is not one std::fmt reads.
#[derive(Debug, PartialEq)]
pub(super) enum InvalidSpec {
    /// A `.` with no precision after it.
    MissingPrecision,
    /// The trait letters after the spec's counts, which name no trait.
    UnknownTrait(String),
}

impl std::fmt::Display for InvalidSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingPrecision => {
                f.write_str("invalid format string: expected a precision after `.`")
            }
            Self::UnknownTrait(name) => write!(f, "unknown format trait `{name}`"),
        }
    }
}

impl RawSpec {
    pub(super) fn parse(spec: &str) -> Result<Self, InvalidSpec> {
        let chars: Vec<char> = spec.chars().collect();
        let mut at = 0;
        let mut fill = ' ';
        let mut align = None;
        if chars.len() >= 2 && matches!(chars[1], '<' | '^' | '>') {
            fill = chars[0];
            align = Some(chars[1]);
            at = 2;
        } else if chars.first().is_some_and(|c| matches!(c, '<' | '^' | '>')) {
            align = Some(chars[0]);
            at = 1;
        }
        let sign = chars.get(at) == Some(&'+');
        if matches!(chars.get(at), Some('+' | '-')) {
            at += 1;
        }
        let alternate = chars.get(at) == Some(&'#');
        at += usize::from(alternate);
        // `0$` is a width argument, not the `0` flag.
        let zero = chars.get(at) == Some(&'0') && chars.get(at + 1) != Some(&'$');
        at += usize::from(zero);
        let integer = |at: &mut usize| -> Option<usize> {
            let start = *at;
            while chars.get(*at).is_some_and(char::is_ascii_digit) {
                *at += 1;
            }
            chars[start..*at].iter().collect::<String>().parse().ok()
        };
        let identifier = |at: usize| -> Option<(String, usize)> {
            let start = at;
            let mut end = at;
            if !chars
                .get(end)
                .is_some_and(|c| c.is_alphabetic() || *c == '_')
            {
                return None;
            }
            while chars
                .get(end)
                .is_some_and(|c| c.is_alphanumeric() || *c == '_')
            {
                end += 1;
            }
            Some((chars[start..end].iter().collect(), end))
        };
        // `count := integer | integer '$' | identifier '$'`.
        let count = |at: &mut usize| -> Option<CountRef> {
            let start = *at;
            if let Some(value) = integer(at) {
                if chars.get(*at) == Some(&'$') {
                    *at += 1;
                    return Some(CountRef::Argument(Argument::Index(value)));
                }
                return Some(CountRef::Literal(value));
            }
            if let Some((name, end)) = identifier(start)
                && chars.get(end) == Some(&'$')
            {
                *at = end + 1;
                return Some(CountRef::Argument(Argument::Name(name)));
            }
            None
        };
        let width = count(&mut at);
        let precision = if chars.get(at) == Some(&'.') {
            at += 1;
            if chars.get(at) == Some(&'*') {
                at += 1;
                Some(CountRef::Star)
            } else {
                Some(count(&mut at).ok_or(InvalidSpec::MissingPrecision)?)
            }
        } else {
            None
        };
        let kind = match chars[at..].iter().collect::<String>().as_str() {
            "" => Kind::Display,
            "?" => Kind::Debug,
            "x" => Kind::Radix(16),
            "X" => Kind::Radix(-16),
            "o" => Kind::Radix(8),
            "b" => Kind::Radix(2),
            "x?" => Kind::HexDebug(16),
            "X?" => Kind::HexDebug(-16),
            "e" => Kind::Exp { upper: false },
            "E" => Kind::Exp { upper: true },
            other => return Err(InvalidSpec::UnknownTrait(other.to_string())),
        };
        Ok(Self {
            fill,
            align,
            sign,
            alternate,
            zero,
            width,
            precision,
            kind,
        })
    }

    /// The argument `key` (before the `:`) names: `None` for the next
    /// positional one.
    pub(super) fn argument_of(&self, key: &str) -> Option<Argument> {
        if key.is_empty() {
            None
        } else if let Ok(index) = key.parse::<usize>() {
            Some(Argument::Index(index))
        } else {
            Some(Argument::Name(key.to_string()))
        }
    }

    pub(super) fn resolved(self, width: Option<Count>, precision: Option<Count>) -> Spec {
        Spec {
            fill: self.fill,
            align: self.align,
            sign: self.sign,
            alternate: self.alternate,
            zero: self.zero,
            width,
            precision,
            kind: self.kind,
        }
    }
}

/// A placeholder's spec with its width and precision resolved.
#[derive(Clone)]
pub(super) struct Spec {
    pub(super) fill: char,
    pub(super) align: Option<char>,
    pub(super) sign: bool,
    pub(super) alternate: bool,
    pub(super) zero: bool,
    pub(super) width: Option<Count>,
    pub(super) precision: Option<Count>,
    pub(super) kind: Kind,
}

impl Spec {
    /// A Rawr block that pads the text named `name` as Rust would: `width`
    /// and `precision` are Rawr expressions (a literal or a local).
    pub(super) fn render(
        &self,
        name: &str,
        number: bool,
        width: Option<&str>,
        precision: Option<&str>,
    ) -> String {
        let width = width.unwrap_or("0");
        let mut out = format!("{{ let s = \"\" + {name};");
        if let Some(precision) = precision {
            out += &format!(" if s.len() > {precision} {{ s = s.sub_string(0, {precision}); }}");
        }
        if number {
            out +=
                " let sign = \"\"; if s.starts_with(\"-\") { sign = \"-\"; s = s.sub_string(1); }";
            if self.sign {
                out += " else { sign = \"+\"; }";
            }
            let radix = match self.kind {
                Kind::Radix(radix) | Kind::HexDebug(radix) => Some(radix),
                _ => None,
            };
            if let (true, Some(radix)) = (self.alternate, radix) {
                let prefix = match radix {
                    2 => "0b",
                    8 => "0o",
                    _ => "0x",
                };
                out += &format!(" sign += \"{prefix}\";");
            }
            if self.zero {
                out += &format!(
                    " let p = \"\"; p.pad({width} - sign.len() - s.len(), '0'); s = sign + p + s; s }}"
                );
                return out;
            }
            out += " s = sign + s;";
        }
        let fill = format!("{:?}", self.fill);
        let align = self.align.unwrap_or(if number { '>' } else { '<' });
        out += &match align {
            '<' => format!(" s.pad({width}, {fill}); s }}"),
            '>' => format!(" let p = \"\"; p.pad({width} - s.len(), {fill}); p + s }}"),
            _ => format!(
                " let p = \"\"; p.pad(({width} - s.len()) / 2, {fill}); s = p + s; s.pad({width}, {fill}); s }}"
            ),
        };
        out
    }
}

/// Rust's `{:#?}` from the one-line `Debug` text: a `{`, `[` or `(` with
/// members opens an indented line per member, each ending in `,`. Only the
/// one space the one-line text writes after a `{` or a `,` goes, so a
/// member's padding (`{:#6?}`) stays.
pub(super) fn pretty_debug(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let skip_spaces = |at: usize| {
        if chars.get(at) == Some(&' ') {
            at + 1
        } else {
            at
        }
    };
    let newline = |out: &mut String, depth: usize| {
        out.push('\n');
        out.push_str(&" ".repeat(depth * 4));
    };
    let mut out = String::new();
    let mut depth = 0;
    let mut at = 0;
    // Whether each open bracket is a struct's `{ `.
    let mut open = Vec::new();
    while let Some(&character) = chars.get(at) {
        match character {
            // A string or char literal, with its escapes, as it is.
            '"' | '\'' => {
                out.push(character);
                at += 1;
                while let Some(&inner) = chars.get(at) {
                    out.push(inner);
                    if inner == '\\' {
                        at += 1;
                        out.extend(chars.get(at));
                    } else if inner == character {
                        break;
                    }
                    at += 1;
                }
            }
            '{' | '[' | '(' => {
                let close = match character {
                    '{' => '}',
                    '[' => ']',
                    _ => ')',
                };
                // A struct's `S { a: 1 }` writes a space after its `{`; a
                // map's or set's `{1: 2}` does not.
                let named = at >= 2
                    && chars[at - 1] == ' '
                    && (chars[at - 2].is_alphanumeric() || chars[at - 2] == '_');
                let named = character == '{' && named;
                let next = if named { skip_spaces(at + 1) } else { at + 1 };
                out.push(character);
                if chars.get(next) == Some(&close) {
                    out.push(close);
                    at = next;
                } else {
                    open.push(named);
                    depth += 1;
                    newline(&mut out, depth);
                    at = next;
                    continue;
                }
            }
            ',' => {
                out.push(',');
                at = skip_spaces(at + 1);
                // `(1,)`: the closing bracket adds the comma.
                if matches!(chars.get(at), Some('}' | ']' | ')')) {
                    out.pop();
                } else {
                    newline(&mut out, depth);
                }
                continue;
            }
            ' ' if chars.get(at + 1) == Some(&'}') && open.last() == Some(&true) => {}
            '}' | ']' | ')' => {
                open.pop();
                out.push(',');
                depth = depth.saturating_sub(1);
                newline(&mut out, depth);
                out.push(character);
            }
            _ => out.push(character),
        }
        at += 1;
    }
    out
}

/// `value`'s `Debug` text, a raw identifier's keyword bare as Rust's
/// derive writes it (`bare_names`).
pub(super) fn debug_value(value: &Dynamic, models: &Models) -> String {
    debug_value_spec(value, models, None)
}

/// `value`'s `Debug` text under a `{:?}` spec (`DebugSpec`), which Rust's
/// derives pass down to each field: a width pads each number, as in
/// `Some(       1)`, and `{:x?}` writes each integer in hexadecimal.
pub(super) fn debug_value_spec(
    value: &Dynamic,
    models: &Models,
    spec: Option<&DebugSpec>,
) -> String {
    let text = debug_text(value, models, spec);
    match crate::tokenizer::bare_names(&text) {
        std::borrow::Cow::Borrowed(_) => text,
        std::borrow::Cow::Owned(bare) => bare,
    }
}

pub(super) fn debug_text(value: &Dynamic, models: &Models, spec: Option<&DebugSpec>) -> String {
    // An `Rc` or `Arc` writes its value; a `Weak` `(Weak)`.
    if let Some(inner) = counted::value(value) {
        return debug_value_spec(&inner, models, spec);
    }
    if counted::is_weak(value) {
        return "(Weak)".into();
    }
    if let Some((name, inner)) = cell::named_value(value) {
        // std's `Debug` of a `Mutex` no thread poisoned.
        if cell::is_mutex(value) {
            return format!(
                "Mutex {{ data: {}, poisoned: false, .. }}",
                debug_value_spec(&inner, models, spec)
            );
        }
        return format!(
            "{name} {{ value: {} }}",
            debug_value_spec(&inner, models, spec)
        );
    }
    if let Some(reverse) = value.read_lock::<std::cmp::Reverse<Dynamic>>() {
        return format!("Reverse({})", debug_value_spec(&reverse.0, models, spec));
    }
    // A `Cow` writes what it derefs to.
    if let Some(inner) = cow::payload(value) {
        return debug_value_spec(&inner, models, spec);
    }
    if let Some(option) = crate::option::read(value) {
        return match option {
            Some(inner) => format!("Some({})", debug_value_spec(&inner, models, spec)),
            None => "None".to_string(),
        };
    }
    if let Some(result) = crate::result::read(value) {
        return match result {
            Ok(inner) => format!("Ok({})", debug_value_spec(&inner, models, spec)),
            Err(inner) => format!("Err({})", debug_value_spec(&inner, models, spec)),
        };
    }
    match &value.0 {
        Union::Struct(value, ..) => {
            let Some(Nominal::Struct { item, .. }) = models.by_identity.get(value.identity())
            else {
                return value.to_string();
            };
            // `S` of `struct S;`, as Rust's `debug_struct` without fields.
            if item.fields.is_empty() {
                return item.name.text.to_string();
            }
            // `W(1, "a")`, as Rust's `debug_tuple`.
            if item.form == crate::item::StructForm::Tuple {
                let fields = item.fields.iter().zip(value.fields());
                let fields =
                    fields.map(|(field, value)| debug_typed(value, &field.ty, models, spec));
                return format!(
                    "{}({})",
                    item.name.text,
                    fields.collect::<Vec<_>>().join(", ")
                );
            }
            let mut output = format!("{} {{ ", item.name.text);
            for (index, (field, value)) in item.fields.iter().zip(value.fields()).enumerate() {
                if index > 0 {
                    output.push_str(", ");
                }
                write!(
                    output,
                    "{}: {}",
                    field.name.text,
                    debug_typed(value, &field.ty, models, spec)
                )
                .expect("writing debug output into String cannot fail");
            }
            output.push_str(" }");
            output
        }
        Union::Enum(value, ..) => {
            let Some(Nominal::Enum { item, .. }) = models.by_identity.get(value.identity()) else {
                return value.to_string();
            };
            let Some(variant) = item
                .variants
                .iter()
                .find(|variant| variant.name.text == value.variant().name())
            else {
                return value.to_string();
            };
            // Rust's derived `Debug` names the variant alone.
            if value.payload().is_empty() {
                return variant.name.text.to_string();
            }
            // `V { x: 1, y: "a" }` of a struct variant.
            if let Some(fields) = &variant.fields {
                let fields = fields
                    .iter()
                    .zip(value.payload().iter().zip(variant.payload.iter()))
                    .map(|(field, (value, ty))| {
                        format!("{}: {}", field.text, debug_typed(value, ty, models, spec))
                    });
                return format!(
                    "{} {{ {} }}",
                    variant.name.text,
                    fields.collect::<Vec<_>>().join(", ")
                );
            }
            format!(
                "{}({})",
                variant.name.text,
                value
                    .payload()
                    .iter()
                    .zip(variant.payload.iter())
                    .map(|(value, ty)| debug_typed(value, ty, models, spec))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        _ => match spec {
            Some(spec) => spec.leaf(value, DebugLeaf::of_value(value)),
            None => format!("{value:?}"),
        },
    }
}

/// `{:?}` of a field or payload of declared type `ty`: an `Ordering`,
/// an integer at runtime, prints its variant, also inside an `Option`,
/// `Vec`, `Box` or tuple; a builtin number prints as its declared type.
pub(super) fn debug_typed(
    value: &Dynamic,
    ty: &type_ref::Type,
    models: &Models,
    spec: Option<&DebugSpec>,
) -> String {
    match (ty, &value.0) {
        (type_ref::Type::Path(path), Union::Int(..) | Union::Float(..))
            if path.target == type_ref::Target::Builtin
                && let Some(leaf) = DebugLeaf::of_name(path.name.as_str()) =>
        {
            spec.unwrap_or(&DebugSpec::PLAIN).leaf(value, leaf)
        }
        // `ParseIntError` and its kin are their messages at runtime, and
        // print std's `Debug` of the error with that message.
        (type_ref::Type::Path(path), Union::Str(text, ..))
            if path.target == type_ref::Target::Builtin
                && let Some(error) = ParseError::named(path.name.as_str()) =>
        {
            error
                .texts()
                .iter()
                .find(|(message, _)| *message == text.as_str())
                .map_or_else(|| text.to_string(), |(_, debug)| (*debug).to_owned())
        }
        (type_ref::Type::Path(path), Union::Int(value, ..))
            if path.target == type_ref::Target::Builtin && path.name.as_str() == "Ordering" =>
        {
            match value {
                ..0 => "Less",
                0 => "Equal",
                _ => "Greater",
            }
            .into()
        }
        (type_ref::Type::Tuple { members, .. }, Union::Array(values, ..))
            if members.len() == values.len() =>
        {
            let members = values
                .iter()
                .zip(members.iter())
                .map(|(value, ty)| debug_typed(value, ty, models, spec))
                .collect::<Vec<_>>();
            if members.len() == 1 {
                format!("({},)", members[0])
            } else {
                format!("({})", members.join(", "))
            }
        }
        (_, _)
            if let Some(inner) = ty.reverse_element()
                && let Some(reverse) = value.read_lock::<std::cmp::Reverse<Dynamic>>() =>
        {
            format!("Reverse({})", debug_typed(&reverse.0, inner, models, spec))
        }
        // `{k: v}` and `{k}`, as Rust's maps and sets print.
        (_, Union::Array(values, ..)) if let Some((_, arguments)) = ty.keyed_arguments() => {
            let entries = values.iter().map(|entry| match (&arguments[..], &entry.0) {
                ([key, value], Union::Array(pair, ..)) if pair.len() == 2 => format!(
                    "{}: {}",
                    debug_typed(&pair[0], key, models, spec),
                    debug_typed(&pair[1], value, models, spec)
                ),
                ([key], _) => debug_typed(entry, key, models, spec),
                _ => debug_value_spec(entry, models, spec),
            });
            format!("{{{}}}", entries.collect::<Vec<_>>().join(", "))
        }
        (_, Union::Array(values, ..))
            if let Some(element) = ty
                .sequence_element()
                .or_else(|| ty.array_element().map(|(element, _)| element)) =>
        {
            format!(
                "[{}]",
                values
                    .iter()
                    .map(|value| debug_typed(value, element, models, spec))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        _ if let Some(element) = ty.option_element()
            && let Some(Some(inner)) = crate::option::read(value) =>
        {
            format!("Some({})", debug_typed(&inner, element, models, spec))
        }
        _ if let Some(element) = ty.box_element() => debug_typed(value, element, models, spec),
        _ if let Some(element) = ty.counted_element()
            && let Some(inner) = counted::value(value) =>
        {
            debug_typed(&inner, element, models, spec)
        }

        _ => debug_value_spec(value, models, spec),
    }
}

/// A builtin leaf of a `Debug` text, which a `{:?}` spec formats.
#[derive(Clone, Copy)]
pub(super) enum DebugLeaf {
    /// An integer of `bits` bits, signed or not.
    Int {
        bits: u32,
        signed: bool,
    },
    Float {
        single: bool,
    },
    Bool,
    Unit,
    /// A string, a char or another value, which ignore the spec.
    Other,
}

impl DebugLeaf {
    /// The leaf of a builtin type's name, for a declared field type.
    pub(super) fn of_name(name: &str) -> Option<Self> {
        let int = |bits, signed| Some(Self::Int { bits, signed });
        match name {
            "i8" => int(8, true),
            "u8" => int(8, false),
            "i16" => int(16, true),
            "u16" => int(16, false),
            "i32" => int(32, true),
            "u32" => int(32, false),
            "i64" | "isize" => int(64, true),
            "u64" | "usize" => int(64, false),
            "f32" => Some(Self::Float { single: true }),
            "f64" => Some(Self::Float { single: false }),
            _ => None,
        }
    }

    /// The leaf of a runtime value of unknown declared type.
    pub(super) fn of_value(value: &Dynamic) -> Self {
        match &value.0 {
            Union::Int(..) => Self::Int {
                bits: 64,
                signed: true,
            },
            Union::Float(..) => Self::Float { single: false },
            Union::Bool(..) => Self::Bool,
            Union::Unit(..) => Self::Unit,
            _ => Self::Other,
        }
    }

    /// A leaf by the code `PreludeOp::DebugLeaf` takes: 1 an `f32`, 2 an
    /// `f64`, 3 a `bool`, 4 `()`, and 1000 plus the bits for a signed
    /// integer or 2000 plus the bits for an unsigned one.
    pub(super) fn from_code(code: INT) -> Self {
        match code {
            1 => Self::Float { single: true },
            2 => Self::Float { single: false },
            3 => Self::Bool,
            4 => Self::Unit,
            1000.. => Self::Int {
                bits: u32::try_from(code % 1000).unwrap_or(64),
                signed: code < 2000,
            },
            _ => Self::Other,
        }
    }
}

/// A `{:?}` spec as Rust's `Formatter` carries it through a derived
/// `Debug`: `[hex, fill, align, flags, width, precision]` at runtime, with
/// `hex` 16 or -16 for `{:x?}` or `{:X?}`, `align` a char or `()`, `flags`
/// 1 for `+`, 2 for `#` and 4 for `0`, and -1 for no width or precision.
pub(super) struct DebugSpec {
    pub(super) hex: Option<bool>,
    pub(super) fill: char,
    pub(super) align: Option<char>,
    pub(super) sign: bool,
    pub(super) alternate: bool,
    pub(super) zero: bool,
    pub(super) width: Option<usize>,
    pub(super) precision: Option<usize>,
}

impl DebugSpec {
    pub(super) const PLAIN: Self = Self {
        hex: None,
        fill: ' ',
        align: None,
        sign: false,
        alternate: false,
        zero: false,
        width: None,
        precision: None,
    };

    pub(super) fn read(spec: &Dynamic) -> Self {
        let Ok(parts) = spec.as_array_ref() else {
            return Self::PLAIN;
        };
        let int = |at: usize| parts.get(at).and_then(|part| part.as_int().ok());
        let count = |at: usize| int(at).and_then(|value| usize::try_from(value).ok());
        let flags = int(3).unwrap_or_default();
        Self {
            hex: match int(0) {
                Some(16) => Some(false),
                Some(-16) => Some(true),
                _ => None,
            },
            fill: parts
                .get(1)
                .and_then(|fill| fill.as_char().ok())
                .unwrap_or(' '),
            align: parts.get(2).and_then(|align| align.as_char().ok()),
            sign: flags & 1 != 0,
            alternate: flags & 2 != 0,
            zero: flags & 4 != 0,
            width: count(4),
            precision: count(5),
        }
    }

    /// `text` cut to the precision and padded to the width, as
    /// `Formatter::pad` does, aligned left by default.
    pub(super) fn pad(&self, text: &str) -> String {
        let text: String = match self.precision {
            Some(precision) => text.chars().take(precision).collect(),
            None => text.to_string(),
        };
        self.align_text(&text, '<')
    }

    pub(super) fn align_text(&self, text: &str, default: char) -> String {
        let length = text.chars().count();
        let Some(width) = self.width.filter(|width| *width > length) else {
            return text.to_string();
        };
        let padding = width - length;
        let (before, after) = match self.align.unwrap_or(default) {
            '<' => (0, padding),
            '^' => (padding / 2, padding - padding / 2),
            _ => (padding, 0),
        };
        let fill = |count: usize| std::iter::repeat_n(self.fill, count).collect::<String>();
        format!("{}{text}{}", fill(before), fill(after))
    }

    /// A number's sign, prefix and digits, padded as
    /// `Formatter::pad_integral` does: `0` pads between the sign and the
    /// digits.
    pub(super) fn pad_number(&self, negative: bool, prefix: &str, digits: &str) -> String {
        let mut head = if negative {
            "-".to_string()
        } else if self.sign {
            "+".to_string()
        } else {
            String::new()
        };
        head.push_str(prefix);
        if self.zero
            && let Some(width) = self.width
        {
            let length = head.chars().count() + digits.chars().count();
            let zeros = "0".repeat(width.saturating_sub(length));
            return format!("{head}{zeros}{digits}");
        }
        self.align_text(&format!("{head}{digits}"), '>')
    }

    /// `value`'s `Debug` text as a leaf of kind `leaf`.
    pub(super) fn leaf(&self, value: &Dynamic, leaf: DebugLeaf) -> String {
        match (leaf, &value.0) {
            (DebugLeaf::Int { bits, signed }, Union::Int(value, ..)) => {
                let value = *value;
                if let Some(upper) = self.hex {
                    let all = value as u64;
                    let masked = if bits >= 64 {
                        all
                    } else {
                        all & ((1 << bits) - 1)
                    };
                    let digits = if upper {
                        format!("{masked:X}")
                    } else {
                        format!("{masked:x}")
                    };
                    let prefix = if self.alternate { "0x" } else { "" };
                    return self.pad_number(false, prefix, &digits);
                }
                if signed {
                    self.pad_number(value < 0, "", &value.unsigned_abs().to_string())
                } else {
                    self.pad_number(false, "", &(value as u64).to_string())
                }
            }
            (DebugLeaf::Float { single }, Union::Float(value, ..)) => {
                let value = **value;
                let text = match (self.precision, single) {
                    (Some(precision), true) => format!("{:.precision$}", value as f32),
                    (Some(precision), false) => format!("{value:.precision$}"),
                    (None, true) => format!("{:?}", value as f32),
                    (None, false) => format!("{value:?}"),
                };
                if value.is_nan() {
                    return self.align_text(&text, '>');
                }
                match text.strip_prefix('-') {
                    Some(digits) => self.pad_number(true, "", digits),
                    None => self.pad_number(false, "", &text),
                }
            }
            (DebugLeaf::Bool, Union::Bool(value, ..)) => self.pad(&value.to_string()),
            (DebugLeaf::Unit, Union::Unit(..)) => self.pad("()"),
            _ => format!("{value:?}"),
        }
    }
}

impl<'a> Parser<'a> {
    /// `format!` with `{}`, `{0}`, `{name}` (a named argument or a captured
    /// local) and `{{`/`}}`. Arguments are evaluated once, in order.
    pub(super) fn parse_format(&mut self, start: Span, opened: bool) -> Result<ParsedExpr, Error> {
        self.settle_parses()?;
        if !opened {
            self.expect(Token::LeftParen, "'(' after `format!`")?;
        }
        let (template, template_at) = match self.next() {
            (Token::StringConstant(template), position) => (template.to_string(), position),
            (_, position) => {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "`format!` without a string literal".into(),
                    source: point_span(position),
                })
            }
        };
        // The literal, up to the token after it.
        let template_span = span(
            template_at,
            self.input.peek().map_or(template_at, |(_, at)| *at),
        );
        let mut arguments = Vec::<(Option<ImmutableString>, ParsedExpr)>::new();
        let mut borrowed = Vec::new();
        while matches!(self.peek(), Token::Comma) {
            self.next();
            if matches!(self.peek(), Token::RightParen) {
                break;
            }
            let mut ahead = self.input.clone();
            let named = matches!(ahead.next(), Some((Token::Identifier(_), _)))
                && matches!(ahead.next(), Some((Token::Equals, _)));
            let name = if named {
                let (name, _) = self.expect_name("named `format!` argument")?;
                self.next();
                Some(name)
            } else {
                None
            };
            // A format argument is borrowed, and the borrow lasts until the
            // macro has read every argument, as rustc's `format_args!` has
            // it: a later argument that moves or changes it is refused.
            let mut argument = self.parse_expr()?;
            self.format_argument_borrow(&argument, &mut borrowed);
            self.hold_temporary(&mut argument)?;
            arguments.push((name, argument));
        }
        let end = self.expect(Token::RightParen, "')' after `format!` arguments")?;
        self.flow().use_held(&borrowed);
        let expression = span(start.start(), end);
        let positional = arguments
            .iter()
            .take_while(|(name, _)| name.is_none())
            .count();
        if arguments[positional..]
            .iter()
            .any(|(name, _)| name.is_none())
        {
            return Err(self.unsupported("positional `format!` arguments after named ones"));
        }

        let mut pieces = Vec::<Result<String, (usize, Spec)>>::new();
        let mut text = String::new();
        let mut next_positional = 0;
        let mut counted = BTreeSet::<usize>::new();
        let invalid = |reason: String| Error::InvalidHostOperation {
            item: "format!".into(),
            reason: reason.into(),
            operation: expression,
        };
        let mut chars = template.chars().peekable();
        while let Some(character) = chars.next() {
            match character {
                '{' if chars.peek() == Some(&'{') => {
                    chars.next();
                    text.push('{');
                }
                '}' if chars.peek() == Some(&'}') => {
                    chars.next();
                    text.push('}');
                }
                '{' => {
                    let mut written = String::new();
                    loop {
                        match chars.next() {
                            Some('}') => break,
                            Some(character) => written.push(character),
                            None => {
                                return Err(invalid(
                                    "invalid format string: expected `}` but string was terminated"
                                        .into(),
                                ))
                            }
                        }
                    }
                    let (key, raw) = match written.split_once(':') {
                        Some((name, spec)) => (name.trim().to_string(), spec.to_string()),
                        None => (written.trim().to_string(), String::new()),
                    };
                    let spec = RawSpec::parse(&raw).map_err(|error| invalid(error.to_string()))?;
                    // Each `{}`/`{:.*}` takes the next positional argument;
                    // `.*` takes one for the precision first.
                    let take_positional = |next: &mut usize| {
                        *next += 1;
                        *next - 1
                    };
                    let resolve = |this: &mut Self,
                                   arguments: &mut Vec<(Option<ImmutableString>, ParsedExpr)>,
                                   reference: &Argument|
                     -> Result<usize, Error> {
                        match reference {
                            Argument::Index(index) => {
                                if *index >= positional {
                                    return Err(invalid(format!(
                                        "invalid reference to positional argument {index} (there {})",
                                        match positional {
                                            1 => "is 1 argument".to_string(),
                                            count => format!("are {count} arguments"),
                                        }
                                    )));
                                }
                                Ok(*index)
                            }
                            Argument::Name(name) => {
                                if let Some(index) = arguments
                                    .iter()
                                    .position(|(named, _)| named.as_deref() == Some(name.as_str()))
                                {
                                    return Ok(index);
                                }
                                // Rust 2021's inline argument: any value of
                                // that name in scope. A local is borrowed
                                // below; any other name (a `const`, a
                                // `static`, an imported constant) is read
                                // as the expression `name` reads, followed
                                // by a `,` so nothing after the macro
                                // continues it.
                                // A closure around the macro captures a
                                // local of the name, as one naming it does.
                                this.record_capture(&name.as_str().into());
                                let Some(binding) = this.bindings.get(name.as_str()) else {
                                    let at = expression.start();
                                    // `{self:?}` in a method reads its
                                    // receiver, which is the keyword's token.
                                    let token = match name.as_str() {
                                        "self" => Token::SelfValue,
                                        _ => Token::Identifier(Box::new(name.as_str().into())),
                                    };
                                    this.splice_tokens(vec![(token, at), (Token::Comma, at)]);
                                    let captured = this.parse_expr()?;
                                    this.expect(Token::Comma, "an inline `format!` argument")?;
                                    arguments.push((Some(name.as_str().into()), captured));
                                    return Ok(arguments.len() - 1);
                                };
                                if !binding.available {
                                    // At the name in the string, which
                                    // ends where the chars left start.
                                    let reference = this
                                        .template_name_span(
                                            template_span,
                                            &template,
                                            &written,
                                            template.chars().count()
                                                - chars.clone().count()
                                                - written.chars().count()
                                                - 1,
                                            name,
                                        )
                                        .unwrap_or(expression);
                                    return Err(this.moved_value(name, binding, reference, true));
                                }
                                // The macro borrows the local where it runs.
                                this.mention(
                                    name.as_str(),
                                    binding.declaration,
                                    &binding.ty,
                                    expression,
                                );
                                this.flow().shared(expression.start());
                                // A `&mut` local is the place it borrows.
                                let source = match this.aliases.get(name.as_str()) {
                                    Some((aliased, Lvalue::Place { place, .. }))
                                        if *aliased == binding.declaration =>
                                    {
                                        place.clone()
                                    }
                                    _ => origin::Fragment::plain(name.clone()),
                                };
                                let captured = ParsedExpr {
                                    prelude: origin::Fragment::default(),
                                    source,
                                    ty: binding.ty.clone(),
                                    span: expression,
                                    lvalue: None,
                                    outcome: crate::host::usage::Outcome::Conservative,
                                    evaluation: crate::host::usage::Evaluation::Required,
                                };
                                arguments.push((Some(name.as_str().into()), captured));
                                Ok(arguments.len() - 1)
                            }
                        }
                    };
                    let count = |this: &mut Self,
                                 arguments: &mut Vec<(Option<ImmutableString>, ParsedExpr)>,
                                 next: &mut usize,
                                 count: &Option<CountRef>|
                     -> Result<Option<Count>, Error> {
                        Ok(match count {
                            None => None,
                            Some(CountRef::Literal(value)) => Some(Count::Literal(*value)),
                            Some(CountRef::Star) => {
                                let index = take_positional(next);
                                if index >= positional {
                                    return Err(invalid(
                                        "`.*` needs a precision argument before the value".into(),
                                    ));
                                }
                                Some(Count::Argument(index))
                            }
                            Some(CountRef::Argument(reference)) => {
                                Some(Count::Argument(resolve(this, arguments, reference)?))
                            }
                        })
                    };
                    let precision =
                        count(self, &mut arguments, &mut next_positional, &spec.precision)?;
                    let width = count(self, &mut arguments, &mut next_positional, &spec.width)?;
                    let index = match &spec.argument_of(&key) {
                        None => {
                            let index = next_positional;
                            next_positional += 1;
                            if index >= positional {
                                return Err(invalid(format!(
                                    "{} positional argument{} in format string, but {}",
                                    index + 1,
                                    if index == 0 { "" } else { "s" },
                                    match positional {
                                        0 => "no arguments were given".to_string(),
                                        1 => "there is 1 argument".to_string(),
                                        count => format!("there are {count} arguments"),
                                    }
                                )));
                            }
                            index
                        }
                        Some(reference) => resolve(self, &mut arguments, reference)?,
                    };
                    for count in [&width, &precision].into_iter().flatten() {
                        if let Count::Argument(argument) = count {
                            counted.insert(*argument);
                        }
                    }
                    pieces.push(Ok(std::mem::take(&mut text)));
                    pieces.push(Err((index, spec.resolved(width, precision))));
                }
                '}' => return Err(invalid("invalid format string: unmatched `}` found".into())),
                character => text.push(character),
            }
        }
        pieces.push(Ok(text));

        // Each argument is evaluated once, in order, into a local.
        let mut source = origin::Fragment::plain("{\n");
        let mut names = Vec::new();
        let mut types = Vec::new();
        for (index, (name, argument)) in arguments.iter_mut().enumerate() {
            let used = pieces
                .iter()
                .any(|piece| matches!(piece, Err((used, _)) if *used == index));
            if !used && !counted.contains(&index) {
                return Err(invalid(if name.is_some() {
                    "named argument never used".into()
                } else {
                    "argument never used".into()
                }));
            }
            self.strip_string_reference(argument);
            if counted.contains(&index) {
                // A width or precision is a `usize`.
                self.coerce_expr(&Ty::Int(Int::Usize), argument)?;
            }
            let mut ty = self.inference.resolve(&argument.ty);
            // A reference, box, `Rc` or `Arc` displays as its target.
            loop {
                ty = match ty {
                    Ty::Reference { target, .. } | Ty::Boxed(target) => {
                        self.inference.resolve(&target)
                    }
                    Ty::Counted(_, target) => {
                        self.counted_value(argument);
                        self.inference.resolve(&target)
                    }
                    other => {
                        ty = other;
                        break;
                    }
                };
            }
            // The macro borrows a local where it is (D568). Rust's borrow
            // keeps the later arguments from changing it. One a later
            // argument moved, which rustc refuses (E0505), is read first.
            let moved = |name: &str| {
                self.bindings
                    .get(name)
                    .is_some_and(|local| !local.available)
            };
            if argument.is_local() && !moved(argument.source.as_str()) {
                names.push(argument.source.as_str().to_string());
            } else {
                let temporary = self.temporary("format");
                source.push_text(format!("let {temporary} = "));
                source.push(argument.rendered_source());
                source.push_text(";\n");
                names.push(temporary);
            }
            types.push((ty, argument.span));
        }

        let mut text = origin::Fragment::plain("\"\"");
        for piece in pieces {
            let (index, spec) = match piece {
                Ok(piece) if piece.is_empty() => continue,
                Ok(piece) => {
                    text.push_text(format!(" + {piece:?}"));
                    continue;
                }
                Err(placeholder) => placeholder,
            };
            let (ty, at) = types[index].clone();
            // A formatting argument is sized (rustc's E0277): `*s` of a
            // `&str` is no argument, `s` is.
            if ty == Ty::UnsizedStr {
                return Err(Error::MissingCapability {
                    owner: "str".into(),
                    capability: "Sized".into(),
                    operation: at,
                    derive: false,
                    required_by: None,
                });
            }
            let name = origin::Fragment::plain(names[index].as_str());
            let count = |count: &Option<Count>| {
                count.as_ref().map(|count| match count {
                    Count::Literal(value) => value.to_string(),
                    Count::Argument(argument) => names[*argument].clone(),
                })
            };
            let width = count(&spec.width);
            let precision = count(&spec.precision);
            let float = matches!(ty, Ty::Float(_));
            let int = match ty {
                Ty::Int(kind) => Some(kind),
                _ => None,
            };
            let number = float || int.is_some();
            let mismatch = |expected: &str| Error::TypeMismatch {
                expected: expected.into(),
                actual: ty.name(&self.models),
                expression: at,
            };
            // A value whose type a later call settles, as a closure
            // parameter: its piece is written once the types are known.
            if matches!(ty, Ty::Infer(_))
                && matches!(spec.kind, Kind::Display | Kind::Debug)
                && spec.width.is_none()
                && !spec.sign
            {
                let placeholder = format_placeholder(self.pending_formats.len());
                self.pending_formats.push(Pending {
                    placeholder: placeholder.clone(),
                    ty: types[index].0.clone(),
                    name: names[index].clone(),
                    debug: spec.kind == Kind::Debug,
                    alternate: spec.alternate,
                    precision: precision.clone(),
                    span: at,
                });
                text.push_text(format!(" + {placeholder}"));
                continue;
            }
            let f32_mode = if matches!(ty, Ty::Float(Float::F32)) {
                2
            } else {
                0
            };
            // A spec on `{:?}` of a value other than a number, which Rust's
            // `Formatter` passes down to each leaf, as in `Some(       1)`.
            let hex = match spec.kind {
                Kind::HexDebug(radix) => Some(radix),
                _ => None,
            };
            let spread = matches!(spec.kind, Kind::Debug | Kind::HexDebug(_))
                && !number
                && !matches!(ty, Ty::Infer(_))
                && (hex.is_some()
                    || width.is_some()
                    || precision.is_some()
                    || spec.sign
                    || spec.zero);
            let value = if spread {
                // `pretty_debug` reads brackets, commas and quotes, so a fill
                // of one would be read as structure.
                if spec.alternate && width.is_some() && "{}[](),'\"".contains(spec.fill) {
                    return Err(self.unsupported("a bracket, comma or quote fill in `{:#?}`"));
                }
                let parts = self.temporary("spec");
                let align = spec
                    .align
                    .map_or_else(|| "()".to_string(), |align| format!("{align:?}"));
                let flags =
                    i64::from(spec.sign) + 2 * i64::from(spec.alternate) + 4 * i64::from(spec.zero);
                source.push_text(format!(
                    "let {parts} = [{}, {:?}, {align}, {flags}, {}, {}];\n",
                    hex.unwrap_or(0),
                    spec.fill,
                    width.as_deref().unwrap_or("-1"),
                    precision.as_deref().unwrap_or("-1"),
                ));
                self.debug_spec = Some(parts);
                let rendered = self.debug_source(&ty, name, at);
                self.debug_spec = None;
                let rendered = rendered?;
                if spec.alternate {
                    self.prelude_op(PreludeOp::PrettyDebug, [rendered])
                } else {
                    rendered
                }
            } else {
                match spec.kind {
                    Kind::Exp { upper } => {
                        if !number {
                            return Err(mismatch("a number for `{:e}`"));
                        }
                        let unsigned = int.is_some_and(|kind| !kind.signed());
                        let mode = i64::from(upper)
                            + f32_mode
                            + if int.is_some() { 4 } else { 0 }
                            + if unsigned { 8 } else { 0 };
                        let precision = precision.clone().unwrap_or_else(|| "-1".into());
                        self.prelude_op(
                            PreludeOp::FormatExp,
                            [name, precision.into(), mode.to_string().into()],
                        )
                    }
                    Kind::HexDebug(_) if float => {
                        let debug = 1 + f32_mode;
                        let precision = precision.clone().unwrap_or_else(|| "-1".into());
                        self.prelude_op(
                            PreludeOp::FormatFloat,
                            [name, precision.into(), debug.to_string().into()],
                        )
                    }
                    Kind::Radix(radix) | Kind::HexDebug(radix) => {
                        let Some(kind) = int else {
                            return Err(mismatch(if matches!(spec.kind, Kind::Radix(_)) {
                                "an integer for this `format!` spec"
                            } else {
                                "an integer for `{:x?}`"
                            }));
                        };
                        self.prelude_op(
                            PreludeOp::FormatBits,
                            [
                                name,
                                radix.to_string().into(),
                                (if matches!(kind, Int::Var(_)) {
                                    32
                                } else {
                                    kind.bits()
                                })
                                .to_string()
                                .into(),
                            ],
                        )
                    }
                    Kind::Debug | Kind::Display if float => {
                        let debug = i64::from(spec.kind == Kind::Debug) + f32_mode;
                        let precision = precision.clone().unwrap_or_else(|| "-1".into());
                        self.prelude_op(
                            PreludeOp::FormatFloat,
                            [name, precision.into(), debug.to_string().into()],
                        )
                    }
                    Kind::Debug => {
                        if spec.width.is_some()
                            && !matches!(
                                ty,
                                Ty::Int(_) | Ty::String | Ty::Str | Ty::Char | Ty::Bool
                            )
                        {
                            return Err(self.unsupported("a `{:?}` width on a compound value"));
                        }
                        let rendered = self.debug_source(&ty, name, at)?;
                        if spec.alternate {
                            self.prelude_op(PreludeOp::PrettyDebug, [rendered])
                        } else {
                            rendered
                        }
                    }
                    Kind::Display => match self.display_piece(&ty, name) {
                        Some(text) => text,
                        None => {
                            return Err(Error::TypeMismatch {
                                expected: "a type with `Display` in `format!`".into(),
                                actual: ty.name(&self.models),
                                expression: at,
                            });
                        }
                    },
                }
            };
            let prefixed = spec.alternate
                && int.is_some()
                && matches!(spec.kind, Kind::Radix(_) | Kind::HexDebug(_));
            let truncated = precision.is_some() && !number && spec.kind == Kind::Display;
            // `str`'s and `char`'s `Debug` ignore width and precision.
            let quoted = spec.kind == Kind::Debug && matches!(ty, Ty::String | Ty::Str | Ty::Char);
            if !quoted
                && !spread
                && (width.is_some() || spec.sign && number || prefixed || truncated)
            {
                let temporary = self.temporary("format");
                source.push_text(format!("let {temporary} = "));
                source.push(value);
                source.push_text(";\n");
                let rendered = spec.render(
                    &temporary,
                    number,
                    width.as_deref(),
                    precision.as_deref().filter(|_| truncated),
                );
                text.push_text(format!(" + {rendered}"));
            } else {
                text.push_text(" + ");
                text.push(value);
            }
        }
        source.push(text);
        source.push_text("\n}");
        Ok(self.operator_expr(source, Ty::String, expression))
    }

    /// A panic macro's own message, `format!`'s text of its arguments, or
    /// `None` without any.
    #[inline(never)]
    pub(super) fn panic_format(
        &mut self,
        parts: Vec<Vec<(Token, Position)>>,
        name_span: Span,
        end: Position,
    ) -> Result<Option<origin::Fragment>, Error> {
        if parts.iter().all(Vec::is_empty) {
            return Ok(None);
        }
        let mut spliced = Vec::new();
        for (index, part) in parts.into_iter().enumerate() {
            if index > 0 {
                spliced.push((Token::Comma, end));
            }
            spliced.extend(part);
        }
        spliced.push((Token::RightParen, end));
        self.splice_tokens(spliced);
        Ok(Some(self.parse_format(name_span, true)?.rendered_source()))
    }

    /// `assert_eq!(a, b, ..)` and `assert_ne!`: as Rust expands them, both
    /// sides borrowed once and compared, and on failure a panic showing
    /// both with `{:?}`.
    #[inline(never)]
    pub(super) fn parse_assert_compare(
        &mut self,
        (operator, symbol): (Token, &str),
        tokens: Vec<(Token, Position)>,
        name_span: Span,
        end: Position,
    ) -> Result<ParsedExpr, Error> {
        // rustc's errors for missing operands, as its `macro_rules!`
        // matcher reports them: a leading `,` is no rule's, and arguments
        // that end early end at the `)`, or are the whole call when none.
        if let Some((Token::Comma, comma)) = tokens.first() {
            return Err(Error::Syntax {
                message: "no rules expected `,`".into(),
                label: "no rules expected this token in macro call".into(),
                help: None,
                at: point_span(*comma),
            });
        }
        let none = tokens.is_empty();
        let mut parts = split_arguments(tokens).into_iter();
        let (Some(left), Some(right)) = (parts.next(), parts.next()) else {
            let at = if none {
                span(name_span.start(), text_end(end, ")"))
            } else {
                point_span(end)
            };
            return Err(Error::Syntax {
                message: "unexpected end of macro invocation".into(),
                label: "missing tokens in macro arguments".into(),
                help: None,
                at,
            });
        };
        if left.is_empty() || right.is_empty() {
            return Err(Error::UnsupportedNominalSyntax {
                construct: "an assertion without two values".into(),
                source: span(name_span.start(), end),
            });
        }
        let rest: Vec<_> = parts.collect();
        let at = name_span.start();
        let left_name = self.temporary("left");
        let right_name = self.temporary("right");
        let ident =
            |name: &str, position: Position| (Token::Identifier(Box::new(name.into())), position);
        let mut spliced = vec![
            (Token::LeftBrace, at),
            (Token::Let, at),
            ident(&left_name, at),
        ];
        spliced.extend([
            (Token::Equals, at),
            (Token::Ampersand, at),
            (Token::LeftParen, at),
        ]);
        let left_end = left.last().map_or(at, |(_, position)| *position);
        spliced.extend(left);
        spliced.extend([(Token::RightParen, left_end), (Token::SemiColon, left_end)]);
        spliced.extend([
            (Token::Let, at),
            ident(&right_name, at),
            (Token::Equals, at),
        ]);
        spliced.extend([(Token::Ampersand, at), (Token::LeftParen, at)]);
        let right_end = right.last().map_or(at, |(_, position)| *position);
        spliced.extend(right);
        spliced.extend([
            (Token::RightParen, right_end),
            (Token::SemiColon, right_end),
        ]);
        spliced.extend([(Token::If, at), (Token::Bang, at), (Token::LeftParen, at)]);
        spliced.extend([(Token::Multiply, at), ident(&left_name, at), (operator, at)]);
        spliced.extend([
            (Token::Multiply, at),
            ident(&right_name, at),
            (Token::RightParen, at),
        ]);
        spliced.extend([
            (Token::LeftBrace, at),
            ident("panic", at),
            (Token::Bang, at),
        ]);
        spliced.push((Token::LeftParen, at));
        let custom = !rest.iter().all(Vec::is_empty);
        let colon = if custom { ": {}" } else { "" };
        let template = format!(
            "assertion `left {symbol} right` failed{colon}\n  left: {{:?}}\n right: {{:?}}"
        );
        spliced.push((Token::StringConstant(Box::new(template.into())), at));
        if custom {
            spliced.extend([(Token::Comma, at), ident("format", at), (Token::Bang, at)]);
            spliced.push((Token::LeftParen, at));
            for (index, part) in rest.into_iter().enumerate() {
                if index > 0 {
                    spliced.push((Token::Comma, end));
                }
                spliced.extend(part);
            }
            spliced.push((Token::RightParen, end));
        }
        spliced.extend([
            (Token::Comma, at),
            ident(&left_name, at),
            (Token::Comma, at),
        ]);
        spliced.extend([ident(&right_name, at), (Token::RightParen, at)]);
        spliced.extend([(Token::RightBrace, end), (Token::RightBrace, end)]);
        self.splice_tokens(spliced);
        let mut value = self.parse_block_expr()?;
        value.span = span(name_span.start(), end);
        Ok(value)
    }
}

impl<'a> Parser<'a> {
    /// The `{}` text of `value`, a script type with a hand-written `Display`
    /// or a generic value bounded by `Display`.
    /// `value` of type `ty` as `{}` writes it, except a float's.
    pub(super) fn display_piece(
        &mut self,
        ty: &Ty,
        value: origin::Fragment,
    ) -> Option<origin::Fragment> {
        if let Some(text) = self.display_text(ty, value.clone()) {
            return Some(text);
        }
        // An imported value: its Rust `Display`, which `cargo check` holds
        // the type to.
        let target = match ty {
            Ty::Reference { target, .. } => self.inference.resolve(target),
            ty => ty.clone(),
        };
        if let Ty::Host { proof, .. } = &target
            && proof.definition.identity().imported().is_some()
        {
            return Some(self.prelude_op(PreludeOp::DisplayText, [value]));
        }
        // A facade type with `to_string`, as std's `io::Error`: its Rust
        // `Display`, which the facade registers.
        if let Ty::Host { proof, .. } = &target
            && self
                .models
                .host_items(&format!("{}::to_string", proof.definition.path()))
                .is_some()
        {
            return Some(self.prelude_op(PreludeOp::DisplayText, [value]));
        }
        match ty {
            Ty::Int(kind @ (Int::U64 | Int::Usize)) => Some(self.prelude_op(
                PreludeOp::FormatBits,
                [value, "10".into(), kind.bits().to_string().into()],
            )),
            Ty::String | Ty::Str => Some(self.prelude_op(PreludeOp::DisplayText, [value])),
            Ty::Int(_) | Ty::Float(_) | Ty::Char | Ty::Bool => Some(value),
            _ => None,
        }
    }

    /// Write each pending `format!` piece over its placeholder, now that
    /// its value's type is known.
    pub(super) fn finish_formats(&mut self) -> Result<(), Error> {
        for pending in std::mem::take(&mut self.pending_formats) {
            let mut ty = self.inference.resolve(&pending.ty);
            let mut name = origin::Fragment::plain(pending.name.as_str());
            loop {
                ty = match ty {
                    Ty::Reference { target, .. } | Ty::Boxed(target) => {
                        self.inference.resolve(&target)
                    }
                    Ty::Counted(_, target) => {
                        name = self.counted_call(PreludeOp::CountedGet, name, []);
                        self.inference.resolve(&target)
                    }
                    other => {
                        ty = other;
                        break;
                    }
                };
            }
            let actual = ty.name(&self.models);
            let mismatch = |expected: &str| Error::TypeMismatch {
                expected: expected.into(),
                actual: actual.clone(),
                expression: pending.span,
            };
            let expected = if pending.debug {
                "a type with `Debug` in `format!`"
            } else {
                "a type with `Display` in `format!`"
            };
            let rendered = match &ty {
                // A type nothing settles is rustc's E0282 at the first
                // `let` whose type holds it.
                Ty::Infer(_) => {
                    return Err(self
                        .unsettled(&ty, pending.span, None)
                        .unwrap_or_else(|| mismatch(expected)));
                }
                // A value that never comes is never written.
                Ty::Never => name,
                Ty::Float(kind) => {
                    let mode = i64::from(pending.debug) + if *kind == Float::F32 { 2 } else { 0 };
                    let precision = pending.precision.clone().unwrap_or_else(|| "-1".into());
                    self.prelude_op(
                        PreludeOp::FormatFloat,
                        [name, precision.into(), mode.to_string().into()],
                    )
                }
                _ if pending.precision.is_some() && !matches!(ty, Ty::Int(_)) => {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "a `format!` precision on a value whose type is written later; write the closure parameter's type".into(),
                        source: pending.span,
                    });
                }
                _ if pending.debug => {
                    let rendered = self.debug_source(&ty, name, pending.span)?;
                    if pending.alternate {
                        self.prelude_op(PreludeOp::PrettyDebug, [rendered])
                    } else {
                        rendered
                    }
                }
                _ => match self.display_piece(&ty, name) {
                    Some(text) => text,
                    None => return Err(mismatch(expected)),
                },
            };
            let rendered = rendered.as_str().to_string();
            if rendered.len() > pending.placeholder.len() {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "a long `format!` piece of a value whose type is written later; write the closure parameter's type".into(),
                    source: pending.span,
                });
            }
            let replacement = format!("{rendered:<width$}", width = pending.placeholder.len());
            self.output
                .replace_same_length(&pending.placeholder, &replacement);
            for closure in &mut self.lifted {
                closure
                    .body
                    .replace_same_length(&pending.placeholder, &replacement);
            }
        }
        Ok(())
    }

    pub(super) fn display_text(
        &mut self,
        ty: &Ty,
        value: origin::Fragment,
    ) -> Option<origin::Fragment> {
        if self.std_error(ty, true) {
            return Some(self.error_text(value, false));
        }
        match ty {
            Ty::Script(id, _) => self
                .models
                .std_impl(*id, "Display", "fmt")
                .map(|definition| Self::std_call(definition, value, &["\"\"".into()])),
            Ty::Param(_) | Ty::Projection(..) | Ty::Dyn(..)
                if self.type_implements(ty, "Display") =>
            {
                Some(Self::std_dispatch(
                    "Display",
                    "fmt",
                    value,
                    &["\"\"".into()],
                ))
            }
            // A `ParseIntError` and kin is its message.
            Ty::ParseError(error) => Some(self.parse_error_display(*error, value)),
            Ty::Thread(thread::Kind::SendError | thread::Kind::RecvError, _) => {
                Some(self.thread_text(value, false))
            }
            // A `Cow` writes what it derefs to.
            Ty::Cow(borrowed) => {
                let payload = self.prelude_op(PreludeOp::CowValue, [value]);
                let borrowed = cow::payload_ty(borrowed);
                Some(
                    self.display_text(&borrowed, payload.clone())
                        .unwrap_or(payload),
                )
            }
            _ => None,
        }
    }

    /// `value.fmt(f)` after its `fmt`: `write!(f, "{}", value)`, or
    /// `{:?}` in a `Debug` impl.
    pub(super) fn parse_value_fmt(
        &mut self,
        value: ParsedExpr,
        formatter: ImmutableString,
    ) -> Result<ParsedExpr, Error> {
        let open = self.expect(Token::LeftParen, "'(' after `fmt`")?;
        self.expect_name("the formatter of `fmt`")?;
        let close = self.expect(Token::RightParen, "')' after the formatter")?;
        let template = if self.impl_trait.as_deref() == Some("Debug") {
            "{:?}"
        } else {
            "{}"
        };
        let temporary = self.temporary("fmt");
        let identifier = |name: &str| Token::Identifier(Box::new(name.into()));
        self.splice_tokens(vec![
            (identifier("write"), open),
            (Token::Bang, open),
            (Token::LeftParen, open),
            (identifier(&formatter), open),
            (Token::Comma, open),
            (Token::StringConstant(Box::new(template.into())), open),
            (Token::Comma, open),
            (identifier(&temporary), open),
            (Token::RightParen, close),
        ]);
        let previous = self.bindings.insert(
            temporary.as_str().into(),
            Binding {
                ty: value.ty.clone(),
                mutable: false,
                available: true,
                declaration: value.span,
                kind: BindingKind::Local,
            },
        );
        let written = self.parse_primary();
        match previous {
            Some(previous) => {
                self.bindings.insert(temporary.as_str().into(), previous);
            }
            None => {
                self.bindings.remove(temporary.as_str());
            }
        }
        let written = written?;
        let mut source = origin::Fragment::plain(format!("{{ let {temporary} = "));
        source.push(value.rendered_source());
        source.push_text("; ");
        source.push(written.rendered_source());
        source.push_text(" }");
        let expression = span(value.span.start(), close);
        Ok(self.operator_expr(source, written.ty, expression))
    }

    /// `print!`, `println!`, `eprint!`, `eprintln!` after the name:
    /// `format!`'s text, written through the engine's output callback. Kept
    /// out of [`Self::parse_name_primary`]'s recursive frame.
    #[inline(never)]
    pub(super) fn parse_print(
        &mut self,
        (stderr, newline): (bool, bool),
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        self.next();
        // `println!()` lexes its parentheses as one `()` token.
        let (mut text, end) = if newline && matches!(self.peek(), Token::Unit) {
            let (_, position) = self.next();
            (origin::Fragment::plain("\"\""), position)
        } else {
            self.expect(
                Token::LeftParen,
                "'(' and a format string after the macro name",
            )?;
            let text = self.parse_format(name_span, true)?;
            let end = text.span.end();
            (text.rendered_source(), end)
        };
        if newline {
            let mut line = origin::Fragment::plain("(");
            line.push(text);
            line.push_text(") + \"\\n\"");
            text = line;
        }
        let stderr = origin::Fragment::plain(if stderr { "true" } else { "false" });
        let source = self.prelude_op(PreludeOp::Print, [text, stderr]);
        Ok(self.operator_expr(source, Ty::Unit, span(name_span.start(), end)))
    }

    /// `f.debug_struct(name)`, `f.debug_tuple(name)` or `f.debug_list()`
    /// after the formatter `f`, with its `.field(..)` or `.entry(..)` calls
    /// and the `.finish()` or `.finish_non_exhaustive()` that ends it: the
    /// text std's builders write (`Life { cells: 3 }`, `Pair(1, 2)`,
    /// `[1, 2]`) into the formatter, and `Ok(())`. `{:#?}` lays it out as a
    /// derived `Debug`'s.
    pub(super) fn parse_debug_builder(
        &mut self,
        formatter: ParsedExpr,
        builder: &str,
    ) -> Result<ParsedExpr, Error> {
        let (mut arguments, _) = self.parse_call_arguments("debug builder arguments")?;
        let mut end;
        let (open, close) = match builder {
            "debug_struct" => (" { ", " }"),
            "debug_tuple" => ("(", ")"),
            _ => ("[", "]"),
        };
        let mut text = if builder == "debug_list" {
            if !arguments.is_empty() {
                return Err(self.unsupported("`debug_list` with arguments"));
            }
            origin::Fragment::plain("\"\"")
        } else {
            let [name] = arguments.as_mut_slice() else {
                return Err(self.unsupported("a debug builder without its one name"));
            };
            self.coerce_expr(&Ty::Str, name)?;
            name.rendered_source().surrounded("(", ")")
        };
        let mut parts = Vec::new();
        let mut non_exhaustive = false;
        loop {
            if !matches!(self.peek(), Token::Period) {
                return Err(self.unsupported("a debug builder without `.finish()`"));
            }
            self.next();
            let (method, method_span) = self.expect_name("a debug builder method")?;
            let (mut arguments, call_end) =
                self.parse_call_arguments("debug builder method arguments")?;
            end = call_end;
            match (builder, method.as_str(), arguments.as_mut_slice()) {
                (_, "finish", []) => break,
                ("debug_struct", "finish_non_exhaustive", []) => {
                    non_exhaustive = true;
                    break;
                }
                ("debug_struct", "field", [name, field]) => {
                    self.coerce_expr(&Ty::Str, name)?;
                    let debug =
                        self.debug_source(&field.ty, field.rendered_source(), field.span)?;
                    let mut part = name.rendered_source().surrounded("(", ")");
                    part.push_text(" + \": \" + ");
                    part.push(debug.surrounded("(", ")"));
                    parts.push(part);
                }
                ("debug_tuple", "field", [field]) | ("debug_list", "entry", [field]) => {
                    let debug =
                        self.debug_source(&field.ty, field.rendered_source(), field.span)?;
                    parts.push(debug.surrounded("(", ")"));
                }
                _ => {
                    return Err(Error::UnknownMember {
                        owner: "Formatter".into(),
                        member: method,
                        reference: method_span,
                    });
                }
            }
        }
        if non_exhaustive {
            parts.push(origin::Fragment::plain("\"..\""));
        }
        if !parts.is_empty() || builder == "debug_list" || non_exhaustive {
            text.push_text(format!(" + {open:?} + "));
            if parts.is_empty() {
                text.push_text("\"\"");
            }
            text.push(origin::Fragment::join(parts, " + \", \" + "));
            text.push_text(format!(" + {close:?}"));
        }
        let mut source = origin::Fragment::plain("{ ");
        source.push(formatter.rendered_source());
        source.push_text(" += (");
        source.push(text);
        source.push_text("); ");
        source.push(self.prelude_op(PreludeOp::Ok, ["()".into()]));
        source.push_text(" }");
        Ok(self.operator_expr(
            source,
            Ty::Result(Box::new(Ty::Unit), Box::new(Ty::Unit)),
            span(formatter.span.start(), end),
        ))
    }
}
