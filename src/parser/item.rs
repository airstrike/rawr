//! Parser for source-level Rawr item headers.

use super::ParseResult;
use crate::item::type_ref;
use crate::project::cargo::features::{Enabled, Operator};
use crate::tokenizer::{Token, TokenStream, TokenizerControl};
use crate::{Engine, ImmutableString, Position, Span, PERR};

/// The items of a file, and the spans of its inner attributes (`#![..]`),
/// which the legacy script does not see.
pub(super) struct ParsedFile {
    pub items: Box<[crate::item::Parsed]>,
    pub masked: Vec<Span>,
    /// The spans of the items a `#[cfg(..)]` left out, which are masked too.
    pub disabled: Vec<Span>,
    pub imports: Box<[crate::item::Use]>,
    pub inner_attributes: Box<[crate::item::Attribute]>,
    /// The `macro_rules!` definitions among the items.
    pub macros: Box<[crate::item::MacroRules]>,
}

type ParsedItems = (Box<[crate::item::Parsed]>, Vec<Span>);

std::thread_local! {
    /// The items and `use` imports declared inside function bodies of the
    /// file being parsed. They join the file's items, each declared in its
    /// innermost block (`Header::scope`).
    static NESTED: core::cell::RefCell<Vec<crate::item::Parsed>> =
        const { core::cell::RefCell::new(Vec::new()) };
    static INNER: core::cell::RefCell<Vec<crate::item::Attribute>> =
        const { core::cell::RefCell::new(Vec::new()) };
    /// The spans of the items a `#[cfg(..)]` left out of the file being
    /// parsed, which the legacy script does not see either.
    static SKIPPED: core::cell::RefCell<Vec<Span>> = const { core::cell::RefCell::new(Vec::new()) };
    /// The `macro_rules!` definitions among the items of the file being
    /// parsed.
    static MACROS: core::cell::RefCell<Vec<crate::item::MacroRules>> =
        const { core::cell::RefCell::new(Vec::new()) };
    /// Whether the file being parsed builds as `rustc --test` builds it:
    /// `cfg(test)` holds.
    static TEST: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
    /// The features Cargo enables for the package of the file being parsed,
    /// and the options rustc sets for its target: what its `cfg(..)`s read.
    static FEATURES: core::cell::RefCell<Enabled> = core::cell::RefCell::default();
}

/// The items of `source`, parsed for `engine` with `features` enabled,
/// `cfg(test)` holding when they build the crate for its tests.
pub(super) fn parse(engine: &Engine, features: &Enabled, source: &str) -> ParseResult<ParsedFile> {
    let outer = NESTED.take();
    let outer_skipped = SKIPPED.take();
    let outer_inner = INNER.take();
    let outer_macros = MACROS.take();
    let outer_test = TEST.replace(features.test());
    let outer_features = FEATURES.replace(features.clone());
    let parsed = parse_items(engine, source);
    TEST.set(outer_test);
    FEATURES.set(outer_features);
    let mut nested = NESTED.replace(outer);
    let skipped = SKIPPED.replace(outer_skipped);
    let mut inner_attributes = INNER.replace(outer_inner).into_boxed_slice();
    let mut macros = MACROS.replace(outer_macros);
    let (mut items, mut inner) = parsed?;
    attribute_text(source, &mut items, &mut inner_attributes);
    for definition in &mut macros {
        fill_attribute_text(source, &mut definition.header.attributes);
    }
    attribute_text(source, &mut nested, &mut []);
    inner.extend(skipped.iter().copied());
    // Preserve source authority before compatibility hoisting discards duplicates.
    let mut imports = items
        .iter()
        .chain(&nested)
        .filter_map(|item| match item {
            crate::item::Parsed::Use(used) => Some(used.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    imports.sort_by_key(|used| used.span.start());
    // A body's items join the file's, each declared in its block.
    if !nested.is_empty() {
        let mut all = items.into_vec();
        all.extend(nested);
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
    Ok(ParsedFile {
        items,
        masked: inner,
        disabled: skipped,
        imports: imports.into_boxed_slice(),
        inner_attributes,
        macros: macros.into_boxed_slice(),
    })
}

/// Whether `item` is a `use` of the `rawr` crate's attribute and nothing
/// else: `use rawr::preview;`, which marked `.rs` files write so that rustc
/// accepts `#[preview]`.
pub(crate) fn is_attribute_import(item: &crate::item::Parsed) -> bool {
    let crate::item::Parsed::Use(used) = item else {
        return false;
    };
    let bindings = used.bindings();
    !bindings.is_empty()
        && bindings.iter().all(|(_, path)| {
            matches!(path.as_ref(), [krate, name]
                if krate.as_str() == "rawr" && name.as_str() == "preview")
        })
}

fn parse_items(engine: &Engine, source: &str) -> ParseResult<ParsedItems> {
    let inputs = [source];
    let (stream, tokenizer_control) = engine.lex(&inputs);
    tokenizer_control.borrow_mut().rust = true;
    let input = &mut stream.peekable();
    let mut items = Vec::new();
    let mut inner = Vec::new();
    let mut scan = ScanState::default();
    let mut at_top = true;
    let mut after_name = false;

    loop {
        let next = input.peek().expect("token streams always end with EOF");
        if matches!(&next.0, Token::Reserved(text) if text.as_str() == "#!") {
            let position = next.1;
            if !at_top || scan.delimiter_depth != 0 {
                return Err(inner_attribute_error(
                    "an inner attribute (`#![..]`) is only permitted before a file's first item",
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
                scan.next(input);
            }
            (Token::RightBrace | Token::RightParen | Token::RightBracket, _) => {
                scan.next(input);
            }
            (token, _) if scan.delimiter_depth == 0 && is_attribute_marker(token) => {
                let (_, position) = scan
                    .next(input)
                    .expect("peeked attribute marker must exist");

                if matches!(input.peek(), Some((Token::LeftBracket, _)))
                    && let Some(item) =
                        parse_item_after_attribute_marker(input, &tokenizer_control, position)?
                {
                    items.push(item);
                }
            }
            // `private fn f()`: a name before `fn` is rustc's syntax error.
            (Token::Fn, position) if scan.delimiter_depth == 0 && after_name => {
                return Err(unexpected(
                    "expected one of `!`, `.`, `::`, `;`, `?`, `{`, `}`, or an operator, \
                     found keyword `fn`"
                        .into(),
                    "expected one of 8 possible tokens",
                    None,
                    &Token::Fn,
                    *position,
                ));
            }
            (Token::Fn, _) if scan.delimiter_depth == 0 => {
                if let Some(item) = parse_private_function(input, &tokenizer_control)? {
                    items.push(item);
                }
            }
            (Token::Const, _) if scan.delimiter_depth == 0 => {
                // `const NAME: T = value;` is an item; untyped `const` stays
                // a legacy script statement.
                let start = expect(input, Token::Const, "to start a constant")?;
                if matches!(input.peek(), Some((Token::Underscore, _))) {
                    parse_anonymous_const(input, &tokenizer_control, start)?;
                    continue;
                }
                // `const fn f()` is a function rawr may also call at run time.
                if matches!(input.peek(), Some((Token::Fn, _))) {
                    items.push(crate::item::Parsed::Function(parse_function(
                        input,
                        &tokenizer_control,
                        Box::default(),
                        None,
                        start,
                        false,
                    )?));
                    continue;
                }
                if input.peek().is_some_and(|(token, _)| is_name(token)) {
                    let name = parse_name(input, "after 'const'")?;
                    if matches!(input.peek(), Some((Token::Equals, _))) {
                        return Err(missing_const_type(&name));
                    }
                    if matches!(input.peek(), Some((Token::Colon, _))) {
                        let header = crate::item::Header {
                            attributes: Box::default(),
                            visibility: None,
                            span: Span::NONE,
                            scope: None,
                        };
                        items.push(crate::item::Parsed::Function(parse_const_rest(
                            input, header, start, name,
                        )?));
                    }
                }
            }
            (Token::Identifier(text), _)
                if scan.delimiter_depth == 0 && text.as_str() == "unsafe" =>
            {
                let start = input.peek().expect("peeked unsafe").1;
                items.extend(parse_unsafe_item(
                    input,
                    &tokenizer_control,
                    Box::default(),
                    None,
                    start,
                )?);
            }
            (Token::Reserved(text), _) if scan.delimiter_depth == 0 && text.as_str() == "async" => {
                items.extend(parse_item(input, &tokenizer_control)?);
            }
            (Token::Reserved(text), _)
                if scan.delimiter_depth == 0 && text.as_str() == "static" =>
            {
                let start = input.peek().expect("peeked static").1;
                items.push(parse_static(input, Box::default(), None, start)?);
            }
            (token, position) if scan.delimiter_depth == 0 && is_word(token, "extern") => {
                let start = *position;
                scan.next(input);
                // `extern "C" ..` is passed over as before.
                if input
                    .peek()
                    .is_some_and(|(token, _)| is_word(token, "crate"))
                {
                    items.push(crate::item::Parsed::Use(parse_extern_crate(
                        input,
                        Box::default(),
                        None,
                        start,
                    )?));
                }
            }
            (token, _) if scan.delimiter_depth == 0 && begins_item(token) => {
                items.extend(parse_item(input, &tokenizer_control)?);
            }
            (Token::Identifier(name), _)
                if scan.delimiter_depth == 0
                    && matches!(name.as_str(), "trait" | "type" | "macro_rules") =>
            {
                items.extend(parse_item(input, &tokenizer_control)?);
            }
            _ => {
                let depth = scan.delimiter_depth;
                let token = scan.next(input);
                after_name = depth == 0
                    && token.is_some_and(|(token, _)| {
                        matches!(token, Token::Identifier(_) | Token::Reserved(_))
                    });
                continue;
            }
        }
        after_name = false;
    }

    resolve_const_lengths(&mut items)?;
    Ok((items.into_boxed_slice(), inner))
}

/// `[T; N]` naming a crate `const N: usize = 3;` reads as `[T; 3]`, as rustc
/// evaluates the constant; a length naming anything else is an error.
fn resolve_const_lengths(items: &mut [crate::item::Parsed]) -> ParseResult<()> {
    use crate::item::Parsed;
    let value = |function: &crate::item::Function| {
        usize::try_from(
            function
                .body_syntax
                .integer()
                .filter(|_| function.constant)?,
        )
        .ok()
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
            type_ref::Type::Tuple { members, .. } => members
                .iter_mut()
                .try_for_each(|member| visit(member, lengths)),
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
                    type_ref::Type::Path(owner) => {
                        owner.segments.last().map(|last| last.name.text.to_string())
                    }
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
            Parsed::Module(_) | Parsed::Use(_) => {}
        }
    }
    Ok(())
}

/// The lint attributes rustc reads; Rawr implements none of these lints, so
/// it accepts and ignores them, as rustc does a lint it does not know.
const LINT_ATTRIBUTES: &[&str] = &["allow", "warn", "deny", "forbid", "expect"];

/// The crate attributes that leave a program's meaning as it is under
/// `rawr run`: a build setting (the crate's name and type, the Windows
/// subsystem, the compiler's limits, `no_builtins`, a debugger's
/// visualizer) or documentation. Rawr accepts and ignores them, as rustc
/// ignores what does not apply to the build at hand.
const IGNORED_CRATE_ATTRIBUTES: &[&str] = &[
    "crate_name",
    "crate_type",
    "windows_subsystem",
    "recursion_limit",
    "type_length_limit",
    "no_builtins",
    "debugger_visualizer",
    "doc",
];

/// One inner attribute after its `#!`. A lint, build-setting or
/// documentation attribute is accepted and ignored.
/// `#![cfg_attr(predicate, ..)]` applies its attributes, each under the same
/// rules, when its predicate holds, and none of them when it does not. Any
/// other inner attribute is an error naming it.
fn parse_inner_attribute(input: &mut TokenStream, hash: Position) -> ParseResult<Span> {
    expect(input, Token::LeftBracket, "after '#!' in an attribute")?;
    let path = parse_path(input)?;
    let close = if attribute_name(&path) == "cfg_attr" {
        expect(input, Token::LeftParen, "after `cfg_attr`")?;
        let enabled = parse_cfg_predicate(input)?;
        expect(input, Token::Comma, "after a `cfg_attr` predicate")?;
        while !matches!(input.peek(), Some((Token::RightParen, _))) {
            let (_, position) = *input.peek().expect("token streams always end with EOF");
            let applied = parse_meta_item(input)?;
            if enabled {
                check_inner_attribute(&applied, position)?;
            }
            if matches!(input.peek(), Some((Token::Comma, _))) {
                input.next();
            }
        }
        expect(input, Token::RightParen, "to close `cfg_attr`")?;
        expect(input, Token::RightBracket, "to close an attribute")?
    } else {
        check_inner_attribute(&path, hash)?;
        skip_to_attribute_end(input)?
    };
    let span = Span::new(hash, after(close, 1));
    INNER.with_borrow_mut(|inner| {
        inner.push(crate::item::Attribute {
            path,
            arguments: Box::default(),
            span,
            text: ImmutableString::new(),
        });
    });
    Ok(span)
}

/// One attribute inside `cfg_attr(..)`: a path, then `(..)` or `= value`,
/// as `doc = include_str!("README.md")`, up to the next `,` or the `)`.
fn parse_meta_item(input: &mut TokenStream) -> ParseResult<type_ref::Path> {
    let path = parse_path(input)?;
    parse_meta_arguments(input)?;
    Ok(path)
}

/// Pass over the `(..)` or `= value` of an attribute inside `cfg_attr(..)`,
/// up to the next `,` or the `)`.
fn parse_meta_arguments(input: &mut TokenStream) -> ParseResult<()> {
    let mut depth = 0usize;
    loop {
        match input.peek().expect("token streams always end with EOF") {
            (Token::Comma | Token::RightParen, _) if depth == 0 => return Ok(()),
            (Token::LeftBrace | Token::LeftParen | Token::LeftBracket | Token::MapStart, _) => {
                depth += 1;
            }
            (Token::RightBrace | Token::RightParen | Token::RightBracket, _) => {
                depth = depth.saturating_sub(1);
            }
            (Token::EOF, position) => {
                return Err(PERR::MissingToken(
                    Token::RightParen.into(),
                    "to close `cfg_attr`".into(),
                )
                .into_err(*position));
            }
            _ => {}
        }
        input.next();
    }
}

fn attribute_name(path: &type_ref::Path) -> String {
    path.segments
        .iter()
        .map(|segment| segment.name.text.as_str())
        .collect::<Vec<_>>()
        .join("::")
}

/// Accept an inner attribute Rawr ignores; refuse any other, naming it.
fn check_inner_attribute(path: &type_ref::Path, position: Position) -> ParseResult<()> {
    let name = attribute_name(path);
    if LINT_ATTRIBUTES.contains(&name.as_str()) || IGNORED_CRATE_ATTRIBUTES.contains(&name.as_str())
    {
        return Ok(());
    }
    let reason = match name.as_str() {
        "no_std" | "no_core" | "no_implicit_prelude" => {
            "every file runs with the standard library and its prelude"
        }
        "no_main" => "a file runs from its `fn main`, as rustc builds it",
        "feature" => "unstable features are not available",
        _ => {
            "only lint attributes, `doc` and build settings such as `windows_subsystem` are \
             accepted as crate attributes"
        }
    };
    Err(inner_attribute_error(&format!(
        "unsupported inner attribute `#![{name}]`: {reason}"
    ))
    .into_err(position))
}

fn inner_attribute_error(message: &str) -> PERR {
    PERR::BadInput(crate::LexError::ImproperSymbol("#!".into(), message.into()))
}

#[derive(Default)]
struct ScanState {
    delimiter_depth: usize,
}

impl ScanState {
    fn inside_body() -> Self {
        Self { delimiter_depth: 1 }
    }

    fn next(&mut self, input: &mut TokenStream) -> Option<(Token, Position)> {
        let (token, position) = input.next()?;

        match &token {
            Token::LeftBrace | Token::LeftParen | Token::LeftBracket | Token::MapStart => {
                self.delimiter_depth += 1;
            }
            Token::RightBrace | Token::RightParen | Token::RightBracket => {
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

/// Whether `token` starts an item that a function body may declare and
/// Rawr hoists: `struct`, `enum`, `impl`, `trait` or `type`. A `static`
/// stays in its block, the only place that names it (D862).
fn begins_body_item(token: &Token) -> bool {
    match token {
        Token::Struct | Token::Enum | Token::Impl => true,
        Token::Identifier(name) => matches!(name.as_str(), "trait" | "type"),
        _ => false,
    }
}

/// Whether `token` starts an item that may follow a statement in a body.
fn begins_statement_item(token: &Token) -> bool {
    match token {
        Token::Fn
        | Token::Struct
        | Token::Enum
        | Token::Impl
        | Token::Mod
        | Token::Use
        | Token::Const => true,
        Token::Identifier(name) => name.as_str() == "trait",
        _ => false,
    }
}

fn is_attribute_marker(token: &Token) -> bool {
    matches!(token, Token::Hash)
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
    let mut attributes = parse_attribute_after_marker(input, hash)?;

    while input
        .peek()
        .is_some_and(|(token, _)| is_attribute_marker(token))
    {
        attributes.extend(parse_attribute(input)?);
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
        SKIPPED.with_borrow_mut(|skipped| skipped.push(Span::new(start, end)));
        return Ok(None);
    }
    if input.peek().is_some_and(
        |(token, _)| matches!(token, Token::Identifier(text) if text.as_str() == "unsafe"),
    ) {
        return parse_unsafe_item(input, tokenizer_control, attributes, visibility, start);
    }
    if input.peek().is_some_and(
        |(token, _)| matches!(token, Token::Identifier(text) if text.as_str() == "macro_rules"),
    ) {
        let definition = parse_macro_rules(input, attributes, visibility, start)?;
        MACROS.with_borrow_mut(|macros| macros.push(definition));
        return Ok(None);
    }
    if input
        .peek()
        .is_some_and(|(token, _)| is_word(token, "extern"))
    {
        input.next();
        return parse_extern_crate(input, attributes, visibility, start)
            .map(|used| Some(crate::item::Parsed::Use(used)));
    }
    if matches!(input.peek(), Some((Token::Const, _))) {
        input.next();
        if matches!(input.peek(), Some((Token::Underscore, _))) {
            parse_anonymous_const(input, tokenizer_control, start)?;
            return Ok(None);
        }
        return parse_const_item(input, tokenizer_control, attributes, visibility, start).map(Some);
    }
    match parse_enabled_item(input, tokenizer_control, attributes, visibility, start)? {
        crate::item::Parsed::Impl(item) => marker_impl(item, None),
        item => Ok(Some(item)),
    }
}

/// `unsafe impl Send for T {}`: the only `unsafe` item Rawr reads. Any
/// other `unsafe` item is not supported.
fn parse_unsafe_item(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    attributes: Box<[crate::item::Attribute]>,
    visibility: Option<Span>,
    start: Position,
) -> ParseResult<Option<crate::item::Parsed>> {
    let (_, position) = input.next().expect("peeked unsafe");
    if !matches!(input.peek(), Some((Token::Impl, _))) {
        return Err(PERR::BadInput(crate::LexError::ImproperSymbol(
            "unsafe".into(),
            "`unsafe` items other than `unsafe impl Send` and `unsafe impl Sync` are not \
             supported in Rawr yet"
                .into(),
        ))
        .into_err(position));
    }
    let item = parse_impl(input, tokenizer_control, attributes, visibility, start)?;
    marker_impl(item, Some(position))
}

/// An impl of an auto trait (`Send`, `Sync`, `Unpin` and the unwind-safety
/// markers) says what every Rawr value already is, so it is checked as
/// rustc checks it and then dropped. `unsafe` is required for `Send` and
/// `Sync` (E0200) and refused elsewhere (E0197, E0199).
fn marker_impl(
    item: crate::item::Impl,
    unsafety: Option<Position>,
) -> ParseResult<Option<crate::item::Parsed>> {
    let bad = |message: String, position: Position| {
        Err(
            PERR::BadInput(crate::LexError::ImproperSymbol("impl".into(), message))
                .into_err(position),
        )
    };
    let Some(trait_) = item.trait_.as_ref() else {
        return match unsafety {
            Some(position) => bad(
                "inherent impls cannot be unsafe, as in Rust (E0197)".into(),
                position,
            ),
            None => Ok(Some(crate::item::Parsed::Impl(item))),
        };
    };
    let name = trait_
        .segments
        .last()
        .map_or_else(String::new, |last| last.name.text.to_string());
    if !is_auto_trait(trait_) {
        return match unsafety {
            Some(position) => bad(
                format!("implementing the trait `{name}` is not unsafe, as in Rust (E0199)"),
                position,
            ),
            None => Ok(Some(crate::item::Parsed::Impl(item))),
        };
    }
    let unsafe_trait = matches!(name.as_str(), "Send" | "Sync");
    match unsafety {
        None if unsafe_trait => bad(
            format!("the trait `{name}` requires an `unsafe impl` declaration, as in Rust (E0200)"),
            trait_.span.start(),
        ),
        Some(position) if !unsafe_trait => bad(
            format!("implementing the trait `{name}` is not unsafe, as in Rust (E0199)"),
            position,
        ),
        _ => {
            if let Some(function) = item.functions.first() {
                return bad(
                    format!(
                        "method `{}` is not a member of trait `{name}`, as in Rust (E0407)",
                        function.name.text
                    ),
                    function.name.span.start(),
                );
            }
            if let Some((associated, _)) = item.associated.first() {
                return bad(
                    format!(
                        "type `{}` is not a member of trait `{name}`, as in Rust (E0437)",
                        associated.text
                    ),
                    associated.span.start(),
                );
            }
            SKIPPED.with_borrow_mut(|skipped| skipped.push(item.span));
            Ok(None)
        }
    }
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
        (Token::Fn, _) => parse_function(
            input,
            tokenizer_control,
            attributes,
            visibility,
            start,
            false,
        )
        .map(crate::item::Parsed::Function),
        (Token::Reserved(text), _) if text.as_str() == "async" => {
            parse_async_function(input, tokenizer_control, attributes, visibility, start)
                .map(crate::item::Parsed::Function)
        }
        (Token::Const, _) => {
            input.next();
            parse_const_item(input, tokenizer_control, attributes, visibility, start)
        }
        (Token::Reserved(text), _) if text.as_str() == "static" => {
            parse_static(input, attributes, visibility, start)
        }
        (_, position) => Err(PERR::MissingToken(
            "item".into(),
            "after item attributes or visibility".into(),
        )
        .into_err(*position)),
    }
}

/// `macro_rules! name { .. }`, or with `(..);` or `[..];`, checked as
/// rustc checks a definition.
fn parse_macro_rules(
    input: &mut TokenStream,
    attributes: Box<[crate::item::Attribute]>,
    visibility: Option<Span>,
    start: Position,
) -> ParseResult<crate::item::MacroRules> {
    let keyword = input.next().expect("peeked `macro_rules`");
    let rules_start = keyword.1;
    let mut tokens = vec![keyword];
    tokens.push((
        Token::Bang,
        expect(input, Token::Bang, "after `macro_rules`")?,
    ));
    let name = parse_name(input, "after `macro_rules!`")?;
    tokens.push((
        Token::Identifier(Box::new(name.text.as_str().into())),
        name.span.start(),
    ));
    let (open, position) = input.next().expect("token streams always end with EOF");
    let close = match open {
        Token::LeftBrace | Token::MapStart => Token::RightBrace,
        Token::LeftParen => Token::RightParen,
        Token::LeftBracket => Token::RightBracket,
        token => {
            return Err(unexpected(
                format!("expected one of `(`, `[`, or `{{`, found {}", found(&token)),
                "expected one of `(`, `[`, or `{`",
                None,
                &token,
                position,
            ));
        }
    };
    let braced = close == Token::RightBrace;
    tokens.push((open, position));
    let mut depth = 0_usize;
    let mut end = loop {
        let (token, position) = input.next().expect("token streams always end with EOF");
        match &token {
            Token::LeftBrace | Token::LeftParen | Token::LeftBracket | Token::MapStart => {
                depth += 1;
            }
            Token::RightBrace | Token::RightParen | Token::RightBracket if depth > 0 => depth -= 1,
            token if *token == close => {
                tokens.push((close.clone(), position));
                break position;
            }
            Token::EOF => {
                return Err(
                    PERR::MissingToken(close.into(), "to close a macro definition".into())
                        .into_err(position),
                );
            }
            Token::LexError(error) => return Err(error.clone().into_err(position)),
            _ => {}
        }
        tokens.push((token, position));
    };
    if !braced {
        end = expect(input, Token::SemiColon, "after a macro definition")?;
    }
    crate::resolve::macro_rules::Rules::from_tokens(&tokens).map_err(|malformed| {
        PERR::Unexpected {
            message: malformed.message().into(),
            label: malformed.label().into(),
            help: malformed.help().map(Into::into),
            span: Span::new(malformed.at(), after(malformed.at(), 1)),
        }
        .into_err(malformed.at())
    })?;
    let end = after(end, 1);
    Ok(crate::item::MacroRules {
        header: crate::item::Header {
            attributes,
            visibility,
            span: Span::new(start, end),
            scope: None,
        },
        name,
        span: Span::new(rules_start, end),
    })
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
            scope: None,
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
    {
        let name = parse_name(input, "after 'static'")?;
        // Read as a `const`: a move out of it (rustc's E0507) is left to
        // rustc.
        #[cfg(test)]
        crate::resolve::borrowck::note_unlowered("a `static` item, read as a `const`", name.span);
        parse_const_rest(
            input,
            crate::item::Header {
                attributes,
                visibility,
                span: Span::NONE,
                scope: None,
            },
            start,
            name,
        )
        .map(crate::item::Parsed::Function)
    }
}

/// The rest of a named `const` item after `const`: `const NAME: T = v;`,
/// or `const fn f()`, a function rawr may also call at run time.
fn parse_const_item(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    attributes: Box<[crate::item::Attribute]>,
    visibility: Option<Span>,
    start: Position,
) -> ParseResult<crate::item::Parsed> {
    if matches!(input.peek(), Some((Token::Fn, _))) {
        return parse_function(
            input,
            tokenizer_control,
            attributes,
            visibility,
            start,
            false,
        )
        .map(crate::item::Parsed::Function);
    }
    let name = parse_name(input, "after 'const'")?;
    parse_const_rest(
        input,
        crate::item::Header {
            attributes,
            visibility,
            span: Span::NONE,
            scope: None,
        },
        start,
        name,
    )
    .map(crate::item::Parsed::Function)
}

/// Whether `token` is the word `word`, as the lexer reads `crate` and
/// `extern`: an identifier or a reserved word.
fn is_word(token: &Token, word: &str) -> bool {
    matches!(token, Token::Identifier(text) | Token::Reserved(text) if text.as_str() == word)
}

/// The rest of `extern crate name;` or `extern crate name as alias;`, after
/// `extern`: an import of the crate's root, as `use name as alias;` is.
fn parse_extern_crate(
    input: &mut TokenStream,
    attributes: Box<[crate::item::Attribute]>,
    visibility: Option<Span>,
    start: Position,
) -> ParseResult<crate::item::Use> {
    let (token, position) = input.next().expect("token streams always end with EOF");
    if !is_word(&token, "crate") {
        return Err(PERR::MissingToken("crate".into(), "after `extern`".into()).into_err(position));
    }
    let name = parse_name(input, "after `extern crate`")?;
    let alias = parse_use_alias(input)?;
    let semicolon = expect(input, Token::SemiColon, "after an `extern crate` item")?;
    let span = Span::new(start, after(semicolon, 1));
    Ok(crate::item::Use {
        scope: None,
        header: crate::item::Header {
            attributes,
            visibility,
            span,
            scope: None,
        },
        tree: crate::item::UseTree::Path(
            type_ref::Path {
                span: name.span,
                segments: Box::new([type_ref::Segment {
                    span: name.span,
                    name,
                    arguments: Box::default(),
                }]),
                global: true,
            },
            alias,
        ),
        span,
    })
}

/// The rest of `const _: T = { .. };` after `const`: an unnamed constant,
/// as a derive writes to hold its impls. The block's items join the file's
/// items as a function body's do, its imports scoped to the block
/// (`extern crate serde as _serde;`). Rust evaluates the block once and
/// nothing names it, so a block of items alone has nothing to run.
fn parse_anonymous_const(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    start: Position,
) -> ParseResult<()> {
    expect(input, Token::Underscore, "to name an unnamed constant")?;
    expect(input, Token::Colon, "after `const _`")?;
    // The constant's type, which nothing the block holds names.
    while !matches!(input.peek(), Some((Token::Equals | Token::EOF, _))) {
        input.next();
    }
    expect(input, Token::Equals, "after the type of `const _`")?;
    let open = expect(input, Token::LeftBrace, "to start the block of `const _`")?;
    let (_, body) = parse_body_syntax(input, tokenizer_control, open)?;
    if let Some(token) = body
        .tokens()
        .iter()
        .find(|token| !matches!(token.kind(), crate::item::BodyTokenKind::Semicolon))
    {
        return Err(PERR::Unexpected {
            message: "a statement in the block of `const _` is not supported yet".into(),
            label: "only items are supported here".into(),
            help: None,
            span: token.span(),
        }
        .into_err(token.span().start()));
    }
    let semicolon = expect(input, Token::SemiColon, "after the block of `const _`")?;
    SKIPPED.with_borrow_mut(|skipped| skipped.push(Span::new(start, after(semicolon, 1))));
    Ok(())
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
            scope: None,
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
        attributes.extend(parse_attribute(input)?);
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

/// One `cfg` predicate: `test` (as the engine builds: rustc's `--test`),
/// `feature = "x"` (a feature Cargo enables for the package), `not(..)`,
/// `all(..)`, `any(..)`, and any other option as rustc sets it for the
/// target Cargo builds the package for (`target_os = ".."`, `unix`).
fn parse_cfg_predicate(input: &mut TokenStream) -> ParseResult<bool> {
    let name = parse_name(input, "in a `cfg` predicate")?;
    if let Some(operator) = Operator::named(&name.text) {
        expect(input, Token::LeftParen, "after a `cfg` operator")?;
        let mut values = Vec::new();
        while !matches!(input.peek(), Some((Token::RightParen, _))) {
            values.push(parse_cfg_predicate(input)?);
            if matches!(input.peek(), Some((Token::Comma, _))) {
                input.next();
            }
        }
        expect(input, Token::RightParen, "to close a `cfg` operator")?;
        return Ok(operator.holds(&values));
    }
    if matches!(input.peek(), Some((Token::Equals, _))) {
        input.next();
        return Ok(match input.next() {
            Some((Token::StringConstant(value), _)) => {
                FEATURES.with_borrow(|features| features.cfg(&name.text, Some(&value), TEST.get()))
            }
            _ => false,
        });
    }
    Ok(FEATURES.with_borrow(|features| features.cfg(&name.text, None, TEST.get())))
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
                return Err(
                    PERR::MissingToken(Token::SemiColon.into(), "to end an item".into())
                        .into_err(position),
                );
            }
            _ => {}
        }
    }
}

/// The attributes on a field or an enum variant. Only `#[cfg(..)]` and a
/// variant's `#[default]` mean anything to Rawr. Every attribute, such as a
/// derive helper like `#[serde(default = "..")]`, is kept as written for the
/// Rust a later pass writes; Rawr leaves a helper inert, as rustc does.
#[derive(Default)]
struct MemberAttributes {
    default: Option<Span>,
    disabled: bool,
    attributes: Vec<crate::item::Attribute>,
}

fn parse_member_attributes(input: &mut TokenStream) -> ParseResult<MemberAttributes> {
    let mut attributes = MemberAttributes::default();
    while input
        .peek()
        .is_some_and(|(token, _)| is_attribute_marker(token))
    {
        let (_, hash) = input.next().expect("peeked attribute marker must exist");
        expect(input, Token::LeftBracket, "after '#' in an attribute")?;
        let path = parse_path(input)?;
        let name = (path.segments.len() == 1).then(|| path.segments[0].name.text.as_str());
        let close = if name == Some("cfg") {
            expect(input, Token::LeftParen, "after `cfg`")?;
            attributes.disabled |= !parse_cfg_predicate(input)?;
            expect(input, Token::RightParen, "to close `cfg`")?;
            expect(input, Token::RightBracket, "to close an attribute")?
        } else {
            skip_to_attribute_end(input)?
        };
        let span = Span::new(hash, after(close, 1));
        if name == Some("default") {
            attributes.default = Some(span);
        }
        attributes.attributes.push(crate::item::Attribute {
            path,
            arguments: Box::default(),
            span,
            text: ImmutableString::new(),
        });
    }
    Ok(attributes)
}

/// Fill each attribute's `text` from `source`, which every span of `items`
/// and `inner` points into.
fn attribute_text(
    source: &str,
    items: &mut [crate::item::Parsed],
    inner: &mut [crate::item::Attribute],
) {
    use crate::item::Parsed;
    let fill = |attributes: &mut [crate::item::Attribute]| fill_attribute_text(source, attributes);
    let function = |function: &mut crate::item::Function| fill(&mut function.header.attributes);
    fill(inner);
    for item in items {
        match item {
            Parsed::Module(item) => fill(&mut item.header.attributes),
            Parsed::Use(item) => fill(&mut item.header.attributes),
            Parsed::Struct(item) => {
                fill(&mut item.header.attributes);
                for field in &mut item.fields {
                    fill(&mut field.attributes);
                }
            }
            Parsed::Enum(item) => {
                fill(&mut item.header.attributes);
                for variant in &mut item.variants {
                    fill(&mut variant.attributes);
                    for attributes in &mut variant.field_attributes {
                        fill(attributes);
                    }
                }
            }
            Parsed::Impl(item) => {
                fill(&mut item.header.attributes);
                item.functions.iter_mut().for_each(&function);
            }
            Parsed::Trait(item) => {
                fill(&mut item.header.attributes);
                item.functions.iter_mut().for_each(&function);
            }
            Parsed::Function(item) => function(item),
            Parsed::TypeAlias(item) => fill(&mut item.header.attributes),
        }
    }
}

/// Each attribute's text between `#[` and `]`, from `source`.
fn fill_attribute_text(source: &str, attributes: &mut [crate::item::Attribute]) {
    for attribute in attributes {
        let text = attribute.span.text(source);
        let text = text.strip_prefix('#').unwrap_or(text).trim_start();
        let text = text.strip_prefix('[').unwrap_or(text);
        let text = text.strip_suffix(']').unwrap_or(text);
        attribute.text = text.trim().into();
    }
}

/// Pass over the rest of an attribute up to and including its `]`, and
/// return where that `]` is.
fn skip_to_attribute_end(input: &mut TokenStream) -> ParseResult<Position> {
    let mut depth = 0usize;
    loop {
        match input.next().expect("token streams always end with EOF") {
            (Token::LeftBrace | Token::LeftParen | Token::LeftBracket | Token::MapStart, _) => {
                depth += 1;
            }
            (Token::RightBracket, position) if depth == 0 => return Ok(position),
            (Token::RightBrace | Token::RightParen | Token::RightBracket, _) => {
                depth = depth.saturating_sub(1);
            }
            (Token::EOF, position) => {
                return Err(PERR::MissingToken(
                    Token::RightBracket.into(),
                    "to close an attribute".into(),
                )
                .into_err(position));
            }
            _ => {}
        }
    }
}

/// One outer attribute: the attributes it stands for, each spanning it.
/// That is the attribute itself, or what a `#[cfg_attr(predicate, ..)]`
/// applies when its predicate holds.
fn parse_attribute(input: &mut TokenStream) -> ParseResult<Vec<crate::item::Attribute>> {
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
) -> ParseResult<Vec<crate::item::Attribute>> {
    expect(input, Token::LeftBracket, "after '#' in an attribute")?;
    let path = parse_path(input)?;
    if path.segments.len() == 1 && path.segments[0].name.text == "cfg_attr" {
        return parse_cfg_attr(input, path, hash);
    }
    parse_attribute_body(input, path, hash).map(|attribute| vec![attribute])
}

/// `#[cfg_attr(predicate, attribute, ..)]` after its path: each attribute
/// it applies when the predicate holds, under the same rules as the one
/// written alone. When it does not hold, it is kept as written, inert.
fn parse_cfg_attr(
    input: &mut TokenStream,
    path: type_ref::Path,
    hash: Position,
) -> ParseResult<Vec<crate::item::Attribute>> {
    expect(input, Token::LeftParen, "after `cfg_attr`")?;
    let enabled = parse_cfg_predicate(input)?;
    expect(input, Token::Comma, "after a `cfg_attr` predicate")?;
    let mut applied = Vec::new();
    while !matches!(input.peek(), Some((Token::RightParen, _))) {
        let mut path = parse_path(input)?;
        let mut arguments = Vec::new();
        let named = |name: &str| path.segments.len() == 1 && path.segments[0].name.text == name;
        if named("derive") && matches!(input.peek(), Some((Token::LeftParen, _))) {
            input.next();
            while !matches!(input.peek(), Some((Token::RightParen, _))) {
                arguments.push(parse_path(input)?);
                if matches!(input.peek(), Some((Token::Comma, _))) {
                    input.next();
                }
            }
            expect(input, Token::RightParen, "to close attribute arguments")?;
        } else if named("cfg") {
            expect(input, Token::LeftParen, "after `cfg`")?;
            if !parse_cfg_predicate(input)? {
                path.segments[0].name.text = CFG_DISABLED.into();
            }
            expect(input, Token::RightParen, "to close `cfg`")?;
        } else {
            parse_meta_arguments(input)?;
        }
        applied.push((path, arguments));
        if matches!(input.peek(), Some((Token::Comma, _))) {
            input.next();
        }
    }
    expect(input, Token::RightParen, "to close `cfg_attr`")?;
    let close = expect(input, Token::RightBracket, "to close an attribute")?;
    let span = Span::new(hash, after(close, 1));
    let attribute =
        |(path, arguments): (type_ref::Path, Vec<type_ref::Path>)| crate::item::Attribute {
            path,
            arguments: arguments.into_boxed_slice(),
            span,
            text: ImmutableString::new(),
        };
    if !enabled {
        return Ok(vec![attribute((path, Vec::new()))]);
    }
    Ok(applied.into_iter().map(attribute).collect())
}

/// An outer attribute after its path, through its `]`.
fn parse_attribute_body(
    input: &mut TokenStream,
    path: type_ref::Path,
    hash: Position,
) -> ParseResult<crate::item::Attribute> {
    let mut arguments = Vec::new();

    // `#[cfg(predicate)]`, under the engine's build (`parse_cfg_predicate`).
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
            text: ImmutableString::new(),
        });
    }

    // A runtime's entry, `#[tokio::main(flavor = "current_thread")]`: its
    // arguments are the runtime's, which the shadow crate's native entry
    // passes on as written (D513.4). `#[preview(..)]`'s are rustc's alone.
    if (crate::item::is_attribute_path(&path, "preview")
        || path.segments.len() > 1
            && path
                .segments
                .last()
                .is_some_and(|segment| segment.name.text == "main"))
        && matches!(input.peek(), Some((Token::LeftParen, _)))
    {
        let mut depth = 0_usize;
        loop {
            match input.next().expect("token streams always end with EOF") {
                (Token::LeftParen, _) => depth += 1,
                (Token::RightParen, _) => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                (Token::EOF, position) => {
                    return Err(PERR::MissingToken(
                        Token::RightParen.into(),
                        "to close attribute arguments".into(),
                    )
                    .into_err(position));
                }
                _ => {}
            }
        }
    }

    // `derive`'s arguments are paths. Another attribute's may be any
    // tokens, as a derive helper's `#[serde(rename_all = "camelCase")]` or
    // `#[doc = ".."]` are: they are kept as text, with no path arguments.
    let derive = path.segments.len() == 1 && path.segments[0].name.text == "derive";
    let paths_only =
        |token: &Token| is_name(token) || matches!(token, Token::DoubleColon | Token::RightParen);
    if !derive
        && !matches!(
            input.peek(),
            Some((Token::LeftParen | Token::RightBracket, _))
        )
    {
        let close = skip_to_attribute_end(input)?;
        return Ok(crate::item::Attribute {
            path,
            arguments: Box::default(),
            span: Span::new(hash, after(close, 1)),
            text: ImmutableString::new(),
        });
    }

    if matches!(input.peek(), Some((Token::LeftParen, _))) {
        input.next();

        while !matches!(input.peek(), Some((Token::RightParen, _))) {
            if !derive && !input.peek().is_some_and(|(token, _)| paths_only(token)) {
                arguments.clear();
                skip_to_close_paren(input)?;
                break;
            }
            arguments.push(parse_path(input)?);

            match input.peek().expect("token streams always end with EOF") {
                (Token::Comma, _) => {
                    input.next();
                    if matches!(input.peek(), Some((Token::RightParen, _))) {
                        break;
                    }
                }
                (Token::RightParen, _) => break,
                (_, _) if !derive => {
                    arguments.clear();
                    skip_to_close_paren(input)?;
                    break;
                }
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
        text: ImmutableString::new(),
    })
}

/// Pass over an attribute's arguments up to, not including, the `)` that
/// closes them.
fn skip_to_close_paren(input: &mut TokenStream) -> ParseResult<()> {
    let mut depth = 0usize;
    loop {
        match input.peek().expect("token streams always end with EOF") {
            (Token::RightParen, _) if depth == 0 => return Ok(()),
            (Token::LeftBrace | Token::LeftParen | Token::LeftBracket | Token::MapStart, _) => {
                depth += 1;
            }
            (Token::RightBrace | Token::RightParen | Token::RightBracket, _) => {
                depth = depth.saturating_sub(1);
            }
            (Token::EOF, position) => {
                return Err(PERR::MissingToken(
                    Token::RightParen.into(),
                    "to close attribute arguments".into(),
                )
                .into_err(*position));
            }
            _ => {}
        }
        input.next();
    }
}

fn parse_use(
    input: &mut TokenStream,
    attributes: Box<[crate::item::Attribute]>,
    visibility: Option<Span>,
    start: Position,
) -> ParseResult<crate::item::Use> {
    expect(input, Token::Use, "to start an import")?;
    // `use ::image as img;`: a leading `::` names an outside crate.
    let global = matches!(input.peek(), Some((Token::DoubleColon, _)));
    if global {
        input.next();
    }
    // `use self::m::Item;` starts at this module.
    let first = match input.peek() {
        Some((Token::SelfValue, position)) if !global => {
            let span = token_span(*position, "self");
            input.next();
            crate::item::Name {
                span,
                text: "self".into(),
            }
        }
        _ => parse_use_segment(input, "as the first import path segment")?,
    };
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
        global,
    };

    let mut tree = if let Some(star) = glob {
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
        let tree = if matches!(&group, crate::item::UseTree::Group { members, .. } if members.is_empty())
        {
            trees.next().unwrap_or(group)
        } else {
            group
        };
        extra_uses.extend(trees);
        tree
    } else {
        crate::item::UseTree::Path(path, parse_use_alias(input)?)
    };

    // Flattened members retain the root's absolute-path meaning.
    for tree in core::iter::once(&mut tree).chain(extra_uses.iter_mut()) {
        match tree {
            crate::item::UseTree::Path(path, _)
            | crate::item::UseTree::Group { prefix: path, .. }
            | crate::item::UseTree::Glob { prefix: path, .. } => path.global = global,
        }
    }

    let semicolon = expect(input, Token::SemiColon, "to terminate an import")?;
    let end = after(semicolon, 1);
    let header = crate::item::Header {
        attributes,
        visibility,
        span: Span::new(start, end),
        scope: None,
    };
    for tree in extra_uses {
        let used = crate::item::Use {
            scope: None,
            header: header.clone(),
            tree,
            span: Span::new(start, end),
        };
        NESTED.with_borrow_mut(|nested| nested.push(crate::item::Parsed::Use(used)));
    }

    Ok(crate::item::Use {
        scope: None,
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
    let (mut generics, mut lifetimes) = parse_generics(input)?;
    parse_where_with(input, &mut generics, None, &mut lifetimes)?;

    if matches!(input.peek(), Some((Token::SemiColon, _))) {
        let semicolon = expect(input, Token::SemiColon, "to terminate a unit struct")?;
        let end = after(semicolon, 1);
        return Ok(crate::item::Struct {
            header: crate::item::Header {
                attributes,
                visibility,
                span: Span::new(start, end),
                scope: None,
            },
            name,
            generics: generics.into_boxed_slice(),
            lifetimes: lifetimes.clone(),
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
                let field_attributes = parse_member_attributes(input)?;
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
                    attributes: field_attributes.attributes.into_boxed_slice(),
                });
                consume_comma_or_end(input, Token::RightParen, "between tuple struct fields")?;
            }
            expect(input, Token::RightParen, "to close a tuple struct")?;
        }
        parse_where_with(input, &mut generics, None, &mut lifetimes)?;
        let semicolon = expect(input, Token::SemiColon, "to terminate a tuple struct")?;
        let end = after(semicolon, 1);
        return Ok(crate::item::Struct {
            header: crate::item::Header {
                attributes,
                visibility,
                span: Span::new(start, end),
                scope: None,
            },
            name,
            generics: generics.into_boxed_slice(),
            lifetimes: lifetimes.clone(),
            fields: fields.into_boxed_slice(),
            form: crate::item::StructForm::Tuple,
            span: Span::new(start, end),
        });
    }

    let open = expect(input, Token::LeftBrace, "after a struct name")?;
    let header_end = after(open, 1);
    let mut fields = Vec::new();

    while !matches!(input.peek(), Some((Token::RightBrace, _))) {
        let field_attributes = parse_member_attributes(input)?;
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
        if !field_attributes.disabled {
            fields.push(crate::item::Field {
                name: field_name,
                ty,
                span,
                attributes: field_attributes.attributes.into_boxed_slice(),
            });
        }

        consume_comma_or_end(input, Token::RightBrace, "between struct fields")?;
    }

    let close = expect(input, Token::RightBrace, "to close a struct")?;
    let end = after(close, 1);

    Ok(crate::item::Struct {
        header: crate::item::Header {
            attributes,
            visibility,
            span: Span::new(start, header_end),
            scope: None,
        },
        name,
        generics: generics.into_boxed_slice(),
        lifetimes: lifetimes.clone(),
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
    let (mut generics, mut lifetimes) = parse_generics(input)?;
    parse_where_with(input, &mut generics, None, &mut lifetimes)?;
    let open = expect(input, Token::LeftBrace, "after an enum name")?;
    let header_end = after(open, 1);
    let mut variants = Vec::new();

    while !matches!(input.peek(), Some((Token::RightBrace, _))) {
        // `#[default]` marks the variant a derived `Default` builds; other
        // variant attributes have no effect.
        let variant_attributes = parse_member_attributes(input)?;
        let default = variant_attributes.default;
        let variant_name = parse_name(input, "as an enum variant")?;
        let mut payload = Vec::new();
        let mut field_attributes = Vec::new();
        let mut fields = None;
        let mut end = variant_name.span.end();

        if matches!(input.peek(), Some((Token::LeftParen, _))) {
            input.next();

            while !matches!(input.peek(), Some((Token::RightParen, _))) {
                field_attributes.push(
                    parse_member_attributes(input)?
                        .attributes
                        .into_boxed_slice(),
                );
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
                field_attributes.push(
                    parse_member_attributes(input)?
                        .attributes
                        .into_boxed_slice(),
                );
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
            attributes: variant_attributes.attributes.into_boxed_slice(),
            field_attributes: field_attributes.into_boxed_slice(),
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
            scope: None,
        },
        name,
        generics: generics.into_boxed_slice(),
        lifetimes,
        variants: variants.into_boxed_slice(),
        span: Span::new(start, end),
    })
}

fn parse_impl(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    attributes: Box<[crate::item::Attribute]>,
    visibility: Option<Span>,
    start: Position,
) -> ParseResult<crate::item::Impl> {
    expect(input, Token::Impl, "to start an impl block")?;
    let (mut generics, mut lifetimes) = parse_generics(input)?;
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
            && path
                .segments
                .last()
                .is_some_and(|last| crate::resolve::std_trait::is_operator(&last.name.text))
        {
            owner = (**target).clone();
        }
        (Some(path), owner)
    } else {
        (None, first)
    };
    parse_where_with(input, &mut generics, None, &mut lifetimes)?;
    let open = expect(input, Token::LeftBrace, "after an impl owner")?;
    let header_end = after(open, 1);
    let mut functions = Vec::new();
    let mut associated = Vec::new();

    while !matches!(input.peek(), Some((Token::RightBrace, _))) {
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
            if matches!(input.peek(), Some((Token::Reserved(text), _)) if text.as_str() == "async")
            {
                functions.push(parse_async_function(
                    input,
                    tokenizer_control,
                    attributes,
                    visibility,
                    function_start,
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

        let position = input.peek().expect("token streams always end with EOF").1;
        return Err(PERR::MissingToken(
            "fn".into(),
            "for an associated function inside an impl block".into(),
        )
        .into_err(position));
    }

    let close = expect(input, Token::RightBrace, "to close an impl block")?;
    let end = after(close, 1);

    // `Self` in a signature names the impl's owner, and `Self::Output` (or
    // `<Self as Trait>::Output`) the impl's associated type, as in Rust.
    let implemented = trait_
        .as_ref()
        .and_then(|path| path.segments.last())
        .map(|last| last.name.text.as_str());
    for (_, ty) in &mut associated {
        replace_self(ty, &owner);
    }
    for function in &mut functions {
        for parameter in function.parameters.iter_mut() {
            replace_associated(&mut parameter.ty, &associated, &owner, implemented);
            replace_self(&mut parameter.ty, &owner);
        }
        if let Some(output) = &mut function.output {
            replace_associated(output, &associated, &owner, implemented);
            replace_self(output, &owner);
        }
    }

    Ok(crate::item::Impl {
        header: crate::item::Header {
            attributes,
            visibility,
            span: Span::new(start, header_end),
            scope: None,
        },
        generics: generics.into_boxed_slice(),
        lifetimes,
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
fn replace_associated(
    ty: &mut type_ref::Type,
    associated: &[(crate::item::Name, type_ref::Type)],
    owner: &type_ref::Type,
    implemented: Option<&str>,
) {
    match ty {
        // `<Self as Trait>::Name`: the impl's own type when `Trait` is the
        // trait it implements, else `<Owner as Trait>::Name`.
        type_ref::Type::Path(path)
            if path.segments.len() == 3
                && path.segments[0].name.text == "Self"
                && path.segments[0].arguments.is_empty()
                && path.segments[1].name.text == type_ref::QUALIFIED_SEGMENT
                && path.segments[2].arguments.is_empty() =>
        {
            let written = match &path.segments[1].arguments[..] {
                [type_ref::Argument::Type(type_ref::Type::Path(written))] => {
                    written.segments.last().map(|last| last.name.text.as_str())
                }
                _ => None,
            };
            let name = &path.segments[2].name;
            if written.is_some()
                && written == implemented
                && let Some((_, bound)) = associated
                    .iter()
                    .find(|(declared, _)| declared.text == name.text)
            {
                *ty = bound.clone();
            } else if let type_ref::Type::Path(owner) = owner {
                path.segments = owner
                    .segments
                    .iter()
                    .chain(&path.segments[1..])
                    .cloned()
                    .collect();
            }
        }
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
                        replace_associated(argument, associated, owner, implemented);
                    }
                }
            }
        }
        type_ref::Type::Tuple { members, .. } => {
            for member in members.iter_mut() {
                replace_associated(member, associated, owner, implemented);
            }
        }
        type_ref::Type::Reference { target, .. } => {
            replace_associated(target, associated, owner, implemented);
        }
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
    // `trait Convert<T>`, `trait IntoContent<'a, Message>`.
    let (generics, lifetimes) = parse_generics(input)?;
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
    let mut functions = Vec::new();
    let mut provided = Vec::new();
    let mut associated = Vec::new();
    let mut associated_bounds = Vec::new();
    let mut associated_projections = Vec::new();

    while !matches!(input.peek(), Some((Token::RightBrace, _))) {
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
                // `type Output: Clone + Shape;`, `type Fore: Iterator<Item =
                // Self::For>;`.
                let mut projections = Vec::new();
                let bounds = if matches!(input.peek(), Some((Token::Colon, _))) {
                    input.next();
                    parse_bounds(input, &mut projections, None)?
                } else {
                    Vec::new()
                };
                associated_projections.push(projections.into_boxed_slice());
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
            scope: None,
        },
        name,
        functions: functions.into_boxed_slice(),
        provided: provided.into_boxed_slice(),
        associated: associated.into_boxed_slice(),
        associated_bounds: associated_bounds.into_boxed_slice(),
        associated_projections: associated_projections.into_boxed_slice(),
        supertraits: supertraits.into_boxed_slice(),
        generics: generics.into_boxed_slice(),
        lifetimes,
        span: Span::new(start, end),
    })
}

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
    let generics = parse_generics(input)?;
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
            scope: None,
        },
        start,
        name,
        generics,
        open.then_some(None),
        declaration,
    )
}

/// `async fn`: a function that returns `impl Future<Output = T>` of its
/// declared output `T`, and whose body runs as an `async move` block.
fn parse_async_function(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    attributes: Box<[crate::item::Attribute]>,
    visibility: Option<Span>,
    start: Position,
) -> ParseResult<crate::item::Function> {
    let (_, at) = input.next().expect("the caller peeked `async`");
    let mut function = parse_function(
        input,
        tokenizer_control,
        attributes,
        visibility,
        start,
        false,
    )?;
    let keyword = token_span(at, "async");
    let output = function
        .output
        .take()
        .unwrap_or(type_ref::Type::Unit(keyword));
    function.output = Some(future_type(output, keyword));
    function.asynchronous = true;
    Ok(function)
}

/// `impl Future<Output = output>`, as `parse_impl_bounds` reads it.
fn future_type(output: type_ref::Type, at: Span) -> type_ref::Type {
    let path = |text: String, arguments: Vec<type_ref::Argument>| type_ref::Path {
        segments: Box::new([type_ref::Segment {
            name: crate::item::Name {
                text: text.into(),
                span: at,
            },
            arguments: arguments.into_boxed_slice(),
            span: at,
        }]),
        span: at,
        global: false,
    };
    let bound = path("Future".into(), Vec::new());
    let binding = path(
        format!("{}Future=Output", type_ref::BINDING_PREFIX),
        vec![type_ref::Argument::Type(output)],
    );
    type_ref::Type::Path(path(
        type_ref::IMPL_TYPE.into(),
        vec![
            type_ref::Argument::Type(type_ref::Type::Path(bound)),
            type_ref::Argument::Type(type_ref::Type::Path(binding)),
        ],
    ))
}

/// Parse a function after its name and `(` (`parameters` is `Some`, holding
/// a first parameter name already consumed), or after `()` (`None`).
fn parse_function_rest(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    header: crate::item::Header,
    start: Position,
    name: crate::item::Name,
    (mut generics, mut lifetimes): (Vec<crate::item::GenericParam>, crate::item::Lifetimes),
    parameters_open: Option<Option<crate::item::Name>>,
    declaration: bool,
) -> ParseResult<crate::item::Function> {
    let mut receiver = None;
    let mut receiver_lifetime = None;
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
                    expect_parameter_type(input)?;
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
                    // receiver's lifetime (`&'a self`) is kept for the
                    // borrow checker.
                    let ampersand = *position;
                    input.next();
                    let lifetime = if matches!(input.peek(), Some((Token::Apostrophe, _))) {
                        Some(parse_lifetime(input)?.name)
                    } else {
                        None
                    };
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
                        receiver_lifetime = lifetime;
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
                    expect_parameter_type(input)?;
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
        Some(parse_output_type(input)?)
    } else {
        None
    };
    parse_where_with(input, &mut generics, None, &mut lifetimes)?;
    lift_impl_parameters(&mut generics, &mut parameters);
    erase_fn_bounds(
        &mut generics,
        &mut parameters,
        &mut output,
        &lifetimes.bounds,
    );

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
            lifetimes,
            receiver_lifetime,
            receiver,
            parameters: parameters.into_boxed_slice(),
            output,
            body_span: semicolon,
            body_syntax: crate::item::BodySyntax::new(Vec::new()),
            constant: false,
            asynchronous: false,
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
        lifetimes,
        receiver_lifetime,
        receiver,
        parameters: parameters.into_boxed_slice(),
        output,
        body_span,
        body_syntax,
        constant: false,
        asynchronous: false,
        span,
    })
}

/// `<T, U: Clone + Debug, 'a>` after an item name, when present.
fn parse_generic_params(input: &mut TokenStream) -> ParseResult<Vec<crate::item::GenericParam>> {
    parse_generics(input).map(|(generics, _)| generics)
}

/// [`parse_generic_params`], with the lifetime parameters and the outlives
/// bounds written on them (`'b: 'a`, `T: 'a`).
fn parse_generics(
    input: &mut TokenStream,
) -> ParseResult<(Vec<crate::item::GenericParam>, crate::item::Lifetimes)> {
    let mut generics = Vec::new();
    let mut lifetimes = Vec::new();
    let mut outlives = Vec::new();
    if !matches!(input.peek(), Some((Token::LessThan, _))) {
        return Ok((generics, crate::item::Lifetimes::default()));
    }
    input.next();
    // The second `>` of a `>>` closing a bound's arguments closes the list.
    let mut split = None;
    while !matches!(input.peek(), Some((Token::GreaterThan, _))) {
        if matches!(input.peek(), Some((Token::Apostrophe, _))) {
            let lifetime = parse_lifetime(input)?.name;
            if matches!(input.peek(), Some((Token::Colon, _))) {
                input.next();
                outlives.push((lifetime.clone(), parse_lifetime(input)?.name));
                while matches!(input.peek(), Some((Token::Plus, _))) {
                    input.next();
                    outlives.push((lifetime.clone(), parse_lifetime(input)?.name));
                }
            }
            lifetimes.push(lifetime);
        } else {
            let name = parse_name(input, "as a generic parameter")?;
            let mut projections = Vec::new();
            let bounds = if matches!(input.peek(), Some((Token::Colon, _))) {
                input.next();
                let mut bounded = Vec::new();
                let bounds =
                    parse_bounds_with(input, &mut projections, Some(&mut split), &mut bounded)?;
                outlives.extend(bounded.into_iter().map(|lifetime| (name.clone(), lifetime)));
                bounds
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
                lifted: false,
            });
        }
        if split.is_some() {
            return Ok((generics, crate::item::Lifetimes::new(lifetimes, outlives)));
        }
        consume_comma_or_end(input, Token::GreaterThan, "between generic parameters")?;
    }
    expect(input, Token::GreaterThan, "to close generic parameters")?;
    Ok((generics, crate::item::Lifetimes::new(lifetimes, outlives)))
}

/// `use<'a, T>` among `impl Trait` bounds: Rust's precise capturing, which
/// Rawr's types need no record of. Where it ends, when it was there.
///
/// `Option<impl Future + use<>>` ends in `>>`, one token: its second `>`
/// closes the enclosing list, which `split` then records.
fn skip_precise_capture(
    input: &mut TokenStream,
    split: Option<&mut Option<Position>>,
) -> ParseResult<Option<Position>> {
    if !matches!(input.peek(), Some((Token::Use, _))) {
        return Ok(None);
    }
    input.next();
    expect(input, Token::LessThan, "after `use` in a bound")?;
    loop {
        match input.next() {
            Some((Token::GreaterThan, position)) => return Ok(Some(after(position, 1))),
            Some((Token::RightShift, position)) if split.is_some() => {
                let end = after(position, 1);
                if let Some(split) = split {
                    *split = Some(end);
                }
                return Ok(Some(end));
            }
            Some((Token::EOF, position)) => {
                return Err(
                    PERR::MissingToken(">".into(), "to close `use<..>`".into()).into_err(position)
                );
            }
            None => unreachable!("token streams always end with EOF"),
            Some(_) => {}
        }
    }
}

/// `Bound + Bound`: trait paths; lifetime bounds are erased. Associated
/// type equalities (`Shape<Output = i64>`) go to `projections`.
fn parse_bounds(
    input: &mut TokenStream,
    projections: &mut Vec<(crate::item::Name, crate::item::Name, type_ref::Type)>,
    closing: Option<&mut Option<Position>>,
) -> ParseResult<Vec<type_ref::Path>> {
    parse_bounds_with(input, projections, closing, &mut Vec::new())
}

/// [`parse_bounds`], with the lifetimes among them: `'a` of `T: Clone + 'a`.
fn parse_bounds_with(
    input: &mut TokenStream,
    projections: &mut Vec<(crate::item::Name, crate::item::Name, type_ref::Type)>,
    mut closing: Option<&mut Option<Position>>,
    lifetimes: &mut Vec<crate::item::Name>,
) -> ParseResult<Vec<type_ref::Path>> {
    let mut bounds = Vec::new();
    loop {
        // `for<'a> Fn(&'a T)`: the bound for every `'a`.
        let binder = parse_binder(input)?;
        let first_bound = bounds.len();
        let first_projection = projections.len();
        if !binder.is_empty() && matches!(input.peek(), Some((Token::Apostrophe, _))) {
            let position = input.peek().expect("token streams always end with EOF").1;
            return Err(PERR::MissingToken(
                "a trait".into(),
                "after a higher-ranked bound's lifetimes".into(),
            )
            .into_err(position));
        }
        if matches!(input.peek(), Some((Token::Apostrophe, _))) {
            lifetimes.push(parse_lifetime(input)?.name);
        } else if skip_precise_capture(input, closing.as_deref_mut())?.is_some() {
        } else if let Some((Token::Identifier(name) | Token::Reserved(name), position)) =
            input.peek()
            && is_fn_trait(name)
        {
            // `F: Fn(A) -> B`: a function type bound.
            let start = *position;
            let mutable = name.as_str() != "Fn";
            input.next();
            bounds.push(parse_fn_signature(input, start, mutable, false)?);
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
                    erase_bounds_lifetimes(
                        &mut bounds[first_bound..],
                        &mut projections[first_projection..],
                        &binder,
                    );
                    return Ok(bounds);
                }
                _ => reject_split(split)?,
            }
        }
        erase_bounds_lifetimes(
            &mut bounds[first_bound..],
            &mut projections[first_projection..],
            &binder,
        );
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
    self_bounds: Option<&mut Vec<type_ref::Path>>,
) -> ParseResult<()> {
    parse_where_with(
        input,
        generics,
        self_bounds,
        &mut crate::item::Lifetimes::default(),
    )
}

/// [`parse_where_clause`], adding its outlives bounds (`'b: 'a`, `T: 'a`)
/// to `lifetimes`.
fn parse_where_with(
    input: &mut TokenStream,
    generics: &mut [crate::item::GenericParam],
    mut self_bounds: Option<&mut Vec<type_ref::Path>>,
    lifetimes: &mut crate::item::Lifetimes,
) -> ParseResult<()> {
    if !matches!(input.peek(), Some((Token::Identifier(name), _)) if name.as_str() == "where") {
        return Ok(());
    }
    input.next();
    let mut outlives = lifetimes.bounds.to_vec();
    while !matches!(input.peek(), Some((Token::LeftBrace | Token::SemiColon, _))) {
        // `'b: 'a + 'c`.
        if matches!(input.peek(), Some((Token::Apostrophe, _))) {
            let lifetime = parse_lifetime(input)?.name;
            expect(input, Token::Colon, "after a `where` clause lifetime")?;
            loop {
                outlives.push((lifetime.clone(), parse_lifetime(input)?.name));
                if !matches!(input.peek(), Some((Token::Plus, _))) {
                    break;
                }
                input.next();
            }
            if matches!(input.peek(), Some((Token::Comma, _))) {
                input.next();
            }
            continue;
        }
        // `for<'a> &'a T: Sized`: the predicate for every `'a`.
        let binder = parse_binder(input)?;
        let unsized_start = match input.peek() {
            Some((Token::LeftBracket, _)) => Some("[_]"),
            Some((Token::Identifier(word), _)) if matches!(word.as_str(), "dyn" | "str") => {
                Some(if word.as_str() == "str" {
                    "str"
                } else {
                    "dyn _"
                })
            }
            _ => None,
        };
        let mut predicate = parse_type(input)?;
        erase_bound_lifetimes(&mut predicate, &binder);
        expect(input, Token::Colon, "after a `where` clause type")?;
        let mut projections = Vec::new();
        let mut bounded = Vec::new();
        let mut bounds = parse_bounds_with(input, &mut projections, None, &mut bounded)?;
        erase_bounds_lifetimes(&mut bounds, &mut projections, &binder);
        let name = match &predicate {
            type_ref::Type::Path(path)
                if !path.global
                    && path.segments.len() == 1
                    && path.segments[0].arguments.is_empty()
                    && (path.segments[0].name.text == "Self"
                        || generics.iter().any(|parameter| {
                            parameter.name.text == path.segments[0].name.text
                        })) =>
            {
                path.segments[0].name.clone()
            }
            _ => {
                where_other_type(&predicate, unsized_start, &bounds, &projections, &bounded)?;
                if matches!(input.peek(), Some((Token::Comma, _))) {
                    input.next();
                }
                continue;
            }
        };
        outlives.extend(bounded.into_iter().map(|lifetime| (name.clone(), lifetime)));
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
    lifetimes.bounds = outlives.into_boxed_slice();
    Ok(())
}

/// A `where` predicate on a type other than a declared parameter (`for<'a>
/// &'a T: Sized`). Rawr reads only `Sized` and the auto traits there, which
/// hold for every sized type, as rustc proves them; any other bound is an
/// error. `unsized_start` names the unsized type the predicate's first
/// token starts (`str`, `[_]`, `dyn _`), which is not `Sized` (E0277).
fn where_other_type(
    predicate: &type_ref::Type,
    unsized_start: Option<&str>,
    bounds: &[type_ref::Path],
    projections: &[crate::item::Equality],
    lifetimes: &[crate::item::Name],
) -> ParseResult<()> {
    let unsized_path = matches!(predicate, type_ref::Type::Path(path)
        if unsized_start == Some("[_]")
            && path.segments[0].name.text == type_ref::SLICE_TYPE
            || matches!(unsized_start, Some("str" | "dyn _")));
    let other = bounds
        .iter()
        .find(|bound| !is_sized_trait(bound) && !is_auto_trait(bound))
        .map(|bound| bound.span)
        .or_else(|| projections.first().map(|(_, name, _)| name.span))
        .or_else(|| lifetimes.first().map(|lifetime| lifetime.span));
    if let Some(span) = other {
        return Err(PERR::MissingToken(
            "a declared generic parameter".into(),
            "for a `where` clause bound other than `Sized` or an auto trait".into(),
        )
        .into_err(span.start()));
    }
    if unsized_path && bounds.iter().any(is_sized_trait) {
        return Err(PERR::BadInput(crate::LexError::ImproperSymbol(
            "Sized".into(),
            format!(
                "the size of a `{}` is not known at compile time, as in Rust (E0277)",
                unsized_start.unwrap_or("_")
            ),
        ))
        .into_err(predicate.span().start()));
    }
    Ok(())
}

/// Replace each `Self` in `ty` with `owner`.
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
fn parse_associated_const(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    attributes: Box<[crate::item::Attribute]>,
    visibility: Option<Span>,
    start: Position,
    declaration: bool,
) -> ParseResult<crate::item::Function> {
    input.next();
    // `const fn new() -> Self` is an associated function.
    if matches!(input.peek(), Some((Token::Fn, _))) {
        return parse_function(
            input,
            tokenizer_control,
            attributes,
            visibility,
            start,
            declaration,
        );
    }
    let name = parse_name(input, "after 'const'")?;
    let header = crate::item::Header {
        attributes,
        visibility,
        span: Span::NONE,
        scope: None,
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
            lifetimes: crate::item::Lifetimes::default(),
            receiver_lifetime: None,
            receiver: None,
            parameters: Box::default(),
            output: Some(output),
            body_span: semicolon,
            body_syntax: crate::item::BodySyntax::new(Vec::new()),
            constant: true,
            asynchronous: false,
            span: Span::new(start, semicolon.end()),
        });
    }
    let equals = expect(input, Token::Equals, "after a constant type")?;
    finish_associated_const(input, header, start, name, output, equals)
}

/// The value of an associated const after its `=`, its body as a
/// top-level const's is.
fn finish_associated_const(
    input: &mut TokenStream,
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
        match scan.next(input).expect("token streams always end with EOF") {
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
        lifetimes: crate::item::Lifetimes::default(),
        receiver_lifetime: None,
        receiver: None,
        parameters: Box::default(),
        output: Some(output),
        body_span: Span::new(value_start, semicolon),
        body_syntax: crate::item::BodySyntax::new(tokens),
        constant: true,
        asynchronous: false,
        span,
    })
}

/// Parse `: T = value;` after `const NAME`.
fn parse_const_rest(
    input: &mut TokenStream,
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
        match scan.next(input).expect("token streams always end with EOF") {
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
        lifetimes: crate::item::Lifetimes::default(),
        receiver_lifetime: None,
        receiver: None,
        parameters: Box::default(),
        output: Some(output),
        body_span: Span::new(value_start, semicolon),
        body_syntax: crate::item::BodySyntax::new(tokens),
        constant: true,
        asynchronous: false,
        span,
    })
}

/// rustc's report for `const NAME = value;`.
fn missing_const_type(name: &crate::item::Name) -> crate::ParseError {
    PERR::Unexpected {
        message: "missing type for `const` item".into(),
        label: "provide a type for the constant".into(),
        help: None,
        span: name.span,
    }
    .into_err(name.span.start())
}

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

/// A function's output after its `->`: a type, or the never type `!`,
/// the builtin path `!`, which stable Rust names only there.
fn parse_output_type(input: &mut TokenStream) -> ParseResult<type_ref::Type> {
    let Some((Token::Bang, position)) = input.peek() else {
        return parse_type(input);
    };
    let span = token_span(*position, "!");
    input.next();
    Ok(type_ref::Type::Path(type_ref::Path {
        segments: Box::new([type_ref::Segment {
            name: crate::item::Name {
                text: type_ref::NEVER_TYPE.into(),
                span,
            },
            arguments: Box::default(),
            span,
        }]),
        span,
        global: false,
    }))
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

/// `Sized`, bare or by its std path. Every type a Rawr value has is sized,
/// so a bound by it asks nothing; as a supertrait it makes the trait dyn
/// incompatible, as in Rust.
pub(crate) fn is_sized_trait(path: &type_ref::Path) -> bool {
    let Some((last, prefix)) = path.segments.split_last() else {
        return false;
    };
    last.arguments.is_empty()
        && last.name.text == "Sized"
        && prefix.iter().all(|segment| {
            segment.arguments.is_empty()
                && matches!(segment.name.text.as_str(), "std" | "core" | "marker")
        })
}

/// `for<'a, 'b>` before a bound, a `where` predicate or a function type:
/// the lifetimes it binds, or none without one.
fn parse_binder(input: &mut TokenStream) -> ParseResult<Vec<crate::item::Name>> {
    if !matches!(input.peek(), Some((Token::For, _))) {
        return Ok(Vec::new());
    }
    input.next();
    expect(
        input,
        Token::LessThan,
        "after `for` in a higher-ranked bound",
    )?;
    let mut lifetimes = Vec::new();
    while !matches!(input.peek(), Some((Token::GreaterThan, _))) {
        lifetimes.push(parse_lifetime(input)?.name);
        consume_comma_or_end(input, Token::GreaterThan, "between bound lifetimes")?;
    }
    expect(
        input,
        Token::GreaterThan,
        "to close a higher-ranked bound's lifetimes",
    )?;
    Ok(lifetimes)
}

/// `ty` with each of the lifetimes a `for<'a>` binds elided: `for<'a>
/// Fn(&'a T)` is `Fn(&T)` and `for<'a> fn(&'a T)` is `fn(&T)`, as Rust
/// reads an elided lifetime there, for every lifetime.
fn erase_bound_lifetimes(ty: &mut type_ref::Type, bound: &[crate::item::Name]) {
    match ty {
        type_ref::Type::Unit(_) => {}
        type_ref::Type::Tuple { members, .. } => {
            for member in members.iter_mut() {
                erase_bound_lifetimes(member, bound);
            }
        }
        type_ref::Type::Path(path) => erase_path_lifetimes(path, bound),
        type_ref::Type::Reference {
            lifetime, target, ..
        } => {
            if lifetime
                .as_ref()
                .is_some_and(|lifetime| bound.iter().any(|name| name.text == lifetime.name.text))
            {
                *lifetime = None;
            }
            erase_bound_lifetimes(target, bound);
        }
    }
}

/// [`erase_bound_lifetimes`] of the bounds a `for<'a>` binder starts and
/// their equalities.
fn erase_bounds_lifetimes(
    bounds: &mut [type_ref::Path],
    projections: &mut [crate::item::Equality],
    binder: &[crate::item::Name],
) {
    if binder.is_empty() {
        return;
    }
    for bound in bounds {
        erase_path_lifetimes(bound, binder);
    }
    for (_, _, ty) in projections {
        erase_bound_lifetimes(ty, binder);
    }
}

/// [`erase_bound_lifetimes`] of a path's arguments: a lifetime argument
/// the binder binds is `'_`.
fn erase_path_lifetimes(path: &mut type_ref::Path, bound: &[crate::item::Name]) {
    for segment in path.segments.iter_mut() {
        for argument in segment.arguments.iter_mut() {
            match argument {
                type_ref::Argument::Type(ty) => erase_bound_lifetimes(ty, bound),
                type_ref::Argument::Lifetime(lifetime)
                    if bound.iter().any(|name| name.text == lifetime.name.text) =>
                {
                    lifetime.name.text = "_".into();
                }
                type_ref::Argument::Lifetime(_) => {}
            }
        }
    }
}

/// A type whose last generic list may close with the first `>` of a `>>`;
/// the second is left in `split` for the enclosing list.
fn parse_type_split(
    input: &mut TokenStream,
    split: &mut Option<Position>,
) -> ParseResult<type_ref::Type> {
    // `for<'a> fn(&'a T) -> &'a T`: a function pointer for every `'a`.
    if matches!(input.peek(), Some((Token::For, _))) {
        let bound = parse_binder(input)?;
        if !matches!(input.peek(), Some((Token::Fn, _))) {
            let position = input.peek().expect("token streams always end with EOF").1;
            return Err(PERR::MissingToken(
                "fn".into(),
                "after a higher-ranked type's lifetimes".into(),
            )
            .into_err(position));
        }
        let mut ty = parse_type_split(input, split)?;
        erase_bound_lifetimes(&mut ty, &bound);
        return Ok(ty);
    }
    if let Some((Token::Unit, position)) = input.peek() {
        let position = *position;
        input.next();
        return Ok(type_ref::Type::Unit(token_span(position, "()")));
    }

    // `!` outside a function's output is unstable in Rust.
    if let Some((Token::Bang, position)) = input.peek() {
        return Err(
            crate::resolve::std_path::experimental_never(token_span(*position, "!")).into(),
        );
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
            type_ref::SLICE_TYPE.into()
        };
        let end = expect(
            input,
            Token::RightBracket,
            "to close an array or slice type",
        )?;
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
            global: false,
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
        // `dyn for<'a> Fn(&'a T)`.
        let binder = parse_binder(input)?;
        if let Some((Token::Identifier(name) | Token::Reserved(name), _)) = input.peek()
            && is_fn_trait(name)
        {
            let mutable = name.as_str() != "Fn";
            input.next();
            let mut path = parse_fn_signature(input, start, mutable, false)?;
            erase_path_lifetimes(&mut path, &binder);
            return skip_fn_bounds(input, split, path).map(type_ref::Type::Path);
        }
        // `dyn Iterator<Item = T>` is Rawr's iterator of `T`, as `impl
        // Iterator<Item = T>` is: a chain or the array of its items.
        if binder.is_empty()
            && let Some((Token::Identifier(name), _)) = input.peek()
            && name.as_str() != "IntoIterator"
            && let Some(name) = iterator_trait(name)
        {
            input.next();
            return parse_iterator_type(input, split, name, start);
        }
        // `dyn Shape<Output = i64>`: each binding is the argument
        // `=Output<i64>` of the trait's segment.
        let mut bindings = Vec::new();
        let mut path = parse_path_bindings(input, split, Some(&mut bindings))?;
        erase_path_lifetimes(&mut path, &binder);
        for (_, ty) in &mut bindings {
            erase_bound_lifetimes(ty, &binder);
        }
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
                    global: false,
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
            if let Some(end) = skip_precise_capture(input, Some(split))? {
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
            global: false,
        }));
    }

    // `fn(A) -> B`, `impl Fn(A) -> B` and `dyn Fn(A) -> B`: Rawr's one
    // function type; `impl Iterator<Item = T>`.
    match fn_type_start(input)? {
        Some(ImplStart::Fn(start, mutable, binder, pointer)) => {
            let mut path = parse_fn_signature(input, start, mutable, pointer)?;
            erase_path_lifetimes(&mut path, &binder);
            return skip_fn_bounds(input, split, path).map(type_ref::Type::Path);
        }
        Some(ImplStart::Iterator(name, start)) => {
            return parse_iterator_type(input, split, name, start)
        }
        Some(ImplStart::Bounds(start, binder)) => {
            let mut ty = parse_impl_bounds(input, split, start)?;
            erase_bound_lifetimes(&mut ty, &binder);
            return Ok(ty);
        }
        None => {}
    }

    expect_type_path(input)?;
    parse_path_split(input, split).map(type_ref::Type::Path)
}

/// rustc's syntax error where a type's path should start and none does:
/// "expected type, found `)`". A `?` there starts a bound (`?Sized`), whose
/// trait is then expected: "expected identifier, found `,`".
fn expect_type_path(input: &mut TokenStream) -> ParseResult<()> {
    let mut bound = false;
    while matches!(input.peek(), Some((Token::Reserved(symbol), _)) if symbol.as_str() == "?") {
        input.next();
        bound = true;
    }
    let (token, position) = input.peek().expect("token streams always end with EOF");
    let path = is_name(token)
        || matches!(token, Token::DoubleColon | Token::SelfValue)
        || matches!(token, Token::Reserved(keyword) if keyword.as_str() == "super");
    if path || matches!(token, Token::LexError(_)) {
        return Ok(());
    }
    let expected = if bound { "identifier" } else { "type" };
    Err(unexpected(
        format!("expected {expected}, found {}", found(token)),
        &format!("expected {expected}"),
        None,
        token,
        *position,
    ))
}

/// `impl Fn(A) -> B + 'a + Send`: the lifetimes and auto traits after a
/// function type. Rawr's one function type keeps the lifetimes, which bound
/// what the function value captures, for the borrow checker (D572.4).
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
            bound_fn_type(&mut path, lifetime);
            continue;
        }
        if let Some(end) = skip_precise_capture(input, Some(split))? {
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

/// The function type `path` is bounded by `lifetime`: what its value
/// captures outlives it.
fn bound_fn_type(path: &mut type_ref::Path, lifetime: type_ref::Lifetime) {
    if let [segment] = &mut path.segments[..] {
        let mut arguments = std::mem::take(&mut segment.arguments).into_vec();
        arguments.push(type_ref::Argument::Lifetime(lifetime));
        segment.arguments = arguments.into_boxed_slice();
    }
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
    /// `fn(..)` or `impl Fn*(..)`, its head consumed, whether its call
    /// takes it `&mut` (`FnMut`, `FnOnce`), the lifetimes a `for<'a>`
    /// after `impl` binds, and whether it is a `fn` pointer.
    Fn(Position, bool, Vec<crate::item::Name>, bool),
    /// `impl Iterator` (or another iterator trait), its builtin name.
    Iterator(&'static str, Position),
    /// `impl Bound + Bound` of other traits, after `impl`, and the
    /// lifetimes a `for<'a>` after `impl` binds.
    Bounds(Position, Vec<crate::item::Name>),
}

/// The start of a function type, after consuming its `fn` or `impl Fn*`
/// head; `None`, consuming nothing, before any other type.
fn fn_type_start(input: &mut TokenStream) -> ParseResult<Option<ImplStart>> {
    let start = match input.peek() {
        Some((Token::Fn, position)) => {
            let position = *position;
            input.next();
            return Ok(Some(ImplStart::Fn(position, false, Vec::new(), true)));
        }
        Some((Token::Impl, position)) => *position,
        _ => return Ok(None),
    };
    input.next();
    // `impl for<'a> Fn(&'a T)`.
    let binder = parse_binder(input)?;
    let iterator = match input.peek() {
        Some((Token::Identifier(name) | Token::Reserved(name), _)) if is_fn_trait(name) => {
            let mutable = name.as_str() != "Fn";
            input.next();
            return Ok(Some(ImplStart::Fn(start, mutable, binder, false)));
        }
        Some((Token::Identifier(name), _)) if binder.is_empty() => iterator_trait(name),
        _ => None,
    };
    Ok(Some(match iterator {
        Some(name) => {
            input.next();
            ImplStart::Iterator(name, start)
        }
        None => ImplStart::Bounds(start, binder),
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
                    text: format!("{}{}={}", type_ref::BINDING_PREFIX, owner.text, name.text)
                        .into(),
                    span: owner.span,
                },
                arguments: Box::new([type_ref::Argument::Type(ty)]),
                span,
            }]),
            span,
            global: false,
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
        global: false,
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
            type_ref::Type::Path(path) if matches!(&path.segments[..], [segment] if segment.name.text == type_ref::IMPL_TYPE) =>
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
                    .map(|bound| {
                        bound
                            .segments
                            .last()
                            .map_or("", |last| last.name.text.as_str())
                    })
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
                    lifted: true,
                });
                *ty = type_ref::Type::Path(type_ref::Path {
                    span: path.span,
                    segments: Box::new([type_ref::Segment {
                        name,
                        arguments: Box::default(),
                        span: path.span,
                    }]),
                    global: false,
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
        if let Some(end) = skip_precise_capture(input, Some(split))? {
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
        global: false,
    })
}

/// `Fn`, `FnMut` or `FnOnce`: Rawr's function values implement all three.
fn is_fn_trait(name: &str) -> bool {
    matches!(name, "Fn" | "FnMut" | "FnOnce")
}

/// `(A, B) -> C` after a function type's head: the builtin path `fn` whose
/// arguments are the inputs, then the output (`()` when omitted). A `fn`
/// pointer's output may be `!`; an `Fn` trait's is its `Output` type,
/// where `!` is unstable.
fn parse_fn_signature(
    input: &mut TokenStream,
    start: Position,
    mutable: bool,
    pointer: bool,
) -> ParseResult<type_ref::Path> {
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
        let output = if pointer {
            parse_output_type(input)?
        } else {
            parse_type(input)?
        };
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
                text: if mutable {
                    type_ref::FN_MUT_TYPE
                } else {
                    type_ref::FN_TYPE
                }
                .into(),
                span: token_span(start, "fn"),
            },
            arguments: arguments.into_boxed_slice(),
            span,
        }]),
        span,
        global: false,
    })
}

/// A generic parameter bounded by `Fn(A) -> B` is that function type, as
/// Rawr has one function type: each use of the parameter in the signature
/// is replaced by it, bounded by the parameter's `outlives` bounds, and the
/// parameter is dropped.
fn erase_fn_bounds(
    generics: &mut Vec<crate::item::GenericParam>,
    parameters: &mut [crate::item::Parameter],
    output: &mut Option<type_ref::Type>,
    outlives: &[(crate::item::Name, crate::item::Name)],
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
            matches!(&bound.segments[..], [segment] if type_ref::is_fn_type(&segment.name.text))
        }) else {
            return true;
        };
        let mut bound = bound.clone();
        for (_, lifetime) in outlives
            .iter()
            .filter(|(parameter, _)| parameter.text == generic.name.text)
        {
            bound_fn_type(
                &mut bound,
                type_ref::Lifetime {
                    name: lifetime.clone(),
                    span: lifetime.span,
                },
            );
        }
        let replacement = type_ref::Type::Path(bound);
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
    // `::image::ImageError`: a leading `::` names an outside crate.
    let global = matches!(input.peek(), Some((Token::DoubleColon, _)));
    if global {
        input.next();
    }

    loop {
        // `self::m::Item`: a path from this module, as rustc reads it.
        let name = match input.peek() {
            Some((Token::SelfValue, position)) if segments.is_empty() && !global => {
                let span = token_span(*position, "self");
                input.next();
                crate::item::Name {
                    span,
                    text: "self".into(),
                }
            }
            _ => parse_use_segment(input, "as a type path segment")?,
        };
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
        global,
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

fn parse_body_syntax(
    input: &mut TokenStream,
    tokenizer_control: &TokenizerControl,
    open: Position,
) -> ParseResult<(Span, crate::item::BodySyntax)> {
    let mut scan = ScanState::inside_body();
    // Each block owns only imports not already assigned by a nested block/body.
    let mut scopes = vec![(1, open, NESTED.with_borrow(|nested| nested.len()))];
    let mut tokens = Vec::new();
    // Whether a statement may start here: a nested `fn` item.
    let mut boundary = true;
    // Whether the last token can end an expression, and what that depends
    // on: `pub(crate)` and `extern "C"` lead items, not expressions.
    let mut ends_expression = false;
    let mut after_pub = false;
    let mut after_extern = false;
    let mut visibility = None;
    // The delimiter depths inside the braces of open struct literals, as
    // `S { x: 1 }`, which ends an expression as `1` does.
    let mut literals: Vec<usize> = Vec::new();
    // The heads of `if`, `while`, `for`, `match` and `->` whose block has
    // not opened yet: there `x {` opens the block, as in Rust.
    let mut heads: Vec<Head> = Vec::new();
    // The delimiter depths inside the arguments of open macro invocations,
    // as `m!(..)`, whose tokens need not be Rust expressions.
    let mut macros: Vec<usize> = Vec::new();
    // Whether the last tokens are a macro's name and `!`, and its own name
    // after `macro_rules!`: a delimiter here opens the macro's tokens.
    let mut invocation = false;
    // Whether the last token is a name that a struct literal's `{` may
    // follow.
    let mut after_name = false;
    // Where a `const` that starts a statement is: `const fn f()` is a
    // nested `fn` item.
    let mut const_start: Option<Position> = None;

    loop {
        if (boundary || const_start.is_some())
            && let Some((Token::Fn, position)) = input.peek()
        {
            let position = match const_start.take() {
                Some(start) => {
                    tokens.pop();
                    start
                }
                None => *position,
            };
            let function = parse_function(
                input,
                tokenizer_control,
                Box::default(),
                None,
                position,
                false,
            )?;
            NESTED.with_borrow_mut(|nested| nested.push(crate::item::Parsed::Function(function)));
            continue;
        }
        // `const NAME: T = value;` is an item of its block, as in Rust: the
        // block names it before its declaration too, and so do the items
        // nested there (#1631). A macro's tokens are its own.
        if let Some(start) = const_start
            && macros.is_empty()
            && let Some((Token::Identifier(word), _)) = input.peek()
            && !matches!(word.as_str(), "unsafe" | "extern")
        {
            const_start = None;
            tokens.pop();
            let name = parse_name(input, "after 'const'")?;
            if matches!(input.peek(), Some((Token::Equals, _))) {
                return Err(missing_const_type(&name));
            }
            let header = crate::item::Header {
                attributes: Box::default(),
                visibility: None,
                span: Span::NONE,
                scope: None,
            };
            let constant = parse_const_rest(input, header, start, name)?;
            NESTED.with_borrow_mut(|nested| nested.push(crate::item::Parsed::Function(constant)));
            // A statement may start after the item, as after its `;`.
            boundary = true;
            ends_expression = false;
            after_name = false;
            continue;
        }
        // `struct`, `enum`, `impl`, `trait` and `type` items inside a body,
        // with their attributes, join the file's items as `fn` items do.
        if boundary
            && let Some((token, position)) = input.peek()
            && (is_attribute_marker(token) || begins_body_item(token))
        {
            let start = *position;
            let mut attributes = Vec::new();
            while input
                .peek()
                .is_some_and(|(token, _)| is_attribute_marker(token))
            {
                attributes.extend(parse_attribute(input)?);
            }
            if input
                .peek()
                .is_some_and(|(token, _)| begins_body_item(token))
            {
                let item = parse_item_with_leading(
                    input,
                    tokenizer_control,
                    attributes.into_boxed_slice(),
                    None,
                    start,
                )?;
                if let Some(item) = item {
                    NESTED.with_borrow_mut(|nested| nested.push(item));
                }
                continue;
            }
            // A statement's attribute, as `#[allow(..)]`, has no effect.
            if !attributes.is_empty() {
                continue;
            }
        }
        // `unsafe impl Send for T {}` is an item; `unsafe { .. }` is a block.
        if boundary
            && let Some((Token::Identifier(text), position)) = input.peek()
            && text.as_str() == "unsafe"
        {
            let start = *position;
            let (token, position) = scan.next(input).expect("peeked unsafe must exist");
            if matches!(input.peek(), Some((Token::Impl, _))) {
                let item = parse_impl(input, tokenizer_control, Box::default(), None, start)?;
                if let Some(item) = marker_impl(item, Some(start))? {
                    NESTED.with_borrow_mut(|nested| nested.push(item));
                }
                continue;
            }
            tokens.push(body_token(token, position));
            boundary = false;
            ends_expression = false;
            after_name = false;
            after_pub = false;
            after_extern = false;
            continue;
        }
        if boundary && let Some((Token::Use, position)) = input.peek() {
            let position = *position;
            let used = parse_use(input, Box::default(), None, position)?;
            NESTED.with_borrow_mut(|nested| nested.push(crate::item::Parsed::Use(used)));
            continue;
        }
        // `extern crate serde as _serde;` in a block imports the crate's
        // root there, as a `use` does.
        if boundary
            && let Some((token, position)) = input.peek()
            && is_word(token, "extern")
        {
            let start = *position;
            let (token, position) = scan.next(input).expect("peeked extern must exist");
            if input
                .peek()
                .is_some_and(|(token, _)| is_word(token, "crate"))
            {
                let used = parse_extern_crate(input, Box::default(), None, start)?;
                NESTED.with_borrow_mut(|nested| nested.push(crate::item::Parsed::Use(used)));
                continue;
            }
            tokens.push(body_token(token, position));
            boundary = false;
            ends_expression = false;
            after_name = false;
            after_pub = false;
            after_extern = true;
            continue;
        }
        // `{ 2 fn f() {} }`: an expression that is not block-like needs a
        // `;` before an item, as in Rust.
        if !boundary
            && ends_expression
            && let Some((token, position)) = input.peek()
            && begins_statement_item(token)
        {
            return Err(PERR::MissingToken(
                Token::SemiColon.into(),
                "after the expression before this item".into(),
            )
            .into_err(*position));
        }
        // `if x == 0 2`: a condition, then a token that neither continues
        // it nor opens its block, as rustc reports it.
        if ends_expression
            && macros.is_empty()
            && heads.last().is_some_and(|head| {
                head.condition && !head.pattern && head.depth == scan.delimiter_depth
            })
            && let Some((token, position)) = input.peek()
            && begins_operand(token)
        {
            return Err(unexpected(
                format!("expected `{{`, found {}", found(token)),
                "expected `{`",
                None,
                token,
                *position,
            ));
        }
        const_start = match input.peek() {
            Some((Token::Const, position)) if boundary => Some(*position),
            _ => None,
        };
        if let Some((token, _)) = input.peek() {
            boundary = matches!(
                token,
                Token::LeftBrace | Token::RightBrace | Token::SemiColon
            );
            let closes_visibility =
                matches!(token, Token::RightParen) && visibility == Some(scan.delimiter_depth);
            if closes_visibility {
                visibility = None;
            }
            if matches!(token, Token::LeftParen) && after_pub {
                visibility = Some(scan.delimiter_depth + 1);
            }
            ends_expression = !closes_visibility
                && match token {
                    Token::Identifier(name) => !matches!(
                        name.as_str(),
                        "unsafe" | "safe" | "extern" | "default" | "auto" | "raw"
                    ),
                    Token::StringConstant(_) => !after_extern,
                    Token::FloatConstant(_) => true,
                    Token::IntegerConstant(_)
                    | Token::ByteConstant(_)
                    | Token::CharConstant(_)
                    | Token::True
                    | Token::False
                    | Token::RightParen
                    | Token::RightBracket => true,
                    Token::Reserved(word) => word.as_str() == "?",
                    _ => false,
                };
            after_pub = matches!(token, Token::Pub);
            after_extern = matches!(token, Token::Identifier(name) if name.as_str() == "extern");
            let depth = scan.delimiter_depth;
            match token {
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace if invocation => {
                    macros.push(depth + 1);
                }
                Token::RightParen | Token::RightBracket | Token::RightBrace
                    if macros.last() == Some(&depth) =>
                {
                    macros.pop();
                }
                _ => {}
            }
            invocation = match token {
                Token::Bang => after_name,
                Token::Identifier(_) => invocation,
                _ => false,
            };
            match token {
                Token::If | Token::While => heads.push(Head::condition(depth, false)),
                Token::ThinArrow => heads.push(Head {
                    depth,
                    pattern: false,
                    condition: false,
                }),
                Token::For => heads.push(Head::condition(depth, true)),
                Token::Reserved(word) if word.as_str() == "match" => {
                    heads.push(Head::condition(depth, false));
                }
                Token::Let => {
                    if let Some(head) = heads.last_mut()
                        && head.depth == depth
                    {
                        head.pattern = true;
                    }
                }
                Token::Equals | Token::In => {
                    if let Some(head) = heads.last_mut()
                        && head.depth == depth
                    {
                        head.pattern = false;
                    }
                }
                // A match arm's guard ends at its `=>`, with no block.
                Token::SemiColon | Token::DoubleArrow => {
                    while heads.last().is_some_and(|head| head.depth >= depth) {
                        heads.pop();
                    }
                }
                Token::LeftBrace => match heads.last() {
                    Some(head) if head.depth == depth => {
                        if !head.pattern {
                            heads.pop();
                        }
                    }
                    _ if after_name => literals.push(depth + 1),
                    _ => {}
                },
                Token::RightBrace if literals.last() == Some(&depth) => {
                    literals.pop();
                    boundary = false;
                    ends_expression = true;
                }
                _ => {}
            }
            after_name = matches!(token, Token::Identifier(name) if !matches!(
                name.as_str(),
                "unsafe" | "move" | "async" | "try" | "extern" | "union"
            ));
        }
        let (token, position) = scan.next(input).expect("token streams always end with EOF");
        if matches!(token, Token::LeftBrace) {
            scopes.push((
                scan.delimiter_depth,
                position,
                NESTED.with_borrow(|nested| nested.len()),
            ));
        } else if matches!(token, Token::RightBrace)
            && scopes
                .last()
                .is_some_and(|(depth, _, _)| *depth == scan.delimiter_depth + 1)
        {
            let (_, start, first) = scopes.pop().expect("matched enclosing block");
            let scope = Span::new(start, after(position, 1));
            NESTED.with_borrow_mut(|nested| {
                for item in &mut nested[first..] {
                    item.enclose(scope);
                }
            });
        }
        match (token, position) {
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
                tokens.push(body_token(token, position));
            }
        }
    }
}

/// The head of an `if`, `while`, `for`, `match` or `->` in a body, whose
/// block has not opened yet.
struct Head {
    /// The delimiter depth the head is at.
    depth: usize,
    /// Whether a pattern is being read: after `let` until `=`, and after
    /// `for` until `in`, where `S { x }` is a pattern.
    pattern: bool,
    /// Whether the head is a condition or a scrutinee, which an expression
    /// ends and a block follows: all but a closure's `->`.
    condition: bool,
}

impl Head {
    fn condition(depth: usize, pattern: bool) -> Self {
        Self {
            depth,
            pattern,
            condition: true,
        }
    }
}

/// Whether `token` starts an operand and nothing else: a literal or a name,
/// which never continues the expression before it.
fn begins_operand(token: &Token) -> bool {
    match token {
        Token::IntegerConstant(_)
        | Token::FloatConstant(_)
        | Token::StringConstant(_)
        | Token::ByteConstant(_)
        | Token::CharConstant(_)
        | Token::True
        | Token::False => true,
        Token::Identifier(name) => !matches!(name.as_str(), "as" | "else" | "in"),
        _ => false,
    }
}

/// A private top-level `fn`: a typed item, as in Rust.
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
        scope: None,
    };
    let generics = if matches!(input.peek(), Some((Token::LessThan, _))) {
        parse_generics(input)?
    } else {
        (Vec::new(), crate::item::Lifetimes::default())
    };
    let open = if matches!(input.peek(), Some((Token::Unit, _))) {
        input.next();
        None
    } else {
        expect(input, Token::LeftParen, "after a function name")?;
        Some(None)
    };
    Ok(Some(crate::item::Parsed::Function(parse_function_rest(
        input,
        tokenizer_control,
        header,
        start,
        name,
        generics,
        open,
        false,
    )?)))
}

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
        Token::Multiply => (Kind::Star, 1),
        Token::PowerOf => (Kind::StarStar, 2),
        Token::Ampersand => (Kind::Ampersand, 1),
        Token::And => (Kind::AndAnd, 2),
        Token::LessThan => (Kind::LessThan, 1),
        Token::GreaterThan => (Kind::GreaterThan, 1),
        Token::As => (Kind::As, 2),
        Token::SelfValue => (Kind::SelfValue, 4),
        Token::Mut => (Kind::Mut, 3),
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
            global: false,
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
    trees.push(crate::item::UseTree::Path(
        path_of(&segments),
        parse_use_alias(input)?,
    ));
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
                    global: false,
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

/// Whether `token` is a name.
fn is_name(token: &Token) -> bool {
    matches!(token, Token::Identifier(_))
}

fn parse_name(input: &mut TokenStream, context: &str) -> ParseResult<crate::item::Name> {
    match input.next().expect("token streams always end with EOF") {
        (Token::Identifier(name), position) => Ok(crate::item::Name {
            span: token_span(position, &name),
            text: (*name).into(),
        }),
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

/// The `:` after a parameter's name. A parameter with no type is rustc's
/// parse error: `fn sink(value)`.
fn expect_parameter_type(input: &mut TokenStream) -> ParseResult<Position> {
    match input.next().expect("token streams always end with EOF") {
        (Token::Colon, position) => Ok(position),
        (Token::LexError(error), position) => Err(error.into_err(position)),
        (token, position) => Err(unexpected(
            format!("expected one of `:`, `@`, or `|`, found {}", found(&token)),
            "expected one of `:`, `@`, or `|`",
            Some("if this is a parameter name, give it a type"),
            &token,
            position,
        )),
    }
}

/// rustc's name for a token it did not expect: `` `)` ``, `` keyword `fn` ``,
/// `` reserved keyword `do` `` or `` `<eof>` ``.
pub(crate) fn found(token: &Token) -> String {
    /// Rust's keywords, which rustc names as keywords.
    const KEYWORDS: [&str; 38] = [
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
        "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
        "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait",
        "true", "type", "unsafe", "use", "where", "while",
    ];
    /// The words Rust reserves for later, which rustc names as reserved
    /// keywords.
    const RESERVED: [&str; 14] = [
        "abstract", "become", "box", "do", "final", "gen", "macro", "override", "priv", "try",
        "typeof", "unsized", "virtual", "yield",
    ];
    match token {
        Token::EOF => "`<eof>`".into(),
        Token::Unit => "`(`".into(),
        token => {
            let text = token.to_string();
            let text = crate::tokenizer::raw_names(&text);
            if KEYWORDS.contains(&text.as_ref()) {
                format!("keyword `{text}`")
            } else if RESERVED.contains(&text.as_ref()) {
                format!("reserved keyword `{text}`")
            } else {
                format!("`{text}`")
            }
        }
    }
}

/// Syntax Rust does not have at `token`, as rustc reports it.
pub(crate) fn unexpected(
    message: String,
    label: &str,
    help: Option<&str>,
    token: &Token,
    position: Position,
) -> crate::ParseError {
    let width = match token {
        Token::EOF => 0,
        Token::Unit => 1,
        token => token.to_string().chars().count(),
    };
    PERR::Unexpected {
        message,
        label: label.into(),
        help: help.map(Into::into),
        span: Span::new(position, after(position, width)),
    }
    .into_err(position)
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

    fn texts(attributes: &[crate::item::Attribute]) -> Vec<&str> {
        attributes
            .iter()
            .map(|attribute| attribute.text.as_str())
            .collect()
    }

    #[test]
    fn keeps_item_field_and_variant_attributes_as_written() {
        let source = r#"
#[derive(Debug, Clone)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[doc = "A task"]
struct Task {
    #[serde(default = "Uuid::new_v4")]
    id: i64,
    #[cfg(test)]
    missing: String,
    #[serde(skip)] #[allow(dead_code)]
    pub weight: i64,
}
struct Pair(#[serde(with = "m")] i64, i64);
enum Filter {
    #[serde(rename = "all")]
    #[default]
    All,
    Done { #[serde(skip_serializing_if = "Option::is_none")] count: i64 },
    Both(#[serde(with = "m")] i64, i64),
}
"#;
        let unit = Engine::new().parse_unit(source).unwrap();
        let Parsed::Struct(task) = &unit.items()[0] else {
            panic!("expected the struct Task")
        };
        assert_eq!(
            texts(&task.header.attributes),
            [
                "derive(Debug, Clone)",
                r#"serde(rename_all = "camelCase", deny_unknown_fields)"#,
                r#"doc = "A task""#,
            ]
        );
        assert_eq!(task.header.attributes[0].arguments.len(), 2);
        assert!(task.header.attributes[1].arguments.is_empty());
        assert_eq!(
            task.header.attributes[1].path.segments[0].name.text,
            "serde"
        );
        let fields = task
            .fields
            .iter()
            .map(|field| (field.name.text.as_str(), texts(&field.attributes)))
            .collect::<Vec<_>>();
        assert_eq!(
            fields,
            [
                ("id", vec![r#"serde(default = "Uuid::new_v4")"#]),
                ("weight", vec!["serde(skip)", "allow(dead_code)"]),
            ]
        );

        let Parsed::Struct(pair) = &unit.items()[1] else {
            panic!("expected the struct Pair")
        };
        assert_eq!(texts(&pair.fields[0].attributes), [r#"serde(with = "m")"#]);
        assert!(pair.fields[1].attributes.is_empty());

        let Parsed::Enum(filter) = &unit.items()[2] else {
            panic!("expected the enum Filter")
        };
        let [all, done, both] = &*filter.variants else {
            panic!("expected three variants")
        };
        assert_eq!(
            texts(&all.attributes),
            [r#"serde(rename = "all")"#, "default"]
        );
        assert_eq!(all.default, Some(all.attributes[1].span));
        assert!(all.field_attributes.is_empty());
        assert!(done.attributes.is_empty());
        assert_eq!(done.field_attributes.len(), 1);
        assert_eq!(
            texts(&done.field_attributes[0]),
            [r#"serde(skip_serializing_if = "Option::is_none")"#]
        );
        assert_eq!(both.field_attributes.len(), 2);
        assert_eq!(texts(&both.field_attributes[0]), [r#"serde(with = "m")"#]);
        assert!(both.field_attributes[1].is_empty());
    }

    #[test]
    fn a_derive_keeps_rejecting_arguments_that_are_not_paths() {
        assert!(Engine::new()
            .parse_unit("#[derive(Debug = 1)] struct Value;")
            .is_err());
    }

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
    fn items_after_an_expression_need_a_semicolon_unless_it_is_block_like() {
        for (source, line, column) in [
            (
                "fn main() {\n    let v = { 1\n        struct S { x: i64 }\n    };\n}",
                3,
                9,
            ),
            ("fn main() {\n    let w = { 2 fn f() {} };\n}", 2, 17),
            ("fn main() {\n    let s = S { x: 1 } struct B;\n}", 2, 24),
            ("fn main() {\n    S { x: T { y: 2 } } fn f() {}\n}", 2, 25),
        ] {
            let error = Engine::new().parse_unit(source).unwrap_err();
            assert!(
                error.to_string().contains("';' after the expression"),
                "{source}: {error}"
            );
            assert_eq!(error.position(), Position::new(line, column), "{source}");
        }

        let unit = Engine::new()
            .parse_unit(
                "fn main() {\n    struct A;\n    let x = 1;\n    if x > 0 {} struct B;\n    \
                 let w = { loop { break; } fn f() {} 5 };\n    m! { x } enum E { V }\n    \
                 let f: fn(i64) -> i64 = g;\n    if x == S {} struct C;\n    \
                 while let S { a } = s {} struct D;\n    match s { S { a } => {} } struct E;\n    \
                 for S { a } in v {} struct F;\n    let c = |s: S| -> S { s }; struct G;\n    \
                 unsafe {} struct H;\n}",
            )
            .unwrap();
        assert_eq!(unit.items().len(), 11);
    }

    /// A condition without its block is rustc's syntax error at the token
    /// after it, before the braces it shifts can close the `impl` early and
    /// leave a later method a free function with `self`.
    #[test]
    fn a_condition_without_its_block_is_the_first_error() {
        let source = "pub struct App;\n\nimpl App {\n    pub fn update(&mut self, x: i32) -> i32 {\n        \
                      match x {\n            _ => {\n                let next = if x == 0 2\n                    \
                      1\n                } else {\n                    0\n                };\n                \
                      next\n            }\n        }\n    }\n\n    pub fn view(&self) -> i32 {\n        \
                      0\n    }\n}\n";
        let error = Engine::new().parse_unit(source).unwrap_err();
        assert!(
            error.to_string().contains("expected `{`, found `2`"),
            "{error}"
        );
        assert_eq!(error.position(), Position::new(7, 38));

        for (source, found) in [
            ("fn f(x: bool) {\n    while x y {}\n}", "`y`"),
            ("fn f(v: Vec<i32>) {\n    for x in v 3 {}\n}", "`3`"),
            ("fn f(x: i32) {\n    match x \"a\" {}\n}", "`\"a\"`"),
        ] {
            let error = Engine::new().parse_unit(source).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains(&format!("expected `{{`, found {found}")),
                "{source}: {error}"
            );
        }

        Engine::new()
            .parse_unit(
                "fn f(v: Option<i32>, c: bool) -> i32 {\n    let n = match v {\n        \
                 Some(x) if c => x,\n        ref y => 0,\n    };\n    \
                 m!(if c 2 else 3);\n    if let ref z = v { 1 } else { n }\n}",
            )
            .unwrap();
    }

    #[test]
    fn ordinary_scripts_and_new_function_remain_unchanged() {
        let engine = Engine::new();
        let ast = engine.compile("fn new() -> i64 { 40 + 2 } new()").unwrap();

        assert_eq!(engine.eval_ast::<i64>(&ast).unwrap(), 42);
        assert_eq!(engine.eval::<char>("'a'").unwrap(), 'a');
    }

    #[test]
    fn backtick_strings_are_rusts_unknown_token() {
        let source = "let y = 1;\nlet greeting = `hello ${y}`;\nstruct Marker {}\n";
        let error = Engine::new().compile(source).unwrap_err();
        assert!(
            error.to_string().contains("unknown start of token: `"),
            "{error}"
        );
    }

    #[test]
    fn item_syntax_inside_literals_and_comments_is_not_discovered() {
        let engine = Engine::new();
        let source = r##"
let quoted = "struct Fake {}";
let character = '}';
let raw = r#"enum Fake {}"#;
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
}
