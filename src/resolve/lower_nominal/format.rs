//! Format strings and their specs: `format!`'s arguments, `Debug` text,
//! and the panic and assert macros that format.

use super::literal;
use super::lvalue::Lvalue;
use super::models::{Models, Nominal};
use super::obligation::Obligation;
use super::op::{Op, PreludeOp};
use super::range;
use super::ty::{contains_inference, CellKind, Float, Int, Ty};
use super::{
    cargo_env, cell, counted, cow, origin, point_span, span, split_arguments, stringify_tokens,
    text_end, thread, token_text, Binding, BindingKind, ParseError, ParsedExpr, Parser, STR_PIECES,
};
use crate::resolve::{type_ref, Error, Placeholder};
use crate::tokenizer::Token;
use crate::types::dynamic::Union;
use crate::{capability, Dynamic, ImmutableString, Position, Span, INT};
use std::collections::BTreeSet;
use std::fmt::Write;
use std::ops::Range;

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

/// The local a `fmt` body keeps its caller's spec in (`open_formatter`).
pub(super) const FORMAT_SPEC: &str = "__rawr_fmt_spec";

/// The placeholder of a pending `format!` piece, which the piece replaces
/// once its type is known (`finish_formats`).
pub(super) fn format_placeholder(index: usize) -> String {
    format!("__rawr_nominal_format_{index:010}")
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
    /// `-`, which std keeps for `Formatter::sign_minus` alone.
    pub(super) minus: bool,
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
        let minus = chars.get(at) == Some(&'-');
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
            minus,
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
            minus: self.minus,
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
    pub(super) minus: bool,
    pub(super) alternate: bool,
    pub(super) zero: bool,
    pub(super) width: Option<Count>,
    pub(super) precision: Option<Count>,
    pub(super) kind: Kind,
}

impl Spec {
    /// Whether the spec sets anything: a width, a precision, an alignment,
    /// a fill or a flag.
    pub(super) fn is_set(&self) -> bool {
        self.width.is_some()
            || self.precision.is_some()
            || self.align.is_some()
            || self.fill != ' '
            || self.sign
            || self.minus
            || self.alternate
            || self.zero
    }

    /// The spec as a `fmt` body's formatter carries it at runtime
    /// (`DebugSpec`), with `width` and `precision` Rawr expressions.
    pub(super) fn runtime(&self, width: Option<&str>, precision: Option<&str>) -> String {
        let align = self
            .align
            .map_or_else(|| "()".to_string(), |align| format!("{align:?}"));
        let flags = i64::from(self.sign)
            + 2 * i64::from(self.alternate)
            + 4 * i64::from(self.zero)
            + 8 * i64::from(self.minus);
        format!(
            "[0, {:?}, {align}, {flags}, {}, {}]",
            self.fill,
            width.unwrap_or("-1"),
            precision.unwrap_or("-1"),
        )
    }

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
    // A stored `&mut` writes what it refers to, as `&mut T`'s `Debug`.
    if let Some(referent) = crate::Engine::stored_referent(value) {
        return debug_value_spec(&referent, models, spec);
    }
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
    // std's ranges write their bounds around `..` or `..=`, as std's
    // `Debug` of each does.
    if let Some(bounds) = range::read(value) {
        let bound = |bound: &Option<Dynamic>| {
            bound
                .as_ref()
                .map_or_else(String::new, |bound| debug_value_spec(bound, models, spec))
        };
        let dots = if bounds.inclusive() { "..=" } else { ".." };
        return format!("{}{dots}{}", bound(&bounds.start), bound(&bounds.end));
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
        // A sequence writes each item through this same text, so a
        // program's struct or enum inside it prints by its derive.
        Union::Array(values, ..) => {
            let items = values
                .iter()
                .map(|value| debug_value_spec(value, models, spec))
                .collect::<Vec<_>>();
            // A tuple a generic parameter took (`TUPLE_TAG`).
            match (value.tag() == crate::types::dynamic::TUPLE_TAG, &items[..]) {
                (true, [member]) => format!("({member},)"),
                (true, _) => format!("({})", items.join(", ")),
                (false, _) => format!("[{}]", items.join(", ")),
            }
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
        // A stored `&mut` writes what it refers to (D1113).
        (type_ref::Type::Reference { target, .. }, Union::Array(..))
            if let Some(referent) = crate::Engine::stored_referent(value) =>
        {
            debug_typed(&referent, target, models, spec)
        }
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

/// Whether a local of type `ty` prints otherwise by its type than by its
/// runtime value alone: a tuple (an array at runtime), an `Ordering` (an
/// integer), an unsigned 64-bit integer, an `f32` or a parse error (its
/// message), a map or set (a sequence of its entries, which only its type
/// writes as `{k: v}`), or a value holding one.
pub(super) fn needs_declared(ty: &Ty) -> bool {
    match ty {
        Ty::Tuple(_)
        | Ty::Keyed(..)
        | Ty::Ordering
        | Ty::ParseError(_)
        | Ty::Int(Int::U64 | Int::Usize)
        | Ty::Float(Float::F32) => true,
        Ty::Option(inner)
        | Ty::Array { element: inner, .. }
        | Ty::Reference { target: inner, .. }
        | Ty::Boxed(inner)
        | Ty::Counted(_, inner)
        | Ty::Reverse(inner) => needs_declared(inner),
        Ty::Result(ok, err) => needs_declared(ok) || needs_declared(err),
        _ => false,
    }
}

/// Register [`super::naming::DEBUG_DECLARED`] in `module`, the `Debug`
/// text a debugger reads a local by its type through (#512): called with
/// the value, its function's name, the local's name and the line the
/// function is at, it gives the text, or `()` when `declared` keeps no
/// type for that local, which then prints as its value does. A name
/// declared more than once is the last declaration at or above the line;
/// with no line (0), the declarations' one type, if they have one.
#[cfg(feature = "debugging")]
pub(super) fn register_declared_debug(
    module: &mut crate::Module,
    models: &crate::Shared<Models>,
    declared: std::collections::BTreeMap<ImmutableString, Vec<super::entry::Declared>>,
) {
    use std::any::TypeId;
    let models = crate::Shared::clone(models);
    let func: crate::Shared<crate::func::native::FnAny> = crate::Shared::new(move |_, args| {
        let function = args[1].clone_cast::<ImmutableString>();
        let name = args[2].clone_cast::<ImmutableString>();
        let line = usize::try_from(args[3].as_int().unwrap_or_default()).unwrap_or_default();
        let candidates = declared
            .get(&function)
            .into_iter()
            .flatten()
            .filter(|local| local.name == name.as_str())
            .collect::<Vec<_>>();
        let ty = if line > 0 {
            candidates
                .iter()
                .rev()
                .find(|local| local.line <= line)
                .or(candidates.first())
                .and_then(|local| local.ty.as_ref())
        } else {
            candidates
                .first()
                .and_then(|first| first.ty.as_ref())
                .filter(|ty| {
                    candidates
                        .iter()
                        .all(|local| local.ty.as_ref() == Some(*ty))
                })
        };
        Ok(ty.map_or(Dynamic::UNIT, |ty| {
            ImmutableString::from(debug_declared(args[0], ty, &models)).into()
        }))
    });
    crate::FuncRegistration::new(super::naming::DEBUG_DECLARED)
        .in_internal_namespace()
        .set_into_module_raw(
            module,
            [
                TypeId::of::<Dynamic>(),
                TypeId::of::<ImmutableString>(),
                TypeId::of::<ImmutableString>(),
                TypeId::of::<INT>(),
            ],
            crate::func::RawrFunc::Pure {
                func,
                has_context: false,
                is_pure: true,
                is_volatile: false,
            },
        );
}

/// `{:?}` of a local of declared type `ty` (#512), as a debugger shows it:
/// a tuple as a tuple, an `Ordering` as its variant, a number as its
/// declared kind, through what holds them. A value whose shape isn't the
/// type's prints as it is.
#[cfg(feature = "debugging")]
pub(super) fn debug_declared(value: &Dynamic, ty: &Ty, models: &Models) -> String {
    let each = |values: &[Dynamic], ty: &Ty| {
        values
            .iter()
            .map(|value| debug_declared(value, ty, models))
            .collect::<Vec<_>>()
    };
    match (ty, &value.0) {
        (Ty::Int(_) | Ty::Float(_), Union::Int(..) | Union::Float(..)) => {
            match DebugLeaf::of_name(&ty.name(models)) {
                Some(leaf) => DebugSpec::PLAIN.leaf(value, leaf),
                None => debug_value(value, models),
            }
        }
        (Ty::Ordering, Union::Int(ordering, ..)) => match ordering {
            ..0 => "Less",
            0 => "Equal",
            _ => "Greater",
        }
        .into(),
        (Ty::ParseError(error), Union::Str(text, ..)) => error
            .texts()
            .iter()
            .find(|(message, _)| *message == text.as_str())
            .map_or_else(|| text.to_string(), |(_, debug)| (*debug).to_owned()),
        (Ty::Tuple(members), Union::Array(values, ..)) if members.len() == values.len() => {
            let members = values
                .iter()
                .zip(members.iter())
                .map(|(value, ty)| debug_declared(value, ty, models))
                .collect::<Vec<_>>();
            if members.len() == 1 {
                format!("({},)", members[0])
            } else {
                format!("({})", members.join(", "))
            }
        }
        (Ty::Array { element, .. }, Union::Array(values, ..)) => {
            format!("[{}]", each(values, element).join(", "))
        }
        (Ty::Keyed(kind, arguments), Union::Array(values, ..)) => {
            let entries = values.iter().map(|entry| match (&arguments[..], &entry.0) {
                ([key, item], Union::Array(pair, ..)) if kind.is_map() && pair.len() == 2 => {
                    format!(
                        "{}: {}",
                        debug_declared(&pair[0], key, models),
                        debug_declared(&pair[1], item, models)
                    )
                }
                ([key], _) => debug_declared(entry, key, models),
                _ => debug_value(entry, models),
            });
            format!("{{{}}}", entries.collect::<Vec<_>>().join(", "))
        }
        (Ty::Reference { target: inner, .. } | Ty::Boxed(inner), _) => {
            debug_declared(value, inner, models)
        }
        (Ty::Counted(_, inner), _) if let Some(held) = counted::value(value) => {
            debug_declared(&held, inner, models)
        }
        (Ty::Reverse(inner), _)
            if let Some(reverse) = value.read_lock::<std::cmp::Reverse<Dynamic>>() =>
        {
            format!("Reverse({})", debug_declared(&reverse.0, inner, models))
        }
        (Ty::Option(inner), _) if let Some(option) = crate::option::read(value) => match option {
            Some(held) => format!("Some({})", debug_declared(&held, inner, models)),
            None => "None".into(),
        },
        (Ty::Result(ok, err), _) if let Some(result) = crate::result::read(value) => match result {
            Ok(held) => format!("Ok({})", debug_declared(&held, ok, models)),
            Err(held) => format!("Err({})", debug_declared(&held, err, models)),
        },
        _ => debug_value(value, models),
    }
}

/// What a `fmt` body asks its formatter of its spec: `f.width()`,
/// `f.fill()` and kin, by the code `PreludeOp::FormatPart` takes.
#[derive(Clone, Copy)]
pub(super) enum FormatPart {
    Width,
    Precision,
    Fill,
    Alternate,
    SignPlus,
    SignMinus,
    ZeroPad,
    /// `f.align()`: std's `fmt::Alignment` of the spec's `<`, `>` or `^`.
    Align,
}

impl FormatPart {
    /// The part the formatter's method `name` reads.
    pub(super) fn of_method(name: &str) -> Option<Self> {
        Some(match name {
            "width" => Self::Width,
            "precision" => Self::Precision,
            "fill" => Self::Fill,
            "alternate" => Self::Alternate,
            "sign_plus" => Self::SignPlus,
            "sign_minus" => Self::SignMinus,
            "sign_aware_zero_pad" => Self::ZeroPad,
            "align" => Self::Align,
            _ => return None,
        })
    }

    /// The type the part's method gives, `alignment` std's
    /// `fmt::Alignment` as Rawr's std holds it.
    pub(super) fn ty(self, alignment: impl FnOnce() -> Option<Ty>) -> Option<Ty> {
        let count = || Ty::Option(Box::new(Ty::Int(Int::Usize)));
        Some(match self {
            Self::Width | Self::Precision => count(),
            Self::Fill => Ty::Char,
            Self::Alternate | Self::SignPlus | Self::SignMinus | Self::ZeroPad => Ty::Bool,
            Self::Align => Ty::Option(Box::new(alignment()?)),
        })
    }

    pub(super) const fn code(self) -> INT {
        self as INT
    }

    pub(super) fn from_code(code: INT) -> Self {
        match code {
            0 => Self::Width,
            1 => Self::Precision,
            2 => Self::Fill,
            3 => Self::Alternate,
            4 => Self::SignPlus,
            5 => Self::SignMinus,
            6 => Self::ZeroPad,
            _ => Self::Align,
        }
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
            Union::Float(_, tag, _) => Self::Float {
                single: *tag == crate::types::dynamic::SINGLE_TAG,
            },
            Union::Bool(..) => Self::Bool,
            Union::Unit(..) => Self::Unit,
            _ => Self::Other,
        }
    }

    /// The code (`from_code`) of a value of `ty` that is a leaf: a
    /// number, a `bool` or `()`.
    pub(super) fn code_of(ty: &Ty) -> Option<INT> {
        Some(match ty {
            // An integer literal's type defaults to `i32`.
            Ty::Int(Int::Var(_)) => 1032,
            Ty::Int(kind) => INT::from(kind.bits()) + if kind.signed() { 1000 } else { 2000 },
            Ty::Float(Float::F32) => 1,
            Ty::Float(_) => 2,
            Ty::Bool => 3,
            Ty::Unit => 4,
            _ => return None,
        })
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
/// 1 for `+`, 2 for `#`, 4 for `0` and 8 for `-`, and -1 for no width or
/// precision. Any other value is no spec.
pub(super) struct DebugSpec {
    pub(super) hex: Option<bool>,
    pub(super) fill: char,
    pub(super) align: Option<char>,
    pub(super) sign: bool,
    pub(super) minus: bool,
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
        minus: false,
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
            minus: flags & 8 != 0,
            alternate: flags & 2 != 0,
            zero: flags & 4 != 0,
            width: count(4),
            precision: count(5),
        }
    }

    /// What the formatter's method `part` reads (`FormatPart`).
    pub(super) fn part(&self, part: FormatPart) -> Dynamic {
        let count = |count: Option<usize>| match count {
            Some(count) => crate::option::some(Dynamic::from(count as INT)),
            None => crate::option::none(),
        };
        match part {
            FormatPart::Width => count(self.width),
            FormatPart::Precision => count(self.precision),
            FormatPart::Fill => self.fill.into(),
            FormatPart::Alternate => self.alternate.into(),
            FormatPart::SignPlus => self.sign.into(),
            FormatPart::SignMinus => self.minus.into(),
            FormatPart::ZeroPad => self.zero.into(),
            FormatPart::Align => self
                .align
                .and_then(crate::std_facade::alignment)
                .map_or_else(crate::option::none, |align| {
                    crate::option::some(Dynamic::from(align))
                }),
        }
    }

    /// `value`'s `Display` text as a leaf of kind `leaf` writes it under
    /// this spec: a number as `Formatter::pad_integral` pads it, anything
    /// else (its text) as `Formatter::pad` does.
    pub(super) fn display(&self, value: &Dynamic, leaf: DebugLeaf) -> String {
        // A float of a type the caller doesn't know writes as Rust's
        // `f64`, or as the `f32` its tag says it is (`SINGLE_TAG`).
        let leaf = match (leaf, &value.0) {
            (DebugLeaf::Other, Union::Float(..)) => DebugLeaf::of_value(value),
            _ => leaf,
        };
        match (leaf, &value.0) {
            (DebugLeaf::Int { signed: true, .. }, Union::Int(value, ..)) => {
                self.pad_number(*value < 0, "", &value.unsigned_abs().to_string())
            }
            (DebugLeaf::Int { signed: false, .. }, Union::Int(value, ..)) => {
                self.pad_number(false, "", &(*value as u64).to_string())
            }
            (DebugLeaf::Float { single }, Union::Float(value, ..)) => {
                let value = **value;
                let text = match (self.precision, single) {
                    (Some(precision), true) => format!("{:.precision$}", value as f32),
                    (Some(precision), false) => format!("{value:.precision$}"),
                    (None, true) => format!("{}", value as f32),
                    (None, false) => format!("{value}"),
                };
                if value.is_nan() {
                    return self.align_text(&text, '>');
                }
                match text.strip_prefix('-') {
                    Some(digits) => self.pad_number(true, "", digits),
                    None => self.pad_number(false, "", &text),
                }
            }
            // An integer of a type the caller doesn't know writes its own
            // text, padded as a number.
            (DebugLeaf::Other, Union::Int(..)) => {
                let text = value.to_string();
                match text.strip_prefix('-') {
                    Some(digits) => self.pad_number(true, "", digits),
                    None => self.pad_number(false, "", &text),
                }
            }
            (_, Union::Str(text, ..)) => self.pad(text),
            _ => self.pad(&value.to_string()),
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

/// Where a format string's `{}` and `{:?}` placeholders are, which an error
/// marks: the literal, its value, and each one's argument, the trait it
/// formats with, its characters in the value and whether it names its
/// argument itself.
#[derive(Default)]
struct Placements {
    literal: Span,
    template: String,
    placeholders: Vec<(Span, &'static str, Range<usize>, bool)>,
}

impl Placements {
    /// Each placeholder's argument, trait and place in `text`, the source.
    fn spans(&self, text: &str) -> Vec<(Span, &'static str, Placeholder)> {
        self.placeholders
            .iter()
            .filter_map(|(argument, trait_name, characters, inline)| {
                let at = literal::value_span(
                    self.literal.text(text),
                    self.literal.start(),
                    &self.template,
                    characters.clone(),
                )?;
                Some((
                    *argument,
                    *trait_name,
                    Placeholder {
                        at,
                        inline: *inline,
                    },
                ))
            })
            .collect()
    }
}

impl<'a> Parser<'a> {
    /// `format!` with `{}`, `{0}`, `{name}` (a named argument or a captured
    /// local) and `{{`/`}}`. Arguments are evaluated once, in order. A
    /// value whose type lacks the trait its placeholder formats with is
    /// refused at that placeholder too, as rustc marks it.
    pub(super) fn parse_format(&mut self, start: Span, opened: bool) -> Result<ParsedExpr, Error> {
        let mut placed = Placements::default();
        self.parse_placed_format(start, opened, &mut placed)
            .map_err(|error| error.formatted(&placed.spans(self.text)))
    }

    /// [`Self::parse_format`], with where its placeholders are in
    /// `placed`.
    fn parse_placed_format(
        &mut self,
        start: Span,
        opened: bool,
        placed: &mut Placements,
    ) -> Result<ParsedExpr, Error> {
        self.settle_parses()?;
        let forwarded = std::mem::take(&mut self.forward_spec);
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
            // it: a later argument that moves or changes it is refused. A
            // moved local given alone is rustc's "borrow of moved value".
            let mut ahead = self.input.clone();
            let alone = matches!(ahead.next(), Some((Token::Identifier(_), _)))
                && matches!(ahead.next(), Some((Token::Comma | Token::RightParen, _)));
            let mut argument = self.parse_expr().map_err(|error| match error {
                Error::MovedValue { .. } if alone => error.borrowing(),
                error => error,
            })?;
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
        // Each `{}` and `{:?}` piece's argument, the trait it formats with,
        // its placeholder's characters in the value and whether it names
        // its argument itself.
        let mut placeholders = Vec::<(usize, &'static str, Range<usize>, bool)>::new();
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
                    // The placeholder, braces and all, in the value.
                    let opening = template.chars().count()
                        - chars.clone().count()
                        - written.chars().count()
                        - 2;
                    let placeholder = opening..opening + written.chars().count() + 2;
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
                                    // A `let x;` without a value yet is
                                    // E0381 once the body's types are
                                    // settled, or E0282 where its type
                                    // stays unknown, as an expression's is.
                                    if let Some(first) =
                                        this.deferred.get(&binding.declaration).copied()
                                        && !this.moved_at.contains_key(&binding.declaration)
                                    {
                                        this.uninitialized.push(super::UninitializedUse {
                                            name: name.as_str().into(),
                                            declaration: binding.declaration,
                                            reference,
                                            ty: binding.ty.clone(),
                                            possibly: first.is_some(),
                                            operator: None,
                                        });
                                    } else {
                                        return Err(
                                            this.moved_value(name, binding, reference, true)
                                        );
                                    }
                                }
                                // A `&mut` local held on a place a call or
                                // a setup reaches (`let r = o.insert(1);`)
                                // is read as the expression `r` reads it.
                                if let Some((aliased, Lvalue::Nested(_))) =
                                    this.aliases.get(name.as_str())
                                    && *aliased == binding.declaration
                                {
                                    let at = expression.start();
                                    let token = Token::Identifier(Box::new(name.as_str().into()));
                                    this.splice_tokens(vec![(token, at), (Token::Comma, at)]);
                                    let captured = this.parse_expr()?;
                                    this.expect(Token::Comma, "an inline `format!` argument")?;
                                    arguments.push((Some(name.as_str().into()), captured));
                                    return Ok(arguments.len() - 1);
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
                    let trait_name = match spec.kind {
                        Kind::Display => Some("Display"),
                        Kind::Debug | Kind::HexDebug(_) => Some("Debug"),
                        Kind::Radix(_) | Kind::Exp { .. } => None,
                    };
                    if let Some(trait_name) = trait_name {
                        // A name no explicit argument has is the value it
                        // names, which the placeholder captures.
                        let inline = matches!(spec.argument_of(&key), Some(Argument::Name(_)))
                            && arguments[index].1.span == expression;
                        placeholders.push((index, trait_name, placeholder, inline));
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
            let (display, debug) = pieces
                .iter()
                .fold((false, false), |(display, debug), piece| match piece {
                    Err((used, spec)) if *used == index => (
                        display || spec.kind == Kind::Display,
                        debug || spec.kind == Kind::Debug,
                    ),
                    _ => (display, debug),
                });
            // A reference, box, `Rc` or `Arc` displays as its target.
            loop {
                // `where &'a T: Display`: the value writes as the parameter
                // the clause bounds it as.
                if (display || debug)
                    && let Some(parameter) = self.assumed_for(&ty, |this, ty| {
                        (!display || this.type_implements(ty, "Display"))
                            && (!debug || this.type_provides(ty, capability::Kind::Debug))
                    })
                {
                    ty = parameter;
                    break;
                }
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
            types.push((ty, self.whole_expression(argument.span)));
        }
        placed.literal = template_span;
        placed.placeholders = placeholders
            .into_iter()
            .map(|(index, trait_name, placeholder, inline)| {
                (types[index].1, trait_name, placeholder, inline)
            })
            .collect();
        placed.template = template;

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
                    declared: None,
                    required_by: None,
                    formatted: None,
                    chain: crate::resolve::Chain::default(),
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
            // A `fmt` body's `value.fmt(f)` formats with the body's own
            // spec; a spec on a type whose `Display` is the program's (or a
            // generic value's) goes to that `fmt` body as its formatter.
            // Either way the body applies the spec, not a padding after.
            let runtime = if forwarded {
                Some(FORMAT_SPEC.to_string())
            } else if spec.is_set() && self.display_takes_spec(&ty) {
                let parts = self.temporary("spec");
                source.push_text(format!(
                    "let {parts} = {};\n",
                    spec.runtime(width.as_deref(), precision.as_deref())
                ));
                Some(parts)
            } else {
                None
            };
            let taken = runtime.is_some() && spec.kind == Kind::Display;
            let mismatch = |expected: &str| Error::TypeMismatch {
                expected: expected.into(),
                actual: ty.name(&self.models),
                expression: at,
            };
            // A value whose type a later call settles, as a closure
            // parameter: its piece is written once the types are known.
            if contains_inference(&ty)
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
            let value = if let Some(runtime) = runtime.filter(|_| taken) {
                self.display_with_spec(&ty, name, &runtime)
                    .ok_or_else(|| self.undisplayable(&ty, at))?
            } else if spread {
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
                let rendered = self.debug_argument(&ty, name, at);
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
                                Ty::Int(_)
                                    | Ty::String
                                    | Ty::Str
                                    | Ty::Char
                                    | Ty::Bool
                                    | Ty::Arguments
                            )
                        {
                            return Err(self.unsupported("a `{:?}` width on a compound value"));
                        }
                        let rendered = self.debug_argument(&ty, name, at)?;
                        if spec.alternate {
                            self.prelude_op(PreludeOp::PrettyDebug, [rendered])
                        } else {
                            rendered
                        }
                    }
                    Kind::Display => match self.display_piece(&ty, name) {
                        Some(text) => text,
                        None => return Err(self.undisplayable(&ty, at)),
                    },
                }
            };
            let prefixed = spec.alternate
                && int.is_some()
                && matches!(spec.kind, Kind::Radix(_) | Kind::HexDebug(_));
            let truncated = precision.is_some() && !number && spec.kind == Kind::Display;
            // `str`'s and `char`'s `Debug` ignore width and precision, and
            // so does each of `fmt::Arguments`'s writes, as rustc's.
            let quoted = spec.kind == Kind::Debug && matches!(ty, Ty::String | Ty::Str | Ty::Char)
                || ty == Ty::Arguments;
            if !quoted
                && !spread
                && !taken
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
        // A moved local compared alone is borrowed by the expansion's
        // `&$left`, which rustc labels as the whole call.
        let lone: Vec<ImmutableString> = [&left, &right]
            .into_iter()
            .filter_map(|operand| match operand.as_slice() {
                [(Token::Identifier(name), _)] => Some(name.as_str().into()),
                _ => None,
            })
            .collect();
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
        let call = span(name_span.start(), text_end(end, ")"));
        let mut value = self.parse_block_expr().map_err(|error| match error {
            Error::MovedValue { ref name, .. } if lone.contains(name) => error.borrowed_by(call),
            error => error,
        })?;
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
        // `where &'a T: Display`: the value writes as the parameter the
        // clause bounds it as.
        if let Some(parameter) =
            self.assumed_for(ty, |this, ty| this.type_implements(ty, "Display"))
        {
            return self.display_text(&parameter, value);
        }
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
            // `format_args!`'s `Display` writes the text it formats.
            Ty::String | Ty::Str | Ty::Arguments => {
                Some(self.prelude_op(PreludeOp::DisplayText, [value]))
            }
            Ty::Int(_) | Ty::Float(_) | Ty::Char | Ty::Bool => Some(value),
            _ => None,
        }
    }

    /// rustc's E0277 for a value of `ty`, which has no `Display`, at `at`
    /// in a `format!`.
    fn undisplayable(&self, ty: &Ty, at: Span) -> Error {
        // The program's impl, whose own bounds the type's arguments fail:
        // the innermost type they need it of.
        if let Ty::Script(id, _) = ty
            && self.models.implements("Display", *id)
        {
            return self.unmet_bound(ty, Obligation::Trait("Display".into()), at, None);
        }
        Error::MissingCapability {
            owner: ty.name(&self.models),
            capability: "Display".into(),
            operation: at,
            derive: false,
            declared: self.declared_name(ty),
            required_by: None,
            formatted: None,
            chain: crate::resolve::Chain::default(),
        }
    }

    /// Write each pending `format!` piece over its placeholder, now that
    /// its value's type is known.
    pub(super) fn finish_formats(&mut self) -> Result<(), Error> {
        for pending in std::mem::take(&mut self.pending_formats) {
            let mut ty = self.inference.resolve(&pending.ty);
            let mut name = origin::Fragment::plain(pending.name.as_str());
            loop {
                // `where &'a T: Display`: the value writes as the
                // parameter the clause bounds it as.
                let debug = pending.debug;
                if let Some(parameter) = self.assumed_for(&ty, |this, ty| {
                    if debug {
                        this.type_provides(ty, capability::Kind::Debug)
                    } else {
                        this.type_implements(ty, "Display")
                    }
                }) {
                    ty = parameter;
                    break;
                }
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
                    let rendered = self.debug_argument(&ty, name, pending.span)?;
                    if pending.alternate {
                        self.prelude_op(PreludeOp::PrettyDebug, [rendered])
                    } else {
                        rendered
                    }
                }
                _ => match self.display_piece(&ty, name) {
                    Some(text) => text,
                    None => return Err(self.undisplayable(&ty, pending.span)),
                },
            };
            let rendered = rendered.as_str();
            self.output.replace(&pending.placeholder, rendered);
            for closure in &mut self.lifted {
                closure.body.replace(&pending.placeholder, rendered);
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
            Ty::Script(id, arguments) => self
                .models
                .std_impl_at(*id, arguments, "Display", "fmt")
                .filter(|_| self.impl_holds("Display", *id, arguments))
                .map(|definition| Self::std_call(definition, value, &["\"\"".into()])),
            Ty::Param(..) | Ty::Projection(..) | Ty::Dyn(..)
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
            Ty::Thread(thread::Kind::SendError, _) => Some(self.thread_text(value, false)),
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

    /// Whether a value of `ty` is written by a `fmt` body, which takes the
    /// spec it is formatted with: a script type's hand-written `Display`,
    /// or a generic value's by its bound.
    fn display_takes_spec(&self, ty: &Ty) -> bool {
        if self.std_error(ty, true) {
            return false;
        }
        match ty {
            Ty::Script(id, arguments) => {
                self.models.std_impl(*id, "Display", "fmt").is_some()
                    && self.impl_holds("Display", *id, arguments)
            }
            Ty::Param(..) | Ty::Projection(..) | Ty::Dyn(..) => self.type_implements(ty, "Display"),
            _ => false,
        }
    }

    /// `value` of type `ty` as `{}` writes it under the runtime spec
    /// `spec` (`DebugSpec`): a `fmt` body takes the spec as its
    /// formatter, and a builtin is padded by it, as std's `Display` pads.
    fn display_with_spec(
        &mut self,
        ty: &Ty,
        value: origin::Fragment,
        spec: &str,
    ) -> Option<origin::Fragment> {
        if self.display_takes_spec(ty) {
            return Some(match ty {
                Ty::Script(id, arguments) => Self::std_call(
                    self.models.std_impl_at(*id, arguments, "Display", "fmt")?,
                    value,
                    &[spec.into()],
                ),
                _ => Self::std_dispatch("Display", "fmt", value, &[spec.into()]),
            });
        }
        let (value, code) = match DebugLeaf::code_of(ty) {
            Some(code) => (value, code),
            None => (self.display_piece(ty, value)?, 0),
        };
        Some(self.prelude_op(
            PreludeOp::FormatSpecDisplay,
            [value, spec.into(), code.to_string().into()],
        ))
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
            self.forward_spec = true;
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

impl<'a> Parser<'a> {
    /// The formatter `name` is the text the body writes, a mutable
    /// `String`, which the body gives back (`wrap_formatter`).
    pub(super) fn bind_formatter(&mut self, name: ImmutableString) {
        if let Some(binding) = self.bindings.get_mut(name.as_str()) {
            binding.ty = Ty::String;
            binding.mutable = true;
        }
        self.formatter = Some(name);
        self.return_blocked = Some("from a `fmt` body; use its tail");
    }

    /// `panic!`, `assert!`, `assert_eq!`, `assert_ne!`, their `debug_`
    /// forms, `unreachable!`, `todo!` and `unimplemented!` after the `!`,
    /// with Rust's messages. A panic throws its message.
    #[inline(never)]
    pub(super) fn parse_panic_macro(
        &mut self,
        name: &str,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let unit = matches!(self.peek(), Token::Unit);
        let (tokens, end) = self.take_argument_tokens()?;
        let call = span(name_span.start(), end);
        // The `)` that closes the arguments, which `()` starts before.
        let closing = if unit { text_end(end, "(") } else { end };
        let name = name.strip_prefix("debug_").unwrap_or(name);
        if let Some(operator) = match name {
            "assert_eq" => Some((Token::EqualsTo, "==")),
            "assert_ne" => Some((Token::NotEqualsTo, "!=")),
            _ => None,
        } {
            return self.parse_assert_compare(operator, tokens, name_span, closing);
        }
        if name == "assert" {
            // rustc's errors for a missing condition: none at all, or an
            // argument list that starts with its comma.
            if let Some((Token::Comma, comma)) = tokens.first() {
                return Err(Error::Syntax {
                    message: "expected expression, found `,`".into(),
                    label: "expected expression".into(),
                    help: None,
                    at: point_span(*comma),
                });
            }
            if tokens.is_empty() {
                return Err(Error::Syntax {
                    message: "macro requires a boolean expression as an argument".into(),
                    label: "boolean expression required".into(),
                    help: None,
                    at: span(name_span.start(), text_end(closing, ")")),
                });
            }
            let mut parts = split_arguments(tokens);
            let condition_tokens = parts.remove(0);
            let text = format!("assertion failed: {}", token_text(&condition_tokens));
            let mut spliced = condition_tokens;
            spliced.push((Token::RightParen, end));
            self.splice_tokens(spliced);
            let condition = self.parse_expr()?;
            self.expect(Token::RightParen, "')' after the condition of `assert!`")?;
            self.require_type(&Ty::Bool, &condition.ty, condition.span)?;
            let message = match self.panic_format(parts, name_span, end)? {
                Some(message) => message,
                None => origin::Fragment::plain(format!("{text:?}")),
            };
            let mut source = origin::Fragment::plain("if !(");
            source.push(condition.rendered_source());
            source.push_text(") { __rawr_throw ");
            source.push(message);
            source.push_text("; }");
            return Ok(self.operator_expr(source, Ty::Unit, call));
        }
        let prefix = match name {
            "todo" => Some("not yet implemented"),
            "unimplemented" => Some("not implemented"),
            "unreachable" => Some("internal error: entered unreachable code"),
            _ => None,
        };
        let message = match (
            self.panic_format(split_arguments(tokens), name_span, end)?,
            prefix,
        ) {
            (Some(message), None) => message,
            (Some(message), Some(prefix)) => {
                let mut text = origin::Fragment::plain(format!("{:?} + (", format!("{prefix}: ")));
                text.push(message);
                text.push_text(")");
                text
            }
            (None, prefix) => {
                origin::Fragment::plain(format!("{:?}", prefix.unwrap_or("explicit panic")))
            }
        };
        let mut source = origin::Fragment::plain("{ __rawr_throw ");
        source.push(message);
        source.push_text("; }");
        Ok(self.operator_expr(source, Ty::Never, call))
    }

    /// `dbg!(value)`: `[file:line:column] value = {value:#?}` on stderr, as
    /// std's `dbg!` writes it, and the value itself. `dbg!()` writes only
    /// where it is.
    pub(super) fn parse_dbg(&mut self, name_span: Span) -> Result<ParsedExpr, Error> {
        self.next();
        let at = name_span.start();
        let file = match self.source_file() {
            Some(file) => cargo_env::file_path(&file),
            None => self.source_identity(),
        };
        let place = format!(
            "[{file}:{}:{}]",
            at.line().unwrap_or_default(),
            at.position().unwrap_or_default()
        );
        if matches!(self.peek(), Token::Unit) {
            let (_, end) = self.next();
            let text = origin::Fragment::plain(format!("{:?}", format!("{place}\n")));
            let source = self.prelude_op(PreludeOp::Print, [text, "true".into()]);
            return Ok(self.operator_expr(source, Ty::Unit, span(name_span.start(), end)));
        }
        self.expect(Token::LeftParen, "'(' after `dbg!`")?;
        let tokens = self.input.clone();
        let mut value = self.parse_expr()?;
        let stop = self.position();
        if matches!(self.peek(), Token::Comma) {
            self.next();
        }
        if !matches!(self.peek(), Token::RightParen) {
            return Err(self.unsupported("`dbg!` of more than one value"));
        }
        let end = self.expect(Token::RightParen, "')' after `dbg!`")?;
        let tokens = tokens
            .take_while(|(_, position)| *position != stop)
            .collect::<Vec<_>>();
        let key = self.models.sources.key(self.models.context.index());
        let written = self
            .models
            .sources
            .resolve(key)
            .ok()
            .and_then(|source| stringify_tokens(source.text(), &tokens, stop))
            .unwrap_or_else(|| {
                tokens
                    .iter()
                    .map(|(token, _)| token.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            });
        self.move_local(&mut value)?;
        let held = self.temporary("dbg");
        let debug = self.debug_source(&value.ty, held.as_str().into(), value.span)?;
        let pretty = self.prelude_op(PreludeOp::PrettyDebug, [debug]);
        let mut text = origin::Fragment::plain(format!("{:?} + ", format!("{place} {written} = ")));
        text.push(pretty);
        text.push_text(" + \"\\n\"");
        let print = self.prelude_op(PreludeOp::Print, [text, "true".into()]);
        let mut source = origin::Fragment::plain(format!("{{ let {held} = "));
        source.push(value.rendered_source());
        source.push_text("; ");
        source.push(print);
        source.push_text(format!("; {held} }}"));
        Ok(self.operator_expr(source, value.ty, span(name_span.start(), end)))
    }

    /// The `{:?}` text of `value`, an option at runtime of an `element`
    /// once its first variant of `names`: `Some(..)` or `None` of an
    /// `Option`, `Ready(..)` or `Pending` of a `Poll`.
    fn variant_debug(
        &mut self,
        element: &Ty,
        value: origin::Fragment,
        [some, none]: [&str; 2],
        span: Span,
    ) -> Result<origin::Fragment, Error> {
        let option = self.temporary("debug");
        let mut source = origin::Fragment::plain(format!("{{ let {option} = "));
        source.push(value);
        source.push_text("; if ");
        let test = self.prelude_op(PreludeOp::IsSome, [option.as_str().into()]);
        let inner = self.prelude_op(PreludeOp::Unwrap, [option.as_str().into()]);
        let inner = self.debug_source(element, inner, span)?;
        source.push(test);
        source.push_text(format!(" {{ \"{some}(\" + "));
        source.push(inner);
        source.push_text(format!(" + \")\" }} else {{ \"{none}\" }} }}"));
        Ok(source)
    }

    /// [`Self::debug_source`] of a format argument, whose missing `Debug`
    /// rustc reports at the innermost type that lacks it, with each impl
    /// that needed it: `Opaque` of a `Vec<Opaque>`.
    fn debug_argument(
        &mut self,
        ty: &Ty,
        value: origin::Fragment,
        span: Span,
    ) -> Result<origin::Fragment, Error> {
        self.debug_source(ty, value, span).map_err(|mut error| {
            if let Error::MissingCapability {
                owner,
                capability,
                chain,
                ..
            } = &mut error
                && !self.type_provides(ty, capability::Kind::Debug)
            {
                // The chain from the argument's own type, which an error
                // from inside it starts short of.
                let unmet = self.unmet(ty, Obligation::Capability(capability::Kind::Debug));
                if unmet.ty.name(&self.models) == *owner
                    && unmet.obligation.name() == capability.as_str()
                {
                    *chain = crate::resolve::Chain::new(unmet.chain);
                }
            }
            error
        })
    }

    /// A `String` expression rendering `value` of type `ty` as Rust's
    /// `{:?}` does, when `ty` is `Debug`.
    pub(super) fn debug_source(
        &mut self,
        ty: &Ty,
        value: origin::Fragment,
        span: Span,
    ) -> Result<origin::Fragment, Error> {
        let mut ty = self.inference.resolve(ty);
        // `let c: Result<Color, _> = v.try_into();`: the error type is the
        // impl's, which the target the annotation gave selects.
        if matches!(ty, Ty::Infer(_)) {
            self.settle_try_from_errors();
            ty = self.inference.resolve(&ty);
        }
        if self.std_error(&ty, false) {
            return Ok(self.error_text(value, true));
        }
        // std's `Debug` of a `dyn Any`, which its value writes
        // (`any_object.rs`).
        if self.std_any(&ty, false) {
            return Ok(self.prelude_op(PreludeOp::GenericDebug, [value]));
        }
        // A `Cow` writes what it derefs to.
        if let Ty::Cow(borrowed) = &ty {
            let payload = self.prelude_op(PreludeOp::CowValue, [value]);
            return self.debug_source(&cow::payload_ty(borrowed), payload, span);
        }
        // `where Vec<T>: Debug`: the value writes as the parameter the
        // clause bounds it as.
        if let Some(parameter) = self.assumed_for(&ty, |this, ty| {
            this.type_provides(ty, capability::Kind::Debug)
        }) {
            return self.debug_source(&parameter, value, span);
        }
        // A derive or an impl whose own bounds the type's arguments fail:
        // `W<Opaque>` of `#[derive(Debug)] struct W<T>(T)`.
        if matches!(ty, Ty::Script(..)) && !self.type_provides(&ty, capability::Kind::Debug) {
            let error = self.unmet_bound(
                &ty,
                Obligation::Capability(capability::Kind::Debug),
                span,
                None,
            );
            if matches!(&error, Error::MissingCapability { chain, .. } if !chain.requirements().is_empty())
            {
                return Err(error);
            }
        }
        if let Some(spec) = self.debug_spec.clone() {
            // A leaf under a `{:?}` spec: a number, `bool` or `()` takes it.
            let leaf = match &ty {
                // An integer literal's type defaults to `i32`.
                Ty::Int(Int::Var(_)) => Some(1032),
                Ty::Int(kind) => {
                    Some(i64::from(kind.bits()) + if kind.signed() { 1000 } else { 2000 })
                }
                Ty::Float(Float::F32) => Some(1),
                Ty::Float(_) => Some(2),
                Ty::Bool => Some(3),
                Ty::Unit => Some(4),
                _ => None,
            };
            if let Some(code) = leaf {
                return Ok(self.prelude_op(
                    PreludeOp::FormatDebugLeaf,
                    [value, spec.as_str().into(), code.to_string().into()],
                ));
            }
            match &ty {
                Ty::Script(id, ..) if self.models.std_impl(*id, "Debug", "fmt").is_none() => {
                    let model = self.models.definition(*id).clone();
                    self.require_capability(&model, capability::Kind::Debug, span)?;
                    return Ok(
                        self.prelude_op(PreludeOp::FormatSpecDebug, [value, spec.as_str().into()])
                    );
                }
                Ty::Script(..)
                | Ty::Param(..)
                | Ty::Projection(..)
                | Ty::Time(_)
                | Ty::Host { .. } => {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: format!(
                            "a width, precision, sign or `x?` in `{{:?}}` of `{}`, whose `Debug` \
                             is its own or generic",
                            ty.name(&self.models)
                        )
                        .into(),
                        source: span,
                    });
                }
                _ => {}
            }
        }
        Ok(match &ty {
            Ty::Reference { target, .. } | Ty::Boxed(target) => {
                self.debug_source(target, value, span)?
            }
            // An `Rc` or `Arc` writes its value's `Debug`, and a `Weak`
            // `(Weak)`, as std's do.
            Ty::Counted(_, target) => {
                let value = self.counted_call(PreludeOp::CountedGet, value, []);
                self.debug_source(target, value, span)?
            }
            Ty::Weak(..) => origin::Fragment::plain("\"(Weak)\""),

            // A generic value, an associated type's by its bounds, or a
            // trait object by its trait's.
            Ty::Param(..) | Ty::Projection(..) | Ty::Dyn(..) => {
                if !self.type_provides(&ty, capability::Kind::Debug) {
                    return Err(Error::MissingCapability {
                        owner: ty.name(&self.models),
                        capability: "Debug".into(),
                        operation: span,
                        derive: matches!(ty, Ty::Script(..)),
                        declared: self.declared_name(&ty),
                        required_by: None,
                        formatted: None,
                        chain: crate::resolve::Chain::default(),
                    });
                }
                if self.models.std_traits.contains("Debug") {
                    Self::std_dispatch("Debug", "fmt", value, &["\"\"".into()])
                } else {
                    self.prelude_op(PreludeOp::GenericDebug, [value])
                }
            }
            Ty::Unit => "\"()\"".into(),
            // std's `Debug` of `!`: a value that never comes is never written.
            Ty::Never => value,
            Ty::Int(Int::U64 | Int::Usize) => {
                self.prelude_op(PreludeOp::FormatRadix, [value, "10".into()])
            }
            Ty::Float(kind) => {
                let mode = if *kind == Float::F32 { "3" } else { "1" };
                self.prelude_op(PreludeOp::FormatFloat, [value, "-1".into(), mode.into()])
            }
            Ty::Bool | Ty::Int(_) => value.surrounded("(\"\" + ", ")"),
            // A `ParseIntError` and kin: its `Debug` by its message.
            Ty::ParseError(error) => {
                let message = self.temporary("debug");
                let mut source = origin::Fragment::plain(format!("{{ let {message} = "));
                source.push(value);
                source.push_text(format!("; __rawr_switch {message} {{ "));
                for (text, debug) in error.texts() {
                    source.push_text(format!("{text:?} => {debug:?}, "));
                }
                source.push_text(format!("_ => {message} }} }}"));
                source
            }
            // Rust's own `Debug` text of a `Duration` (`1.5s`) or `Instant`.
            Ty::Time(_) => {
                crate::host::debug::register::<std::time::Duration>();
                crate::host::debug::register::<crate::Instant>();
                self.prelude_op(PreludeOp::GenericDebug, [value])
            }
            Ty::Ordering => {
                let ordering = self.temporary("debug");
                let mut source = origin::Fragment::plain(format!("{{ let {ordering} = "));
                source.push(value);
                source.push_text(format!(
                    "; if {ordering} < 0 {{ \"Less\" }} else if {ordering} == 0 {{ \"Equal\" }} \
                     else {{ \"Greater\" }} }}"
                ));
                source
            }
            // `fmt::Arguments`'s `Debug` writes its text unquoted (#1561).
            Ty::Arguments => self.prelude_op(PreludeOp::DisplayText, [value]),
            // Rust's `escape_debug`: quotes, `\n`, `\\`, `\u{..}` and kin.
            Ty::String | Ty::Str | Ty::Char => {
                let code = STR_PIECES
                    .iter()
                    .position(|name| *name == "debug")
                    .expect("a listed piece")
                    .to_string();
                self.prelude_op(
                    PreludeOp::StrPieces,
                    [value, code.into(), "()".into(), "()".into()],
                )
            }
            Ty::Option(element) => self.variant_debug(element, value, ["Some", "None"], span)?,
            // std's `Poll<T>`, an `Option` at runtime: `Ready(..)` or
            // `Pending`.
            ty if let Some(element) = self.poll_output(ty) => {
                self.variant_debug(&element, value, ["Ready", "Pending"], span)?
            }
            Ty::Result(ok, err) => {
                let result = self.temporary("debug");
                let mut source = origin::Fragment::plain(format!("{{ let {result} = "));
                source.push(value);
                source.push_text("; if ");
                let test = self.prelude_op(PreludeOp::IsOk, [result.as_str().into()]);
                let ok_value = self.prelude_op(PreludeOp::UnwrapOk, [result.as_str().into()]);
                let ok_value = self.debug_source(ok, ok_value, span)?;
                let err_value = self.prelude_op(PreludeOp::UnwrapErr, [result.as_str().into()]);
                let err_value = self.debug_source(err, err_value, span)?;
                source.push(test);
                source.push_text(" { \"Ok(\" + ");
                source.push(ok_value);
                source.push_text(" + \")\" } else { \"Err(\" + ");
                source.push(err_value);
                source.push_text(" + \")\" } }");
                source
            }
            Ty::Tuple(members) => {
                let tuple = self.temporary("debug");
                let mut source = origin::Fragment::plain(format!("{{ let {tuple} = "));
                source.push(value);
                source.push_text("; \"(\"");
                for (index, member) in members.iter().enumerate() {
                    let rendered =
                        self.debug_source(member, format!("{tuple}[{index}]").into(), span)?;
                    source.push_text(if index == 0 { " + " } else { " + \", \" + " });
                    source.push(rendered);
                }
                source.push_text(if members.len() == 1 {
                    " + \",)\" }"
                } else {
                    " + \")\" }"
                });
                source
            }
            Ty::Array { element, .. } => {
                let receiver = self.temporary("debug_values");
                let mut routed = origin::Fragment::plain(format!("{{ let {receiver} = "));
                routed.push(value);
                routed.push_text("; if ");
                routed.push(self.prelude_op(PreludeOp::NativePlace, [receiver.as_str().into()]));
                routed.push_text(" { ");
                routed.push(self.prelude_op(PreludeOp::GenericDebug, [receiver.as_str().into()]));
                routed.push_text(" } else ");
                let item = self.temporary("debug");
                let text = self.temporary("debug");
                let rendered = self.debug_source(element, item.as_str().into(), span)?;
                let mut source =
                    origin::Fragment::plain(format!("{{ let {text} = \"[\"; for {item} in "));
                source.push(receiver.as_str().into());
                source.push_text(format!(
                    " {{ if {text} != \"[\" {{ {text} += \", \"; }} {text} += "
                ));
                source.push(rendered);
                source.push_text(format!("; }} {text} + \"]\" }}"));
                routed.push(source);
                routed.push_text(" }");
                routed
            }
            Ty::Script(id, arguments) if self.models.std_impl(*id, "Debug", "fmt").is_some() => {
                let definition = self
                    .models
                    .std_impl_at(*id, arguments, "Debug", "fmt")
                    .expect("guarded by the match arm");
                Self::std_call(definition, value, &["\"\"".into()])
            }
            Ty::Reverse(inner) => {
                let unwrapped = self.prelude_op(PreludeOp::ReverseInner, [value]);
                self.debug_source(inner, unwrapped, span)?
                    .surrounded("(\"Reverse(\" + ", " + \")\")")
            }
            // std's `Debug` of a `Mutex` no thread poisoned.
            Ty::Cell(CellKind::Mutex, inner) => {
                let unwrapped = self.prelude_op(PreludeOp::CellGet, [value]);
                self.debug_source(inner, unwrapped, span)?
                    .surrounded("(\"Mutex { data: \" + ", " + \", poisoned: false, .. }\")")
            }
            Ty::Cell(kind, inner) => {
                let unwrapped = self.prelude_op(PreludeOp::CellGet, [value]);
                let open = format!("(\"{} {{ value: \" + ", kind.name());
                self.debug_source(inner, unwrapped, span)?
                    .surrounded(&open, " + \" }\")")
            }
            // `{1: "a", 2: "b"}` and `{1, 2}`, in key order.
            Ty::Keyed(kind, arguments) => {
                let item = self.temporary("debug");
                let text = self.temporary("debug");
                let rendered = if kind.is_map() {
                    let mut pair =
                        self.debug_source(&arguments[0], format!("{item}[0]").into(), span)?;
                    pair.push_text(" + \": \" + ");
                    pair.push(self.debug_source(
                        &arguments[1],
                        format!("{item}[1]").into(),
                        span,
                    )?);
                    pair
                } else {
                    self.debug_source(&arguments[0], item.as_str().into(), span)?
                };
                let mut source =
                    origin::Fragment::plain(format!("{{ let {text} = \"{{\"; for {item} in "));
                source.push(value);
                source.push_text(format!(
                    " {{ if {text} != \"{{\" {{ {text} += \", \"; }} {text} += "
                ));
                source.push(rendered);
                source.push_text(format!("; }} {text} + \"}}\" }}"));
                source
            }
            // std's ranges show as Rust's `0..5`, `'a'..='e'`, `0.5..` and
            // `..` do: their runtime values are std's, of their bounds'
            // values.
            range if let Some((_, element)) = range::parts(range) => {
                if !self.type_provides(&element, capability::Kind::Debug) {
                    return Err(Error::MissingCapability {
                        owner: ty.name(&self.models),
                        capability: "Debug".into(),
                        operation: span,
                        derive: false,
                        declared: self.declared_name(&ty),
                        required_by: None,
                        formatted: None,
                        chain: crate::resolve::Chain::default(),
                    });
                }
                self.prelude_op(PreludeOp::GenericDebug, [value])
            }
            // A facade type providing `Debug`: Rust's text, as its facade
            // registered it (`host::debug`).
            Ty::Host { proof, .. }
                if proof
                    .definition
                    .capabilities()
                    .contains(capability::Kind::Debug) =>
            {
                self.prelude_op(PreludeOp::GenericDebug, [value])
            }
            Ty::Script(id, ..) => {
                let model = self.models.definition(*id).clone();
                self.require_capability(&model, capability::Kind::Debug, span)?;
                let helper = self.push_op(Op::Debug {
                    definition: model.definition().clone(),
                });
                origin::Fragment::call(&helper, [value])
            }
            Ty::Thread(..) => self.thread_text(value, true),
            // A local's type nothing has given yet, as the element of a
            // `Vec::new()` only `.into()`s go in: rustc needs it (E0282).
            Ty::Infer(_) if let Some(error) = self.unsettled(&ty, span, None) => {
                return Err(error);
            }
            _ => {
                return Err(Error::MissingCapability {
                    owner: ty.name(&self.models),
                    capability: "Debug".into(),
                    operation: span,
                    derive: matches!(ty, Ty::Script(..)),
                    declared: self.declared_name(&ty),
                    required_by: None,
                    formatted: None,
                    chain: crate::resolve::Chain::default(),
                })
            }
        })
    }

    /// A `fmt` body's formatter comes in as the spec its caller formats
    /// with (a `DebugSpec`, or `""` for none), as std's `Formatter` carries
    /// one: the body keeps it in [`FORMAT_SPEC`], which `pad` and the
    /// formatter's `width`, `fill` and kin read, and writes its text into
    /// the formatter from empty, which `wrap_formatter` gives back.
    pub(super) fn open_formatter(&mut self, body: Span) {
        let Some(formatter) = self.formatter.clone() else {
            return;
        };
        self.write_written(
            format!("let {FORMAT_SPEC} = {formatter}; {formatter} = \"\";"),
            body,
        );
    }

    /// A `fmt` body returns the text it wrote: its `fmt::Result` tail is
    /// evaluated, then the formatter is the function's value.
    pub(super) fn wrap_formatter(&mut self, body: Span) {
        let Some(formatter) = self.formatter.clone() else {
            return;
        };
        let (value, original) = match self.last_expression.take() {
            Some((start, original)) => {
                let mut value = self.output.split_off(start);
                value.trim_end();
                (value, original)
            }
            None => (origin::Fragment::plain("()"), body),
        };
        let mut rendered = origin::Fragment::plain("{ let __rawr_fmt_result = ");
        rendered.push(value);
        rendered.push_text(format!("; {formatter} }}"));
        self.write_output(rendered, original);
    }

    pub(super) fn write_output(
        &mut self,
        rendered: impl Into<origin::Fragment>,
        original: Span,
    ) -> usize {
        let start = self.output.len();
        let mut rendered = rendered.into();
        rendered.cover(original);
        self.output.push(rendered);
        self.output.push_text("\n");
        start
    }

    /// Write `rendered`, statements the compiler adds around the program's
    /// own, such as a parameter's check where a body starts: an error in
    /// them is located at `original`, but a debugger doesn't stop at them.
    pub(super) fn write_written(&mut self, rendered: impl Into<origin::Fragment>, original: Span) {
        let mut rendered = rendered.into();
        rendered.cover_written(original);
        self.output.push(rendered);
        self.output.push_text("\n");
    }

    /// Run `parse` over statements the compiler spliced in itself, such as
    /// a parameter's binding where a body starts, and mark every origin it
    /// writes as written: an error in them is located where the tokens
    /// were placed, but a debugger doesn't stop at them.
    pub(super) fn parse_written<T>(
        &mut self,
        parse: impl FnOnce(&mut Self) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let start = self.output.len();
        let parsed = parse(self)?;
        let mut written = self.output.split_off(start);
        written.mark_written();
        self.output.push(written);
        Ok(parsed)
    }
}

/// `=` or a compound assignment.
/// The panic macros of Rust's prelude (`parse_panic_macro`).
pub(super) const PANIC_MACROS: &[&str] = &[
    "panic",
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "unreachable",
    "todo",
    "unimplemented",
];
