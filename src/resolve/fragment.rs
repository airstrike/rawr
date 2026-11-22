//! How much of a macro call's input one `macro_rules!` fragment takes, as
//! rustc's parser reads it: an `expr` is one expression, a `ty` one type, a
//! `pat` one pattern, and so on. The input is token trees, so a delimited
//! group is one tree and its contents never end a fragment.

use super::macro_rules::{Delimiter, Fragment, Tree};
use crate::tokenizer::Token;

/// How one `fragment` reads at the start of a call's input, as rustc's
/// macro matcher reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Read {
    /// It takes this many trees.
    Taken(usize),
    /// The first tree can't start one, so the rule doesn't match here.
    Unstarted,
    /// It starts but rustc's parser fails on it, which ends the expansion
    /// with the parser's error rather than trying more rules.
    Broken(Broken),
}

/// Where a fragment's parse failed: the tree it failed at, the input's
/// length when it ran out, and what the parser expected there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Broken {
    at: usize,
    expected: Expected,
}

/// What rustc's parser expected where a fragment's parse failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Expected {
    Expression,
    Type,
    Pattern,
    Identifier,
    /// An item's keyword, where an `item` fragment starts with none.
    Item,
}

impl Broken {
    /// The index of the tree the parse failed at, the input's length when
    /// it ran out.
    pub(crate) const fn at(self) -> usize {
        self.at
    }

    pub(crate) const fn expected(self) -> Expected {
        self.expected
    }
}

/// How `fragment` reads at the start of `input`. Whether it starts is
/// rustc's `nonterminal_may_begin_with`: a fragment that starts commits the
/// matcher to rustc's parser, whose failure is an error. A failure whose
/// expectation the reader doesn't know is read as no start, so the call is
/// a no-rules error rather than a parser error rawr can't word.
pub(crate) fn read(fragment: Fragment, input: &[Tree]) -> Read {
    let mut reader = Reader {
        trees: input,
        at: 0,
        failed: None,
    };
    if !reader.may_begin(fragment) {
        return Read::Unstarted;
    }
    let read = match fragment {
        Fragment::Tt => reader.bump_any(),
        Fragment::Ident => reader.ident(),
        Fragment::Literal => reader.literal(),
        Fragment::Lifetime => reader.lifetime(),
        Fragment::Block => reader.group(Delimiter::Brace),
        Fragment::Vis => {
            reader.vis();
            Some(())
        }
        Fragment::Expr => reader.expr(Context::Fragment),
        Fragment::Stmt => reader.stmt(),
        Fragment::Ty => reader.ty(),
        Fragment::Path => reader.type_path(),
        Fragment::Pat => reader.pat(),
        Fragment::PatParam => reader.pat_alternative(),
        Fragment::Item => reader.item(),
        Fragment::Meta => reader.meta(),
    };
    match (read, reader.failed) {
        (Some(()), _) => Read::Taken(reader.at),
        // The furthest failure is the one that stopped the read only when
        // the read got no further.
        (None, Some(broken)) if broken.at >= reader.at => Read::Broken(broken),
        (None, _) => Read::Unstarted,
    }
}

/// Whether `token` is a literal of Rust's.
pub(crate) fn is_literal(token: &Token) -> bool {
    matches!(
        token,
        Token::IntegerConstant(_)
            | Token::FloatConstant(_)
            | Token::CharConstant(_)
            | Token::StringConstant(_)
            | Token::ByteStringConstant(_)
            | Token::ByteConstant(_)
            | Token::True
            | Token::False
    )
}

/// Where an expression is read, which decides what it may hold at its
/// top level.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    /// A fragment, or any other expression: a path followed by `{..}` is a
    /// struct expression.
    Fragment,
    /// An `if` or `while` condition: the first `{..}` after an operand is
    /// the body, and `let` binds a pattern.
    Condition,
    /// A `match` scrutinee or a `for` loop's iterator: the first `{..}`
    /// after an operand is the body.
    Scrutinee,
}

/// Words that never start an expression, a type or a pattern.
const NOT_OPERANDS: &[&str] = &[
    "as",
    "else",
    "in",
    "where",
    "fn",
    "struct",
    "enum",
    "impl",
    "mod",
    "use",
    "pub",
    "type",
    "trait",
    "extern",
    "mut",
    "ref",
    "dyn",
    "priv",
    "await",
    "box",
    "macro_rules",
];

/// The suffixes a number literal may carry, which the lexer reads as a
/// name after it.
const SUFFIXES: &[&str] = &[
    "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize", "f16",
    "f32", "f64", "f128",
];

struct Reader<'a> {
    trees: &'a [Tree],
    at: usize,
    /// The furthest place a parse found no start of what it expected.
    failed: Option<Broken>,
}

impl<'a> Reader<'a> {
    fn tree(&self, ahead: usize) -> Option<&'a Tree> {
        self.trees.get(self.at + ahead)
    }

    /// Note that what the parse `expected` doesn't start here, unless a
    /// parse already failed further on, and fail.
    fn expected<T>(&mut self, expected: Expected) -> Option<T> {
        if self.failed.is_none_or(|failed| self.at > failed.at) {
            self.failed = Some(Broken {
                at: self.at,
                expected,
            });
        }
        None
    }

    /// Whether the first tree may start `fragment`, as rustc's
    /// `nonterminal_may_begin_with` decides. A call's or a group's end
    /// starts none.
    fn may_begin(&self, fragment: Fragment) -> bool {
        let Some(tree) = self.tree(0) else {
            return false;
        };
        let word = self.word(0);
        let word = word.as_deref();
        match fragment {
            Fragment::Tt | Fragment::Item | Fragment::Stmt | Fragment::Vis => true,
            Fragment::Ident => word.is_some_and(|word| word != "_"),
            Fragment::Block => matches!(tree, Tree::Group(Delimiter::Brace, ..)),
            Fragment::Lifetime => self.at_token(&Token::Apostrophe),
            Fragment::Literal => {
                self.token(0).is_some_and(is_literal)
                    || matches!(self.token(0), Some(Token::Minus | Token::UnaryMinus))
            }
            Fragment::Path | Fragment::Meta => word.is_some() || self.at_token(&Token::DoubleColon),
            // `let` and `const` are left out for compatibility, as in Rust
            // 2021's `expr`.
            Fragment::Expr => {
                self.starts_expr(Context::Fragment) && !matches!(word, Some("let" | "const"))
            }
            Fragment::Ty => match tree {
                Tree::Group(delimiter, ..) => *delimiter != Delimiter::Brace,
                Tree::Leaf(..) => {
                    matches!(
                        self.token(0),
                        Some(
                            Token::Bang
                                | Token::Multiply
                                | Token::Ampersand
                                | Token::And
                                | Token::Apostrophe
                                | Token::LessThan
                                | Token::LeftShift
                                | Token::DoubleColon
                                | Token::Underscore
                        )
                    ) || self.at_question()
                        || word.is_some_and(|word| {
                            !NOT_OPERANDS.contains(&word)
                                || matches!(word, "fn" | "impl" | "dyn" | "extern")
                        })
                }
            },
            Fragment::Pat | Fragment::PatParam => match tree {
                Tree::Group(delimiter, ..) => *delimiter != Delimiter::Brace,
                Tree::Leaf(token, _) => {
                    word.is_some()
                        || is_literal(token)
                        || matches!(
                            token,
                            Token::Ampersand
                                | Token::And
                                | Token::Minus
                                | Token::UnaryMinus
                                | Token::Apostrophe
                                | Token::LessThan
                                | Token::LeftShift
                                | Token::DoubleColon
                                | Token::Underscore
                                | Token::Bang
                                | Token::ExclusiveRange
                                | Token::InclusiveRange
                        )
                        || self.at_reserved("...")
                        || fragment == Fragment::Pat && matches!(token, Token::Pipe)
                }
            },
        }
    }

    fn token(&self, ahead: usize) -> Option<&'a Token> {
        match self.tree(ahead)? {
            Tree::Leaf(token, _) => Some(token),
            Tree::Group(..) => None,
        }
    }

    /// The keyword or name ahead, as it is written.
    fn word(&self, ahead: usize) -> Option<String> {
        let token = self.token(ahead)?;
        if is_literal(token) {
            return None;
        }
        let text = match token {
            Token::Identifier(name) => name.to_string(),
            token => token.to_string(),
        };
        let mut chars = text.chars();
        let first = chars.next()?;
        ((first.is_alphabetic() || first == '_') && chars.all(|c| c.is_alphanumeric() || c == '_'))
            .then_some(text)
    }

    fn at_word(&self, word: &str) -> bool {
        self.word(0).is_some_and(|found| found == word)
    }

    fn at_token(&self, token: &Token) -> bool {
        self.token(0) == Some(token)
    }

    fn at_question(&self) -> bool {
        matches!(self.token(0), Some(Token::Reserved(text)) if text.as_str() == "?")
    }

    fn at_group(&self, delimiter: Delimiter) -> bool {
        matches!(self.tree(0), Some(Tree::Group(found, ..)) if *found == delimiter)
    }

    fn bump(&mut self) {
        self.at += 1;
    }

    fn bump_any(&mut self) -> Option<()> {
        self.tree(0)?;
        self.bump();
        Some(())
    }

    fn eat_token(&mut self, token: &Token) -> bool {
        let found = self.at_token(token);
        if found {
            self.bump();
        }
        found
    }

    fn eat_word(&mut self, word: &str) -> bool {
        let found = self.at_word(word);
        if found {
            self.bump();
        }
        found
    }

    fn expect_token(&mut self, token: &Token) -> Option<()> {
        self.eat_token(token).then_some(())
    }

    fn group(&mut self, delimiter: Delimiter) -> Option<()> {
        self.at_group(delimiter).then(|| self.bump())
    }

    /// `#[..]` attributes, any number.
    fn attributes(&mut self) {
        while self.at_token(&Token::Hash)
            && matches!(self.tree(1), Some(Tree::Group(Delimiter::Bracket, ..)))
        {
            self.at += 2;
        }
    }

    fn ident(&mut self) -> Option<()> {
        let word = self.word(0)?;
        (word != "_").then(|| self.bump())
    }

    /// A number's suffix after it, when it has one.
    fn suffix(&mut self) {
        if let Some(Token::Identifier(name)) = self.token(0)
            && SUFFIXES.contains(&name.as_str())
        {
            self.bump();
        }
    }

    /// A literal: `-` may start a number, as rustc's `literal` reads it.
    fn literal(&mut self) -> Option<()> {
        let negative = matches!(self.token(0), Some(Token::Minus | Token::UnaryMinus));
        let token = self.token(usize::from(negative))?;
        let number = matches!(token, Token::IntegerConstant(_) | Token::FloatConstant(_));
        if !is_literal(token) || negative && !number {
            return None;
        }
        self.at += 1 + usize::from(negative);
        if number {
            self.suffix();
        }
        Some(())
    }

    fn lifetime(&mut self) -> Option<()> {
        self.expect_token(&Token::Apostrophe)?;
        if self.eat_token(&Token::Underscore) {
            return Some(());
        }
        self.word(0)?;
        self.bump();
        Some(())
    }

    /// A visibility, which may be none: `pub`, and `(crate)`, `(self)`,
    /// `(super)` or `(in path)` after it.
    fn vis(&mut self) {
        if !self.eat_word("pub") {
            return;
        }
        if let Some(Tree::Group(Delimiter::Paren, _, inner, _)) = self.tree(0)
            && matches!(
                Reader {
                    trees: inner,
                    at: 0,
                    failed: None,
                }
                .word(0)
                .as_deref(),
                Some("crate" | "self" | "super" | "in")
            )
        {
            self.bump();
        }
    }

    /// The `<..>` ahead, through the `>` that closes it.
    fn angles(&mut self) -> Option<()> {
        let mut depth = 0_usize;
        loop {
            match self.token(0) {
                Some(Token::LessThan) => depth += 1,
                Some(Token::LeftShift) => depth += 2,
                Some(Token::GreaterThan) => depth = depth.checked_sub(1)?,
                Some(Token::RightShift) => depth = depth.checked_sub(2)?,
                Some(Token::GreaterThanEqualsTo) => depth = depth.checked_sub(1)?,
                Some(Token::SemiColon) => return None,
                None if self.tree(0).is_none() => return None,
                _ => {}
            }
            self.bump();
            if depth == 0 {
                return Some(());
            }
        }
    }

    /// A path segment's name: any name or keyword a path may hold.
    fn segment(&mut self) -> Option<()> {
        let Some(word) = self.word(0) else {
            return self.expected(Expected::Identifier);
        };
        if NOT_OPERANDS.contains(&word.as_str()) {
            return self.expected(Expected::Identifier);
        }
        self.bump();
        Some(())
    }

    /// A path as an expression or a pattern writes it, its generic
    /// arguments after `::`: `a::b::<T>::c`, `<T as Trait>::f`.
    fn expr_path(&mut self) -> Option<()> {
        self.eat_token(&Token::DoubleColon);
        if self.at_token(&Token::LessThan) {
            self.angles()?;
        } else {
            self.segment()?;
        }
        while self.at_token(&Token::DoubleColon) {
            self.bump();
            if self.at_token(&Token::LessThan) {
                self.angles()?;
            } else {
                self.segment()?;
            }
        }
        Some(())
    }

    /// A path as a type writes it: `a::b<T>::C`, `Fn(A) -> B`.
    fn type_path(&mut self) -> Option<()> {
        self.eat_token(&Token::DoubleColon);
        if self.at_token(&Token::LessThan) {
            self.angles()?;
        } else {
            self.segment()?;
        }
        loop {
            if self.at_token(&Token::DoubleColon) && self.token(1) == Some(&Token::LessThan) {
                self.bump();
            }
            if self.at_token(&Token::LessThan) {
                self.angles()?;
            } else if self.at_group(Delimiter::Paren) {
                self.bump();
                if self.eat_token(&Token::ThinArrow) {
                    self.ty_no_bounds()?;
                }
            }
            if !self.at_token(&Token::DoubleColon) {
                return Some(());
            }
            self.bump();
            self.segment()?;
        }
    }

    /// A type, with the `+` bounds of an `impl` or `dyn` type.
    fn ty(&mut self) -> Option<()> {
        self.ty_with(true)
    }

    /// A type where a `+` after it is not its own: after `as`, `&` or `->`.
    fn ty_no_bounds(&mut self) -> Option<()> {
        self.ty_with(false)
    }

    fn ty_with(&mut self, bounded: bool) -> Option<()> {
        match self.tree(0) {
            Some(Tree::Group(Delimiter::Paren | Delimiter::Bracket, ..)) => {
                self.bump();
                return Some(());
            }
            None | Some(Tree::Group(Delimiter::Brace, ..)) => {
                return self.expected(Expected::Type);
            }
            Some(Tree::Leaf(..)) => {}
        }
        match self.token(0)? {
            Token::Bang | Token::Underscore => {
                self.bump();
                return Some(());
            }
            Token::Ampersand | Token::And => {
                self.bump();
                if self.at_token(&Token::Apostrophe) {
                    self.lifetime()?;
                }
                self.eat_word("mut");
                return self.ty_no_bounds();
            }
            Token::Multiply => {
                self.bump();
                if !self.eat_word("const") && !self.eat_word("mut") {
                    return None;
                }
                return self.ty_no_bounds();
            }
            Token::LessThan | Token::DoubleColon => return self.type_or_macro(),
            _ => {}
        }
        let Some(word) = self.word(0) else {
            return self.expected(Expected::Type);
        };
        match word.as_str() {
            "fn" | "unsafe" | "extern" => {
                self.bump();
                if word == "extern" && matches!(self.token(0), Some(Token::StringConstant(_))) {
                    self.bump();
                }
                if word != "fn" {
                    return self.ty_with(bounded);
                }
                self.group(Delimiter::Paren)?;
                if self.eat_token(&Token::ThinArrow) {
                    self.ty_no_bounds()?;
                }
                Some(())
            }
            "for" => {
                self.bump();
                self.angles()?;
                self.ty_with(bounded)
            }
            "impl" | "dyn" => {
                self.bump();
                self.bounds(bounded)
            }
            _ => self.type_or_macro(),
        }
    }

    /// A type path, or a macro call where a type goes.
    fn type_or_macro(&mut self) -> Option<()> {
        self.type_path()?;
        self.macro_call();
        Some(())
    }

    /// `!` and the delimited group of a macro call after its path, when
    /// they are ahead.
    fn macro_call(&mut self) -> bool {
        let call = self.at_token(&Token::Bang) && matches!(self.tree(1), Some(Tree::Group(..)));
        if call {
            self.at += 2;
        }
        call
    }

    /// The bounds of an `impl` or `dyn` type, `+` apart when `bounded`.
    fn bounds(&mut self, bounded: bool) -> Option<()> {
        loop {
            if self.at_token(&Token::Apostrophe) {
                self.lifetime()?;
            } else if self.at_group(Delimiter::Paren) {
                self.bump();
            } else {
                if self.at_question() {
                    self.bump();
                }
                if self.eat_word("for") {
                    self.angles()?;
                }
                self.type_path()?;
            }
            if !bounded || !self.at_token(&Token::Plus) && !self.at_token(&Token::UnaryPlus) {
                return Some(());
            }
            self.bump();
        }
    }

    /// Whether an expression in `context` can start ahead.
    fn starts_expr(&self, context: Context) -> bool {
        match self.tree(0) {
            None => false,
            Some(Tree::Group(Delimiter::Brace, ..)) => context == Context::Fragment,
            Some(Tree::Group(..)) => true,
            Some(Tree::Leaf(token, _)) => {
                is_literal(token)
                    || matches!(
                        token,
                        Token::Minus
                            | Token::UnaryMinus
                            | Token::Bang
                            | Token::Multiply
                            | Token::Ampersand
                            | Token::And
                            | Token::Pipe
                            | Token::Or
                            | Token::ExclusiveRange
                            | Token::InclusiveRange
                            | Token::DoubleColon
                            | Token::LessThan
                            | Token::Apostrophe
                            | Token::Hash
                    )
                    || self
                        .word(0)
                        .is_some_and(|word| !NOT_OPERANDS.contains(&word.as_str()))
            }
        }
    }

    /// One expression in `context`: operands joined by binary operators,
    /// casts and ranges.
    fn expr(&mut self, context: Context) -> Option<()> {
        if self.at_token(&Token::ExclusiveRange) || self.at_token(&Token::InclusiveRange) {
            self.bump();
            if !self.starts_expr(context) {
                return Some(());
            }
        }
        loop {
            self.unary(context)?;
            while self.eat_token(&Token::As) {
                self.ty_no_bounds()?;
            }
            let Some(token) = self.token(0) else {
                return Some(());
            };
            if !binary(token) {
                return Some(());
            }
            let range = matches!(token, Token::ExclusiveRange | Token::InclusiveRange);
            self.bump();
            if range && !self.starts_expr(context) {
                return Some(());
            }
        }
    }

    /// An operand with its prefix operators and its postfix ones.
    fn unary(&mut self, context: Context) -> Option<()> {
        loop {
            self.attributes();
            match self.token(0) {
                Some(Token::Minus | Token::UnaryMinus | Token::Bang | Token::Multiply) => {
                    self.bump();
                }
                Some(Token::Ampersand | Token::And) => {
                    self.bump();
                    if self.at_word("raw")
                        && matches!(self.word(1).as_deref(), Some("const" | "mut"))
                    {
                        self.at += 2;
                    } else {
                        self.eat_word("mut");
                    }
                }
                _ => break,
            }
        }
        self.operand(context)?;
        self.postfix()
    }

    /// `.field`, `.method::<T>(..)`, `.await`, `.0`, `(..)`, `[..]` and `?`
    /// after an operand.
    fn postfix(&mut self) -> Option<()> {
        loop {
            if self.at_token(&Token::Period) {
                self.bump();
                match self.token(0)? {
                    Token::IntegerConstant(_) | Token::FloatConstant(_) => self.bump(),
                    _ => {
                        self.word(0)?;
                        self.bump();
                        if self.at_token(&Token::DoubleColon)
                            && self.token(1) == Some(&Token::LessThan)
                        {
                            self.bump();
                            self.angles()?;
                        }
                    }
                }
            } else if self.at_group(Delimiter::Paren)
                || self.at_group(Delimiter::Bracket)
                || self.at_question()
            {
                self.bump();
            } else {
                return Some(());
            }
        }
    }

    /// A closure from its `move`, `async` or first `|`.
    fn closure(&mut self, context: Context) -> Option<()> {
        self.eat_word("async");
        self.eat_word("move");
        if !self.eat_token(&Token::Or) {
            self.expect_token(&Token::Pipe)?;
            while !self.eat_token(&Token::Pipe) {
                self.bump_any()?;
            }
        }
        if self.eat_token(&Token::ThinArrow) {
            self.ty_no_bounds()?;
            return self.group(Delimiter::Brace);
        }
        self.expr(context)
    }

    /// The `{..}` body after a condition, and `else` with its own.
    fn body(&mut self) -> Option<()> {
        self.group(Delimiter::Brace)
    }

    fn operand(&mut self, context: Context) -> Option<()> {
        let Some(tree) = self.tree(0) else {
            return self.expected(Expected::Expression);
        };
        match tree {
            Tree::Group(..) => {
                self.bump();
                return Some(());
            }
            Tree::Leaf(token, _) if is_literal(token) => {
                let number = matches!(token, Token::IntegerConstant(_) | Token::FloatConstant(_));
                self.bump();
                if number {
                    self.suffix();
                }
                return Some(());
            }
            Tree::Leaf(Token::Pipe | Token::Or, _) => return self.closure(context),
            Tree::Leaf(Token::LessThan | Token::DoubleColon, _) => {
                return self.path_operand(context)
            }
            Tree::Leaf(Token::Apostrophe, _) => {
                // A label: `'a: loop { .. }`.
                self.lifetime()?;
                self.expect_token(&Token::Colon)?;
                return self.operand(context);
            }
            Tree::Leaf(..) => {}
        }
        let Some(word) = self.word(0) else {
            return self.expected(Expected::Expression);
        };
        match word.as_str() {
            "move" => self.closure(context),
            "async" => {
                if self.at_group_after_words(&["async", "move"]) {
                    self.bump();
                    self.eat_word("move");
                    return self.body();
                }
                self.closure(context)
            }
            "static" if matches!(self.token(1), Some(Token::Pipe | Token::Or)) => {
                self.bump();
                self.closure(context)
            }
            "unsafe" | "const" | "try"
                if matches!(self.tree(1), Some(Tree::Group(Delimiter::Brace, ..))) =>
            {
                self.at += 2;
                Some(())
            }
            "if" => {
                self.bump();
                self.expr(Context::Condition)?;
                self.body()?;
                while self.eat_word("else") {
                    if self.eat_word("if") {
                        self.expr(Context::Condition)?;
                        self.body()?;
                    } else {
                        return self.body();
                    }
                }
                Some(())
            }
            "match" => {
                self.bump();
                self.expr(Context::Scrutinee)?;
                self.body()
            }
            "loop" => {
                self.bump();
                self.body()
            }
            "while" => {
                self.bump();
                self.expr(Context::Condition)?;
                self.body()
            }
            "for" => {
                self.bump();
                self.pat()?;
                if !self.eat_word("in") {
                    return None;
                }
                self.expr(Context::Scrutinee)?;
                self.body()
            }
            "let" if context == Context::Condition => {
                self.bump();
                self.pat()?;
                self.expect_token(&Token::Equals)?;
                self.unary(context)
            }
            "return" | "yield" | "break" | "continue" | "become" => {
                self.bump();
                if matches!(word.as_str(), "break" | "continue")
                    && self.at_token(&Token::Apostrophe)
                {
                    self.lifetime()?;
                }
                if word != "continue" && self.starts_expr(context) {
                    self.expr(context)?;
                }
                Some(())
            }
            "let" | "const" | "unsafe" | "try" | "static" => self.expected(Expected::Expression),
            word if NOT_OPERANDS.contains(&word) => self.expected(Expected::Expression),
            _ => self.path_operand(context),
        }
    }

    /// Whether the trees ahead are the `words` that are there, in turn,
    /// then a `{..}` group: `async {` or `async move {`.
    fn at_group_after_words(&self, words: &[&str]) -> bool {
        let mut ahead = 0;
        for word in words {
            if self.word(ahead).as_deref() == Some(*word) {
                ahead += 1;
            }
        }
        matches!(self.tree(ahead), Some(Tree::Group(Delimiter::Brace, ..)))
    }

    /// A path as an operand: with a macro call's `!(..)` after it, or, in a
    /// fragment, a struct expression's `{..}`.
    fn path_operand(&mut self, context: Context) -> Option<()> {
        self.expr_path()?;
        if !self.macro_call() && context == Context::Fragment && self.at_group(Delimiter::Brace) {
            self.bump();
        }
        Some(())
    }

    /// A pattern, its alternatives `|` apart, as `pat` reads one in Rust
    /// 2021.
    fn pat(&mut self) -> Option<()> {
        self.eat_token(&Token::Pipe);
        self.pat_alternative()?;
        while self.eat_token(&Token::Pipe) {
            self.pat_alternative()?;
        }
        Some(())
    }

    /// A pattern with no `|` at its top level, as `pat_param` reads one.
    fn pat_alternative(&mut self) -> Option<()> {
        match self.tree(0) {
            None | Some(Tree::Group(Delimiter::Brace, ..)) => {
                return self.expected(Expected::Pattern);
            }
            Some(Tree::Group(..)) => {
                self.bump();
                return Some(());
            }
            Some(Tree::Leaf(..)) => {}
        }
        match self.token(0)? {
            Token::Underscore => {
                self.bump();
                return Some(());
            }
            Token::Ampersand | Token::And => {
                self.bump();
                self.eat_word("mut");
                return self.pat_alternative();
            }
            Token::ExclusiveRange | Token::InclusiveRange => {
                self.bump();
                if self.starts_bound() {
                    self.bound()?;
                }
                return Some(());
            }
            _ => {}
        }
        if self.at_word("ref") || self.at_word("mut") {
            self.eat_word("ref");
            self.eat_word("mut");
            self.ident()?;
            return self.binding_rest();
        }
        if self.starts_bound() {
            let single = self.word(0).is_some()
                && !matches!(self.token(1), Some(Token::DoubleColon | Token::Bang));
            self.bound()?;
            if single && self.at_reserved("@") {
                return self.binding_rest();
            }
            if self.macro_call() {
                return Some(());
            }
            if self.at_group(Delimiter::Paren) || self.at_group(Delimiter::Brace) {
                self.bump();
                return Some(());
            }
            if self.at_token(&Token::ExclusiveRange)
                || self.at_token(&Token::InclusiveRange)
                || self.at_reserved("...")
            {
                self.bump();
                if self.starts_bound() {
                    self.bound()?;
                }
            }
            return Some(());
        }
        self.expected(Expected::Pattern)
    }

    fn at_reserved(&self, text: &str) -> bool {
        matches!(self.token(0), Some(Token::Reserved(found)) if found.as_str() == text)
    }

    /// `@ pattern` after a binding's name, when it is ahead.
    fn binding_rest(&mut self) -> Option<()> {
        if self.at_reserved("@") {
            self.bump();
            self.pat_alternative()?;
        }
        Some(())
    }

    /// Whether a range pattern's bound, a literal or a path, is ahead.
    fn starts_bound(&self) -> bool {
        match self.token(0) {
            Some(token) if is_literal(token) => true,
            Some(Token::Minus | Token::UnaryMinus) => self.token(1).is_some_and(is_literal),
            Some(Token::DoubleColon | Token::LessThan) => true,
            Some(_) => self
                .word(0)
                .is_some_and(|word| !NOT_OPERANDS.contains(&word.as_str())),
            None => false,
        }
    }

    /// A range pattern's bound: a literal or a path.
    fn bound(&mut self) -> Option<()> {
        if self.literal().is_some() {
            return Some(());
        }
        self.expr_path()
    }

    /// A statement without the `;` after it: a `let`, an item or an
    /// expression.
    fn stmt(&mut self) -> Option<()> {
        self.attributes();
        if self.eat_word("let") {
            self.pat()?;
            if self.eat_token(&Token::Colon) {
                self.ty()?;
            }
            if self.eat_token(&Token::Equals) {
                self.expr(Context::Fragment)?;
                if self.eat_word("else") {
                    self.body()?;
                }
            }
            return Some(());
        }
        if self.starts_item() {
            return self.item();
        }
        self.expr(Context::Fragment)
    }

    /// Whether an item starts ahead, past its attributes.
    fn starts_item(&self) -> bool {
        let Some(word) = self.word(0) else {
            return false;
        };
        match word.as_str() {
            "fn" | "struct" | "enum" | "trait" | "impl" | "mod" | "use" | "type" | "extern"
            | "pub" | "static" | "macro_rules" => true,
            "const" => !matches!(self.tree(1), Some(Tree::Group(Delimiter::Brace, ..))),
            "unsafe" | "async" | "default" | "union" => {
                matches!(self.tree(1), Some(Tree::Leaf(..))) && self.word(1).is_some()
            }
            _ => false,
        }
    }

    /// An item with its attributes, through its `;` or the `{..}` that
    /// ends it.
    fn item(&mut self) -> Option<()> {
        self.attributes();
        self.vis();
        let Some(word) = self.word(0) else {
            return match self.tree(0) {
                Some(_) => self.expected(Expected::Item),
                None => None,
            };
        };
        if word == "macro_rules" && self.token(1) == Some(&Token::Bang) {
            self.at += 2;
            self.segment()?;
            if !matches!(self.tree(0), Some(Tree::Group(..))) {
                return None;
            }
            self.bump();
            return self.item_end_after_call();
        }
        let (start, failed) = (self.at, self.failed);
        if self.type_path().is_some() && self.macro_call() {
            return self.item_end_after_call();
        }
        // A path that isn't a macro call's is no failure of the item's.
        self.at = start;
        self.failed = failed;
        let semicolon_only = match word.as_str() {
            "const" => !matches!(
                self.word(1).as_deref(),
                Some("fn" | "unsafe" | "async" | "extern")
            ),
            "static" | "type" | "use" => true,
            "extern" => self.at_word_ahead(1, "crate"),
            _ => false,
        };
        loop {
            match self.tree(0)? {
                Tree::Leaf(Token::SemiColon, _) => {
                    self.bump();
                    return Some(());
                }
                Tree::Group(Delimiter::Brace, ..) if !semicolon_only => {
                    self.bump();
                    return Some(());
                }
                _ => self.bump(),
            }
        }
    }

    fn at_word_ahead(&self, ahead: usize, word: &str) -> bool {
        self.word(ahead).is_some_and(|found| found == word)
    }

    /// The `;` after an item's macro call in `(..)` or `[..]`, which one in
    /// `{..}` needs none of.
    fn item_end_after_call(&mut self) -> Option<()> {
        let braced = matches!(
            self.trees.get(self.at - 1),
            Some(Tree::Group(Delimiter::Brace, ..))
        );
        if braced {
            self.eat_token(&Token::SemiColon);
            return Some(());
        }
        self.expect_token(&Token::SemiColon)
    }

    /// An attribute's contents: a path, then a delimited group or `= expr`.
    fn meta(&mut self) -> Option<()> {
        if self.at_word("unsafe") && matches!(self.tree(1), Some(Tree::Group(Delimiter::Paren, ..)))
        {
            self.at += 2;
            return Some(());
        }
        self.eat_token(&Token::DoubleColon);
        self.segment()?;
        while self.eat_token(&Token::DoubleColon) {
            self.segment()?;
        }
        if matches!(self.tree(0), Some(Tree::Group(..))) {
            self.bump();
        } else if self.eat_token(&Token::Equals) {
            self.expr(Context::Fragment)?;
        }
        Some(())
    }
}

/// Whether `token` joins two operands, or starts a range after one.
fn binary(token: &Token) -> bool {
    matches!(
        token,
        Token::Plus
            | Token::UnaryPlus
            | Token::Minus
            | Token::UnaryMinus
            | Token::Multiply
            | Token::Divide
            | Token::Modulo
            | Token::PowerOf
            | Token::XOr
            | Token::Ampersand
            | Token::Pipe
            | Token::And
            | Token::Or
            | Token::LeftShift
            | Token::RightShift
            | Token::EqualsTo
            | Token::NotEqualsTo
            | Token::LessThan
            | Token::GreaterThan
            | Token::LessThanEqualsTo
            | Token::GreaterThanEqualsTo
            | Token::Equals
            | Token::PlusAssign
            | Token::MinusAssign
            | Token::MultiplyAssign
            | Token::DivideAssign
            | Token::ModuloAssign
            | Token::PowerOfAssign
            | Token::XOrAssign
            | Token::AndAssign
            | Token::OrAssign
            | Token::LeftShiftAssign
            | Token::RightShiftAssign
            | Token::ExclusiveRange
            | Token::InclusiveRange
    )
}
