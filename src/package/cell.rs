//! Parse cells: each source file's parse, kept on disk so another process
//! loading the same program reads it instead of parsing the file again.
//!
//! A cell is keyed by the file's path and holds the SHA-256 of the text it
//! was parsed from. A file whose text changed misses its cell and is parsed.
//! Every cell carries [`STAMP`]: the format, the crate version and a digest
//! of the parser's own sources. A cell written by another build of `rawr`
//! is never decoded. The file is parsed again and the miss is counted as
//! stale.
//!
//! Only a file whose items are all of it is kept: its script, the text left
//! outside the items, is empty. A file with top-level statements is parsed
//! by each process that loads it.
//!
//! A live session loads its program again on each save. Its cells also
//! remember each parse in memory (see [`Cells::remember`]), so a reload
//! parses only the files whose text changed and decodes nothing.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::item::{self, type_ref};
use crate::{parse, preview, source, ImmutableString, Position, Span};

const MAGIC: &[u8; 4] = b"RPCL";

/// Bumped when the encoding below changes.
const FORMAT: u32 = 8;

/// What a cell must have been written by to be read: the format, the crate
/// version, a digest of the parser's sources (see `build.rs`) and the
/// widths that change what a parse holds.
fn stamp() -> String {
    format!(
        "{FORMAT}/{}/{}/{}.{}",
        env!("CARGO_PKG_VERSION"),
        env!("RAWR_PARSER_DIGEST"),
        core::mem::size_of::<crate::INT>(),
        core::mem::size_of::<crate::FLOAT>(),
    )
}

/// Each source's latest parse by path, with the digest of the text it parsed.
type Remembered = BTreeMap<PathBuf, ([u8; 32], parse::Unit)>;

/// The parse cells of one program, in one directory.
#[derive(Debug)]
pub struct Cells {
    directory: PathBuf,
    stamp: String,
    kept: RefCell<BTreeSet<PathBuf>>,
    parsed: Cell<usize>,
    stale: Cell<usize>,
    /// Each source's latest parse by path, when remembering.
    memory: Option<RefCell<Remembered>>,
}

/// What loading a program through [`Cells`] did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Counts {
    /// Sources read from their cells.
    pub kept: usize,
    /// Sources parsed.
    pub parsed: usize,
    /// Of those parsed, sources whose cell another build of `rawr` wrote.
    pub stale: usize,
}

impl Cells {
    /// The cells kept in `directory`. Nothing is read or created yet.
    #[must_use]
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
            stamp: stamp(),
            kept: RefCell::default(),
            parsed: Cell::new(0),
            stale: Cell::new(0),
            memory: None,
        }
    }

    /// Also keep each parse these cells see in memory, for a process that
    /// loads the same program again and again. A later load reads an
    /// unchanged source from memory: no parse and no decode.
    #[must_use]
    pub fn remember(mut self) -> Self {
        self.memory = Some(RefCell::default());
        self
    }

    /// The directory holding the cells.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// What the loads through these cells did so far.
    #[must_use]
    pub fn counts(&self) -> Counts {
        Counts {
            kept: self.kept.borrow().len(),
            parsed: self.parsed.get(),
            stale: self.stale.get(),
        }
    }

    /// Write a cell for each source of `graph` that was parsed rather than
    /// read from its cell. Returns how many were written.
    ///
    /// # Errors
    ///
    /// The directory or a cell cannot be written.
    pub fn save(&self, graph: &super::Graph) -> io::Result<usize> {
        let kept = self.kept.borrow();
        let mut written = 0;
        for (source, unit) in graph.parsed_units() {
            if kept.contains(source.path()) {
                continue;
            }
            let Some(bytes) = self.encode(source.text(), unit) else {
                continue;
            };
            if written == 0 {
                fs::create_dir_all(&self.directory)?;
            }
            let path = self.path(source.path());
            let partial = path.with_extension(format!("{}.partial", std::process::id()));
            fs::write(&partial, bytes)?;
            fs::rename(&partial, &path)?;
            written += 1;
        }
        Ok(written)
    }

    /// The unit parsed earlier from exactly `text` at `path`, if a cell of
    /// this build holds it.
    pub(crate) fn get(&self, path: &Path, text: &str) -> Option<parse::Unit> {
        let text_digest = digest(text);
        if let Some(memory) = &self.memory
            && let Some((held, unit)) = memory.borrow().get(path)
            && *held == text_digest
        {
            self.kept.borrow_mut().insert(path.to_path_buf());
            return Some(unit.clone());
        }
        let bytes = fs::read(self.path(path)).ok()?;
        let mut reader = Reader { bytes: &bytes };
        if reader.take(MAGIC.len())? != MAGIC || reader.text()? != self.stamp {
            self.stale.set(self.stale.get() + 1);
            return None;
        }
        if reader.take(32)? != text_digest.as_slice() {
            return None;
        }
        let unit = decode(&mut reader, text)?;
        self.kept.borrow_mut().insert(path.to_path_buf());
        self.hold(path, text_digest, &unit);
        Some(unit)
    }

    /// Count one source parsed by a load through these cells, and remember
    /// its parse when these cells remember.
    pub(crate) fn parsed(&self, path: &Path, text: &str, unit: &parse::Unit) {
        self.parsed.set(self.parsed.get() + 1);
        self.hold(path, digest(text), unit);
    }

    fn hold(&self, path: &Path, text_digest: [u8; 32], unit: &parse::Unit) {
        if let Some(memory) = &self.memory {
            memory
                .borrow_mut()
                .insert(path.to_path_buf(), (text_digest, unit.clone()));
        }
    }

    fn path(&self, source: &Path) -> PathBuf {
        let key = Sha256::digest(source.as_os_str().as_encoded_bytes());
        let mut name = String::with_capacity(40);
        for byte in &key[..16] {
            name.push_str(&format!("{byte:02x}"));
        }
        name.push_str(".cell");
        self.directory.join(name)
    }

    fn encode(&self, text: &str, unit: &parse::Unit) -> Option<Vec<u8>> {
        let (items, statements, script_source) = unit.parts();
        if statements.is_some() {
            return None;
        }
        let mut writer = Writer(Vec::with_capacity(text.len() * 2));
        writer.0.extend_from_slice(MAGIC);
        writer.text(&self.stamp);
        writer.0.extend_from_slice(&digest(text));
        writer.text(script_source);
        put_all(items, &mut writer);
        Some(writer.0)
    }
}

fn digest(text: &str) -> [u8; 32] {
    Sha256::digest(text.as_bytes()).into()
}

fn decode(reader: &mut Reader, text: &str) -> Option<parse::Unit> {
    let script_source = reader.text()?.to_owned();
    let items = Box::<[item::Parsed]>::get(reader)?;
    reader
        .bytes
        .is_empty()
        .then(|| parse::Unit::new(source::Map::new(text), items, None, script_source))
}

struct Writer(Vec<u8>);

impl Writer {
    fn byte(&mut self, byte: u8) {
        self.0.push(byte);
    }

    /// LEB128.
    fn number(&mut self, mut value: u64) {
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                self.0.push(byte);
                return;
            }
            self.0.push(byte | 0x80);
        }
    }

    fn text(&mut self, text: &str) {
        self.number(text.len() as u64);
        self.0.extend_from_slice(text.as_bytes());
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let (taken, rest) = self.bytes.split_at_checked(count)?;
        self.bytes = rest;
        Some(taken)
    }

    fn byte(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn number(&mut self) -> Option<u64> {
        let mut value = 0_u64;
        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }

    /// A length that cannot be more than the bytes left.
    fn count(&mut self) -> Option<usize> {
        let count = usize::try_from(self.number()?).ok()?;
        (count <= self.bytes.len()).then_some(count)
    }

    fn text(&mut self) -> Option<&'a str> {
        let length = self.count()?;
        std::str::from_utf8(self.take(length)?).ok()
    }
}

/// One encoded value of a parse.
trait Codec: Sized {
    fn put(&self, writer: &mut Writer);
    fn get(reader: &mut Reader) -> Option<Self>;
}

impl Codec for bool {
    fn put(&self, writer: &mut Writer) {
        writer.byte(u8::from(*self));
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        match reader.byte()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
}

impl Codec for u32 {
    fn put(&self, writer: &mut Writer) {
        writer.number(u64::from(*self));
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        Self::try_from(reader.number()?).ok()
    }
}

impl Codec for crate::INT {
    fn put(&self, writer: &mut Writer) {
        writer.number(*self as u64);
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        Some(reader.number()? as Self)
    }
}

impl Codec for usize {
    fn put(&self, writer: &mut Writer) {
        writer.number(*self as u64);
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        Self::try_from(reader.number()?).ok()
    }
}

impl Codec for ImmutableString {
    fn put(&self, writer: &mut Writer) {
        writer.text(self);
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        reader.text().map(Into::into)
    }
}

impl Codec for Position {
    fn put(&self, writer: &mut Writer) {
        writer.number(self.line().unwrap_or(0) as u64);
        writer.number(self.position().unwrap_or(0) as u64);
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        let line = u16::try_from(reader.number()?).ok()?;
        let column = u16::try_from(reader.number()?).ok()?;
        Some(if line == 0 {
            Self::NONE
        } else {
            Self::new(line, column)
        })
    }
}

impl Codec for Span {
    fn put(&self, writer: &mut Writer) {
        self.start().put(writer);
        self.end().put(writer);
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        Some(Self::new(Position::get(reader)?, Position::get(reader)?))
    }
}

impl<T: Codec> Codec for Option<T> {
    fn put(&self, writer: &mut Writer) {
        match self {
            None => writer.byte(0),
            Some(value) => {
                writer.byte(1);
                value.put(writer);
            }
        }
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        match reader.byte()? {
            0 => Some(None),
            1 => T::get(reader).map(Some),
            _ => None,
        }
    }
}

impl<T: Codec> Codec for Box<T> {
    fn put(&self, writer: &mut Writer) {
        (**self).put(writer);
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        T::get(reader).map(Self::new)
    }
}

impl<T: Codec> Codec for Vec<T> {
    fn put(&self, writer: &mut Writer) {
        put_all(self, writer);
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        // Each value takes a byte at least, so `count` bounds the length.
        let count = reader.count()?;
        let mut values = Self::with_capacity(count);
        for _ in 0..count {
            values.push(T::get(reader)?);
        }
        Some(values)
    }
}

fn put_all<T: Codec>(values: &[T], writer: &mut Writer) {
    writer.number(values.len() as u64);
    for value in values {
        value.put(writer);
    }
}

impl<T: Codec> Codec for Box<[T]> {
    fn put(&self, writer: &mut Writer) {
        put_all(self, writer);
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        Vec::get(reader).map(Vec::into_boxed_slice)
    }
}

impl<A: Codec, B: Codec> Codec for (A, B) {
    fn put(&self, writer: &mut Writer) {
        self.0.put(writer);
        self.1.put(writer);
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        Some((A::get(reader)?, B::get(reader)?))
    }
}

impl<A: Codec, B: Codec, C: Codec> Codec for (A, B, C) {
    fn put(&self, writer: &mut Writer) {
        self.0.put(writer);
        self.1.put(writer);
        self.2.put(writer);
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        Some((A::get(reader)?, B::get(reader)?, C::get(reader)?))
    }
}

/// A struct encoded as its fields in order.
macro_rules! record {
    ($($type:path { $($field:ident),* $(,)? })*) => {$(
        impl Codec for $type {
            fn put(&self, writer: &mut Writer) {
                $(self.$field.put(writer);)*
            }
            fn get(reader: &mut Reader) -> Option<Self> {
                Some(Self { $($field: Codec::get(reader)?),* })
            }
        }
    )*};
}

record! {
    item::Name { text, span }
    item::Header { attributes, visibility, span }
    item::Attribute { path, arguments, span, preview }
    item::Module { header, name, span, inline }
    item::Use { header, tree, span }
    item::UseMember { name, alias }
    item::GenericParam { name, bounds, projections, default }
    item::Struct { header, name, generics, lifetimes, fields, form, span }
    item::Field { name, ty, span }
    item::Enum { header, name, generics, lifetimes, variants, span }
    item::Variant { name, payload, fields, default, discriminant, span }
    item::Impl { header, generics, lifetimes, trait_, owner, functions, associated, span }
    item::Trait {
        header, name, functions, provided, associated, associated_bounds,
        supertraits, generics, span,
    }
    item::Function {
        header, name, generics, lifetimes, receiver_lifetime, receiver, parameters, output,
        body_span, body_syntax, constant, asynchronous, span,
    }
    item::Lifetimes { params, bounds }
    item::TypeAlias { header, name, generics, target, span }
    item::Parameter { name, ty, span, pattern }
    type_ref::Path { segments, span, global }
    type_ref::Segment { name, arguments, span }
    type_ref::Lifetime { name, span }
    preview::Syntax { name, group, seed, update, themes, snapshot, size, bind, bound, span }
    preview::Bind { span, tokens }
    preview::Text { value, span }
    preview::Flag { value, span }
    preview::SizeSyntax { width, height, span }
}

impl Codec for item::Parsed {
    fn put(&self, writer: &mut Writer) {
        match self {
            Self::Module(item) => (writer.byte(0), item.put(writer)),
            Self::Use(item) => (writer.byte(1), item.put(writer)),
            Self::Struct(item) => (writer.byte(2), item.put(writer)),
            Self::Enum(item) => (writer.byte(3), item.put(writer)),
            Self::Impl(item) => (writer.byte(4), item.put(writer)),
            Self::Trait(item) => (writer.byte(5), item.put(writer)),
            Self::Function(item) => (writer.byte(6), item.put(writer)),
            Self::TypeAlias(item) => (writer.byte(8), item.put(writer)),
        };
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        Some(match reader.byte()? {
            0 => Self::Module(Codec::get(reader)?),
            1 => Self::Use(Codec::get(reader)?),
            2 => Self::Struct(Codec::get(reader)?),
            3 => Self::Enum(Codec::get(reader)?),
            4 => Self::Impl(Codec::get(reader)?),
            5 => Self::Trait(Codec::get(reader)?),
            6 => Self::Function(Codec::get(reader)?),
            8 => Self::TypeAlias(Codec::get(reader)?),
            _ => return None,
        })
    }
}

impl Codec for item::UseTree {
    fn put(&self, writer: &mut Writer) {
        match self {
            Self::Path(path, alias) => {
                writer.byte(0);
                path.put(writer);
                alias.put(writer);
            }
            Self::Group {
                prefix,
                members,
                span,
            } => {
                writer.byte(1);
                prefix.put(writer);
                members.put(writer);
                span.put(writer);
            }
            Self::Glob { prefix, span } => {
                writer.byte(2);
                prefix.put(writer);
                span.put(writer);
            }
        }
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        Some(match reader.byte()? {
            0 => Self::Path(Codec::get(reader)?, Codec::get(reader)?),
            1 => Self::Group {
                prefix: Codec::get(reader)?,
                members: Codec::get(reader)?,
                span: Codec::get(reader)?,
            },
            2 => Self::Glob {
                prefix: Codec::get(reader)?,
                span: Codec::get(reader)?,
            },
            _ => return None,
        })
    }
}

impl Codec for item::StructForm {
    fn put(&self, writer: &mut Writer) {
        writer.byte(match self {
            Self::Named => 0,
            Self::Tuple => 1,
            Self::Unit => 2,
        });
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        match reader.byte()? {
            0 => Some(Self::Named),
            1 => Some(Self::Tuple),
            2 => Some(Self::Unit),
            _ => None,
        }
    }
}

impl Codec for item::Receiver {
    fn put(&self, writer: &mut Writer) {
        writer.byte(match self {
            Self::Shared(_) => 0,
            Self::Mutable(_) => 1,
            Self::Owned(_) => 2,
            Self::OwnedMutable(_) => 3,
        });
        self.span().put(writer);
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        let tag = reader.byte()?;
        let span = Span::get(reader)?;
        match tag {
            0 => Some(Self::Shared(span)),
            1 => Some(Self::Mutable(span)),
            2 => Some(Self::Owned(span)),
            3 => Some(Self::OwnedMutable(span)),
            _ => None,
        }
    }
}

impl Codec for item::ParameterPattern {
    fn put(&self, writer: &mut Writer) {
        match self {
            Self::Binding { name, mutable } => {
                writer.byte(0);
                name.put(writer);
                mutable.put(writer);
            }
            Self::Discard(span) => {
                writer.byte(1);
                span.put(writer);
            }
            Self::Tuple { members, span } => {
                writer.byte(2);
                members.put(writer);
                span.put(writer);
            }
            Self::Reference {
                mutable,
                pattern,
                span,
            } => {
                writer.byte(3);
                mutable.put(writer);
                pattern.put(writer);
                span.put(writer);
            }
            Self::TupleStruct {
                path,
                members,
                span,
            } => {
                writer.byte(4);
                path.put(writer);
                members.put(writer);
                span.put(writer);
            }
        }
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        Some(match reader.byte()? {
            0 => Self::Binding {
                name: Codec::get(reader)?,
                mutable: Codec::get(reader)?,
            },
            1 => Self::Discard(Codec::get(reader)?),
            2 => Self::Tuple {
                members: Codec::get(reader)?,
                span: Codec::get(reader)?,
            },
            3 => Self::Reference {
                mutable: Codec::get(reader)?,
                pattern: Codec::get(reader)?,
                span: Codec::get(reader)?,
            },
            4 => Self::TupleStruct {
                path: Codec::get(reader)?,
                members: Codec::get(reader)?,
                span: Codec::get(reader)?,
            },
            _ => return None,
        })
    }
}

impl Codec for item::BodySyntax {
    fn put(&self, writer: &mut Writer) {
        put_all(self.tokens(), writer);
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        Vec::get(reader).map(Self::new)
    }
}

impl Codec for item::BodyToken {
    fn put(&self, writer: &mut Writer) {
        use item::BodyTokenKind as Kind;
        let tag = match self.kind() {
            Kind::Identifier(name) => {
                writer.byte(0);
                name.put(writer);
                None
            }
            Kind::Integer(value) => {
                writer.byte(1);
                writer.0.extend_from_slice(&value.to_le_bytes());
                None
            }
            Kind::DoubleColon => Some(2),
            Kind::DoubleArrow => Some(3),
            Kind::Match => Some(4),
            Kind::LeftParen => Some(5),
            Kind::RightParen => Some(6),
            Kind::Unit => Some(7),
            Kind::Comma => Some(8),
            Kind::Dot => Some(9),
            Kind::LeftBrace => Some(10),
            Kind::RightBrace => Some(11),
            Kind::LeftBracket => Some(12),
            Kind::RightBracket => Some(13),
            Kind::Semicolon => Some(14),
            Kind::If => Some(15),
            Kind::OrOr => Some(16),
            Kind::Other => Some(17),
            Kind::Bang => Some(18),
        };
        if let Some(tag) = tag {
            writer.byte(tag);
        }
        self.span().put(writer);
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        use item::BodyTokenKind as Kind;
        let kind = match reader.byte()? {
            0 => Kind::Identifier(Codec::get(reader)?),
            1 => {
                let bytes = reader.take(core::mem::size_of::<crate::INT>())?;
                Kind::Integer(crate::INT::from_le_bytes(bytes.try_into().ok()?))
            }
            2 => Kind::DoubleColon,
            3 => Kind::DoubleArrow,
            4 => Kind::Match,
            5 => Kind::LeftParen,
            6 => Kind::RightParen,
            7 => Kind::Unit,
            8 => Kind::Comma,
            9 => Kind::Dot,
            10 => Kind::LeftBrace,
            11 => Kind::RightBrace,
            12 => Kind::LeftBracket,
            13 => Kind::RightBracket,
            14 => Kind::Semicolon,
            15 => Kind::If,
            16 => Kind::OrOr,
            17 => Kind::Other,
            18 => Kind::Bang,
            _ => return None,
        };
        Some(Self::new(kind, Span::get(reader)?))
    }
}

impl Codec for type_ref::Type {
    fn put(&self, writer: &mut Writer) {
        match self {
            Self::Unit(span) => {
                writer.byte(0);
                span.put(writer);
            }
            Self::Tuple { members, span } => {
                writer.byte(1);
                members.put(writer);
                span.put(writer);
            }
            Self::Path(path) => {
                writer.byte(2);
                path.put(writer);
            }
            Self::Reference {
                mutable,
                lifetime,
                target,
                span,
            } => {
                writer.byte(3);
                mutable.put(writer);
                lifetime.put(writer);
                target.put(writer);
                span.put(writer);
            }
        }
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        Some(match reader.byte()? {
            0 => Self::Unit(Codec::get(reader)?),
            1 => Self::Tuple {
                members: Codec::get(reader)?,
                span: Codec::get(reader)?,
            },
            2 => Self::Path(Codec::get(reader)?),
            3 => Self::Reference {
                mutable: Codec::get(reader)?,
                lifetime: Codec::get(reader)?,
                target: Codec::get(reader)?,
                span: Codec::get(reader)?,
            },
            _ => return None,
        })
    }
}

impl Codec for type_ref::Argument {
    fn put(&self, writer: &mut Writer) {
        match self {
            Self::Type(ty) => {
                writer.byte(0);
                ty.put(writer);
            }
            Self::Lifetime(lifetime) => {
                writer.byte(1);
                lifetime.put(writer);
            }
        }
    }
    fn get(reader: &mut Reader) -> Option<Self> {
        Some(match reader.byte()? {
            0 => Self::Type(Codec::get(reader)?),
            1 => Self::Lifetime(Codec::get(reader)?),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Cells;

    #[test]
    fn a_program_loaded_through_its_cells_is_the_parsed_program() {
        let root = std::env::temp_dir().join(format!("rawr-parse-cells-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        let main = include_str!("../../iced-cli/assets/counter/src/main.rs");
        let main = main.strip_prefix("// rawr\n").unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"cells\"\n").unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            format!("// rawr\nmod view;\n{main}"),
        )
        .unwrap();
        std::fs::write(
            root.join("src/view.rs"),
            "// rawr\n#[preview(name = \"Wide\", size = (400, 200))]\npub fn wide(n: &(i64, Option<String>)) -> i64 { let x = 3; 0 }\n",
        )
        .unwrap();
        let target = crate::project::Target::discover(&root).unwrap();
        let engine = crate::Engine::new();
        let load = |cells: &Cells| crate::package::Graph::load_with_cells(&engine, &target, cells);

        let first = Cells::new(root.join("cells"));
        let parsed = load(&first).unwrap();
        assert_eq!(first.counts().parsed, 2);
        assert_eq!(first.save(&parsed).unwrap(), 2);

        let second = Cells::new(root.join("cells"));
        let kept = load(&second).unwrap();
        assert_eq!(
            second.counts(),
            super::Counts {
                kept: 2,
                parsed: 0,
                stale: 0
            }
        );
        for ((left, a), (right, b)) in parsed.parsed_units().zip(kept.parsed_units()) {
            assert_eq!(left.path(), right.path());
            assert_eq!(a.items(), b.items());
            assert_eq!(a.parts().2, b.parts().2);
            assert_eq!(format!("{:?}", a.parts().1), format!("{:?}", b.parts().1));
        }
        // A program loaded from its cells compiles as the parsed one does.
        std::fs::write(
            root.join("cart.rs"),
            include_str!("../../examples/scripts/cart.rs"),
        )
        .unwrap();
        let cart = crate::project::Target::discover(root.join("cart.rs")).unwrap();
        let cells = Cells::new(root.join("cart"));
        let parsed = crate::package::Graph::load_with_cells(&engine, &cart, &cells).unwrap();
        cells.save(&parsed).unwrap();
        let cells = Cells::new(root.join("cart"));
        let kept = crate::package::Graph::load_with_cells(&engine, &cart, &cells).unwrap();
        assert_eq!(cells.counts().parsed, 0);
        engine.compile_package(kept).unwrap();

        // An edit misses its cell; a cell of another build is stale.
        std::fs::write(
            root.join("src/view.rs"),
            "// rawr\npub fn wide() -> i64 { 1 }\n",
        )
        .unwrap();
        let mut other = Cells::new(root.join("cells"));
        other.stamp.push_str("/other");
        load(&other).unwrap();
        assert_eq!(
            other.counts(),
            super::Counts {
                kept: 0,
                parsed: 2,
                stale: 2
            }
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
