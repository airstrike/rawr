//! A call of the program's own `macro_rules!` macro where an item goes,
//! among a module's items or a block's statements: rustc expands it before
//! it resolves a name, and the items it expands to are that module's or
//! that block's, as written ones are.
//!
//! The expansion's text is laid out on lines past the file's last, which no
//! file has, so its items are the file's own: each has its place in the
//! source, its bodies lower from there, and an `mod name { .. }` among them
//! loads as any inline module does. Every position on those lines is
//! reported at the call ([`Expansion`]).

use std::cell::RefCell;

use crate::diagnostic::source::Expansion;
use crate::resolve::macro_rules::{self, Rules};
use crate::tokenizer::Token;
use crate::{ImmutableString, Position, Shared, Span};

/// The file being parsed, with the expansions laid out after it so far.
pub(super) struct Layout {
    text: String,
    /// How many lines `text` has.
    lines: usize,
    expansions: Vec<Expansion>,
    /// The program's own macros the items define so far, in order.
    rules: Vec<(ImmutableString, Shared<Rules>)>,
    /// The macros a block defines, visible to its end, innermost last.
    local: Vec<(ImmutableString, Shared<Rules>)>,
    /// How many expansions are made, which keeps each one's locals apart.
    hygiene: usize,
}

/// Where the text of one expansion was laid out, to take it back when its
/// tokens hold no item.
pub(super) struct Mark {
    length: usize,
    lines: usize,
    expansions: usize,
}

std::thread_local! {
    static LAYOUT: RefCell<Option<Layout>> = const { RefCell::new(None) };
    /// The macros the next file parsed may call where an item goes before
    /// it defines any: those of the modules around it it comes after.
    static INHERITED: RefCell<Vec<(ImmutableString, Shared<Rules>)>> =
        const { RefCell::new(Vec::new()) };
}

/// Let the next file parsed call `inherited` where an item goes.
pub(super) fn inherit(inherited: &[Shared<Rules>]) {
    INHERITED.set(
        inherited
            .iter()
            .map(|rules| (ImmutableString::from(rules.name()), rules.clone()))
            .collect(),
    );
}

/// Begin laying out the expansions of `source`'s calls; the layout of the
/// file whose parse this one is inside, if any, is given back.
pub(super) fn begin(source: &str) -> Option<Layout> {
    let layout = Layout {
        text: source.to_owned(),
        lines: source.split('\n').count(),
        expansions: Vec::new(),
        rules: INHERITED.take(),
        local: Vec::new(),
        hygiene: 0,
    };
    LAYOUT.replace(Some(layout))
}

/// Go on laying out expansions after `text`, a file's with `expansions`
/// laid out after it already; the layout this one is inside, if any, is
/// given back.
#[cfg(feature = "rust-import")]
pub(super) fn resume(text: &str, expansions: &[Expansion]) -> Option<Layout> {
    let outer = begin(text);
    with(|layout| layout.expansions = expansions.to_vec());
    outer
}

/// The file's text with its expansions laid out after it, and where each
/// is, when any call expanded; the layout `outer` is put back.
pub(super) fn finish(outer: Option<Layout>) -> Option<(String, Box<[Expansion]>)> {
    let layout = LAYOUT.replace(outer)?;
    (!layout.expansions.is_empty()).then(|| (layout.text, layout.expansions.into_boxed_slice()))
}

/// Note `rules`, the definition of a macro among the items, from where it
/// is on.
pub(super) fn define(rules: Rules) {
    with(|layout| {
        layout.rules.push((rules.name().into(), Shared::new(rules)));
    });
}

/// Note `rules`, a macro a block defines, to the block's end
/// ([`truncate_local`]).
pub(super) fn define_local(rules: Rules) {
    with(|layout| {
        layout.local.push((rules.name().into(), Shared::new(rules)));
    });
}

/// How many macros the blocks define so far, which a block's end goes back
/// to.
pub(super) fn local_count() -> usize {
    with(|layout| layout.local.len()).unwrap_or(0)
}

/// Forget the macros the blocks defined after the first `count`.
pub(super) fn truncate_local(count: usize) {
    with(|layout| layout.local.truncate(count));
}

/// The macro `name` names here: a block's, the innermost first, then the
/// latest definition among the items.
pub(super) fn lookup(name: &str) -> Option<Shared<Rules>> {
    with(|layout| {
        layout
            .local
            .iter()
            .rev()
            .chain(layout.rules.iter().rev())
            .find(|(defined, _)| defined.as_str() == name)
            .map(|(_, rules)| rules.clone())
    })
    .flatten()
}

/// The text the call `call` (its name, `!`, delimiters and the tokens
/// between them) expands to, laid out on lines past the file's, with what
/// takes it back: the text has as many lines before it as the expansion
/// starts at, so the lexer gives its tokens their places there. `None`
/// when no rule matches the call, or no file is being laid out, which
/// leaves the call where it is.
pub(super) fn lay_out(
    rules: &Rules,
    call: &[(Token, Position)],
    krate: &str,
    braced_statements: bool,
) -> Option<(String, Mark)> {
    let ((_, at), rest) = call.split_first()?;
    let (_, end) = call.last()?;
    // The tokens inside the delimiters, after the name and `!`.
    let inner = rest.get(2..rest.len().checked_sub(1)?)?;
    let input = macro_rules::trees(inner).ok()?;
    let hygiene = with(|layout| {
        layout.hygiene += 1;
        layout.hygiene
    })?;
    let expansion = rules.expand(&input, krate, *at, *end, hygiene).ok()?;
    Some(place(
        &render(&expansion),
        Span::new(*at, after(*end)),
        rules.name(),
        braced_statements,
    ))
}

/// Lay out `text`, what the call at `call_span` of the macro `name`
/// expands to, on lines past the file's ([`lay_out`]), in braces when it
/// is a block's statements.
pub(super) fn place(
    text: &str,
    call_span: Span,
    name: &str,
    braced_statements: bool,
) -> (String, Mark) {
    let at = call_span.start();
    with(|layout| {
        let mark = Mark {
            length: layout.text.len(),
            lines: layout.lines,
            expansions: layout.expansions.len(),
        };
        // A call on lines a call laid out is reported where that one is.
        let call = layout
            .expansions
            .iter()
            .find(|expansion| {
                at.line()
                    .is_some_and(|line| (expansion.lines.0..=expansion.lines.1).contains(&line))
            })
            .map_or(call_span, |outer| outer.call);
        let (open, close) = if braced_statements {
            ("{\n", "\n}")
        } else {
            ("", "")
        };
        let laid = format!("{open}{text}{close}");
        let first = layout.lines + 1;
        let count = laid.split('\n').count();
        layout.text.push('\n');
        layout.text.push_str(&laid);
        layout.lines += count;
        layout.expansions.push(Expansion {
            lines: (first, first + count - 1),
            call,
            name: name.into(),
        });
        let mut padded = "\n".repeat(first - 1);
        padded.push_str(&laid);
        (padded, mark)
    })
    .expect("a call is laid out while its file is")
}

/// Take back the text laid out since `mark`, and every expansion laid out
/// inside it.
pub(super) fn take_back(mark: Mark) {
    with(|layout| {
        layout.text.truncate(mark.length);
        layout.lines = mark.lines;
        layout.expansions.truncate(mark.expansions);
    });
}

fn with<T>(apply: impl FnOnce(&mut Layout) -> T) -> Option<T> {
    LAYOUT.with_borrow_mut(|layout| layout.as_mut().map(apply))
}

/// The position after `position`'s character.
fn after(mut position: Position) -> Position {
    position.advance();
    position
}

/// The text of `tokens`, as the lexer reads them back: a line ends after
/// each `;`, `{`, `}` and comment, which keeps each line short.
fn render(tokens: &[(Token, Position)]) -> String {
    let mut text = String::new();
    let mut line = Vec::new();
    for (token, position) in tokens {
        line.push((token.clone(), *position));
        if matches!(
            token,
            Token::SemiColon | Token::LeftBrace | Token::RightBrace | Token::Comment(_)
        ) {
            text.push_str(&crate::resolve::lower_nominal::token_text(&line));
            text.push('\n');
            line.clear();
        }
    }
    text.push_str(&crate::resolve::lower_nominal::token_text(&line));
    text
}
