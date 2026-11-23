//! std's `Hash` impls for its own types, read as a program's impls are
//! (`std_trait::impls`): each writes the value into the `Hasher` with the
//! calls std's makes, so a program's `Hasher` sees rustc's sequence. An
//! integer writes itself with its kind's `write_*`, and a slice of them
//! all their native-endian bytes in one `write`. A `str` writes its bytes
//! and `0xff`, a slice and a `Vec` their length before their elements, an
//! `Option` or a `Result` its discriminant as an `isize` before its
//! payload, as their derive does, and a tuple each member in turn.

/// The definition of std's `Hash`, which each impl is of.
pub(super) const HASH: &str = "core::hash::Hash";

/// The integer kinds, each with the `Hasher` method std's impl calls.
const INTEGERS: &[(&str, &str)] = &[
    ("u8", "write_u8"),
    ("u16", "write_u16"),
    ("u32", "write_u32"),
    ("u64", "write_u64"),
    ("u128", "write_u128"),
    ("usize", "write_usize"),
    ("i8", "write_i8"),
    ("i16", "write_i16"),
    ("i32", "write_i32"),
    ("i64", "write_i64"),
    ("i128", "write_i128"),
    ("isize", "write_isize"),
];

/// The impls whose text is the same for every program, `{trait}` the
/// name it knows `Hash` by.
const FIXED: &[&str] = &[
    "impl {trait} for bool {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u8(*self as u8)
    }
}",
    "impl {trait} for char {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u32(*self as u32)
    }
}",
    "impl {trait} for str {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(self.as_bytes());
        state.write_u8(0xff)
    }
}",
    "impl {trait} for String {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(self.as_bytes());
        state.write_u8(0xff)
    }
}",
    "impl {trait} for () {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}",
    "impl<T: {trait}> {trait} for [T] {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_usize(self.len());
        T::hash_slice(self, state)
    }
}",
    "impl<T: {trait}> {trait} for Vec<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_usize(self.len());
        T::hash_slice(self, state)
    }
}",
    "impl<T: {trait}> {trait} for Option<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            None => state.write_isize(0),
            Some(value) => {
                state.write_isize(1);
                value.hash(state)
            }
        }
    }
}",
    "impl<T: {trait}, E: {trait}> {trait} for Result<T, E> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Ok(value) => {
                state.write_isize(0);
                value.hash(state)
            }
            Err(error) => {
                state.write_isize(1);
                error.hash(state)
            }
        }
    }
}",
];

/// The tuples std implements `Hash` for, by their number of members, but
/// the one-member tuple, which is no type rawr reads yet.
const TUPLE_ARITIES: std::ops::RangeInclusive<usize> = 2..=12;

/// The text of each of std's `Hash` impls, `{trait}` the name the program
/// knows the trait by.
pub(super) fn sources() -> impl Iterator<Item = String> {
    let integers = INTEGERS.iter().map(|(kind, write)| {
        format!(
            "impl {{trait}} for {kind} {{
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {{
        state.{write}(*self)
    }}
    fn hash_slice<H: std::hash::Hasher>(data: &[{kind}], state: &mut H) {{
        let mut bytes = Vec::new();
        for piece in data {{
            bytes.extend_from_slice(&piece.to_ne_bytes());
        }}
        state.write(&bytes)
    }}
}}"
        )
    });
    let tuples = TUPLE_ARITIES.map(|arity| {
        let parameters: Vec<String> = (0..arity).map(|index| format!("T{index}")).collect();
        let bounds: Vec<String> = parameters
            .iter()
            .map(|parameter| format!("{parameter}: {{trait}}"))
            .collect();
        let bounded = bounds.join(", ");
        let tuple = format!("({})", parameters.join(", "));
        let members = (0..arity)
            .map(|index| format!("        self.{index}.hash(state);\n"))
            .collect::<String>();
        format!(
            "impl<{bounded}> {{trait}} for {tuple} {{
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {{
{members}    }}
}}"
        )
    });
    FIXED
        .iter()
        .map(|source| (*source).to_owned())
        .chain(integers)
        .chain(tuples)
}

/// The impl `#[derive(Hash)]` writes for one of the program's types: its
/// text, `{trait}` the name the program knows `Hash` by, and the type's
/// name in it with the type it names.
pub(super) struct Derived {
    pub(super) source: String,
    pub(super) name: crate::ImmutableString,
    pub(super) target: super::type_ref::Target,
}

/// The impl `#[derive(Hash)]` writes for each of `items`' structs and
/// enums that derive it, as rustc expands it: each field in turn into the
/// generic `Hasher`, an enum's discriminant first where it has more than
/// one variant, hashed as its `repr` integer (`isize` by default).
pub(super) fn derived(items: &[super::item::Shell]) -> Vec<Derived> {
    use super::item::Kind;
    let mut derived = Vec::new();
    for shell in items {
        let (header, name, generics, regions, target, body) = match &shell.kind {
            Kind::Struct(declared) => (
                &declared.header,
                &declared.name,
                &declared.generics,
                &declared.regions,
                script_target(declared.definition, &declared.key),
                struct_body(declared),
            ),
            Kind::Enum(declared) => (
                &declared.header,
                &declared.name,
                &declared.generics,
                &declared.regions,
                script_target(declared.definition, &declared.key),
                enum_body(declared),
            ),
            _ => continue,
        };
        if !derives_hash(header) {
            continue;
        }
        let lifetimes: Vec<String> = regions
            .params
            .iter()
            .map(|lifetime| match lifetime.strip_prefix('\'') {
                Some(_) => lifetime.to_string(),
                None => format!("'{lifetime}"),
            })
            .collect();
        let mut parameters = lifetimes.clone();
        let mut declared = lifetimes;
        // The impl's own names for the type's parameters, which no field
        // is read by and which leave `H` the hasher's.
        for (index, generic) in generics.iter().enumerate() {
            let parameter = format!("__T{index}");
            declared.push(match &generic.constant {
                Some(ty) => format!("const {parameter}: {}", super::canonical_type_name(ty)),
                None => format!("{parameter}: {{trait}}"),
            });
            parameters.push(parameter);
        }
        let (declared, arguments) = if parameters.is_empty() {
            (String::new(), String::new())
        } else {
            (
                format!("<{}>", declared.join(", ")),
                format!("<{}>", parameters.join(", ")),
            )
        };
        derived.push(Derived {
            source: format!(
                "impl{declared} {{trait}} for {name}{arguments} {{
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {{
{body}    }}
}}",
                name = name.text
            ),
            name: name.text.clone(),
            target,
        });
    }
    derived
}

/// The target a path names the program's type `definition` by.
fn script_target(
    definition: crate::type_def::Id,
    key: &crate::type_def::Key,
) -> super::type_ref::Target {
    super::type_ref::Target::Script {
        definition,
        identity: crate::type_def::Identity::Script(key.clone()),
    }
}

/// Whether `header`'s `#[derive(..)]` names `Hash`, by its name or a path.
fn derives_hash(header: &crate::item::Header) -> bool {
    header
        .attributes
        .iter()
        .filter(|attribute| {
            matches!(attribute.path.segments.as_ref(), [only] if only.name.text == "derive")
        })
        .flat_map(|attribute| attribute.arguments.iter())
        .any(|path| {
            path.segments
                .last()
                .is_some_and(|last| last.name.text == "Hash")
        })
}

/// A derived `hash`'s body for a struct: each field in turn.
fn struct_body(declared: &super::item::Struct) -> String {
    declared
        .fields
        .iter()
        .map(|field| format!("        self.{}.hash(state);\n", field.name.text))
        .collect()
}

/// A derived `hash`'s body for an enum: the discriminant, where it has
/// more than one variant, then the variant's fields in turn.
fn enum_body(declared: &super::item::Enum) -> String {
    use std::fmt::Write;
    let write = repr_write(&declared.header);
    let several = declared.variants.len() > 1;
    let mut discriminant: crate::INT = 0;
    let mut arms = String::new();
    for variant in declared.variants.iter() {
        if let Some(explicit) = variant.discriminant {
            discriminant = explicit;
        }
        let bound: Vec<String> = (0..variant.payload.len())
            .map(|index| format!("__self_{index}"))
            .collect();
        let pattern = match &variant.fields {
            Some(names) => {
                let fields: Vec<String> = names
                    .iter()
                    .zip(&bound)
                    .map(|(name, bound)| format!("{}: {bound}", name.text))
                    .collect();
                format!("Self::{} {{ {} }}", variant.name.text, fields.join(", "))
            }
            None if bound.is_empty() => format!("Self::{}", variant.name.text),
            None => format!("Self::{}({})", variant.name.text, bound.join(", ")),
        };
        let _ = writeln!(arms, "            {pattern} => {{");
        if several {
            let _ = writeln!(arms, "                state.{write}({discriminant});");
        }
        for bound in &bound {
            let _ = writeln!(arms, "                {bound}.hash(state);");
        }
        arms.push_str("            }\n");
        discriminant = discriminant.wrapping_add(1);
    }
    if arms.is_empty() {
        return String::new();
    }
    format!("        match self {{\n{arms}        }}\n")
}

/// The `Hasher` method an enum's discriminant is written with: its
/// `#[repr]` integer's, else `isize`'s.
fn repr_write(header: &crate::item::Header) -> &'static str {
    header
        .attributes
        .iter()
        .filter(|attribute| {
            matches!(attribute.path.segments.as_ref(), [only] if only.name.text == "repr")
        })
        .flat_map(|attribute| attribute.arguments.iter())
        .find_map(|path| {
            let [only] = path.segments.as_ref() else {
                return None;
            };
            INTEGERS
                .iter()
                .find(|(kind, _)| only.name.text == *kind)
                .map(|(_, write)| *write)
        })
        .unwrap_or("write_isize")
}
