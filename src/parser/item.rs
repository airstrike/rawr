//! Parser for source-level Rawr item headers.

use super::ParseResult;
use crate::item::type_ref;
use crate::tokenizer::{Token, TokenStream, TokenizerControl};
use crate::{Engine, Position, Span, PERR};
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// The items of a file, and the spans of its inner attributes (`#![..]`),
/// which the legacy script does not see.
pub(super) type ParsedFile = (Box<[crate::item::Parsed]>, Vec<Span>);

#[cfg(not(feature = "no_std"))]
std::thread_local! {
    /// The `fn` items and `use` imports declared inside function bodies of
    /// the file being parsed. They join the file's items, as Rawr hoists them.
    static NESTED: core::cell::RefCell<Vec<crate::item::Parsed>> =
        const { core::cell::RefCell::new(Vec::new()) };
    /// The spans of the items a `#[cfg(..)]` left out of the file being
    /// parsed, which the legacy script does not see either.
    static SKIPPED: core::cell::RefCell<Vec<Span>> = const { core::cell::RefCell::new(Vec::new()) };
}

pub(super) fn parse(engine: &Engine, source: &str) -> ParseResult<ParsedFile> {
    #[cfg(not(feature = "no_std"))]
    let outer = NESTED.take();
    #[cfg(not(feature = "no_std"))]
    let outer_skipped = SKIPPED.take();
    let parsed = parse_items(engine, source);
    #[cfg(not(feature = "no_std"))]
    let nested = NESTED.replace(outer);
    #[cfg(not(feature = "no_std"))]
    let skipped = SKIPPED.replace(outer_skipped);
    let (mut items, mut inner) = parsed?;
    #[cfg(not(feature = "no_std"))]
    inner.extend(skipped);
    #[cfg(not(feature = "no_std"))]
    if !nested.is_empty() {
        let mut all = items.into_vec();
        for item in nested {
            // One import written in several bodies is hoisted once.
            // A glob binds no names here: its key is its module.
            let key = |used: &crate::item::Use| {
                let mut key = used
                    .bindings()
                    .into_iter()
                    .map(|(name, path)| (name.text, path))
                    .collect::<Vec<_>>();
                if let crate::item::UseTree::Glob { prefix, .. } = &used.tree {
                    let path = prefix
                        .segments
                        .iter()
                        .map(|segment| segment.name.text.clone())
                        .collect();
                    key.push(("*".into(), path));
                }
                key
            };
            if let crate::item::Parsed::Use(used) = &item
                && all.iter().any(|existing| {
                    matches!(existing, crate::item::Parsed::Use(other) if key(other) == key(used))
                })
            {
                continue;
            }
            all.push(item);
        }
        items = all.into_boxed_slice();
    }
    // `use rawr::preview;` names the attribute for rustc. Rawr knows the
    // attribute already, so the import binds nothing, and the legacy script
    // does not see it either.
    if items.iter().any(is_attribute_import) {
        let (dropped, kept): (Vec<_>, Vec<_>) =
            items.into_vec().into_iter().partition(is_attribute_import);
        inner.extend(dropped.iter().map(crate::item::Parsed::span));
        items = kept.into_boxed_slice();
    }
    Ok((items, inner))
}

/// The attributes of the `rawr` crate, which marked `.rs` files name so
/// that rustc accepts them: `#[preview]` and `#[widget]`.
const ATTRIBUTES: [&str; 2] = ["preview", "widget"];

/// Whether `item` is a `use` of the `rawr` crate's attributes and nothing
/// else: `use rawr::preview;` or `use rawr::{preview, widget};`.
fn is_attribute_import(item: &crate::item::Parsed) -> bool {
    let crate::item::Parsed::Use(used) = item else {
        return false;
    };
    let bindings = used.bindings();
    !bindings.is_empty()
        && bindings.iter().all(|(_, path)| {
            matches!(path.as_ref(), [krate, name]
                if krate.as_str() == "rawr" && ATTRIBUTES.contains(&name.as_str()))
        })
}

fn parse_items(engine: &Engine, source: &str) -> ParseResult<ParsedFile> {
    let inputs = [source];
    let (stream, tokenizer_control) = engine.lex(&inputs);
    let input = &mut stream.peekable();
    let mut items = Vec::new();
    let mut inner = Vec::new();
    let mut scan = ScanState::default();
    let mut at_top = true;

    loop {
        let next = input.peek().expect("token streams always end with EOF");
        if matches!(&next.0, Token::Reserved(text) if text.as_str() == "#!") {
            let position = next.1;
            if !at_top || scan.delimiter_depth != 0 {
                return Err(inner_attribute_error(
                    "an inner attribute (`#![..]`) is only permitted at the top of a file, \
                     before its items",
                )
                .into_err(position));
            }
            input.next();
            inner.push(parse_inner_attribute(input, position)?);
            continue;
        }
        at_top = false;
        match input.peek().expect("token streams always end with EOF") {
            (Token::EOF, _) => break,
            (Token::LexError(error), position) => {
                return Err(error.clone().into_err(*position));
            }
            (Token::LeftBrace | Token::LeftParen | Token::LeftBracket | Token::MapStart, _) => {
                scan.next(input, &tokenizer_control);
            }
            (Token::RightBrace | Token::RightParen | Token::RightBracket, _) => {
                scan.next(input, &tokenizer_control);
            }
            (token, _) if scan.delimiter_depth == 0 && is_attribute_marker(token) => {
                let (_, position) = scan
                    .next(input, &tokenizer_control)
                    .expect("peeked attribute marker must exist");

                if matches!(input.peek(), Some((Token::LeftBracket, _))) {
                    if let Some(item) =
                        parse_item_after_attribute_marker(input, &tokenizer_control, position)?
                    {
                        bind_previews(item, &mut items)?;
                    }
                }
            }
            #[cfg(not(feature = "no_function"))]
            (Token::Fn, _) if scan.delimiter_depth == 0 => {
                if let Some(item) = parse_private_function(input, &tokenizer_control)? {
                    items.push(item);
                }
            }
            #[cfg(not(feature = "no_function"))]
            (Token::Const, _) if scan.delimiter_depth == 0 => {
                // `const NAME: T = value;` is an item; untyped `const` stays
                // a legacy script statement.
                let start = expect(input, Token::Const, "to start a constant")?;
                if input.peek().is_some_and(|(token, _)| is_name(token)) {
                    let name = parse_name(input, "after 'const'")?;
                    if matches!(input.peek(), Some((Token::Colon, _))) {
                        let header = crate::item::Header {
                            attributes: Box::default(),
                            visibility: None,
                            span: Span::NONE,
                        };
                        items.push(crate::item::Parsed::Function(parse_const_rest(
                            input,
                            &tokenizer_control,
                            header,
                            start,
                            name,
                        )?));
                    }
                }
            }
            (Token::Identifier(text), position)
                if scan.delimiter_depth == 0 && text.as_str() == "unsafe" =>
            {
                return Err(PERR::BadInput(crate::LexError::ImproperSymbol(
                    "unsafe".into(),
                    "`unsafe` items are not supported in Rawr yet".into(),
                ))
                .into_err(*position));
            }
            (Token::Reserved(text), position)
                if scan.delimiter_depth == 0 && text.as_str() == "async" =>
            {
                return Err(async_unsupported(*position));
            }
            (Token::Reserved(text), _) if scan.delimiter_depth == 0 && text.as_str() == "static" => {
                let start = input.peek().expect("peeked static").1;
                items.push(parse_static(input, &tokenizer_control, Box::default(), None, start)?);
            }
            (token, _) if scan.delimiter_depth == 0 && begins_item(token) => {
                items.extend(parse_item(input, &tokenizer_control)?);
            }
            (Token::Identifier(name), _)
                if scan.delimiter_depth == 0 && matches!(name.as_str(), "trait" | "type") =>
            {
                items.extend(parse_item(input, &tokenizer_control)?);
            }
            _ => {
                scan.next(input, &tokenizer_control);
            }
        }
    }

    resolve_const_lengths(&mut items)?;
    Ok((items.into_boxed_slice(), inner))
}

/// `[T; N]` naming a crate `const N: usize = 3;` reads as `[T; 3]`, as rustc
/// evaluates the constant; a length naming anything else is an error.
fn resolve_const_lengths(items: &mut [crate::item::Parsed]) -> ParseResult<()> {
    use crate::item::Parsed;
    let value = |function: &crate::item::Function| {
        usize::try_from(function.body_syntax.integer().filter(|_| function.constant)?).ok()
    };
    // Crate constants by name, and associated ones as `Type::NAME`.
    let mut lengths = std::collections::BTreeMap::<crate::ImmutableString, usize>::new();
    for item in items.iter() {
        match item {
            Parsed::Function(function) => {
                if let Some(length) = value(function) {
                    lengths.insert(function.name.text.clone(), length);
                }
            }
            Parsed::Impl(item) => {
                let type_ref::Type::Path(owner) = &item.owner else {
                    continue;
                };
                let Some(last) = owner.segments.last() else {
                    continue;
                };
                for function in item.functions.iter() {
                    if let Some(length) = value(function) {
                        lengths.insert(
                            format!("{}::{}", last.name.text, function.name.text).into(),
                            length,
                        );
                    }
                }
            }
            _ => {}
        }
    }
    fn visit(
        ty: &mut type_ref::Type,
        lengths: &std::collections::BTreeMap<crate::ImmutableString, usize>,
    ) -> ParseResult<()> {
        match ty {
            type_ref::Type::Path(path) => {
                for segment in path.segments.iter_mut() {
                    if let Some(name) = type_ref::array_length_name(&segment.name.text) {
                        let Some(length) = lengths.get(name) else {
                            return Err(PERR::MissingSymbol(format!(
                                "an array length `{name}` that names a `const` of an integer \
                                 literal"
                            ))
                            .into_err(segment.span.start()));
                        };
                        segment.name.text = type_ref::array_name(*length).into();
                    }
                    for argument in segment.arguments.iter_mut() {
                        if let type_ref::Argument::Type(ty) = argument {
                            visit(ty, lengths)?;
                        }
                    }
                }
                Ok(())
            }
            type_ref::Type::Reference { target, .. } => visit(target, lengths),
            type_ref::Type::Tuple { members, .. } => {
                members.iter_mut().try_for_each(|member| visit(member, lengths))
            }
            type_ref::Type::Unit(_) => Ok(()),
        }
    }
    let function = |function: &mut crate::item::Function| -> ParseResult<()> {
        for parameter in function.parameters.iter_mut() {
            visit(&mut parameter.ty, &lengths)?;
        }
        function
            .output
            .iter_mut()
            .try_for_each(|output| visit(output, &lengths))
    };
    for item in items.iter_mut() {
        match item {
            Parsed::Struct(item) => {
                for field in item.fields.iter_mut() {
                    visit(&mut field.ty, &lengths)?;
                }
            }
            Parsed::Enum(item) => {
                for variant in item.variants.iter_mut() {
                    for ty in variant.payload.iter_mut() {
                        visit(ty, &lengths)?;
                    }
                }
            }
            Parsed::Impl(item) => {
                // `Self::N` in an impl is its owner's `N`.
                let owner = match &item.owner {
                    type_ref::Type::Path(owner) => owner
                        .segments
                        .last()
                        .map(|last| last.name.text.to_string()),
                    _ => None,
                };
                let mut own = lengths.clone();
                if let Some(owner) = owner {
                    for (name, length) in &lengths {
                        if let Some(constant) = name.strip_prefix(&format!("{owner}::")) {
                            own.insert(format!("Self::{constant}").into(), *length);
                        }
                    }
                }
                visit(&mut item.owner, &own)?;
                for function_item in item.functions.iter_mut() {
                    for parameter in function_item.parameters.iter_mut() {
                        visit(&mut parameter.ty, &own)?;
                    }
                    if let Some(output) = function_item.output.as_mut() {
                        visit(output, &own)?;
                    }
                }
                for (_, ty) in item.associated.iter_mut() {
                    visit(ty, &own)?;
                }
            }
            Parsed::Trait(item) => {
                for function_item in item.functions.iter_mut() {
                    function(function_item)?;
                }
            }
            Parsed::Function(item) => function(item)?,
            Parsed::TypeAlias(item) => visit(&mut item.target, &lengths)?,
            Parsed::Module(_) | Parsed::Use(_) | Parsed::LegacyFunction(_) => {}
        }
    }
    Ok(())
}

/// The lint attributes rustc reads; Rawr implements none of these lints, so
/// it accepts and ignores them, as rustc does a lint it does not know.
const LINT_ATTRIBUTES: &[&str] = &["allow", "warn", "deny", "forbid", "expect"];

/// `#![lint(..)]` after its `#!`: accepted and ignored. Any other inner
/// attribute is an error naming it.
fn parse_inner_attribute(input: &mut TokenStream, hash: Position) -> ParseResult<Span> {
    let attribute = parse_attribute_after_marker(input, hash)?;
    let name = attribute
        .path
        .segments
        .iter()
        .map(|segment| segment.name.text.as_str())
        .collect::<Vec<_>>()
        .join("::");
    if !LINT_ATTRIBUTES.contains(&name.as_str()) {
        return Err(inner_attribute_error(&format!(
            "unsupported inner attribute `#![{name}]`: Rawr accepts only the lint attributes \
             `allow`, `warn`, `deny`, `forbid` and `expect` at the top of a file"
        ))
        .into_err(hash));
    }
    Ok(attribute.span)
}

fn inner_attribute_error(message: &str) -> PERR {
    PERR::BadInput(crate::LexError::ImproperSymbol("#!".into(), message.into()))
}

#[derive(Default)]
struct ScanState {
    delimiter_depth: usize,
    interpolation_depths: Vec<usize>,
    expects_interpolation_brace: bool,
}

impl ScanState {
    #[cfg_attr(feature = "no_function", allow(dead_code))]
    fn inside_body() -> Self {
        Self {
            delimiter_depth: 1,
            ..Self::default()
        }
    }

    fn next(
        &mut self,
        input: &mut TokenStream,
        tokenizer_control: &TokenizerControl,
    ) -> Option<(Token, Position)> {
        let (token, position) = input.next()?;

        match &token {
            Token::InterpolatedString(_) => self.expects_interpolation_brace = true,
            Token::LeftBrace => {
                self.delimiter_depth += 1;
                if self.expects_interpolation_brace {
                    self.interpolation_depths.push(self.delimiter_depth);
                    self.expects_interpolation_brace = false;
                }
            }
            Token::LeftParen | Token::LeftBracket | Token::MapStart => {
                self.delimiter_depth += 1;
            }
            Token::RightBrace => {
                if self.interpolation_depths.last() == Some(&self.delimiter_depth) {
                    self.interpolation_depths.pop();
                    tokenizer_control.borrow_mut().is_within_text = true;
                }
                self.delimiter_depth = self.delimiter_depth.saturating_sub(1);
            }
            Token::RightParen | Token::RightBracket => {
                self.delimiter_depth = self.delimiter_depth.saturating_sub(1);
            }
            _ => {}
        }

        Some((token, position))
    }
}

fn begins_item(token: &Token) -> bool {
    matches!(
        token,
        Token::Pub | Token::Struct | Token::Enum | Token::Impl | Token::Use | Token::Mod
    )
}

fn is_attribute_marker(token: &Token) -> bool {
    match token {
        Token::Hash => true,
        #[cfg(not(feature = "no_custom_syntax"))]
        Token::Custom(symbol) => symbol.as_str() == "#",
        _ => false,
    }
}

fn parse_item(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
) -> ParseResult<Option<crate::item::Parsed>> {
    let (attributes, visibility, start) = parse_leading(input)?;
    parse_item_with_leading(input, tokenizer_control, attributes, visibility, start)
}

fn parse_item_after_attribute_marker(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    hash: Position,
) -> ParseResult<Option<crate::item::Parsed>> {
    let mut attributes = vec![parse_attribute_after_marker(input, hash)?];

    while input
        .peek()
        .is_some_and(|(token, _)| is_attribute_marker(token))
    {
        attributes.push(parse_attribute(input)?);
    }

    let visibility = parse_item_visibility(input);

    parse_item_with_leading(
        input,
        tokenizer_control,
        attributes.into_boxed_slice(),
        visibility,
        hash,
    )
}

fn parse_item_with_leading(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    attributes: Box<[crate::item::Attribute]>,
    visibility: Option<Span>,
    start: Position,
) -> ParseResult<Option<crate::item::Parsed>> {
    if cfg_disabled(&attributes) {
        let end = skip_item(input)?;
        #[cfg(not(feature = "no_std"))]
        SKIPPED.with_borrow_mut(|skipped| skipped.push(Span::new(start, end)));
        return Ok(None);
    }
    parse_enabled_item(input, tokenizer_control, attributes, visibility, start).map(Some)
}

fn parse_enabled_item(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    attributes: Box<[crate::item::Attribute]>,
    visibility: Option<Span>,
    start: Position,
) -> ParseResult<crate::item::Parsed> {
    match input.peek().expect("token streams always end with EOF") {
        (Token::Mod, _) => {
            parse_module(input, attributes, visibility, start).map(crate::item::Parsed::Module)
        }
        (Token::Use, _) => {
            parse_use(input, attributes, visibility, start).map(crate::item::Parsed::Use)
        }
        (Token::Struct, _) => {
            parse_struct(input, attributes, visibility, start).map(crate::item::Parsed::Struct)
        }
        (Token::Enum, _) => {
            parse_enum(input, attributes, visibility, start).map(crate::item::Parsed::Enum)
        }
        (Token::Impl, _) => parse_impl(input, tokenizer_control, attributes, visibility, start)
            .map(crate::item::Parsed::Impl),
        (Token::Identifier(name), _) if name.as_str() == "trait" => {
            parse_trait(input, tokenizer_control, attributes, visibility, start)
                .map(crate::item::Parsed::Trait)
        }
        (Token::Identifier(name), _) if name.as_str() == "type" => {
            parse_type_alias(input, attributes, visibility, start)
                .map(crate::item::Parsed::TypeAlias)
        }
        #[cfg(not(feature = "no_function"))]
        (Token::Fn, _) => parse_function(
            input,
            tokenizer_control,
            attributes,
            visibility,
            start,
            false,
        )
        .map(crate::item::Parsed::Function),
        #[cfg(not(feature = "no_function"))]
        (Token::Const, _) => {
            input.next();
            let name = parse_name(input, "after 'const'")?;
            parse_const_rest(
                input,
                tokenizer_control,
                crate::item::Header {
                    attributes,
                    visibility,
                    span: Span::NONE,
                },
                start,
                name,
            )
            .map(crate::item::Parsed::Function)
        }
        (Token::Reserved(text), _) if text.as_str() == "static" => {
            parse_static(input, tokenizer_control, attributes, visibility, start)
        }
        (Token::Identifier(text), position) if text.as_str() == "unsafe" => {
            Err(PERR::BadInput(crate::LexError::ImproperSymbol(
                "unsafe".into(),
                "`unsafe` items are not supported in Rawr yet".into(),
            ))
            .into_err(*position))
        }
        (_, position) => Err(PERR::MissingToken(
            "item".into(),
            "after item attributes or visibility".into(),
        )
        .into_err(*position)),
    }
}

/// `type Name<T> = Type;`.
fn parse_type_alias(
    input: &mut TokenStream,
    attributes: Box<[crate::item::Attribute]>,
    visibility: Option<Span>,
    start: Position,
) -> ParseResult<crate::item::TypeAlias> {
    input.next();
    let name = parse_name(input, "after 'type'")?;
    let mut generics = parse_generic_params(input)?;
    parse_where_clause(input, &mut generics, None)?;
    expect(input, Token::Equals, "after a type alias name")?;
    let target = parse_type(input)?;
    let semicolon = expect(input, Token::SemiColon, "to terminate a type alias")?;
    let end = after(semicolon, 1);
    Ok(crate::item::TypeAlias {
        header: crate::item::Header {
            attributes,
            visibility,
            span: Span::new(start, end),
        },
        name,
        generics: generics.into_boxed_slice(),
        target,
        span: Span::new(start, end),
    })
}

/// `static NAME: T = value;`: an immutable static is read as a `const`,
/// whose value it always has. A `static mut` is not supported yet.
fn parse_static(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    attributes: Box<[crate::item::Attribute]>,
    visibility: Option<Span>,
    start: Position,
) -> ParseResult<crate::item::Parsed> {
    input.next();
    if let Some((Token::Mut, position)) = input.peek() {
        return Err(PERR::BadInput(crate::LexError::ImproperSymbol(
            "static mut".into(),
            "a `static mut` is not supported in Rawr yet; keep the value in your state".into(),
        ))
        .into_err(*position));
    }
    #[cfg(not(feature = "no_function"))]
    {
        let name = parse_name(input, "after 'static'")?;
        parse_const_rest(
            input,
            tokenizer_control,
            crate::item::Header {
                attributes,
                visibility,
                span: Span::NONE,
            },
            start,
            name,
        )
        .map(crate::item::Parsed::Function)
    }
    #[cfg(feature = "no_function")]
    {
        let _ = (tokenizer_control, attributes, visibility);
        Err(PERR::MissingToken("item".into(), "after 'static'".into()).into_err(start))
    }
}

fn parse_module(
    input: &mut TokenStream,
    attributes: Box<[crate::item::Attribute]>,
    visibility: Option<Span>,
    start: Position,
) -> ParseResult<crate::item::Module> {
    expect(input, Token::Mod, "to start a file module")?;
    let name = parse_name(input, "after 'mod'")?;
    // `mod name { .. }`: its items are read as a module of their own, so
    // its tokens are passed over here.
    let (end, inline) = if matches!(input.peek(), Some((Token::LeftBrace, _))) {
        let (_, open) = input.next().expect("peeked brace must exist");
        let mut depth = 1usize;
        let close = loop {
            match input.next().expect("token streams always end with EOF") {
                (Token::LeftBrace | Token::MapStart, _) => depth += 1,
                (Token::RightBrace, position) => {
                    depth -= 1;
                    if depth == 0 {
                        break position;
                    }
                }
                (Token::EOF, position) => {
                    return Err(PERR::MissingToken(
                        Token::RightBrace.into(),
                        "to close an inline module".into(),
                    )
                    .into_err(position));
                }
                _ => {}
            }
        };
        (after(close, 1), Some(Span::new(after(open, 1), close)))
    } else {
        let semicolon = expect(input, Token::SemiColon, "after a file module name")?;
        (after(semicolon, 1), None)
    };

    Ok(crate::item::Module {
        header: crate::item::Header {
            attributes,
            visibility,
            span: Span::new(start, end),
        },
        name,
        span: Span::new(start, end),
        inline,
    })
}

fn parse_leading(
    input: &mut TokenStream,
) -> ParseResult<(Box<[crate::item::Attribute]>, Option<Span>, Position)> {
    let start = input.peek().expect("token streams always end with EOF").1;
    let mut attributes = Vec::new();

    while input
        .peek()
        .is_some_and(|(token, _)| is_attribute_marker(token))
    {
        attributes.push(parse_attribute(input)?);
    }

    let visibility = parse_item_visibility(input);

    Ok((attributes.into_boxed_slice(), visibility, start))
}

/// An item's `pub`, `pub(crate)`, `pub(super)`, `pub(self)` or `pub(in path)`.
/// Rawr reads a restricted visibility as `pub`. Rust's own check of the
/// restriction is left to rustc.
fn parse_item_visibility(input: &mut TokenStream) -> Option<Span> {
    if !matches!(input.peek(), Some((Token::Pub, _))) {
        return None;
    }
    let (_, position) = input.next().expect("peeked visibility must exist");
    if matches!(input.peek(), Some((Token::LeftParen, _))) {
        input.next();
        while !matches!(
            input.next(),
            Some((Token::RightParen | Token::EOF, _)) | None
        ) {}
    }
    Some(token_span(position, "pub"))
}

/// The attribute path a `#[cfg(..)]` that does not hold is read as.
const CFG_DISABLED: &str = "cfg(false)";

/// Whether an item's attributes leave it out of the build: a `#[cfg(..)]`
/// that does not hold.
fn cfg_disabled(attributes: &[crate::item::Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path.segments.len() == 1 && attribute.path.segments[0].name.text == CFG_DISABLED
    })
}

/// One `cfg` predicate: `test` (false: Rawr builds as rustc does without
/// `--test`), `debug_assertions` (true), `not(..)`, `all(..)`, `any(..)`;
/// any other option, as `feature = "x"`, does not hold.
fn parse_cfg_predicate(input: &mut TokenStream) -> ParseResult<bool> {
    let name = parse_name(input, "in a `cfg` predicate")?;
    match name.text.as_str() {
        operator @ ("not" | "all" | "any") => {
            expect(input, Token::LeftParen, "after a `cfg` operator")?;
            let mut values = Vec::new();
            while !matches!(input.peek(), Some((Token::RightParen, _))) {
                values.push(parse_cfg_predicate(input)?);
                if matches!(input.peek(), Some((Token::Comma, _))) {
                    input.next();
                }
            }
            expect(input, Token::RightParen, "to close a `cfg` operator")?;
            Ok(match operator {
                "not" => !values.first().copied().unwrap_or(false),
                "all" => values.iter().all(|value| *value),
                _ => values.iter().any(|value| *value),
            })
        }
        name => {
            if matches!(input.peek(), Some((Token::Equals, _))) {
                input.next();
                input.next();
                return Ok(false);
            }
            Ok(name == "debug_assertions")
        }
    }
}

/// Pass over one item's tokens: up to a `;` or the `}` closing its body.
/// Returns where the item ends.
fn skip_item(input: &mut TokenStream) -> ParseResult<Position> {
    let mut depth = 0usize;
    loop {
        match input.next().expect("token streams always end with EOF") {
            (Token::LeftBrace | Token::LeftParen | Token::LeftBracket | Token::MapStart, _) => {
                depth += 1;
            }
            (Token::RightParen | Token::RightBracket, _) => depth = depth.saturating_sub(1),
            (Token::RightBrace, position) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if let Some((Token::SemiColon, position)) = input.peek() {
                        let end = after(*position, 1);
                        input.next();
                        return Ok(end);
                    }
                    return Ok(after(position, 1));
                }
            }
            (Token::SemiColon, position) if depth == 0 => return Ok(after(position, 1)),
            (Token::EOF, position) => {
                return Err(PERR::MissingToken(
                    Token::SemiColon.into(),
                    "to end an item".into(),
                )
                .into_err(position));
            }
            _ => {}
        }
    }
}

fn parse_attribute(input: &mut TokenStream) -> ParseResult<crate::item::Attribute> {
    let marker = match input.next().expect("token streams always end with EOF") {
        (token, position) if is_attribute_marker(&token) => position,
        (Token::LexError(error), position) => return Err(error.into_err(position)),
        (_, position) => {
            return Err(
                PERR::MissingToken("#".into(), "to start an attribute".into()).into_err(position),
            );
        }
    };
    parse_attribute_after_marker(input, marker)
}

fn parse_attribute_after_marker(
    input: &mut TokenStream,
    hash: Position,
) -> ParseResult<crate::item::Attribute> {
    expect(input, Token::LeftBracket, "after '#' in an attribute")?;
    let path = parse_path(input)?;
    let is_preview = crate::item::is_attribute_path(&path, "preview");

    if is_preview {
        let mut preview = if matches!(input.peek(), Some((Token::LeftParen, _))) {
            parse_preview_arguments(input, hash)?
        } else {
            crate::preview::Syntax::bare(Span::NONE)
        };
        let close = expect(input, Token::RightBracket, "to close an attribute")?;
        let span = Span::new(hash, after(close, 1));
        preview.span = span;
        return Ok(crate::item::Attribute {
            path,
            arguments: Box::default(),
            span,
            preview: Some(preview),
        });
    }

    let mut arguments = Vec::new();

    // `#[cfg(predicate)]`: Rawr builds as rustc does without `--test`.
    if path.segments.len() == 1 && path.segments[0].name.text == "cfg" {
        expect(input, Token::LeftParen, "after `cfg`")?;
        let enabled = parse_cfg_predicate(input)?;
        expect(input, Token::RightParen, "to close `cfg`")?;
        let close = expect(input, Token::RightBracket, "to close an attribute")?;
        let mut path = path;
        if !enabled {
            // A disabled item is passed over; its attribute says so.
            path.segments[0].name.text = CFG_DISABLED.into();
        }
        return Ok(crate::item::Attribute {
            path,
            arguments: Box::default(),
            span: Span::new(hash, after(close, 1)),
            preview: None,
        });
    }

    if matches!(input.peek(), Some((Token::LeftParen, _))) {
        input.next();

        while !matches!(input.peek(), Some((Token::RightParen, _))) {
            arguments.push(parse_path(input)?);

            match input.peek().expect("token streams always end with EOF") {
                (Token::Comma, _) => {
                    input.next();
                    if matches!(input.peek(), Some((Token::RightParen, _))) {
                        break;
                    }
                }
                (Token::RightParen, _) => break,
                (_, position) => {
                    return Err(PERR::MissingToken(
                        Token::Comma.into(),
                        "between attribute arguments".into(),
                    )
                    .into_err(*position));
                }
            }
        }

        expect(input, Token::RightParen, "to close attribute arguments")?;
    }

    let close = expect(input, Token::RightBracket, "to close an attribute")?;

    Ok(crate::item::Attribute {
        path,
        arguments: arguments.into_boxed_slice(),
        span: Span::new(hash, after(close, 1)),
        preview: None,
    })
}

fn parse_preview_arguments(
    input: &mut TokenStream,
    hash: Position,
) -> ParseResult<crate::preview::Syntax> {
    expect(input, Token::LeftParen, "after 'preview'")?;
    let mut preview = crate::preview::Syntax::bare(Span::NONE);
    let mut seen = std::collections::BTreeSet::new();

    while !matches!(input.peek(), Some((Token::RightParen, _))) {
        let key = parse_name(input, "as a preview option")?;
        // `theme = path` is `themes = [path]`; one or the other.
        let option = if key.text == "theme" {
            "themes".into()
        } else {
            key.text.clone()
        };
        if !seen.insert(option) {
            return Err(
                PERR::MissingSymbol(format!("Duplicate preview option '{}'", key.text))
                    .into_err(key.span.start()),
            );
        }
        expect(input, Token::Equals, "after a preview option")?;

        match key.text.as_str() {
            "name" => preview.name = Some(parse_preview_text(input, "preview name")?),
            "group" => preview.group = Some(parse_preview_text(input, "preview group")?),
            "seed" => preview.seed = Some(parse_path(input)?),
            "update" => preview.update = Some(parse_path(input)?),
            "themes" => preview.themes = parse_preview_themes(input)?,
            "theme" => preview.themes = Box::new([parse_path(input)?]),
            "snapshot" => preview.snapshot = Some(parse_preview_flag(input)?),
            "size" => preview.size = Some(parse_preview_size(input)?),
            "bind" => preview.bind = Some(parse_preview_bind(input)?),
            _ => {
                return Err(PERR::MissingSymbol(format!(
                    "Unknown preview option '{}'; expected name, group, seed, update, theme, themes, snapshot, size, or bind",
                    key.text
                ))
                .into_err(key.span.start()));
            }
        }

        match input.peek().expect("token streams always end with EOF") {
            (Token::Comma, _) => {
                input.next();
                if matches!(input.peek(), Some((Token::RightParen, _))) {
                    break;
                }
            }
            (Token::RightParen, _) => break,
            (_, position) => {
                return Err(PERR::MissingToken(
                    Token::Comma.into(),
                    "between preview options".into(),
                )
                .into_err(*position));
            }
        }
    }

    let close = expect(input, Token::RightParen, "to close preview options")?;
    preview.span = Span::new(hash, after(close, 1));
    Ok(preview)
}

/// `bind = (a, b)`: the balanced, parenthesized argument tokens.
fn parse_preview_bind(input: &mut TokenStream) -> ParseResult<crate::preview::Bind> {
    let open = expect(input, Token::LeftParen, "to start preview `bind` arguments")?;
    let mut tokens = vec![body_token(Token::LeftParen, open)];
    let mut depth = 1_usize;
    let close = loop {
        let (token, position) = input.next().expect("token streams always end with EOF");
        match &token {
            Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                depth += 1;
            }
            Token::RightParen | Token::RightBracket | Token::RightBrace => depth -= 1,
            Token::EOF => {
                return Err(PERR::MissingToken(
                    ")".into(),
                    "to close preview `bind` arguments".into(),
                )
                .into_err(position));
            }
            Token::LexError(error) => return Err(error.clone().into_err(position)),
            _ => {}
        }
        tokens.push(body_token(token, position));
        if depth == 0 {
            break position;
        }
    };
    Ok(crate::preview::Bind {
        span: Span::new(open, after(close, 1)),
        tokens,
    })
}

/// Push a parsed file-level item, and for each `#[preview(name = .., bind =
/// (a, b))]` on a free function a private constant of its parameters' type
/// (their tuple, past one) holding the arguments, as Granita's `bind` does.
/// That constant is the preview's `seed`, and the preview joins the group
/// named after the function.
fn bind_previews(
    item: crate::item::Parsed,
    items: &mut Vec<crate::item::Parsed>,
) -> ParseResult<()> {
    #[cfg(not(feature = "no_function"))]
    if let crate::item::Parsed::Function(mut function) = item {
        let mut constants = Vec::new();
        for (index, attribute) in function.header.attributes.iter_mut().enumerate() {
            let Some(preview) = attribute.preview.as_mut() else {
                continue;
            };
            let Some(bind) = preview.bind.take() else {
                continue;
            };
            let refuse = |message: &str| {
                Err(PERR::MissingSymbol(message.to_owned()).into_err(bind.span.start()))
            };
            if preview.seed.is_some() {
                return refuse("a preview declares `bind` or `seed`, not both");
            }
            if preview.name.is_none() {
                return refuse("a `bind` preview needs a `name`");
            }
            let ty = match function.parameters.as_ref() {
                [] => return refuse("`bind` takes the arguments of a function with parameters"),
                [parameter] => parameter.ty.clone(),
                parameters => type_ref::Type::Tuple {
                    members: parameters
                        .iter()
                        .map(|parameter| parameter.ty.clone())
                        .collect(),
                    span: bind.span,
                },
            };
            let name = crate::item::Name {
                text: format!("__rawr_bind_{}_{index}", function.name.text).into(),
                span: bind.span,
            };
            preview.group.get_or_insert_with(|| crate::preview::Text {
                value: function.name.text.clone(),
                span: function.name.span,
            });
            preview.seed = Some(type_ref::Path {
                segments: Box::new([type_ref::Segment {
                    name: name.clone(),
                    arguments: Box::default(),
                    span: bind.span,
                }]),
                span: bind.span,
            });
            preview.bound = true;
            constants.push(crate::item::Parsed::Function(crate::item::Function {
                header: crate::item::Header {
                    attributes: Box::default(),
                    visibility: None,
                    span: bind.span,
                },
                name,
                generics: Box::default(),
                receiver: None,
                parameters: Box::default(),
                output: Some(ty),
                body_span: bind.span,
                body_syntax: crate::item::BodySyntax::new(bind.tokens),
                constant: true,
                span: bind.span,
            }));
        }
        items.push(crate::item::Parsed::Function(function));
        items.extend(constants);
        return Ok(());
    }
    items.push(item);
    Ok(())
}

fn parse_preview_text(
    input: &mut TokenStream,
    description: &'static str,
) -> ParseResult<crate::preview::Text> {
    match input.next().expect("token streams always end with EOF") {
        (Token::StringConstant(value), position) => {
            let end = after(position, value.chars().count() + 2);
            Ok(crate::preview::Text {
                value: value.as_str().into(),
                span: Span::new(position, end),
            })
        }
        (Token::LexError(error), position) => Err(error.into_err(position)),
        (_, position) => {
            Err(PERR::MissingToken("string literal".into(), description.into()).into_err(position))
        }
    }
}

fn parse_preview_flag(input: &mut TokenStream) -> ParseResult<crate::preview::Flag> {
    match input.next().expect("token streams always end with EOF") {
        (Token::True, position) => Ok(crate::preview::Flag {
            value: true,
            span: token_span(position, "true"),
        }),
        (Token::False, position) => Ok(crate::preview::Flag {
            value: false,
            span: token_span(position, "false"),
        }),
        (Token::LexError(error), position) => Err(error.into_err(position)),
        (_, position) => Err(PERR::MissingToken(
            "boolean literal".into(),
            "for preview snapshot inclusion".into(),
        )
        .into_err(position)),
    }
}

fn parse_preview_themes(
    input: &mut TokenStream,
) -> ParseResult<Box<[crate::item::type_ref::Path]>> {
    expect(input, Token::LeftBracket, "to start preview theme variants")?;
    let mut themes = Vec::new();
    while !matches!(input.peek(), Some((Token::RightBracket, _))) {
        themes.push(parse_path(input)?);
        match input.peek().expect("token streams always end with EOF") {
            (Token::Comma, _) => {
                input.next();
                if matches!(input.peek(), Some((Token::RightBracket, _))) {
                    break;
                }
            }
            (Token::RightBracket, _) => break,
            (_, position) => {
                return Err(PERR::MissingToken(
                    Token::Comma.into(),
                    "between preview theme variants".into(),
                )
                .into_err(*position));
            }
        }
    }
    expect(
        input,
        Token::RightBracket,
        "to close preview theme variants",
    )?;
    Ok(themes.into_boxed_slice())
}

fn parse_preview_size(input: &mut TokenStream) -> ParseResult<crate::preview::SizeSyntax> {
    let open = expect(input, Token::LeftParen, "to start a preview logical size")?;
    let width = parse_preview_dimension(input, "preview width")?;
    expect(input, Token::Comma, "between preview width and height")?;
    let height = parse_preview_dimension(input, "preview height")?;
    let close = expect(input, Token::RightParen, "to close a preview logical size")?;
    Ok(crate::preview::SizeSyntax {
        width,
        height,
        span: Span::new(open, after(close, 1)),
    })
}

fn parse_preview_dimension(input: &mut TokenStream, description: &'static str) -> ParseResult<u32> {
    match input.next().expect("token streams always end with EOF") {
        (Token::IntegerConstant(value), position) => u32::try_from(value)
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                PERR::MissingSymbol(format!("{description} must be a positive u32 literal"))
                    .into_err(position)
            }),
        (Token::LexError(error), position) => Err(error.into_err(position)),
        (_, position) => Err(PERR::MissingToken(
            "positive integer literal".into(),
            description.into(),
        )
        .into_err(position)),
    }
}

fn parse_use(
    input: &mut TokenStream,
    attributes: Box<[crate::item::Attribute]>,
    visibility: Option<Span>,
    start: Position,
) -> ParseResult<crate::item::Use> {
    expect(input, Token::Use, "to start an import")?;
    let first = parse_use_segment(input, "as the first import path segment")?;
    let mut segments = vec![type_ref::Segment {
        span: first.span,
        name: first,
        arguments: Box::default(),
    }];
    let mut grouped = false;
    let mut glob = None;
    let mut extra_uses = Vec::new();

    while matches!(input.peek(), Some((Token::DoubleColon, _))) {
        input.next();

        if matches!(input.peek(), Some((Token::LeftBrace, _))) {
            grouped = true;
            break;
        }
        if let Some((Token::Multiply, position)) = input.peek() {
            glob = Some(token_span(*position, "*"));
            input.next();
            break;
        }

        let name = parse_use_segment(input, "after '::' in an import path")?;
        segments.push(type_ref::Segment {
            span: name.span,
            name,
            arguments: Box::default(),
        });
    }

    let path_span = Span::new(
        segments
            .first()
            .expect("import paths contain one segment")
            .span
            .start(),
        segments
            .last()
            .expect("import paths contain one segment")
            .span
            .end(),
    );
    let path = type_ref::Path {
        segments: segments.into_boxed_slice(),
        span: path_span,
    };

    let tree = if let Some(star) = glob {
        crate::item::UseTree::Glob {
            span: Span::new(path.span.start(), star.end()),
            prefix: path,
        }
    } else if grouped {
        // `use a::{b, c::{d, e}, f::*}`: the paths that go on inside the
        // group are imports of their own, hoisted beside this one.
        let open = expect(input, Token::LeftBrace, "to start an import group")?;
        let mut nested = Vec::new();
        let (members, close) = parse_use_members(input, &path.segments, &mut nested)?;
        let group = crate::item::UseTree::Group {
            prefix: path,
            members: members.into_boxed_slice(),
            span: Span::new(open, after(close, 1)),
        };
        let mut trees = nested.into_iter();
        let tree = if matches!(&group, crate::item::UseTree::Group { members, .. } if members.is_empty()) {
            trees.next().unwrap_or(group)
        } else {
            group
        };
        extra_uses.extend(trees);
        tree
    } else {
        crate::item::UseTree::Path(path, parse_use_alias(input)?)
    };

    let semicolon = expect(input, Token::SemiColon, "to terminate an import")?;
    let end = after(semicolon, 1);
    let header = crate::item::Header {
        attributes,
        visibility,
        span: Span::new(start, end),
    };
    #[cfg(not(feature = "no_std"))]
    for tree in extra_uses {
        let used = crate::item::Use {
            header: header.clone(),
            tree,
            span: Span::new(start, end),
        };
        NESTED.with_borrow_mut(|nested| nested.push(crate::item::Parsed::Use(used)));
    }
    #[cfg(feature = "no_std")]
    if !extra_uses.is_empty() {
        return Err(PERR::MissingToken(
            Token::Comma.into(),
            "between grouped imports".into(),
        )
        .into_err(start));
    }

    Ok(crate::item::Use {
        header,
        tree,
        span: Span::new(start, end),
    })
}

fn parse_struct(
    input: &mut TokenStream,
    attributes: Box<[crate::item::Attribute]>,
    visibility: Option<Span>,
    start: Position,
) -> ParseResult<crate::item::Struct> {
    expect(input, Token::Struct, "to start a struct")?;
    let name = parse_name(input, "after 'struct'")?;
    let mut generics = parse_generic_params(input)?;
    parse_where_clause(input, &mut generics, None)?;

    if matches!(input.peek(), Some((Token::SemiColon, _))) {
        let semicolon = expect(input, Token::SemiColon, "to terminate a unit struct")?;
        let end = after(semicolon, 1);
        return Ok(crate::item::Struct {
            header: crate::item::Header {
                attributes,
                visibility,
                span: Span::new(start, end),
            },
            name,
            generics: generics.into_boxed_slice(),
            fields: Box::default(),
            form: crate::item::StructForm::Unit,
            span: Span::new(start, end),
        });
    }

    // `struct W<T>(pub T, i64) where T: Clone;`: fields `0`, `1`, ...
    if matches!(input.peek(), Some((Token::LeftParen | Token::Unit, _))) {
        let mut fields = Vec::new();
        if matches!(input.next(), Some((Token::LeftParen, _))) {
            while !matches!(input.peek(), Some((Token::RightParen, _))) {
                let ty = match skip_field_visibility(input)? {
                    Some(open) => parse_parenthesized_type(input, open)?,
                    None => parse_type(input)?,
                };
                let span = ty.span();
                fields.push(crate::item::Field {
                    name: crate::item::Name {
                        text: fields.len().to_string().into(),
                        span,
                    },
                    ty,
                    span,
                });
                consume_comma_or_end(input, Token::RightParen, "between tuple struct fields")?;
            }
            expect(input, Token::RightParen, "to close a tuple struct")?;
        }
        parse_where_clause(input, &mut generics, None)?;
        let semicolon = expect(input, Token::SemiColon, "to terminate a tuple struct")?;
        let end = after(semicolon, 1);
        return Ok(crate::item::Struct {
            header: crate::item::Header {
                attributes,
                visibility,
                span: Span::new(start, end),
            },
            name,
            generics: generics.into_boxed_slice(),
            fields: fields.into_boxed_slice(),
            form: crate::item::StructForm::Tuple,
            span: Span::new(start, end),
        });
    }

    let open = expect(input, Token::LeftBrace, "after a struct name")?;
    let header_end = after(open, 1);
    let mut fields = Vec::new();

    while !matches!(input.peek(), Some((Token::RightBrace, _))) {
        if let Some(open) = skip_field_visibility(input)? {
            return Err(PERR::MissingToken(
                "`crate`, `super`, `self` or `in`".into(),
                "in a field's `pub(..)`".into(),
            )
            .into_err(open));
        }
        let field_name = parse_name(input, "as a struct field")?;
        expect(input, Token::Colon, "after a struct field name")?;
        let ty = parse_type(input)?;
        let span = Span::new(field_name.span.start(), ty.span().end());
        fields.push(crate::item::Field {
            name: field_name,
            ty,
            span,
        });

        consume_comma_or_end(input, Token::RightBrace, "between struct fields")?;
    }

    let close = expect(input, Token::RightBrace, "to close a struct")?;
    let end = after(close, 1);

    Ok(crate::item::Struct {
        header: crate::item::Header {
            attributes,
            visibility,
            span: Span::new(start, header_end),
        },
        name,
        generics: generics.into_boxed_slice(),
        fields: fields.into_boxed_slice(),
        form: crate::item::StructForm::Named,
        span: Span::new(start, end),
    })
}

/// A parenthesized type after its `(` at `start`: `()`, `(T)` or a tuple.
fn parse_parenthesized_type(
    input: &mut TokenStream,
    start: Position,
) -> ParseResult<type_ref::Type> {
    let mut members = Vec::new();
    let mut comma = false;

    while !matches!(input.peek(), Some((Token::RightParen, _))) {
        members.push(parse_type(input)?);
        comma |= matches!(input.peek(), Some((Token::Comma, _)));
        consume_comma_or_end(input, Token::RightParen, "between tuple type members")?;
    }

    let close = expect(input, Token::RightParen, "to close a tuple type")?;
    let span = Span::new(start, after(close, 1));

    match members.len() {
        0 => Ok(type_ref::Type::Unit(span)),
        // `(T)` is `T`, as in `&(dyn Shape + Send)`.
        1 if !comma => Ok(members.pop().expect("one member")),
        1 => Err(PERR::MalformedTypeTuple(1, span).into_err(start)),
        _ => Ok(type_ref::Type::Tuple {
            members: members.into_boxed_slice(),
            span,
        }),
    }
}

/// A field's `pub`, `pub(crate)`, `pub(super)` or `pub(in path)`: Rawr
/// keeps no field privacy, so it is read and dropped. After `pub (` that is
/// no restriction, the `(` opening the field's tuple type (`pub (A, B)`).
fn skip_field_visibility(input: &mut TokenStream) -> ParseResult<Option<Position>> {
    if !matches!(input.peek(), Some((Token::Pub, _))) {
        return Ok(None);
    }
    input.next();
    if matches!(input.peek(), Some((Token::LeftParen, _))) {
        let (_, position) = input.next().expect("peeked `(` must exist");
        match input.peek() {
            Some((Token::SelfValue | Token::In, _)) => {}
            Some((Token::Identifier(name) | Token::Reserved(name), _))
                if matches!(name.as_str(), "crate" | "super") => {}
            _ => return Ok(Some(position)),
        }
        while !matches!(
            input.next(),
            Some((Token::RightParen | Token::EOF, _)) | None
        ) {}
    }
    Ok(None)
}

fn parse_enum(
    input: &mut TokenStream,
    attributes: Box<[crate::item::Attribute]>,
    visibility: Option<Span>,
    start: Position,
) -> ParseResult<crate::item::Enum> {
    expect(input, Token::Enum, "to start an enum")?;
    let name = parse_name(input, "after 'enum'")?;
    let mut generics = parse_generic_params(input)?;
    parse_where_clause(input, &mut generics, None)?;
    let open = expect(input, Token::LeftBrace, "after an enum name")?;
    let header_end = after(open, 1);
    let mut variants = Vec::new();

    while !matches!(input.peek(), Some((Token::RightBrace, _))) {
        // `#[default]` marks the variant a derived `Default` builds; other
        // variant attributes have no effect.
        let mut default = None;
        while input
            .peek()
            .is_some_and(|(token, _)| is_attribute_marker(token))
        {
            let attribute = parse_attribute(input)?;
            if attribute.path.segments.len() == 1
                && attribute.path.segments[0].name.text == "default"
            {
                default = Some(attribute.span);
            }
        }
        let variant_name = parse_name(input, "as an enum variant")?;
        let mut payload = Vec::new();
        let mut fields = None;
        let mut end = variant_name.span.end();

        if matches!(input.peek(), Some((Token::LeftParen, _))) {
            input.next();

            while !matches!(input.peek(), Some((Token::RightParen, _))) {
                let ty = parse_type(input)?;
                payload.push(ty);
                consume_comma_or_end(input, Token::RightParen, "between variant payload types")?;
            }

            let close = expect(input, Token::RightParen, "to close a variant payload")?;
            end = after(close, 1);
        } else if matches!(input.peek(), Some((Token::LeftBrace, _))) {
            // A struct variant: `V { x: i64, y: String }`.
            input.next();
            let mut names = Vec::new();

            while !matches!(input.peek(), Some((Token::RightBrace, _))) {
                let field = parse_name(input, "as a variant field")?;
                if names
                    .iter()
                    .any(|known: &crate::item::Name| known.text == field.text)
                {
                    return Err(PERR::DuplicatedProperty(field.text.to_string())
                        .into_err(field.span.start()));
                }
                expect(input, Token::Colon, "after a variant field name")?;
                payload.push(parse_type(input)?);
                names.push(field);
                consume_comma_or_end(input, Token::RightBrace, "between variant fields")?;
            }

            let close = expect(input, Token::RightBrace, "to close a struct variant")?;
            end = after(close, 1);
            fields = Some(names.into_boxed_slice());
        }

        // `Green = 5`, `Low = -1`: an explicit discriminant.
        let mut discriminant = None;
        if matches!(input.peek(), Some((Token::Equals, _))) {
            input.next();
            let negative = matches!(input.peek(), Some((Token::Minus | Token::UnaryMinus, _)));
            if negative {
                input.next();
            }
            match input.next() {
                Some((Token::IntegerConstant(value), position)) => {
                    discriminant = Some(if negative { -value } else { value });
                    end = after(position, value.to_string().len());
                }
                other => {
                    let position = other.map_or(Position::NONE, |(_, position)| position);
                    return Err(PERR::MissingToken(
                        "an integer".into(),
                        "as an enum discriminant".into(),
                    )
                    .into_err(position));
                }
            }
        }

        variants.push(crate::item::Variant {
            span: Span::new(variant_name.span.start(), end),
            name: variant_name,
            payload: payload.into_boxed_slice(),
            fields,
            default,
            discriminant,
        });

        consume_comma_or_end(input, Token::RightBrace, "between enum variants")?;
    }

    let close = expect(input, Token::RightBrace, "to close an enum")?;
    let end = after(close, 1);

    Ok(crate::item::Enum {
        header: crate::item::Header {
            attributes,
            visibility,
            span: Span::new(start, header_end),
        },
        name,
        generics: generics.into_boxed_slice(),
        variants: variants.into_boxed_slice(),
        span: Span::new(start, end),
    })
}

#[cfg_attr(feature = "no_function", allow(unused_variables))]
fn parse_impl(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    attributes: Box<[crate::item::Attribute]>,
    visibility: Option<Span>,
    start: Position,
) -> ParseResult<crate::item::Impl> {
    expect(input, Token::Impl, "to start an impl block")?;
    let mut generics = parse_generic_params(input)?;
    let first = parse_type(input)?;
    // `impl Trait for Type`.
    let (trait_, owner) = if matches!(input.peek(), Some((Token::For, _))) {
        let type_ref::Type::Path(path) = first else {
            let position = input.peek().expect("token streams always end with EOF").1;
            return Err(
                PERR::MissingToken("{".into(), "after an impl owner".into()).into_err(position)
            );
        };
        input.next();
        let mut owner = parse_type(input)?;
        // `impl Add<&V> for &V`: an operator impl for references is the
        // type's own, picked by its `Rhs` (references are values at
        // runtime).
        if let type_ref::Type::Reference { target, .. } = &owner
            && path.segments.last().is_some_and(|last| {
                crate::resolve::OPERATOR_TRAITS
                    .iter()
                    .any(|(_, name, _)| last.name.text == *name)
            })
        {
            owner = (**target).clone();
        }
        (Some(path), owner)
    } else {
        (None, first)
    };
    parse_where_clause(input, &mut generics, None)?;
    let open = expect(input, Token::LeftBrace, "after an impl owner")?;
    let header_end = after(open, 1);
    #[cfg(not(feature = "no_function"))]
    let mut functions = Vec::new();
    #[cfg(feature = "no_function")]
    let functions = Vec::<crate::item::Function>::new();
    let mut associated = Vec::new();

    while !matches!(input.peek(), Some((Token::RightBrace, _))) {
        #[cfg(not(feature = "no_function"))]
        {
            let (attributes, visibility, function_start) = parse_leading(input)?;
            if cfg_disabled(&attributes) {
                skip_item(input)?;
                continue;
            }

            // `type Output = i64;`
            if is_associated_type(input) {
                input.next();
                let name = parse_name(input, "after 'type'")?;
                expect(input, Token::Equals, "after an associated type name")?;
                let ty = parse_type(input)?;
                expect(input, Token::SemiColon, "after an associated type")?;
                associated.push((name, ty));
                continue;
            }
            if matches!(input.peek(), Some((Token::Fn, _))) {
                functions.push(parse_function(
                    input,
                    tokenizer_control,
                    attributes,
                    visibility,
                    function_start,
                    false,
                )?);
                continue;
            }
            if matches!(input.peek(), Some((Token::Const, _))) {
                functions.push(parse_associated_const(
                    input,
                    tokenizer_control,
                    attributes,
                    visibility,
                    function_start,
                    false,
                )?);
                continue;
            }
        }

        let (token, position) = input.peek().expect("token streams always end with EOF");
        if matches!(token, Token::Reserved(text) if text.as_str() == "async") {
            return Err(async_unsupported(*position));
        }
        let position = *position;
        return Err(PERR::MissingToken(
            "fn".into(),
            "for an associated function inside an impl block".into(),
        )
        .into_err(position));
    }

    let close = expect(input, Token::RightBrace, "to close an impl block")?;
    let end = after(close, 1);

    // `Self` in a signature names the impl's owner, and `Self::Output` the
    // impl's associated type, as in Rust.
    for (_, ty) in &mut associated {
        replace_self(ty, &owner);
    }
    #[cfg(not(feature = "no_function"))]
    for function in &mut functions {
        for parameter in function.parameters.iter_mut() {
            replace_associated(&mut parameter.ty, &associated, &owner);
            replace_self(&mut parameter.ty, &owner);
        }
        if let Some(output) = &mut function.output {
            replace_associated(output, &associated, &owner);
            replace_self(output, &owner);
        }
    }

    Ok(crate::item::Impl {
        header: crate::item::Header {
            attributes,
            visibility,
            span: Span::new(start, header_end),
        },
        generics: generics.into_boxed_slice(),
        // `impl Hash for T { fn hash<H: Hasher>(..) }`: Rawr hashes a value
        // by its contents, so the impl only marks `T` as `Hash`.
        functions: if trait_
            .as_ref()
            .and_then(|path| path.segments.last())
            .is_some_and(|last| last.name.text == "Hash")
            && hashes_as_rust(&functions)
        {
            Box::default()
        } else {
            functions.into_boxed_slice()
        },
        trait_,
        owner,
        associated: associated.into_boxed_slice(),
        span: Span::new(start, end),
    })
}

/// `type` starting an associated type item.
fn is_associated_type(input: &mut TokenStream) -> bool {
    matches!(input.peek(), Some((Token::Identifier(name), _)) if name.as_str() == "type")
}

/// Replace each `Self::Name` in `ty` with the impl's associated type
/// `Name`, or with `Owner::Name` when this impl does not declare it.
#[cfg(not(feature = "no_function"))]
fn replace_associated(
    ty: &mut type_ref::Type,
    associated: &[(crate::item::Name, type_ref::Type)],
    owner: &type_ref::Type,
) {
    match ty {
        type_ref::Type::Path(path)
            if path.segments.len() == 2
                && path.segments[0].name.text == "Self"
                && path.segments[0].arguments.is_empty()
                && path.segments[1].arguments.is_empty() =>
        {
            let name = &path.segments[1].name;
            if let Some((_, bound)) = associated
                .iter()
                .find(|(declared, _)| declared.text == name.text)
            {
                *ty = bound.clone();
            } else if let type_ref::Type::Path(owner) = owner {
                path.segments = owner
                    .segments
                    .iter()
                    .cloned()
                    .chain(std::iter::once(path.segments[1].clone()))
                    .collect();
            }
        }
        type_ref::Type::Path(path) => {
            for segment in path.segments.iter_mut() {
                for argument in segment.arguments.iter_mut() {
                    if let type_ref::Argument::Type(argument) = argument {
                        replace_associated(argument, associated, owner);
                    }
                }
            }
        }
        type_ref::Type::Tuple { members, .. } => {
            for member in members.iter_mut() {
                replace_associated(member, associated, owner);
            }
        }
        type_ref::Type::Reference { target, .. } => replace_associated(target, associated, owner),
        type_ref::Type::Unit(_) => {}
    }
}

/// `trait Name { fn required(&self) -> T; fn provided(&self) -> T { .. } }`.
fn parse_trait(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    attributes: Box<[crate::item::Attribute]>,
    visibility: Option<Span>,
    start: Position,
) -> ParseResult<crate::item::Trait> {
    input.next();
    let name = parse_name(input, "after 'trait'")?;
    // `trait Convert<T>`.
    let generics = parse_generic_params(input)?;
    // `trait Named: Shape + Clone`.
    let supertraits = if matches!(input.peek(), Some((Token::Colon, _))) {
        input.next();
        let mut projections = Vec::new();
        let bounds = parse_bounds(input, &mut projections, None)?;
        if let Some((_, name, _)) = projections.first() {
            return Err(PERR::MissingToken(
                "{".into(),
                "after a trait's supertraits (equalities in them are not supported)".into(),
            )
            .into_err(name.span.start()));
        }
        bounds
    } else {
        Vec::new()
    };
    // `where Self: Shape` is a supertrait; `where T: Clone` bounds `T`.
    let mut supertraits = supertraits;
    let mut generics = generics;
    parse_where_clause(input, &mut generics, Some(&mut supertraits))?;
    let open = expect(input, Token::LeftBrace, "after a trait name")?;
    let header_end = after(open, 1);
    #[cfg(not(feature = "no_function"))]
    let mut functions = Vec::new();
    #[cfg(feature = "no_function")]
    let functions = Vec::<crate::item::Function>::new();
    let mut provided = Vec::new();
    let mut associated = Vec::new();
    let mut associated_bounds = Vec::new();

    while !matches!(input.peek(), Some((Token::RightBrace, _))) {
        #[cfg(not(feature = "no_function"))]
        {
            let (attributes, visibility, function_start) = parse_leading(input)?;
            if cfg_disabled(&attributes) {
                skip_item(input)?;
                continue;
            }
            // `type Output;`
            if is_associated_type(input) {
                input.next();
                let name = parse_name(input, "after 'type'")?;
                // `type Output: Clone + Shape;`
                let bounds = if matches!(input.peek(), Some((Token::Colon, _))) {
                    input.next();
                    let mut projections = Vec::new();
                    let bounds = parse_bounds(input, &mut projections, None)?;
                    if let Some((_, name, _)) = projections.first() {
                        return Err(PERR::MissingToken(
                            ";".into(),
                            "after an associated type's bounds (equalities in them are not \
                             supported)"
                                .into(),
                        )
                        .into_err(name.span.start()));
                    }
                    bounds
                } else {
                    Vec::new()
                };
                expect(
                    input,
                    Token::SemiColon,
                    "after an associated type (defaults on associated types are not supported)",
                )?;
                associated.push(name);
                associated_bounds.push(bounds.into_boxed_slice());
                continue;
            }
            if matches!(input.peek(), Some((Token::Fn, _))) {
                let function = parse_function(
                    input,
                    tokenizer_control,
                    attributes,
                    visibility,
                    function_start,
                    true,
                )?;
                // A required function's body span is its `;`.
                provided.push(function.body_span.end() != after(function.body_span.start(), 1));
                functions.push(function);
                continue;
            }
            if matches!(input.peek(), Some((Token::Const, _))) {
                let constant = parse_associated_const(
                    input,
                    tokenizer_control,
                    attributes,
                    visibility,
                    function_start,
                    true,
                )?;
                // A required const has no value.
                provided.push(!constant.body_syntax.tokens().is_empty());
                functions.push(constant);
                continue;
            }
        }
        let position = input.peek().expect("token streams always end with EOF").1;
        return Err(
            PERR::MissingToken("fn".into(), "for a function inside a trait".into())
                .into_err(position),
        );
    }
    let close = expect(input, Token::RightBrace, "to close a trait")?;
    let end = after(close, 1);
    Ok(crate::item::Trait {
        header: crate::item::Header {
            attributes,
            visibility,
            span: Span::new(start, header_end),
        },
        name,
        functions: functions.into_boxed_slice(),
        provided: provided.into_boxed_slice(),
        associated: associated.into_boxed_slice(),
        associated_bounds: associated_bounds.into_boxed_slice(),
        supertraits: supertraits.into_boxed_slice(),
        generics: generics.into_boxed_slice(),
        span: Span::new(start, end),
    })
}

#[cfg(not(feature = "no_function"))]
fn parse_function(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    attributes: Box<[crate::item::Attribute]>,
    visibility: Option<Span>,
    start: Position,
    declaration: bool,
) -> ParseResult<crate::item::Function> {
    expect(input, Token::Fn, "to start a function")?;
    let name = parse_name(input, "after 'fn'")?;
    let generics = parse_generic_params(input)?;
    let open = if matches!(input.peek(), Some((Token::Unit, _))) {
        input.next();
        false
    } else {
        expect(input, Token::LeftParen, "after a function name")?;
        true
    };
    parse_function_rest(
        input,
        tokenizer_control,
        crate::item::Header {
            attributes,
            visibility,
            span: Span::NONE,
        },
        start,
        name,
        generics,
        open.then_some(None),
        declaration,
    )
}

/// Parse a function after its name and `(` (`parameters` is `Some`, holding
/// a first parameter name already consumed), or after `()` (`None`).
#[cfg(not(feature = "no_function"))]
fn parse_function_rest(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    header: crate::item::Header,
    start: Position,
    name: crate::item::Name,
    mut generics: Vec<crate::item::GenericParam>,
    parameters_open: Option<Option<crate::item::Name>>,
    declaration: bool,
) -> ParseResult<crate::item::Function> {
    let mut receiver = None;
    let mut parameters = Vec::new();

    if let Some(mut first) = parameters_open {
        while first.is_some() || !matches!(input.peek(), Some((Token::RightParen, _))) {
            match input.peek().expect("token streams always end with EOF") {
                _ if first.is_some() => {
                    let parameter_name = first.take().expect("guard proves a first parameter");
                    if matches!(
                        input.peek(),
                        Some((Token::LeftParen | Token::DoubleColon, _))
                    ) {
                        let pattern = parse_tuple_struct_parameter(input, parameter_name)?;
                        parameters.push(parse_pattern_parameter(input, pattern, parameters.len())?);
                        consume_comma_or_end(
                            input,
                            Token::RightParen,
                            "between function parameters",
                        )?;
                        continue;
                    }
                    expect(input, Token::Colon, "after a function parameter name")?;
                    let ty = parse_type(input)?;
                    let span = Span::new(parameter_name.span.start(), ty.span().end());
                    parameters.push(crate::item::Parameter {
                        name: parameter_name,
                        ty,
                        span,
                        pattern: None,
                    });
                }
                (Token::Ampersand, position) => {
                    // `&self`, `&mut self`, or a `&pattern` parameter; a
                    // receiver's lifetime (`&'a self`) is erased as in types.
                    let ampersand = *position;
                    input.next();
                    if matches!(input.peek(), Some((Token::Apostrophe, _))) {
                        parse_lifetime(input)?;
                    }
                    let mutable = matches!(input.peek(), Some((Token::Mut, _)));
                    if mutable {
                        input.next();
                    }
                    if let Some((Token::SelfValue, position)) = input.peek() {
                        if !parameters.is_empty() || receiver.is_some() {
                            return malformed_receiver(input);
                        }
                        let span = Span::new(ampersand, after(*position, 4));
                        input.next();
                        receiver = Some(if mutable {
                            crate::item::Receiver::Mutable(span)
                        } else {
                            crate::item::Receiver::Shared(span)
                        });
                    } else {
                        let pattern = parse_parameter_pattern(input)?;
                        let pattern = crate::item::ParameterPattern::Reference {
                            mutable,
                            span: Span::new(ampersand, pattern.span().end()),
                            pattern: Box::new(pattern),
                        };
                        parameters.push(parse_pattern_parameter(input, pattern, parameters.len())?);
                    }
                }
                (Token::SelfValue, position) if parameters.is_empty() && receiver.is_none() => {
                    let span = token_span(*position, "self");
                    input.next();
                    receiver = Some(crate::item::Receiver::Owned(span));
                }
                (Token::Mut, position) => {
                    // `mut self`, or a `mut name` parameter.
                    let start = *position;
                    input.next();
                    if let Some((Token::SelfValue, position)) = input.peek() {
                        if !parameters.is_empty() || receiver.is_some() {
                            return malformed_receiver(input);
                        }
                        let span = Span::new(start, after(*position, 4));
                        input.next();
                        receiver = Some(crate::item::Receiver::OwnedMutable(span));
                    } else {
                        let name = parse_name(input, "as a function parameter")?;
                        let pattern = crate::item::ParameterPattern::Binding {
                            name: crate::item::Name {
                                span: Span::new(start, name.span.end()),
                                ..name
                            },
                            mutable: true,
                        };
                        parameters.push(parse_pattern_parameter(input, pattern, parameters.len())?);
                    }
                }
                (Token::SelfValue, _) => {
                    return malformed_receiver(input);
                }
                (Token::LeftParen | Token::Underscore | Token::And, _) => {
                    let pattern = parse_parameter_pattern(input)?;
                    parameters.push(parse_pattern_parameter(input, pattern, parameters.len())?);
                }
                _ => {
                    let parameter_name = parse_name(input, "as a function parameter")?;
                    // `W(x): W`, a tuple struct's fields.
                    if matches!(
                        input.peek(),
                        Some((Token::LeftParen | Token::DoubleColon, _))
                    ) {
                        let pattern = parse_tuple_struct_parameter(input, parameter_name)?;
                        parameters.push(parse_pattern_parameter(input, pattern, parameters.len())?);
                        consume_comma_or_end(
                            input,
                            Token::RightParen,
                            "between function parameters",
                        )?;
                        continue;
                    }
                    expect(input, Token::Colon, "after a function parameter name")?;
                    let ty = parse_type(input)?;
                    let span = Span::new(parameter_name.span.start(), ty.span().end());
                    parameters.push(crate::item::Parameter {
                        name: parameter_name,
                        ty,
                        span,
                        pattern: None,
                    });
                }
            }

            consume_comma_or_end(input, Token::RightParen, "between function parameters")?;
        }

        expect(input, Token::RightParen, "to close function parameters")?;
    }

    let mut output = if matches!(input.peek(), Some((Token::ThinArrow, _))) {
        input.next();
        Some(parse_type(input)?)
    } else {
        None
    };
    parse_where_clause(input, &mut generics, None)?;
    lift_impl_parameters(&mut generics, &mut parameters);
    erase_fn_bounds(&mut generics, &mut parameters, &mut output);

    // `fn area(&self) -> f64;`: a required trait function has no body.
    if declaration && let Some((Token::SemiColon, position)) = input.peek() {
        let semicolon = Span::new(*position, after(*position, 1));
        input.next();
        return Ok(crate::item::Function {
            header: crate::item::Header {
                span: Span::new(start, semicolon.end()),
                ..header
            },
            name,
            generics: generics.into_boxed_slice(),
            receiver,
            parameters: parameters.into_boxed_slice(),
            output,
            body_span: semicolon,
            body_syntax: crate::item::BodySyntax::new(Vec::new()),
            constant: false,
            span: Span::new(start, semicolon.end()),
        });
    }
    let open = expect(input, Token::LeftBrace, "to start a function body")?;
    let header_end = after(open, 1);
    let (body_span, body_syntax) = parse_body_syntax(input, tokenizer_control, open)?;
    let span = Span::new(start, body_span.end());

    Ok(crate::item::Function {
        header: crate::item::Header {
            span: Span::new(start, header_end),
            ..header
        },
        name,
        generics: generics.into_boxed_slice(),
        receiver,
        parameters: parameters.into_boxed_slice(),
        output,
        body_span,
        body_syntax,
        constant: false,
        span,
    })
}

/// `<T, U: Clone + Debug, 'a>` after an item name, when present. Lifetime
/// parameters are erased.
fn parse_generic_params(input: &mut TokenStream) -> ParseResult<Vec<crate::item::GenericParam>> {
    let mut generics = Vec::new();
    if !matches!(input.peek(), Some((Token::LessThan, _))) {
        return Ok(generics);
    }
    input.next();
    // The second `>` of a `>>` closing a bound's arguments closes the list.
    let mut split = None;
    while !matches!(input.peek(), Some((Token::GreaterThan, _))) {
        if matches!(input.peek(), Some((Token::Apostrophe, _))) {
            parse_lifetime(input)?;
            if matches!(input.peek(), Some((Token::Colon, _))) {
                input.next();
                parse_lifetime(input)?;
                while matches!(input.peek(), Some((Token::Plus, _))) {
                    input.next();
                    parse_lifetime(input)?;
                }
            }
        } else {
            let name = parse_name(input, "as a generic parameter")?;
            let mut projections = Vec::new();
            let bounds = if matches!(input.peek(), Some((Token::Colon, _))) {
                input.next();
                parse_bounds(input, &mut projections, Some(&mut split))?
            } else {
                Vec::new()
            };
            // `Rhs = Self`: a default type, which only traits take.
            let default = if split.is_none() && matches!(input.peek(), Some((Token::Equals, _))) {
                input.next();
                Some(parse_type(input)?)
            } else {
                None
            };
            generics.push(crate::item::GenericParam {
                name,
                bounds: bounds.into_boxed_slice(),
                projections: projections.into_boxed_slice(),
                default,
            });
        }
        if split.is_some() {
            return Ok(generics);
        }
        consume_comma_or_end(input, Token::GreaterThan, "between generic parameters")?;
    }
    expect(input, Token::GreaterThan, "to close generic parameters")?;
    Ok(generics)
}

/// `Bound + Bound`: trait paths; lifetime bounds are erased. Associated
/// type equalities (`Shape<Output = i64>`) go to `projections`.
/// `async fn`, which Rawr does not run yet.
fn async_unsupported(position: Position) -> crate::ParseError {
    PERR::BadInput(crate::LexError::ImproperSymbol(
        "async".into(),
        "`async` functions are not supported in Rawr yet".into(),
    ))
    .into_err(position)
}

/// `use<'a, T>` among `impl Trait` bounds: Rust's precise capturing, which
/// Rawr's types need no record of. Where it ends, when it was there.
fn skip_precise_capture(input: &mut TokenStream) -> ParseResult<Option<Position>> {
    if !matches!(input.peek(), Some((Token::Use, _))) {
        return Ok(None);
    }
    input.next();
    expect(input, Token::LessThan, "after `use` in a bound")?;
    loop {
        match input.next() {
            Some((Token::GreaterThan, position)) => return Ok(Some(after(position, 1))),
            Some((Token::EOF, position)) => {
                return Err(PERR::MissingToken(">".into(), "to close `use<..>`".into())
                    .into_err(position));
            }
            None => unreachable!("token streams always end with EOF"),
            Some(_) => {}
        }
    }
}

fn parse_bounds(
    input: &mut TokenStream,
    projections: &mut Vec<(crate::item::Name, crate::item::Name, type_ref::Type)>,
    mut closing: Option<&mut Option<Position>>,
) -> ParseResult<Vec<type_ref::Path>> {
    let mut bounds = Vec::new();
    loop {
        if matches!(input.peek(), Some((Token::Apostrophe, _))) {
            parse_lifetime(input)?;
        } else if skip_precise_capture(input)?.is_some() {
        } else if let Some((Token::Identifier(name) | Token::Reserved(name), position)) =
            input.peek()
            && is_fn_trait(name)
        {
            // `F: Fn(A) -> B`: a function type bound.
            let start = *position;
            input.next();
            bounds.push(parse_fn_signature(input, start)?);
        } else {
            let mut split = None;
            let mut bindings = Vec::new();
            let bound = parse_path_bindings(input, &mut split, Some(&mut bindings))?;
            // Each equality belongs to its bound's trait.
            let trait_name = &bound
                .segments
                .last()
                .expect("type paths contain at least one segment")
                .name;
            projections.extend(
                bindings
                    .into_iter()
                    .map(|(name, ty)| (trait_name.clone(), name, ty)),
            );
            bounds.push(bound);
            match closing.as_deref_mut() {
                // `<T: Shape<Output = i64>>`: the `>>` also closes the list.
                Some(closing) if split.is_some() => {
                    *closing = split;
                    return Ok(bounds);
                }
                _ => reject_split(split)?,
            }
        }
        // After a bound's closing `>`, the lexer reads `+` as unary.
        if !matches!(input.peek(), Some((Token::Plus | Token::UnaryPlus, _))) {
            return Ok(bounds);
        }
        input.next();
    }
}

/// `where T: Bound + Bound, ..` before an item body: bounds on declared
/// parameters.
/// A trait's `where Self: Shape` adds to its supertraits (`self_bounds`), as
/// Rust reads it.
fn parse_where_clause(
    input: &mut TokenStream,
    generics: &mut [crate::item::GenericParam],
    mut self_bounds: Option<&mut Vec<type_ref::Path>>,
) -> ParseResult<()> {
    if !matches!(input.peek(), Some((Token::Identifier(name), _)) if name.as_str() == "where") {
        return Ok(());
    }
    input.next();
    while !matches!(input.peek(), Some((Token::LeftBrace | Token::SemiColon, _))) {
        let name = parse_name(input, "as a `where` clause parameter")?;
        expect(input, Token::Colon, "after a `where` clause parameter")?;
        let mut projections = Vec::new();
        let bounds = parse_bounds(input, &mut projections, None)?;
        if name.text == "Self"
            && let Some(supertraits) = self_bounds.as_deref_mut()
        {
            if let Some((_, name, _)) = projections.first() {
                return Err(PERR::MissingToken(
                    "{".into(),
                    "after a trait's `where Self:` bounds (equalities in them are not supported)"
                        .into(),
                )
                .into_err(name.span.start()));
            }
            supertraits.extend(bounds);
            if matches!(input.peek(), Some((Token::Comma, _))) {
                input.next();
            }
            continue;
        }
        let Some(parameter) = generics
            .iter_mut()
            .find(|parameter| parameter.name.text == name.text)
        else {
            return Err(PERR::MissingToken(
                "a declared generic parameter".into(),
                format!("for `{}` in a `where` clause", name.text),
            )
            .into_err(name.span.start()));
        };
        let mut all = parameter.bounds.to_vec();
        all.extend(bounds);
        parameter.bounds = all.into_boxed_slice();
        let mut all = parameter.projections.to_vec();
        all.extend(projections);
        parameter.projections = all.into_boxed_slice();
        if matches!(input.peek(), Some((Token::Comma, _))) {
            input.next();
        }
    }
    Ok(())
}

/// Replace each `Self` in `ty` with `owner`.
#[cfg(not(feature = "no_function"))]
fn replace_self(ty: &mut type_ref::Type, owner: &type_ref::Type) {
    match ty {
        type_ref::Type::Path(path)
            if path.segments.len() == 1
                && path.segments[0].name.text == "Self"
                && path.segments[0].arguments.is_empty() =>
        {
            *ty = owner.clone();
        }
        type_ref::Type::Path(path) => {
            for segment in path.segments.iter_mut() {
                for argument in segment.arguments.iter_mut() {
                    if let type_ref::Argument::Type(argument) = argument {
                        replace_self(argument, owner);
                    }
                }
            }
        }
        type_ref::Type::Tuple { members, .. } => {
            for member in members.iter_mut() {
                replace_self(member, owner);
            }
        }
        type_ref::Type::Reference { target, .. } => replace_self(target, owner),
        type_ref::Type::Unit(_) => {}
    }
}

/// `: T` after a pattern parameter: the parameter takes a compiler-private
/// name and the pattern is bound at the top of the body.
#[cfg(not(feature = "no_function"))]
fn parse_pattern_parameter(
    input: &mut TokenStream,
    pattern: crate::item::ParameterPattern,
    index: usize,
) -> ParseResult<crate::item::Parameter> {
    expect(input, Token::Colon, "after a function parameter pattern")?;
    let ty = parse_type(input)?;
    let span = Span::new(pattern.span().start(), ty.span().end());
    Ok(crate::item::Parameter {
        name: crate::item::Name {
            text: format!("__rawr_parameter_{index}").into(),
            span: pattern.span(),
        },
        ty,
        span,
        pattern: Some(pattern),
    })
}

/// An irrefutable parameter pattern: `name`, `mut name`, `_`, `(p, ..)`,
/// `&p`, `&mut p`.
#[cfg(not(feature = "no_function"))]
fn parse_parameter_pattern(input: &mut TokenStream) -> ParseResult<crate::item::ParameterPattern> {
    let (token, start) = input.peek().expect("token streams always end with EOF");
    let start = *start;
    match token {
        Token::Underscore => {
            input.next();
            Ok(crate::item::ParameterPattern::Discard(token_span(
                start, "_",
            )))
        }
        Token::Ampersand | Token::And => {
            let double = matches!(token, Token::And);
            input.next();
            let mutable = matches!(input.peek(), Some((Token::Mut, _)));
            if mutable {
                input.next();
            }
            let pattern = parse_parameter_pattern(input)?;
            let span = Span::new(start, pattern.span().end());
            let inner = crate::item::ParameterPattern::Reference {
                mutable,
                pattern: Box::new(pattern),
                span,
            };
            Ok(if double {
                crate::item::ParameterPattern::Reference {
                    mutable: false,
                    pattern: Box::new(inner),
                    span,
                }
            } else {
                inner
            })
        }
        Token::LeftParen => {
            input.next();
            let mut members = Vec::new();
            while !matches!(input.peek(), Some((Token::RightParen, _))) {
                members.push(parse_parameter_pattern(input)?);
                consume_comma_or_end(input, Token::RightParen, "between tuple pattern members")?;
            }
            let close = expect(input, Token::RightParen, "to close a tuple pattern")?;
            let span = Span::new(start, after(close, 1));
            if members.len() == 1 {
                return Ok(members.pop().expect("one member"));
            }
            Ok(crate::item::ParameterPattern::Tuple {
                members: members.into_boxed_slice(),
                span,
            })
        }
        Token::Mut => {
            input.next();
            let name = parse_name(input, "after `mut` in a parameter pattern")?;
            Ok(crate::item::ParameterPattern::Binding {
                name: crate::item::Name {
                    span: Span::new(start, name.span.end()),
                    ..name
                },
                mutable: true,
            })
        }
        _ => {
            let name = parse_name(input, "in a parameter pattern")?;
            parse_tuple_struct_parameter(input, name)
        }
    }
}

/// After a parameter pattern's first name: `W(a, ..)` or `module::W(..)`
/// of a tuple struct, else the binding `name`.
#[cfg(not(feature = "no_function"))]
fn parse_tuple_struct_parameter(
    input: &mut TokenStream,
    name: crate::item::Name,
) -> ParseResult<crate::item::ParameterPattern> {
    if !matches!(
        input.peek(),
        Some((Token::LeftParen | Token::DoubleColon, _))
    ) {
        return Ok(crate::item::ParameterPattern::Binding {
            name,
            mutable: false,
        });
    }
    let start = name.span.start();
    let mut path = vec![name];
    while matches!(input.peek(), Some((Token::DoubleColon, _))) {
        input.next();
        path.push(parse_name(input, "in a tuple struct pattern's path")?);
    }
    expect(
        input,
        Token::LeftParen,
        "after a tuple struct in a parameter pattern",
    )?;
    let mut members = Vec::new();
    while !matches!(input.peek(), Some((Token::RightParen, _))) {
        members.push(parse_parameter_pattern(input)?);
        consume_comma_or_end(
            input,
            Token::RightParen,
            "between tuple struct pattern members",
        )?;
    }
    let close = expect(input, Token::RightParen, "to close a tuple struct pattern")?;
    Ok(crate::item::ParameterPattern::TupleStruct {
        path: path.into_boxed_slice(),
        members: members.into_boxed_slice(),
        span: Span::new(start, after(close, 1)),
    })
}

/// `const NAME: T = value;` in an impl or trait (`const NAME: T;` when
/// required): a constant function without `self` whose body is the value,
/// which `Type::NAME` and `T::NAME` call.
#[cfg(not(feature = "no_function"))]
fn parse_associated_const(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    attributes: Box<[crate::item::Attribute]>,
    visibility: Option<Span>,
    start: Position,
    declaration: bool,
) -> ParseResult<crate::item::Function> {
    input.next();
    let name = parse_name(input, "after 'const'")?;
    let header = crate::item::Header {
        attributes,
        visibility,
        span: Span::NONE,
    };
    expect(input, Token::Colon, "after a constant name")?;
    let output = parse_type(input)?;
    // `const SIDES: i64;`: a required trait const has no value.
    if declaration && let Some((Token::SemiColon, position)) = input.peek() {
        let semicolon = Span::new(*position, after(*position, 1));
        input.next();
        return Ok(crate::item::Function {
            header: crate::item::Header {
                span: Span::new(start, semicolon.end()),
                ..header
            },
            name,
            generics: Box::default(),
            receiver: None,
            parameters: Box::default(),
            output: Some(output),
            body_span: semicolon,
            body_syntax: crate::item::BodySyntax::new(Vec::new()),
            constant: true,
            span: Span::new(start, semicolon.end()),
        });
    }
    let equals = expect(input, Token::Equals, "after a constant type")?;
    finish_associated_const(
        input,
        tokenizer_control,
        header,
        start,
        name,
        output,
        equals,
    )
}

/// The value of an associated const after its `=`, its body as a
/// top-level const's is.
#[cfg(not(feature = "no_function"))]
fn finish_associated_const(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    header: crate::item::Header,
    start: Position,
    name: crate::item::Name,
    output: type_ref::Type,
    equals: Position,
) -> ParseResult<crate::item::Function> {
    let value_start = input.peek().map_or(equals, |(_, position)| *position);
    let mut scan = ScanState::default();
    let mut tokens = Vec::new();
    let semicolon = loop {
        match scan
            .next(input, tokenizer_control)
            .expect("token streams always end with EOF")
        {
            (Token::SemiColon, position) if scan.delimiter_depth == 0 => break position,
            (Token::EOF, position) => {
                return Err(PERR::MissingToken(
                    Token::SemiColon.into(),
                    "to end a constant".into(),
                )
                .into_err(position));
            }
            (Token::LexError(error), position) => return Err(error.into_err(position)),
            (token, position) => tokens.push(body_token(token, position)),
        }
    };
    let span = Span::new(start, after(semicolon, 1));
    Ok(crate::item::Function {
        header: crate::item::Header {
            span: Span::new(start, value_start),
            ..header
        },
        name,
        generics: Box::default(),
        receiver: None,
        parameters: Box::default(),
        output: Some(output),
        body_span: Span::new(value_start, semicolon),
        body_syntax: crate::item::BodySyntax::new(tokens),
        constant: true,
        span,
    })
}

/// Parse `: T = value;` after `const NAME`.
#[cfg(not(feature = "no_function"))]
fn parse_const_rest(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    header: crate::item::Header,
    start: Position,
    name: crate::item::Name,
) -> ParseResult<crate::item::Function> {
    expect(input, Token::Colon, "after a constant name")?;
    let output = parse_type(input)?;
    let equals = expect(input, Token::Equals, "after a constant type")?;
    let value_start = input.peek().map_or(equals, |(_, position)| *position);
    let mut scan = ScanState::default();
    let mut tokens = Vec::new();
    let semicolon = loop {
        match scan
            .next(input, tokenizer_control)
            .expect("token streams always end with EOF")
        {
            (Token::SemiColon, position) if scan.delimiter_depth == 0 => break position,
            (Token::EOF, position) => {
                return Err(PERR::MissingToken(
                    Token::SemiColon.into(),
                    "to end a constant".into(),
                )
                .into_err(position));
            }
            (Token::LexError(error), position) => return Err(error.into_err(position)),
            (token, position) => tokens.push(body_token(token, position)),
        }
    };
    let span = Span::new(start, after(semicolon, 1));
    Ok(crate::item::Function {
        header: crate::item::Header {
            span: Span::new(start, value_start),
            ..header
        },
        name,
        generics: Box::default(),
        receiver: None,
        parameters: Box::default(),
        output: Some(output),
        body_span: Span::new(value_start, semicolon),
        body_syntax: crate::item::BodySyntax::new(tokens),
        constant: true,
        span,
    })
}

#[cfg(not(feature = "no_function"))]
fn malformed_receiver<T>(input: &mut TokenStream) -> ParseResult<T> {
    match input.next().expect("token streams always end with EOF") {
        (Token::LexError(error), position) => Err(error.into_err(position)),
        (token, position) => {
            let found = token.to_string();
            let span = token_span(position, &found);
            Err(PERR::MalformedReceiver(found, span).into_err(position))
        }
    }
}

fn parse_type(input: &mut TokenStream) -> ParseResult<type_ref::Type> {
    let mut split = None;
    let ty = parse_type_split(input, &mut split)?;
    reject_split(split)?;
    Ok(ty)
}

/// A `>` left over from a `>>` that closed one more generic list than the
/// type opened.
fn reject_split(split: Option<Position>) -> ParseResult<()> {
    match split {
        Some(position) => Err(PERR::MissingToken(
            Token::Comma.into(),
            "instead of an unmatched `>`".into(),
        )
        .into_err(position)),
        None => Ok(()),
    }
}

/// `Send`, `Sync`, `Unpin` and the unwind-safety markers, bare or under
/// `std::marker`/`core::marker`/`std::panic`: the auto traits, which every
/// Rawr value has, so a trait object may add them.
pub(crate) fn is_auto_trait(path: &type_ref::Path) -> bool {
    let Some((last, prefix)) = path.segments.split_last() else {
        return false;
    };
    last.arguments.is_empty()
        && matches!(
            last.name.text.as_str(),
            "Send" | "Sync" | "Unpin" | "UnwindSafe" | "RefUnwindSafe"
        )
        && prefix.iter().all(|segment| {
            segment.arguments.is_empty()
                && matches!(
                    segment.name.text.as_str(),
                    "std" | "core" | "marker" | "panic"
                )
        })
}

/// A type whose last generic list may close with the first `>` of a `>>`;
/// the second is left in `split` for the enclosing list.
fn parse_type_split(
    input: &mut TokenStream,
    split: &mut Option<Position>,
) -> ParseResult<type_ref::Type> {
    if let Some((Token::Unit, position)) = input.peek() {
        let position = *position;
        input.next();
        return Ok(type_ref::Type::Unit(token_span(position, "()")));
    }

    if let Some((Token::LeftParen, position)) = input.peek() {
        let start = *position;
        input.next();
        return parse_parenthesized_type(input, start);
    }

    // `[T]`, a slice: a `Vec<T>` at runtime, as it is in bodies; `[T; N]`,
    // an array of a literal length: the builtin path `[_; N]<T>`.
    if let Some((Token::LeftBracket, position)) = input.peek() {
        let start = *position;
        input.next();
        let element = parse_type(input)?;
        let name = if matches!(input.peek(), Some((Token::SemiColon, _))) {
            input.next();
            match input.next().expect("token streams always end with EOF") {
                (Token::IntegerConstant(length), position) => usize::try_from(length)
                    .map(type_ref::array_name)
                    .map_err(|_| {
                        PERR::MissingSymbol("an array length must be a `usize`".into())
                            .into_err(position)
                    })?,
                (Token::LexError(error), position) => return Err(error.into_err(position)),
                // `[T; N]` of a `const N` (or `Self::N`, `Grid::N`), read
                // once the items are parsed.
                (Token::Identifier(length), _) => {
                    let mut path = length.to_string();
                    while matches!(input.peek(), Some((Token::DoubleColon, _))) {
                        input.next();
                        path.push_str("::");
                        path.push_str(&parse_name(input, "in an array length path")?.text);
                    }
                    type_ref::named_array_name(&path)
                }
                (_, position) => {
                    return Err(PERR::MissingToken(
                        "integer literal or constant".into(),
                        "for an array type's length".into(),
                    )
                    .into_err(position))
                }
            }
        } else {
            "Vec".into()
        };
        let end = expect(input, Token::RightBracket, "to close an array or slice type")?;
        let span = Span::new(start, after(end, 1));
        return Ok(type_ref::Type::Path(type_ref::Path {
            segments: Box::new([type_ref::Segment {
                name: crate::item::Name {
                    text: name.into(),
                    span,
                },
                arguments: Box::new([type_ref::Argument::Type(element)]),
                span,
            }]),
            span,
        }));
    }

    if let Some((Token::Ampersand, position)) = input.peek() {
        let start = *position;
        input.next();
        let lifetime = if matches!(input.peek(), Some((Token::Apostrophe, _))) {
            Some(parse_lifetime(input)?)
        } else {
            None
        };
        let mutable = if matches!(input.peek(), Some((Token::Mut, _))) {
            input.next();
            true
        } else {
            false
        };
        let target = Box::new(parse_type_split(input, split)?);
        let span = Span::new(start, target.span().end());
        return Ok(type_ref::Type::Reference {
            mutable,
            lifetime,
            target,
            span,
        });
    }

    // `dyn Trait`: a trait object type, the path `dyn::Trait`.
    if let Some((Token::Identifier(name), position)) = input.peek()
        && name.as_str() == type_ref::DYN_SEGMENT
    {
        let start = *position;
        input.next();
        if let Some((Token::Identifier(name) | Token::Reserved(name), _)) = input.peek()
            && is_fn_trait(name)
        {
            input.next();
            let path = parse_fn_signature(input, start)?;
            return skip_fn_bounds(input, split, path).map(type_ref::Type::Path);
        }
        // `dyn Shape<Output = i64>`: each binding is the argument
        // `=Output<i64>` of the trait's segment.
        let mut bindings = Vec::new();
        let mut path = parse_path_bindings(input, split, Some(&mut bindings))?;
        if let Some(last) = path.segments.last_mut() {
            let mut arguments = last.arguments.to_vec();
            arguments.extend(bindings.into_iter().map(|(name, ty)| {
                let span = Span::new(name.span.start(), ty.span().end());
                type_ref::Argument::Type(type_ref::Type::Path(type_ref::Path {
                    segments: Box::new([type_ref::Segment {
                        name: crate::item::Name {
                            text: format!("{}{}", type_ref::BINDING_PREFIX, name.text).into(),
                            span: name.span,
                        },
                        arguments: Box::new([type_ref::Argument::Type(ty)]),
                        span,
                    }]),
                    span,
                }))
            }));
            last.arguments = arguments.into_boxed_slice();
        }
        let dyn_segment = type_ref::Segment {
            name: crate::item::Name {
                text: type_ref::DYN_SEGMENT.into(),
                span: token_span(start, "dyn"),
            },
            arguments: Box::default(),
            span: token_span(start, "dyn"),
        };
        path.segments = std::iter::once(dyn_segment)
            .chain(path.segments.into_vec())
            .collect();
        path.span = Span::new(start, path.span.end());
        // `dyn Shape + Send + 'a`: Rust admits only auto traits and
        // lifetimes after the first trait (E0225).
        while split.is_none() && matches!(input.peek(), Some((Token::Plus | Token::UnaryPlus, _))) {
            input.next();
            if matches!(input.peek(), Some((Token::Apostrophe, _))) {
                let lifetime = parse_lifetime(input)?;
                path.span = Span::new(start, lifetime.span.end());
                continue;
            }
            if let Some(end) = skip_precise_capture(input)? {
                path.span = Span::new(start, end);
                continue;
            }
            let extra = parse_path_split(input, split)?;
            if !is_auto_trait(&extra) {
                return Err(PERR::BadInput(crate::LexError::ImproperSymbol(
                    "+".into(),
                    "only auto traits (`Send`, `Sync`, `Unpin`) can be used as additional \
                     traits in a trait object, as in Rust (E0225); declare a trait for the \
                     combination instead"
                        .into(),
                ))
                .into_err(extra.span.start()));
            }
            path.span = Span::new(start, extra.span.end());
        }
        return Ok(type_ref::Type::Path(path));
    }

    // `<T as Trait>::Name`: the associated type `Name` of `T`'s impl of
    // `Trait`, kept as the path `T::as<Trait>::Name`.
    if let Some((Token::LessThan, position)) = input.peek() {
        let start = *position;
        input.next();
        let base = parse_type(input)?;
        let qualified = if let Some((Token::As, position)) = input.peek() {
            let position = *position;
            input.next();
            let trait_path = parse_path(input)?;
            let span = Span::new(position, trait_path.span.end());
            Some(type_ref::Segment {
                name: crate::item::Name {
                    text: type_ref::QUALIFIED_SEGMENT.into(),
                    span: token_span(position, "as"),
                },
                arguments: Box::new([type_ref::Argument::Type(type_ref::Type::Path(trait_path))]),
                span,
            })
        } else {
            None
        };
        expect(input, Token::GreaterThan, "to close a qualified type path")?;
        expect(input, Token::DoubleColon, "after a qualified type path")?;
        let rest = parse_path_split(input, split)?;
        let type_ref::Type::Path(base) = base else {
            return Err(PERR::MissingToken(
                "a type path".into(),
                "in a qualified type path".into(),
            )
            .into_err(start));
        };
        let end = rest.span.end();
        return Ok(type_ref::Type::Path(type_ref::Path {
            segments: base
                .segments
                .into_vec()
                .into_iter()
                .chain(qualified)
                .chain(rest.segments.into_vec())
                .collect(),
            span: Span::new(start, end),
        }));
    }

    // `fn(A) -> B`, `impl Fn(A) -> B` and `dyn Fn(A) -> B`: Rawr's one
    // function type; `impl Iterator<Item = T>`.
    match fn_type_start(input)? {
        Some(ImplStart::Fn(start)) => {
            let path = parse_fn_signature(input, start)?;
            return skip_fn_bounds(input, split, path).map(type_ref::Type::Path);
        }
        Some(ImplStart::Iterator(name, start)) => {
            return parse_iterator_type(input, split, name, start)
        }
        Some(ImplStart::Bounds(start)) => return parse_impl_bounds(input, split, start),
        None => {}
    }

    parse_path_split(input, split).map(type_ref::Type::Path)
}

/// `impl Fn(A) -> B + 'a + Send`: the lifetimes and auto traits after a
/// function type, which Rawr's one function type needs no record of.
fn skip_fn_bounds(
    input: &mut TokenStream,
    split: &mut Option<Position>,
    mut path: type_ref::Path,
) -> ParseResult<type_ref::Path> {
    let start = path.span.start();
    while split.is_none() && matches!(input.peek(), Some((Token::Plus | Token::UnaryPlus, _))) {
        input.next();
        if matches!(input.peek(), Some((Token::Apostrophe, _))) {
            let lifetime = parse_lifetime(input)?;
            path.span = Span::new(start, lifetime.span.end());
            continue;
        }
        if let Some(end) = skip_precise_capture(input)? {
            path.span = Span::new(start, end);
            continue;
        }
        let extra = parse_path_split(input, split)?;
        if !is_auto_trait(&extra) {
            return Err(PERR::BadInput(crate::LexError::ImproperSymbol(
                "+".into(),
                "only lifetimes and auto traits (`Send`, `Sync`, `Unpin`) can be added to a \
                 function trait"
                    .into(),
            ))
            .into_err(extra.span.start()));
        }
        path.span = Span::new(start, extra.span.end());
    }
    Ok(path)
}

/// Whether an `impl Hash` declares Rust's one method,
/// `fn hash<H: Hasher>(&self, state: &mut H)`.
fn hashes_as_rust(functions: &[crate::item::Function]) -> bool {
    let [function] = functions else {
        return false;
    };
    let [generic] = &*function.generics else {
        return false;
    };
    let [parameter] = &*function.parameters else {
        return false;
    };
    function.name.text == "hash"
        && matches!(function.receiver, Some(crate::item::Receiver::Shared(_)))
        && function.output.is_none()
        && matches!(
            &parameter.ty,
            type_ref::Type::Reference { mutable: true, target, .. }
                if matches!(&**target, type_ref::Type::Path(path)
                    if matches!(&path.segments[..], [segment] if segment.name.text == generic.name.text))
        )
}

/// What a type starting with `fn` or `impl` is.
enum ImplStart {
    /// `fn(..)` or `impl Fn*(..)`, its head consumed.
    Fn(Position),
    /// `impl Iterator` (or another iterator trait), its builtin name.
    Iterator(&'static str, Position),
    /// `impl Bound + Bound` of other traits, after `impl`.
    Bounds(Position),
}

/// The start of a function type, after consuming its `fn` or `impl Fn*`
/// head; `None`, consuming nothing, before any other type.
fn fn_type_start(input: &mut TokenStream) -> ParseResult<Option<ImplStart>> {
    let start = match input.peek() {
        Some((Token::Fn, position)) => {
            let position = *position;
            input.next();
            return Ok(Some(ImplStart::Fn(position)));
        }
        Some((Token::Impl, position)) => *position,
        _ => return Ok(None),
    };
    input.next();
    let iterator = match input.peek() {
        Some((Token::Identifier(name) | Token::Reserved(name), _)) if is_fn_trait(name) => {
            input.next();
            return Ok(Some(ImplStart::Fn(start)));
        }
        Some((Token::Identifier(name), _)) => iterator_trait(name),
        _ => None,
    };
    Ok(Some(match iterator {
        Some(name) => {
            input.next();
            ImplStart::Iterator(name, start)
        }
        None => ImplStart::Bounds(start),
    }))
}

/// `impl Bound + Bound` of other traits after `impl` at `start`: the path
/// `impl<Bound, ..>` (`type_ref::IMPL_TYPE`), which a function's parameter
/// reads as an anonymous generic parameter with those bounds, as Rust does,
/// and a facade trait's (`impl Widget<Message>`) as the type it erases to.
fn parse_impl_bounds(
    input: &mut TokenStream,
    split: &mut Option<Position>,
    start: Position,
) -> ParseResult<type_ref::Type> {
    // `impl Shape<Output = i64>`: each equality is the argument
    // `=Shape=Output<i64>` after the bounds, which a parameter's lifted
    // generic takes as its projection.
    let mut projections = Vec::new();
    let bounds = parse_bounds(input, &mut projections, Some(split))?;
    let bindings = projections.into_iter().map(|(owner, name, ty)| {
        let span = Span::new(name.span.start(), ty.span().end());
        type_ref::Path {
            segments: Box::new([type_ref::Segment {
                name: crate::item::Name {
                    text: format!("{}{}={}", type_ref::BINDING_PREFIX, owner.text, name.text).into(),
                    span: owner.span,
                },
                arguments: Box::new([type_ref::Argument::Type(ty)]),
                span,
            }]),
            span,
        }
    });
    let bounds = bounds.into_iter().chain(bindings).collect::<Vec<_>>();
    let end = bounds.last().map_or(start, |bound| bound.span.end());
    let span = Span::new(start, end);
    Ok(type_ref::Type::Path(type_ref::Path {
        segments: Box::new([type_ref::Segment {
            name: crate::item::Name {
                text: type_ref::IMPL_TYPE.into(),
                span: token_span(start, "impl"),
            },
            arguments: bounds
                .into_iter()
                .map(|bound| type_ref::Argument::Type(type_ref::Type::Path(bound)))
                .collect(),
            span,
        }]),
        span,
    }))
}

/// Each `impl Bound` in a parameter's type is an anonymous generic
/// parameter of the function with those bounds (Rust's argument-position
/// `impl Trait`), named `impl_Bound` (`impl_Bound2`, .. when repeated).
fn lift_impl_parameters(
    generics: &mut Vec<crate::item::GenericParam>,
    parameters: &mut [crate::item::Parameter],
) {
    fn lift(ty: &mut type_ref::Type, generics: &mut Vec<crate::item::GenericParam>) {
        match ty {
            type_ref::Type::Path(path)
                if matches!(&path.segments[..], [segment] if segment.name.text == type_ref::IMPL_TYPE) =>
            {
                let segment = &path.segments[0];
                let mut projections = Vec::new();
                let bounds = segment
                    .arguments
                    .iter()
                    .filter_map(|argument| match argument {
                        type_ref::Argument::Type(type_ref::Type::Path(bound)) => {
                            // `=Shape=Output<i64>`: a projection.
                            if let [binding] = &bound.segments[..]
                                && let Some(rest) =
                                    binding.name.text.strip_prefix(type_ref::BINDING_PREFIX)
                                && let Some((owner, name)) = rest.split_once('=')
                                && let [type_ref::Argument::Type(ty)] = &binding.arguments[..]
                            {
                                let named = |text: &str| crate::item::Name {
                                    text: text.into(),
                                    span: binding.name.span,
                                };
                                projections.push((named(owner), named(name), ty.clone()));
                                return None;
                            }
                            Some(bound.clone())
                        }
                        _ => None,
                    })
                    .collect::<Box<[_]>>();
                let base = bounds
                    .iter()
                    .map(|bound| bound.segments.last().map_or("", |last| last.name.text.as_str()))
                    .collect::<Vec<_>>()
                    .join("_");
                let mut text = format!("impl_{base}");
                let mut index = 1;
                while generics.iter().any(|generic| generic.name.text == text) {
                    index += 1;
                    text = format!("impl_{base}{index}");
                }
                let name = crate::item::Name {
                    text: text.as_str().into(),
                    span: segment.name.span,
                };
                generics.push(crate::item::GenericParam {
                    name: name.clone(),
                    bounds,
                    projections: projections.into_boxed_slice(),
                    default: None,
                });
                *ty = type_ref::Type::Path(type_ref::Path {
                    span: path.span,
                    segments: Box::new([type_ref::Segment {
                        name,
                        arguments: Box::default(),
                        span: path.span,
                    }]),
                });
            }
            type_ref::Type::Path(path) => {
                for segment in path.segments.iter_mut() {
                    for argument in segment.arguments.iter_mut() {
                        if let type_ref::Argument::Type(ty) = argument {
                            lift(ty, generics);
                        }
                    }
                }
            }
            type_ref::Type::Reference { target, .. } => lift(target, generics),
            type_ref::Type::Tuple { members, .. } => {
                for member in members.iter_mut() {
                    lift(member, generics);
                }
            }
            type_ref::Type::Unit(_) => {}
        }
    }
    for parameter in parameters {
        lift(&mut parameter.ty, generics);
    }
}

/// The builtin type name of an iterator trait: `Iterator` and its
/// `DoubleEndedIterator`, `ExactSizeIterator` kin, or `IntoIterator`.
fn iterator_trait(name: &str) -> Option<&'static str> {
    match name {
        "Iterator" | "DoubleEndedIterator" | "ExactSizeIterator" => Some(type_ref::ITER_TYPE),
        "IntoIterator" => Some(type_ref::INTO_ITER_TYPE),
        _ => None,
    }
}

/// `<Item = T>` after `impl Iterator`, then any `+ 'a` or auto traits: the
/// builtin iterator type of `T`.
fn parse_iterator_type(
    input: &mut TokenStream,
    split: &mut Option<Position>,
    name: &'static str,
    start: Position,
) -> ParseResult<type_ref::Type> {
    expect(
        input,
        Token::LessThan,
        "after an iterator trait (`<Item = T>`)",
    )?;
    let item_name = parse_name(input, "as `Item` of an iterator trait")?;
    if item_name.text != "Item" {
        return Err(PERR::MissingToken(
            "Item".into(),
            "in an iterator trait's `<Item = T>`".into(),
        )
        .into_err(item_name.span.start()));
    }
    expect(input, Token::Equals, "after `Item` of an iterator trait")?;
    let mut inner = None;
    let item = parse_type_split(input, &mut inner)?;
    let end = match (inner, input.peek()) {
        (Some(position), _) => after(position, 1),
        (None, Some((Token::GreaterThan, position))) => {
            let position = *position;
            input.next();
            after(position, 1)
        }
        // `Option<impl Iterator<Item = i64>>`: the second `>` closes the
        // enclosing list.
        (None, Some((Token::RightShift, position))) => {
            let position = *position;
            input.next();
            *split = Some(after(position, 1));
            after(position, 1)
        }
        (None, _) => {
            expect(
                input,
                Token::GreaterThan,
                "to close an iterator trait's `<Item = T>`",
            )?;
            unreachable!("`expect` fails before any other token");
        }
    };
    let mut span = Span::new(start, end);
    while split.is_none() && matches!(input.peek(), Some((Token::Plus | Token::UnaryPlus, _))) {
        input.next();
        if matches!(input.peek(), Some((Token::Apostrophe, _))) {
            let lifetime = parse_lifetime(input)?;
            span = Span::new(start, lifetime.span.end());
            continue;
        }
        if let Some(end) = skip_precise_capture(input)? {
            span = Span::new(start, end);
            continue;
        }
        let extra = parse_path_split(input, split)?;
        if !is_auto_trait(&extra) {
            return Err(PERR::BadInput(crate::LexError::ImproperSymbol(
                "+".into(),
                "only lifetimes and auto traits can be added to `impl Iterator`".into(),
            ))
            .into_err(extra.span.start()));
        }
        span = Span::new(start, extra.span.end());
    }
    Ok(iterator_type(name, item, span))
}

/// The builtin iterator type `name` of `item`.
fn iterator_type(name: &str, item: type_ref::Type, span: Span) -> type_ref::Type {
    type_ref::Type::Path(type_ref::Path {
        segments: Box::new([type_ref::Segment {
            name: crate::item::Name {
                text: name.into(),
                span,
            },
            arguments: Box::new([type_ref::Argument::Type(item)]),
            span,
        }]),
        span,
    })
}

/// `Fn`, `FnMut` or `FnOnce`: Rawr's function values implement all three.
fn is_fn_trait(name: &str) -> bool {
    matches!(name, "Fn" | "FnMut" | "FnOnce")
}

/// `(A, B) -> C` after a function type's head: the builtin path `fn` whose
/// arguments are the inputs, then the output (`()` when omitted).
fn parse_fn_signature(input: &mut TokenStream, start: Position) -> ParseResult<type_ref::Path> {
    let mut arguments = Vec::new();
    let mut end = if let Some((Token::Unit, position)) = input.peek() {
        let position = *position;
        input.next();
        after(position, 2)
    } else {
        expect(input, Token::LeftParen, "after `fn` in a function type")?;
        while !matches!(input.peek(), Some((Token::RightParen, _))) {
            arguments.push(type_ref::Argument::Type(parse_type(input)?));
            consume_comma_or_end(input, Token::RightParen, "between function type inputs")?;
        }
        after(
            expect(input, Token::RightParen, "to close function type inputs")?,
            1,
        )
    };
    let output = if matches!(input.peek(), Some((Token::ThinArrow, _))) {
        input.next();
        let output = parse_type(input)?;
        end = output.span().end();
        output
    } else {
        type_ref::Type::Unit(Span::new(start, end))
    };
    arguments.push(type_ref::Argument::Type(output));
    let span = Span::new(start, end);
    Ok(type_ref::Path {
        segments: Box::new([type_ref::Segment {
            name: crate::item::Name {
                text: type_ref::FN_TYPE.into(),
                span: token_span(start, "fn"),
            },
            arguments: arguments.into_boxed_slice(),
            span,
        }]),
        span,
    })
}

/// A generic parameter bounded by `Fn(A) -> B` is that function type, as
/// Rawr has one function type: each use of the parameter in the signature
/// is replaced by it, and the parameter is dropped.
#[cfg(not(feature = "no_function"))]
fn erase_fn_bounds(
    generics: &mut Vec<crate::item::GenericParam>,
    parameters: &mut [crate::item::Parameter],
    output: &mut Option<type_ref::Type>,
) {
    generics.retain(|generic| {
        // `I: Iterator<Item = T>` (or `IntoIterator`): `impl Iterator<Item =
        // T>` wherever `I` is written, and `T` for `I::Item`.
        let iterator = generic.bounds.iter().find_map(|bound| {
            let (last, prefix) = bound.segments.split_last()?;
            let name = iterator_trait(&last.name.text)?;
            let modules = prefix
                .iter()
                .all(|module| matches!(module.name.text.as_str(), "std" | "core" | "iter"));
            let (_, _, item) = generic.projections.iter().find(|(trait_name, item, _)| {
                trait_name.text == last.name.text && item.text == "Item"
            })?;
            modules.then(|| iterator_type(name, item.clone(), bound.span))
        });
        if let Some(replacement) = iterator {
            let type_ref::Type::Path(path) = &replacement else {
                unreachable!("an iterator type is a path");
            };
            let [type_ref::Argument::Type(item)] = &path.segments[0].arguments[..] else {
                unreachable!("an iterator type has its item");
            };
            for ty in parameters
                .iter_mut()
                .map(|parameter| &mut parameter.ty)
                .chain(output.iter_mut())
            {
                replace_item_projection(ty, &generic.name.text, item);
                replace_named(ty, &generic.name.text, &replacement);
            }
            return false;
        }
        let Some(bound) = generic.bounds.iter().find(|bound| {
            matches!(&bound.segments[..], [segment] if segment.name.text.as_str() == type_ref::FN_TYPE)
        }) else {
            return true;
        };
        let replacement = type_ref::Type::Path(bound.clone());
        for parameter in parameters.iter_mut() {
            replace_named(&mut parameter.ty, &generic.name.text, &replacement);
        }
        if let Some(output) = output {
            replace_named(output, &generic.name.text, &replacement);
        }
        false
    });
}

/// Replace each path `name::Item` in `ty` with `item`.
#[cfg(not(feature = "no_function"))]
fn replace_item_projection(ty: &mut type_ref::Type, name: &str, item: &type_ref::Type) {
    match ty {
        type_ref::Type::Path(path)
            if matches!(&path.segments[..], [base, member]
                if base.name.text.as_str() == name
                    && base.arguments.is_empty()
                    && member.name.text.as_str() == "Item"
                    && member.arguments.is_empty()) =>
        {
            *ty = item.clone();
        }
        type_ref::Type::Path(path) => {
            for segment in path.segments.iter_mut() {
                for argument in segment.arguments.iter_mut() {
                    if let type_ref::Argument::Type(argument) = argument {
                        replace_item_projection(argument, name, item);
                    }
                }
            }
        }
        type_ref::Type::Tuple { members, .. } => {
            for member in members.iter_mut() {
                replace_item_projection(member, name, item);
            }
        }
        type_ref::Type::Reference { target, .. } => replace_item_projection(target, name, item),
        type_ref::Type::Unit(_) => {}
    }
}

/// Replace each bare path `name` in `ty` with `replacement`.
#[cfg(not(feature = "no_function"))]
fn replace_named(ty: &mut type_ref::Type, name: &str, replacement: &type_ref::Type) {
    match ty {
        type_ref::Type::Path(path)
            if path.segments.len() == 1
                && path.segments[0].name.text.as_str() == name
                && path.segments[0].arguments.is_empty() =>
        {
            *ty = replacement.clone();
        }
        type_ref::Type::Path(path) => {
            for segment in path.segments.iter_mut() {
                for argument in segment.arguments.iter_mut() {
                    if let type_ref::Argument::Type(argument) = argument {
                        replace_named(argument, name, replacement);
                    }
                }
            }
        }
        type_ref::Type::Tuple { members, .. } => {
            for member in members.iter_mut() {
                replace_named(member, name, replacement);
            }
        }
        type_ref::Type::Reference { target, .. } => replace_named(target, name, replacement),
        type_ref::Type::Unit(_) => {}
    }
}

fn parse_path(input: &mut TokenStream) -> ParseResult<type_ref::Path> {
    let mut split = None;
    let path = parse_path_split(input, &mut split)?;
    reject_split(split)?;
    Ok(path)
}

fn parse_path_split(
    input: &mut TokenStream,
    split: &mut Option<Position>,
) -> ParseResult<type_ref::Path> {
    parse_path_bindings(input, split, None)
}

/// A path whose generic arguments may bind associated types
/// (`Shape<Output = i64>`) when `bindings` takes them.
fn parse_path_bindings(
    input: &mut TokenStream,
    split: &mut Option<Position>,
    mut bindings: Option<&mut Vec<(crate::item::Name, type_ref::Type)>>,
) -> ParseResult<type_ref::Path> {
    let mut segments = Vec::new();

    loop {
        let name = parse_use_segment(input, "as a type path segment")?;
        let mut end = name.span.end();
        let mut arguments = Vec::new();

        if matches!(input.peek(), Some((Token::LessThan, _))) {
            input.next();

            // The second `>` of a `>>` closing a nested list closes this one.
            let mut nested = None;
            while !matches!(
                input.peek(),
                Some((Token::GreaterThan | Token::RightShift, _))
            ) {
                if matches!(input.peek(), Some((Token::Apostrophe, _))) {
                    arguments.push(type_ref::Argument::Lifetime(parse_lifetime(input)?));
                } else {
                    let argument = parse_type_split(input, &mut nested)?;
                    if let Some(bindings) = bindings.as_deref_mut()
                        && nested.is_none()
                        && matches!(input.peek(), Some((Token::Equals, _)))
                        && let type_ref::Type::Path(path) = &argument
                        && let [segment] = &path.segments[..]
                        && segment.arguments.is_empty()
                    {
                        input.next();
                        let bound = parse_type_split(input, &mut nested)?;
                        bindings.push((segment.name.clone(), bound));
                    } else {
                        arguments.push(type_ref::Argument::Type(argument));
                    }
                }
                if nested.is_some() || matches!(input.peek(), Some((Token::RightShift, _))) {
                    break;
                }

                consume_comma_or_end(input, Token::GreaterThan, "between generic arguments")?;
            }

            let close = match nested.take() {
                Some(close) => close,
                None if matches!(input.peek(), Some((Token::RightShift, _))) => {
                    let (_, close) = input.next().expect("token streams always end with EOF");
                    *split = Some(after(close, 1));
                    close
                }
                None => expect(input, Token::GreaterThan, "to close generic arguments")?,
            };
            end = after(close, 1);
        }

        let segment_span = Span::new(name.span.start(), end);
        segments.push(type_ref::Segment {
            name,
            arguments: arguments.into_boxed_slice(),
            span: segment_span,
        });

        if !matches!(input.peek(), Some((Token::DoubleColon, _))) {
            break;
        }

        input.next();
    }

    let first = segments
        .first()
        .expect("type paths contain at least one segment")
        .span;
    let last = segments
        .last()
        .expect("type paths contain at least one segment")
        .span;

    Ok(type_ref::Path {
        segments: segments.into_boxed_slice(),
        span: Span::new(first.start(), last.end()),
    })
}

fn parse_lifetime(input: &mut TokenStream) -> ParseResult<type_ref::Lifetime> {
    let apostrophe = expect(input, Token::Apostrophe, "to start a lifetime")?;
    let name = match input.next().expect("token streams always end with EOF") {
        (Token::Identifier(name), position) => crate::item::Name {
            span: token_span(position, &name),
            text: (*name).into(),
        },
        (Token::Underscore, position) => crate::item::Name {
            span: token_span(position, "_"),
            text: "_".into(),
        },
        (Token::Reserved(name), position) if name.as_str() == "static" => crate::item::Name {
            span: token_span(position, &name),
            text: (*name).into(),
        },
        (Token::LexError(error), position) => return Err(error.into_err(position)),
        (_, position) => {
            return Err(
                PERR::MissingToken("lifetime name".into(), "after '\''".into()).into_err(position),
            );
        }
    };

    Ok(type_ref::Lifetime {
        span: Span::new(apostrophe, name.span.end()),
        name,
    })
}

#[cfg(not(feature = "no_function"))]
fn parse_body_syntax(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    open: Position,
) -> ParseResult<(Span, crate::item::BodySyntax)> {
    let mut scan = ScanState::inside_body();
    let mut tokens = Vec::new();
    // Whether a statement may start here: a nested `fn` item.
    let mut boundary = true;
    // Each open parenthesis: the callback roles of an `iced::run(..)` or
    // `iced::application(..)` call it opens, and the argument it is at.
    let mut calls: Vec<Option<(&'static [&'static str], usize)>> = Vec::new();

    loop {
        #[cfg(not(feature = "no_std"))]
        if let Some((Token::Pipe, _)) = input.peek()
            && let Some(Some((roles, index))) = calls.last()
            && let Some(role) = roles.get(*index)
            && tokens.last().is_some_and(|token: &crate::item::BodyToken| {
                matches!(
                    token.kind(),
                    crate::item::BodyTokenKind::LeftParen | crate::item::BodyTokenKind::Comma
                )
            })
        {
            let function = parse_callback_closure(input, tokenizer_control, role)?;
            tokens.push(crate::item::BodyToken::new(
                crate::item::BodyTokenKind::Identifier(function.name.text.clone()),
                function.span,
            ));
            NESTED.with_borrow_mut(|nested| nested.push(crate::item::Parsed::Function(function)));
            continue;
        }
        #[cfg(not(feature = "no_std"))]
        if boundary && let Some((Token::Fn, position)) = input.peek() {
            let position = *position;
            let function =
                parse_function(input, tokenizer_control, Box::default(), None, position, false)?;
            NESTED.with_borrow_mut(|nested| nested.push(crate::item::Parsed::Function(function)));
            continue;
        }
        #[cfg(not(feature = "no_std"))]
        if boundary && let Some((Token::Use, position)) = input.peek() {
            let position = *position;
            let used = parse_use(input, Box::default(), None, position)?;
            NESTED.with_borrow_mut(|nested| nested.push(crate::item::Parsed::Use(used)));
            continue;
        }
        if let Some((token, _)) = input.peek() {
            boundary = matches!(token, Token::LeftBrace | Token::RightBrace | Token::SemiColon);
        }
        match scan
            .next(input, tokenizer_control)
            .expect("token streams always end with EOF")
        {
            (Token::RightBrace, position) if scan.delimiter_depth == 0 => {
                return Ok((
                    Span::new(open, after(position, 1)),
                    crate::item::BodySyntax::new(tokens),
                ));
            }
            (Token::EOF, position) => {
                return Err(PERR::MissingToken(
                    Token::RightBrace.into(),
                    "to close a function body".into(),
                )
                .into_err(position));
            }
            (Token::LexError(error), position) => return Err(error.into_err(position)),
            (token, position) => {
                match token {
                    Token::LeftParen => {
                        let roles: Option<&'static [&'static str]> = match &tokens[..] {
                            [.., iced, colons, name]
                                if matches!(iced.kind(),
                                    crate::item::BodyTokenKind::Identifier(text) if text == "iced")
                                    && matches!(
                                        colons.kind(),
                                        crate::item::BodyTokenKind::DoubleColon
                                    ) =>
                            {
                                match name.kind() {
                                    crate::item::BodyTokenKind::Identifier(text)
                                        if text == "run" =>
                                    {
                                        Some(&["update", "view"])
                                    }
                                    crate::item::BodyTokenKind::Identifier(text)
                                        if text == "application" =>
                                    {
                                        Some(&["boot", "update", "view"])
                                    }
                                    _ => None,
                                }
                            }
                            _ => None,
                        };
                        calls.push(roles.map(|roles| (roles, 0)));
                    }
                    Token::Unit => {}
                    Token::RightParen => {
                        calls.pop();
                    }
                    Token::Comma => {
                        if let Some(Some((_, index))) = calls.last_mut() {
                            *index += 1;
                        }
                    }
                    _ => {}
                }
                tokens.push(body_token(token, position));
            }
        }
    }
}

/// `|state: &mut State, message: Message| body`, a callback of
/// `iced::run(..)` or `iced::application(..)`: a typed function of its
/// parameters, which the application entry names in its place. Its output
/// is the closure's `-> T`, or the unit for an `update`; a `view`'s is
/// `impl Widget<Message>` of the update's Message, which the entry sets.
#[cfg(not(feature = "no_std"))]
fn parse_callback_closure(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    role: &str,
) -> ParseResult<crate::item::Function> {
    let start = expect(input, Token::Pipe, "to start a closure")?;
    let mut parameters = Vec::new();
    while !matches!(input.peek(), Some((Token::Pipe, _))) {
        let name = parse_name(input, "as a closure parameter")?;
        if !matches!(input.peek(), Some((Token::Colon, _))) {
            return Err(PERR::MissingToken(
                Token::Colon.into(),
                format!(
                    "and a type after the {role} closure's parameter `{}`, since Rawr reads \
                     an application callback closure as a function with typed parameters",
                    name.text
                ),
            )
            .into_err(after(name.span.start(), name.text.chars().count())));
        }
        input.next();
        let ty = parse_type(input)?;
        let span = Span::new(name.span.start(), ty.span().end());
        parameters.push(crate::item::Parameter {
            name,
            ty,
            span,
            pattern: None,
        });
        consume_comma_or_end(input, Token::Pipe, "between closure parameters")?;
    }
    expect(input, Token::Pipe, "to end closure parameters")?;
    let output = if matches!(input.peek(), Some((Token::ThinArrow, _))) {
        input.next();
        Some(parse_type(input)?)
    } else {
        None
    };
    let (body_span, body_syntax, constant) =
        if let Some((Token::LeftBrace, open)) = input.peek() {
            let open = *open;
            input.next();
            let (span, syntax) = parse_body_syntax(input, tokenizer_control, open)?;
            (span, syntax, false)
        } else {
            // An expression body runs to the argument's end, as a `const`
            // initializer runs to its `;`.
            let body = input.peek().map_or(start, |(_, position)| *position);
            let mut scan = ScanState::default();
            let mut tokens = Vec::new();
            let end = loop {
                match input.peek().expect("token streams always end with EOF") {
                    (Token::Comma | Token::RightParen, position) if scan.delimiter_depth == 0 => {
                        break *position;
                    }
                    (Token::EOF, position) => {
                        return Err(PERR::MissingToken(
                            Token::RightParen.into(),
                            "to end the application call".into(),
                        )
                        .into_err(*position));
                    }
                    _ => {}
                }
                match scan
                    .next(input, tokenizer_control)
                    .expect("token streams always end with EOF")
                {
                    (Token::LexError(error), position) => return Err(error.into_err(position)),
                    (token, position) => tokens.push(body_token(token, position)),
                }
            };
            (
                Span::new(body, end),
                crate::item::BodySyntax::new(tokens),
                true,
            )
        };
    let span = Span::new(start, body_span.end());
    Ok(crate::item::Function {
        header: crate::item::Header {
            attributes: Box::default(),
            visibility: None,
            span: Span::new(start, body_span.start()),
        },
        name: crate::item::Name {
            text: format!("{CALLBACK_CLOSURE_PREFIX}{role}").into(),
            span: Span::new(start, after(start, 1)),
        },
        generics: Box::default(),
        receiver: None,
        parameters: parameters.into_boxed_slice(),
        output: output.or_else(|| {
            (role == "update").then(|| type_ref::Type::Unit(Span::new(start, start)))
        }),
        body_span,
        body_syntax,
        constant,
        span,
    })
}

/// The name prefix of a function read from an application callback closure.
pub(crate) const CALLBACK_CLOSURE_PREFIX: &str = "__rawr_closure_";

#[cfg(not(feature = "no_function"))]
/// A private top-level `fn` is a typed item when its signature is typed
/// (`fn f(x: T)` or `fn f() -> T`), as in Rust; otherwise it stays a legacy
/// untyped function.
fn parse_private_function(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
) -> ParseResult<Option<crate::item::Parsed>> {
    let start = expect(input, Token::Fn, "to start a private function")?;
    if !input.peek().is_some_and(|(token, _)| is_name(token)) {
        return Ok(None);
    }
    let name = parse_name(input, "after 'fn'")?;
    let header = crate::item::Header {
        attributes: Box::default(),
        visibility: None,
        span: Span::NONE,
    };
    let typed = |function| Ok(Some(crate::item::Parsed::Function(function)));
    // `fn name<T>(..)` is Rust, never legacy.
    if matches!(input.peek(), Some((Token::LessThan, _))) {
        let generics = parse_generic_params(input)?;
        let open = if matches!(input.peek(), Some((Token::Unit, _))) {
            input.next();
            None
        } else {
            expect(input, Token::LeftParen, "after generic parameters")?;
            Some(None)
        };
        return typed(parse_function_rest(
            input,
            tokenizer_control,
            header,
            start,
            name,
            generics,
            open,
            false,
        )?);
    }
    let first = if matches!(input.peek(), Some((Token::Unit, _))) {
        input.next();
        if matches!(input.peek(), Some((Token::ThinArrow, _))) {
            return typed(parse_function_rest(
                input,
                tokenizer_control,
                header,
                start,
                name,
                Vec::new(),
                None,
                false,
            )?);
        }
        None
    } else {
        expect(input, Token::LeftParen, "after a private function name")?;
        // A first parameter written as a pattern is Rust, never legacy.
        if name.text != "main"
            && matches!(
                input.peek(),
                Some((
                    Token::LeftParen
                        | Token::Ampersand
                        | Token::And
                        | Token::Mut
                        | Token::Underscore,
                    _
                ))
            )
        {
            return typed(parse_function_rest(
                input,
                tokenizer_control,
                header,
                start,
                name,
                Vec::new(),
                Some(None),
                false,
            )?);
        }
        if input.peek().is_some_and(|(token, _)| is_name(token)) {
            let first = parse_name(input, "as a function parameter")?;
            // `fn f(x: T)` or `fn f(W(x): W)`.
            if name.text != "main"
                && matches!(
                    input.peek(),
                    Some((Token::Colon | Token::LeftParen | Token::DoubleColon, _))
                )
            {
                return typed(parse_function_rest(
                    input,
                    tokenizer_control,
                    header,
                    start,
                    name,
                    Vec::new(),
                    Some(Some(first)),
                    false,
                )?);
            }
            Some(true)
        } else {
            Some(false)
        }
    };
    parse_legacy_function_outline(input, tokenizer_control, start, name, first)
}

#[cfg(not(feature = "no_function"))]
fn parse_legacy_function_outline(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    start: Position,
    name: crate::item::Name,
    open: Option<bool>,
) -> ParseResult<Option<crate::item::Parsed>> {
    let parameter_count = if let Some(has_parameter) = open {
        let mut depth = 1_usize;
        let mut count = 0_usize;
        let mut has_parameter = has_parameter;

        while depth > 0 {
            match input.next().expect("token streams always end with EOF") {
                (Token::LeftParen, _) => {
                    depth += 1;
                    has_parameter = true;
                }
                (Token::RightParen, _) => depth -= 1,
                (Token::Comma, _) if depth == 1 => {
                    count += usize::from(has_parameter);
                    has_parameter = false;
                }
                (Token::EOF, position) => {
                    return Err(PERR::MissingToken(
                        Token::RightParen.into(),
                        "to close private function parameters".into(),
                    )
                    .into_err(position));
                }
                (Token::LexError(error), position) => return Err(error.into_err(position)),
                _ if depth == 1 => has_parameter = true,
                _ => {}
            }
        }
        count + usize::from(has_parameter)
    } else {
        0
    };

    let open = loop {
        match input.next().expect("token streams always end with EOF") {
            (Token::LeftBrace, position) => break position,
            (Token::EOF, position) => {
                return Err(PERR::MissingToken(
                    Token::LeftBrace.into(),
                    "to start a private function body".into(),
                )
                .into_err(position));
            }
            (Token::LexError(error), position) => return Err(error.into_err(position)),
            _ => {}
        }
    };
    let (body_span, body_syntax) = parse_body_syntax(input, tokenizer_control, open)?;

    let attempts_application = attempts_iced_application(&body_syntax);
    if name.text != "main" && !attempts_application {
        return Ok(None);
    }
    // Rust's `fn main() { .. }`: a function returning `()`. One that starts
    // an iced application stays an outline for the entry diagnostic.
    if parameter_count == 0 && !attempts_application {
        let span = Span::new(start, body_span.end());
        return Ok(Some(crate::item::Parsed::Function(crate::item::Function {
            header: crate::item::Header {
                attributes: Box::default(),
                visibility: None,
                span: Span::new(start, after(open, 1)),
            },
            name,
            generics: Box::default(),
            receiver: None,
            parameters: Box::default(),
            output: None,
            body_span,
            body_syntax,
            constant: false,
            span,
        })));
    }

    Ok(Some(crate::item::Parsed::LegacyFunction(
        crate::item::LegacyFunction {
            name,
            parameter_count,
            body_span,
            body_syntax,
            span: Span::new(start, body_span.end()),
        },
    )))
}

#[cfg(not(feature = "no_function"))]
fn attempts_iced_application(syntax: &crate::item::BodySyntax) -> bool {
    syntax.tokens().windows(3).any(|tokens| {
        matches!(
            tokens[0].kind(),
            crate::item::BodyTokenKind::Identifier(name) if name == "iced"
        ) && matches!(tokens[1].kind(), crate::item::BodyTokenKind::DoubleColon)
            && matches!(
                tokens[2].kind(),
                crate::item::BodyTokenKind::Identifier(name)
                    if name == "run" || name == "application"
            )
    })
}

#[cfg(not(feature = "no_function"))]
fn body_token(token: Token, position: Position) -> crate::item::BodyToken {
    use crate::item::BodyTokenKind as Kind;

    let (kind, width) = match token {
        Token::Identifier(name) => {
            let width = name.chars().count();
            (Kind::Identifier((*name).into()), width)
        }
        Token::Reserved(keyword) if keyword.as_str() == "super" => {
            (Kind::Identifier("super".into()), 5)
        }
        Token::DoubleColon => (Kind::DoubleColon, 2),
        Token::DoubleArrow => (Kind::DoubleArrow, 2),
        Token::Reserved(keyword) if keyword.as_str() == "match" => (Kind::Match, 5),
        Token::LeftParen => (Kind::LeftParen, 1),
        Token::RightParen => (Kind::RightParen, 1),
        Token::Unit => (Kind::Unit, 2),
        Token::Comma => (Kind::Comma, 1),
        Token::Period => (Kind::Dot, 1),
        Token::LeftBrace => (Kind::LeftBrace, 1),
        Token::RightBrace => (Kind::RightBrace, 1),
        Token::LeftBracket | Token::MapStart => (Kind::LeftBracket, 1),
        Token::RightBracket => (Kind::RightBracket, 1),
        Token::SemiColon => (Kind::Semicolon, 1),
        Token::If => (Kind::If, 2),
        Token::Or => (Kind::OrOr, 2),
        Token::Bang => (Kind::Bang, 1),
        ref word if let Some(name) = crate::tokenizer::rust_name(word) => {
            let width = name.len() - "__rawr_".len();
            (Kind::Identifier(name.into()), width)
        }
        Token::IntegerConstant(value) => (Kind::Integer(value), 0),
        _ => (Kind::Other, 0),
    };

    crate::item::BodyToken::new(kind, Span::new(position, after(position, width)))
}

fn consume_comma_or_end(input: &mut TokenStream, end: Token, context: &str) -> ParseResult<()> {
    match input.peek().expect("token streams always end with EOF") {
        (Token::Comma, _) => {
            input.next();
            Ok(())
        }
        (token, _) if token == &end => Ok(()),
        (_, position) => {
            Err(PERR::MissingToken(Token::Comma.into(), context.into()).into_err(*position))
        }
    }
}

/// `as name` after an import path or group member.
fn parse_use_alias(input: &mut TokenStream) -> ParseResult<Option<crate::item::Name>> {
    if !matches!(input.peek(), Some((Token::As, _))) {
        return Ok(None);
    }
    input.next();
    // `use Trait as _;` brings the trait's methods in scope and binds no
    // name: a name no code can write.
    if let Some((Token::Underscore, position)) = input.peek() {
        let position = *position;
        input.next();
        return Ok(Some(crate::item::Name {
            span: token_span(position, "_"),
            text: format!("_ {position:?}").into(),
        }));
    }
    parse_name(input, "after `as` in an import").map(Some)
}

/// The rest of an import tree inside a group after its member path
/// `prefix`: `::name`, `::{..}` or `::*` again, then an optional `as`.
/// Each complete path, group or glob joins `trees`.
fn parse_use_subtree(
    input: &mut TokenStream,
    mut segments: Vec<type_ref::Segment>,
    trees: &mut Vec<crate::item::UseTree>,
) -> ParseResult<()> {
    let path_of = |segments: &[type_ref::Segment]| {
        let span = Span::new(
            segments.first().expect("a path segment").span.start(),
            segments.last().expect("a path segment").span.end(),
        );
        type_ref::Path {
            segments: segments.to_vec().into_boxed_slice(),
            span,
        }
    };
    while matches!(input.peek(), Some((Token::DoubleColon, _))) {
        input.next();
        if let Some((Token::Multiply, position)) = input.peek() {
            let star = token_span(*position, "*");
            input.next();
            let prefix = path_of(&segments);
            trees.push(crate::item::UseTree::Glob {
                span: Span::new(prefix.span.start(), star.end()),
                prefix,
            });
            return Ok(());
        }
        if matches!(input.peek(), Some((Token::LeftBrace, _))) {
            let open = expect(input, Token::LeftBrace, "to start an import group")?;
            let (members, close) = parse_use_members(input, &segments, trees)?;
            if !members.is_empty() {
                trees.push(crate::item::UseTree::Group {
                    prefix: path_of(&segments),
                    members: members.into_boxed_slice(),
                    span: Span::new(open, after(close, 1)),
                });
            }
            return Ok(());
        }
        let name = parse_use_segment(input, "after '::' in an import path")?;
        segments.push(type_ref::Segment {
            span: name.span,
            name,
            arguments: Box::default(),
        });
    }
    trees.push(crate::item::UseTree::Path(path_of(&segments), parse_use_alias(input)?));
    Ok(())
}

/// The members of an import group after its `{`, through its `}`: each
/// `name` or `self` (renamed or not) of `prefix`; a member path that goes on
/// (`b::{c, d}`, `b::*`) joins `trees` whole.
fn parse_use_members(
    input: &mut TokenStream,
    prefix: &[type_ref::Segment],
    trees: &mut Vec<crate::item::UseTree>,
) -> ParseResult<(Vec<crate::item::UseMember>, Position)> {
    let mut members = Vec::new();
    while !matches!(input.peek(), Some((Token::RightBrace, _))) {
        // `use a::{self, *}`: a glob of the group's prefix.
        if let Some((Token::Multiply, position)) = input.peek() {
            let star = token_span(*position, "*");
            input.next();
            let span = Span::new(
                prefix.first().expect("a path segment").span.start(),
                prefix.last().expect("a path segment").span.end(),
            );
            trees.push(crate::item::UseTree::Glob {
                span: Span::new(span.start(), star.end()),
                prefix: type_ref::Path {
                    segments: prefix.to_vec().into_boxed_slice(),
                    span,
                },
            });
            match input.peek().expect("token streams always end with EOF") {
                (Token::Comma, _) => {
                    input.next();
                    continue;
                }
                (Token::RightBrace, _) => break,
                (_, position) => {
                    return Err(PERR::MissingToken(
                        Token::Comma.into(),
                        "between grouped imports".into(),
                    )
                    .into_err(*position));
                }
            }
        }
        let name = match input.peek() {
            Some((Token::SelfValue, position)) => {
                let span = token_span(*position, "self");
                input.next();
                crate::item::Name {
                    span,
                    text: "self".into(),
                }
            }
            _ => parse_use_segment(input, "inside an import group")?,
        };
        if matches!(input.peek(), Some((Token::DoubleColon, _))) {
            let mut segments = prefix.to_vec();
            segments.push(type_ref::Segment {
                span: name.span,
                name,
                arguments: Box::default(),
            });
            parse_use_subtree(input, segments, trees)?;
        } else {
            let alias = parse_use_alias(input)?;
            members.push(crate::item::UseMember { name, alias });
        }
        match input.peek().expect("token streams always end with EOF") {
            (Token::Comma, _) => {
                input.next();
            }
            (Token::RightBrace, _) => break,
            (_, position) => {
                return Err(PERR::MissingToken(
                    Token::Comma.into(),
                    "between grouped imports".into(),
                )
                .into_err(*position));
            }
        }
    }
    let close = expect(input, Token::RightBrace, "to close an import group")?;
    Ok((members, close))
}

/// An import path segment: a name, or `super` for the parent module.
fn parse_use_segment(input: &mut TokenStream, context: &str) -> ParseResult<crate::item::Name> {
    if let Some((Token::Reserved(keyword), position)) = input.peek()
        && keyword.as_str() == "super"
    {
        let span = token_span(*position, "super");
        input.next();
        return Ok(crate::item::Name {
            span,
            text: "super".into(),
        });
    }
    parse_name(input, context)
}

/// Whether `token` is a name: an identifier, or a Rust name the legacy
/// lexer reads as a keyword (`tokenizer::rust_name`).
fn is_name(token: &Token) -> bool {
    matches!(token, Token::Identifier(_)) || crate::tokenizer::rust_name(token).is_some()
}

fn parse_name(input: &mut TokenStream, context: &str) -> ParseResult<crate::item::Name> {
    match input.next().expect("token streams always end with EOF") {
        (Token::Identifier(name), position) => Ok(crate::item::Name {
            span: token_span(position, &name),
            text: (*name).into(),
        }),
        // `switch`, `debug` and kin are legacy keywords but Rust names.
        (ref word, position) if let Some(name) = crate::tokenizer::rust_name(word) => {
            Ok(crate::item::Name {
                span: token_span(position, &name["__rawr_".len()..]),
                text: name.into(),
            })
        }
        (Token::LexError(error), position) => Err(error.into_err(position)),
        (_, position) => {
            Err(PERR::MissingToken("identifier".into(), context.into()).into_err(position))
        }
    }
}

fn expect(input: &mut TokenStream, expected: Token, context: &str) -> ParseResult<Position> {
    match input.next().expect("token streams always end with EOF") {
        (token, position) if token == expected => Ok(position),
        (Token::LexError(error), position) => Err(error.into_err(position)),
        (_, position) => {
            Err(PERR::MissingToken(expected.into(), context.into()).into_err(position))
        }
    }
}

fn token_span(start: Position, text: &str) -> Span {
    Span::new(start, after(start, text.chars().count()))
}

fn after(mut position: Position, width: usize) -> Position {
    for _ in 0..width {
        position.advance();
    }
    position
}

#[cfg(test)]
mod tests {
    use crate::item::{type_ref, Parsed, Receiver, UseTree};
    use crate::{Engine, ImmutableString, Position};

    #[test]
    fn parses_public_file_module_declarations() {
        let unit = Engine::new().parse_unit("pub mod controls;").unwrap();
        let Parsed::Module(module) = &unit.items()[0] else {
            panic!("expected a file module item")
        };

        assert_eq!(module.name.text, "controls");
        assert!(module.header.visibility.is_some());
        assert_eq!(module.span.start(), Position::new(1, 1));
        assert_eq!(module.span.end(), Position::new(1, 18));
    }

    #[test]
    fn parses_positioned_counter_item_headers() {
        let engine = Engine::new();
        let source = r#"
use iced::Center;
use iced::widget::{Column, button, column, text};

#[derive(Default)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
    Changed(i64),
}

impl Counter {
    fn update(&mut self, message: Message) -> () {
        let ignored = message;
    }

    fn view(&self) -> iced::widget::Column<'_, Message> {
        ()
    }
}

pub fn main() -> iced::Result {
    ()
}

let answer = 40 + 2;
answer
"#;

        let unit = engine.parse_unit(source).unwrap();

        assert_eq!(unit.items().len(), 6);
        assert!(unit.items().iter().all(|item| !item.span().is_none()));

        let Parsed::Use(grouped_use) = &unit.items()[1] else {
            panic!("expected grouped use item");
        };
        let UseTree::Group {
            prefix, members, ..
        } = &grouped_use.tree
        else {
            panic!("expected grouped import tree");
        };
        assert_eq!(prefix.segments.len(), 2);
        assert_eq!(members.len(), 4);
        assert!(members.iter().all(|member| !member.name.span.is_none()));

        let Parsed::Struct(counter) = &unit.items()[2] else {
            panic!("expected struct item");
        };
        assert_eq!(counter.name.text, "Counter");
        assert_eq!(counter.name.span.start().line(), Some(6));
        assert_eq!(counter.name.span.start().position(), Some(8));
        assert_eq!(counter.fields.len(), 1);
        assert_eq!(counter.header.attributes.len(), 1);
        assert!(!counter.header.attributes[0].span.is_none());
        assert!(!counter.fields[0].ty.span().is_none());

        let Parsed::Enum(message) = &unit.items()[3] else {
            panic!("expected enum item");
        };
        assert_eq!(message.variants.len(), 2);
        assert_eq!(message.variants[1].payload.len(), 1);

        let Parsed::Impl(counter_impl) = &unit.items()[4] else {
            panic!("expected impl item");
        };
        assert_eq!(counter_impl.functions.len(), 2);
        assert!(matches!(
            counter_impl.functions[0].receiver,
            Some(Receiver::Mutable(_))
        ));
        assert!(matches!(
            counter_impl.functions[1].receiver,
            Some(Receiver::Shared(_))
        ));
        let Some(type_ref::Type::Path(output)) = &counter_impl.functions[1].output else {
            panic!("expected qualified output path");
        };
        assert_eq!(output.segments.len(), 3);
        assert_eq!(output.segments[2].arguments.len(), 2);
        assert!(output
            .segments
            .iter()
            .all(|segment| !segment.span.is_none()));

        let Parsed::Function(main) = &unit.items()[5] else {
            panic!("expected top-level function item");
        };
        assert_eq!(main.name.text, "main");
        assert!(main.header.visibility.is_some());
        assert!(main.output.is_some());
        assert_eq!(unit.source().text(), source);
    }

    #[test]
    fn parses_bounded_preview_metadata_without_general_attribute_expressions() {
        let source = r#"
#[preview]
#[preview(
    name = "forty two",
    group = "counter",
    seed = Counter::forty_two,
    update = Counter::update,
    themes = [Counter::theme, crate::themes::dark],
    snapshot = false,
    size = (640, 480),
)]
fn view(state: &Counter) -> iced::widget::Column<'_, Message> { () }
"#;
        let unit = Engine::new().parse_unit(source).unwrap();
        let Parsed::Function(function) = &unit.items()[0] else {
            panic!("expected preview function");
        };
        let bare = function.header.attributes[0].preview().unwrap();
        assert!(bare.name.is_none());

        let preview = function.header.attributes[1].preview().unwrap();
        assert_eq!(preview.name.as_ref().unwrap().value, "forty two");
        assert_eq!(preview.group.as_ref().unwrap().value, "counter");
        assert_eq!(preview.seed.as_ref().unwrap().segments.len(), 2);
        assert_eq!(preview.update.as_ref().unwrap().segments.len(), 2);
        assert_eq!(preview.themes.len(), 2);
        assert!(!preview.snapshot.unwrap().value);
        assert_eq!(preview.size.unwrap().width, 640);
        assert_eq!(preview.size.unwrap().height, 480);
        assert!(!preview.span.is_none());

        let unit = Engine::new()
            .parse_unit("#[preview(theme = crate::themes::dark)] fn view() {}")
            .unwrap();
        let Parsed::Function(function) = &unit.items()[0] else {
            panic!("expected preview function");
        };
        let preview = function.header.attributes[0].preview().unwrap();
        assert_eq!(preview.themes.len(), 1, "`theme = x` is `themes = [x]`");
    }

    #[test]
    fn rejects_unbounded_or_duplicate_preview_metadata() {
        for (source, expected) in [
            (
                "#[preview(seed = Counter::new())] fn view() {}",
                "between preview options",
            ),
            ("#[preview(name = label)] fn view() {}", "string literal"),
            ("#[preview(snapshot = 1)] fn view() {}", "boolean literal"),
            (
                "#[preview(size = (0, 10))] fn view() {}",
                "must be a positive u32 literal",
            ),
            (
                "#[preview(themes = [theme()])] fn view() {}",
                "between preview theme variants",
            ),
            (
                "#[preview(name = \"a\", name = \"b\")] fn view() {}",
                "Duplicate preview option",
            ),
            (
                "#[preview(unknown = \"x\")] fn view() {}",
                "Unknown preview option",
            ),
            (
                "#[preview(theme = a::b, themes = [c])] fn view() {}",
                "Duplicate preview option",
            ),
        ] {
            let error = Engine::new().parse_unit(source).unwrap_err();
            assert!(error.to_string().contains(expected), "{source}: {error}");
            assert!(!error.position().is_none(), "{source}: {error}");
        }
    }

    #[test]
    fn ordinary_scripts_and_new_function_remain_unchanged() {
        let engine = Engine::new();
        let ast = engine.compile("fn new() { 40 + 2 } new()").unwrap();

        assert_eq!(engine.eval_ast::<i64>(&ast).unwrap(), 42);
        assert_eq!(engine.eval::<char>("'a'").unwrap(), 'a');
    }

    #[test]
    fn interpolated_strings_resume_before_adjacent_items() {
        let engine = Engine::new();
        let source = r#"
let y = "world";
let greeting = `hello ${y}`;
struct Marker {}
greeting
"#;

        let unit = engine.parse_unit(source).unwrap();
        assert_eq!(unit.items().len(), 1);
        assert!(matches!(unit.items()[0], Parsed::Struct(_)));

        let ast = engine.compile(source).unwrap();
        assert_eq!(
            engine.eval_ast::<ImmutableString>(&ast).unwrap(),
            "hello world"
        );
    }

    #[test]
    fn interpolated_strings_inside_item_bodies_do_not_escape_the_body() {
        let engine = Engine::new();
        let source = r#"
pub fn render() {
    let value = 1;
    let label = `outer ${if value > 0 { `inner ${value}` } else { "none" }}`;
}

struct Marker {}
"#;

        let unit = engine.parse_unit(source).unwrap();
        assert_eq!(unit.items().len(), 2);
        assert!(matches!(unit.items()[0], Parsed::Function(_)));
        assert!(matches!(unit.items()[1], Parsed::Struct(_)));
    }

    #[test]
    fn item_syntax_inside_literals_and_comments_is_not_discovered() {
        let engine = Engine::new();
        let source = r##"
let quoted = "struct Fake {}";
let character = '}';
let raw = #"enum Fake {}"#;
// pub fn fake() {}
/* impl Fake { fn fake(&self) {} } */
struct Real {}
quoted
"##;

        let unit = engine.parse_unit(source).unwrap();
        assert_eq!(unit.items().len(), 1);
        let Parsed::Struct(item) = &unit.items()[0] else {
            panic!("expected the only real struct item");
        };
        assert_eq!(item.name.text, "Real");

        let ast = engine.compile(source).unwrap();
        assert_eq!(
            engine.eval_ast::<ImmutableString>(&ast).unwrap(),
            "struct Fake {}"
        );
    }

    #[cfg(not(feature = "no_custom_syntax"))]
    #[test]
    fn registered_hash_syntax_coexists_with_attributes() {
        use crate::INT;

        let mut engine = Engine::new();
        engine.register_custom_syntax_with_state_raw(
            "#",
            |symbols, _, _| match symbols.len() {
                1 => Ok(Some("$int$".into())),
                2 => Ok(None),
                _ => unreachable!(),
            },
            false,
            |_, inputs, _| Ok(inputs[0].get_literal_value::<INT>().unwrap().into()),
        );
        let source = r#"
let answer = #41 + 1;
#[derive(Default)]
struct Marker {}
answer
"#;

        let unit = engine.parse_unit(source).unwrap();
        assert_eq!(unit.items().len(), 1);
        let Parsed::Struct(item) = &unit.items()[0] else {
            panic!("expected attributed struct item");
        };
        assert_eq!(item.header.attributes.len(), 1);

        assert_eq!(engine.eval::<INT>(source).unwrap(), 42);
    }
}
