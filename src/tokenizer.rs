//! Main module defining the lexer and parser.

use rawr_codegen::expose_under_internals;

use crate::engine::Precedence;
use crate::func::native::OnParseTokenCallback;
use crate::{Engine, Identifier, LexError, Position, SmartString, StaticVec, INT, UNSIGNED_INT};
use std::{
    cell::RefCell,
    char, fmt,
    fmt::Write,
    iter::{FusedIterator, Peekable},
    rc::Rc,
    str::{Chars, FromStr},
};

/// _(internals)_ A type containing commands to control the tokenizer.
#[derive(Debug, Clone, Eq, PartialEq, Default, Hash)]
pub struct TokenizerControlBlock {
    /// Global comments.
    #[cfg(feature = "metadata")]
    pub global_comments: String,
    /// Whitespace-compressed version of the script (if any).
    ///
    /// Set to `Some` in order to collect a compressed script.
    pub compressed: Option<String>,
    /// Read the input as Rust rather than as the lowered script, which
    /// spells its own constructs with names no Rust name spells.
    pub rust: bool,
}

impl TokenizerControlBlock {
    /// Create a new `TokenizerControlBlock`.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            #[cfg(feature = "metadata")]
            global_comments: String::new(),
            compressed: None,
            rust: false,
        }
    }
}

/// _(internals)_ A shared object that allows control of the tokenizer from outside.
pub type TokenizerControl = Rc<RefCell<TokenizerControlBlock>>;

type LERR = LexError;

/// Separator character for numbers.
const NUMBER_SEPARATOR: char = '_';

/// A stream of tokens.
pub type TokenStream<'a> = Peekable<TokenIterator<'a>>;

/// _(internals)_ A Rawr language token.
/// Exported under the `internals` feature only.
#[derive(Debug, PartialEq, Clone, Hash)]
#[non_exhaustive]
pub enum Token {
    /// An `INT` constant.
    IntegerConstant(INT),
    /// A `FLOAT` constant, including its text representation.
    FloatConstant(Box<(crate::types::FloatWrapper<crate::FLOAT>, Identifier)>),
    /// A [`Decimal`][rust_decimal::Decimal] constant.
    ///
    /// Requires the `decimal` feature, including its text representation.
    #[cfg(feature = "decimal")]
    DecimalConstant(Box<(rust_decimal::Decimal, Identifier)>),
    /// An identifier.
    Identifier(Box<Identifier>),
    /// A character constant.
    CharConstant(char),
    /// A string constant.
    StringConstant(Box<SmartString>),
    /// A byte string constant: Rust's `b"…"`, `br"…"` or `br#"…"#`.
    ByteStringConstant(Box<Vec<u8>>),
    /// A byte constant: Rust's `b'a'`, a `u8`.
    ByteConstant(u8),
    /// `{`
    LeftBrace,
    /// `}`
    RightBrace,
    /// `(`
    LeftParen,
    /// `)`
    RightParen,
    /// `[`
    LeftBracket,
    /// `]`
    RightBracket,
    /// `()`
    Unit,
    /// `+`
    Plus,
    /// `+` (unary)
    UnaryPlus,
    /// `-`
    Minus,
    /// `-` (unary)
    UnaryMinus,
    /// `*`
    Multiply,
    /// `/`
    Divide,
    /// `%`
    Modulo,
    /// `**`
    PowerOf,
    /// `<<`
    LeftShift,
    /// `>>`
    RightShift,
    /// `;`
    SemiColon,
    /// `:`
    Colon,
    /// `::`
    DoubleColon,
    /// `->`
    ThinArrow,
    /// `=>`
    DoubleArrow,
    /// `#`
    Hash,
    /// `'` when introducing a lifetime marker.
    Apostrophe,
    /// `_`
    Underscore,
    /// `,`
    Comma,
    /// `.`
    Period,
    /// `..`
    ExclusiveRange,
    /// `..=`
    InclusiveRange,
    /// `#{`
    MapStart,
    /// `=`
    Equals,
    /// `true`
    True,
    /// `false`
    False,
    /// `let`
    Let,
    /// `const`
    Const,
    /// `pub`
    Pub,
    /// `struct`
    Struct,
    /// `enum`
    Enum,
    /// `impl`
    Impl,
    /// `use`
    Use,
    /// `mod`
    Mod,
    /// `self`
    SelfValue,
    /// `mut`
    Mut,
    /// `if`
    If,
    /// `else`
    Else,
    /// `__rawr_switch`: the lowered script's `match`.
    Switch,
    /// `do`
    Do,
    /// `while`
    While,
    /// `loop`
    Loop,
    /// `for`
    For,
    /// `in`
    In,
    /// `<`
    LessThan,
    /// `>`
    GreaterThan,
    /// `<=`
    LessThanEqualsTo,
    /// `>=`
    GreaterThanEqualsTo,
    /// `==`
    EqualsTo,
    /// `!=`
    NotEqualsTo,
    /// `!`
    Bang,
    /// `|`
    Pipe,
    /// `||`
    Or,
    /// `^`
    XOr,
    /// `&`
    Ampersand,
    /// `&&`
    And,
    /// `fn`
    Fn,
    /// `continue`
    Continue,
    /// `break`
    Break,
    /// `return`
    Return,
    /// `__rawr_throw`: the lowered script's panic.
    Throw,
    /// `try`
    Try,
    /// `+=`
    PlusAssign,
    /// `-=`
    MinusAssign,
    /// `*=`
    MultiplyAssign,
    /// `/=`
    DivideAssign,
    /// `<<=`
    LeftShiftAssign,
    /// `>>=`
    RightShiftAssign,
    /// `&=`
    AndAssign,
    /// `|=`
    OrAssign,
    /// `^=`
    XOrAssign,
    /// `%=`
    ModuloAssign,
    /// `**=`
    PowerOfAssign,
    /// `__rawr_private`: a function of the lowered script the host does
    /// not call.
    Private,
    /// `as`
    As,
    /// A lexer error.
    LexError(Box<LexError>),
    /// A comment block.
    Comment(Box<String>),
    /// A reserved symbol.
    Reserved(Box<Identifier>),
    /// End of the input stream.
    /// Used as a placeholder for the end of input.
    EOF,
}

impl fmt::Display for Token {
    #[inline(always)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[allow(clippy::enum_glob_use)]
        use Token::*;

        match self {
            IntegerConstant(i) => write!(f, "{i}"),
            FloatConstant(v) => write!(f, "{}", v.0),
            #[cfg(feature = "decimal")]
            DecimalConstant(d) => write!(f, "{}", d.0),
            StringConstant(s) => write!(f, r#""{s}""#),
            ByteStringConstant(s) => write!(f, r#"b"{}""#, s.escape_ascii()),
            ByteConstant(b) => write!(f, "b'{}'", b.escape_ascii()),
            CharConstant(c) => write!(f, "{c}"),
            Identifier(s) => f.write_str(s),
            Reserved(s) => f.write_str(s),
            LexError(err) => write!(f, "{err}"),
            Comment(s) => f.write_str(s),

            EOF => f.write_str("{EOF}"),

            token => f.write_str(token.literal_syntax()),
        }
    }
}

// Table-driven keyword recognizer generated by GNU `gperf` on the file `tools/keywords.txt`.
//
// When adding new keywords, make sure to update `tools/keywords.txt` and re-generate this.

const MIN_KEYWORD_LEN: usize = 1;
const MAX_KEYWORD_LEN: usize = 8;
const MIN_KEYWORD_HASH_VALUE: usize = 1;
const MAX_KEYWORD_HASH_VALUE: usize = 152;

static KEYWORD_ASSOC_VALUES: [u8; 257] = [
    153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153,
    153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 115, 153, 100, 153, 110,
    105, 40, 80, 2, 20, 25, 125, 95, 15, 40, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 55,
    35, 10, 5, 0, 30, 110, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153,
    153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 120, 105, 100, 85, 90, 153, 125, 5,
    0, 125, 35, 10, 100, 153, 20, 0, 153, 10, 0, 45, 55, 0, 153, 50, 55, 5, 0, 153, 0, 0, 35, 153,
    45, 50, 30, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153,
    153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153,
    153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153,
    153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153,
    153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153,
    153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153,
    153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153, 153,
    153,
];
static KEYWORDS_LIST: [(&str, Token); 153] = [
    ("", Token::EOF),
    (">", Token::GreaterThan),
    (">=", Token::GreaterThanEqualsTo),
    (")", Token::RightParen),
    ("", Token::EOF),
    ("const", Token::Const),
    ("=", Token::Equals),
    ("==", Token::EqualsTo),
    ("continue", Token::Continue),
    ("", Token::EOF),
    ("", Token::EOF),
    ("<", Token::LessThan),
    ("<=", Token::LessThanEqualsTo),
    ("for", Token::For),
    ("loop", Token::Loop),
    ("", Token::EOF),
    (".", Token::Period),
    ("<<", Token::LeftShift),
    ("<<=", Token::LeftShiftAssign),
    ("", Token::EOF),
    ("false", Token::False),
    ("*", Token::Multiply),
    ("*=", Token::MultiplyAssign),
    ("let", Token::Let),
    ("", Token::EOF),
    ("while", Token::While),
    ("+", Token::Plus),
    ("+=", Token::PlusAssign),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("}", Token::RightBrace),
    (">>", Token::RightShift),
    (">>=", Token::RightShiftAssign),
    ("", Token::EOF),
    ("", Token::EOF),
    (";", Token::SemiColon),
    ("=>", Token::DoubleArrow),
    ("", Token::EOF),
    ("else", Token::Else),
    ("", Token::EOF),
    ("/", Token::Divide),
    ("/=", Token::DivideAssign),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("{", Token::LeftBrace),
    ("**", Token::PowerOf),
    ("**=", Token::PowerOfAssign),
    ("", Token::EOF),
    ("", Token::EOF),
    ("|", Token::Pipe),
    ("|=", Token::OrAssign),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    (":", Token::Colon),
    ("..", Token::ExclusiveRange),
    ("..=", Token::InclusiveRange),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("try", Token::Try),
    ("true", Token::True),
    ("break", Token::Break),
    ("return", Token::Return),
    ("fn", Token::Fn),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("in", Token::In),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("(", Token::LeftParen),
    ("||", Token::Or),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("^", Token::XOr),
    ("^=", Token::XOrAssign),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("_", Token::Underscore),
    ("::", Token::DoubleColon),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("-", Token::Minus),
    ("-=", Token::MinusAssign),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("]", Token::RightBracket),
    ("()", Token::Unit),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("&", Token::Ampersand),
    ("&=", Token::AndAssign),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("%", Token::Modulo),
    ("%=", Token::ModuloAssign),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("!", Token::Bang),
    ("!=", Token::NotEqualsTo),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("[", Token::LeftBracket),
    ("if", Token::If),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    (",", Token::Comma),
    ("do", Token::Do),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("as", Token::As),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("&&", Token::And),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("", Token::EOF),
    ("#{", Token::MapStart),
];

impl Token {
    /// Is the token a literal symbol?
    #[must_use]
    pub const fn is_literal(&self) -> bool {
        #[allow(clippy::enum_glob_use)]
        use Token::*;

        match self {
            IntegerConstant(..) => false,
            FloatConstant(..) => false,
            #[cfg(feature = "decimal")]
            DecimalConstant(..) => false,
            StringConstant(..)
            | ByteStringConstant(..)
            | ByteConstant(..)
            | CharConstant(..)
            | Identifier(..)
            | Reserved(..) => false,
            LexError(..) | Comment(..) => false,

            EOF => false,

            _ => true,
        }
    }
    /// Get the literal syntax of the token.
    ///
    /// # Panics
    ///
    /// Panics if the token is not a literal symbol.
    #[must_use]
    pub const fn literal_syntax(&self) -> &'static str {
        #[allow(clippy::enum_glob_use)]
        use Token::*;

        match self {
            LeftBrace => "{",
            RightBrace => "}",
            LeftParen => "(",
            RightParen => ")",
            LeftBracket => "[",
            RightBracket => "]",
            Unit => "()",
            Plus => "+",
            UnaryPlus => "+",
            Minus => "-",
            UnaryMinus => "-",
            Multiply => "*",
            Divide => "/",
            SemiColon => ";",
            Colon => ":",
            DoubleColon => "::",
            ThinArrow => "->",
            DoubleArrow => "=>",
            Hash => "#",
            Apostrophe => "'",
            Underscore => "_",
            Comma => ",",
            Period => ".",
            ExclusiveRange => "..",
            InclusiveRange => "..=",
            MapStart => "#{",
            Equals => "=",
            True => "true",
            False => "false",
            Let => "let",
            Const => "const",
            Pub => "pub",
            Struct => "struct",
            Enum => "enum",
            Impl => "impl",
            Use => "use",
            Mod => "mod",
            SelfValue => "self",
            Mut => "mut",
            If => "if",
            Else => "else",
            Switch => "__rawr_switch",
            Do => "do",
            While => "while",
            Loop => "loop",
            For => "for",
            In => "in",
            LessThan => "<",
            GreaterThan => ">",
            Bang => "!",
            LessThanEqualsTo => "<=",
            GreaterThanEqualsTo => ">=",
            EqualsTo => "==",
            NotEqualsTo => "!=",
            Pipe => "|",
            Or => "||",
            Ampersand => "&",
            And => "&&",
            Continue => "continue",
            Break => "break",
            Return => "return",
            Throw => "__rawr_throw",
            Try => "try",
            PlusAssign => "+=",
            MinusAssign => "-=",
            MultiplyAssign => "*=",
            DivideAssign => "/=",
            LeftShiftAssign => "<<=",
            RightShiftAssign => ">>=",
            AndAssign => "&=",
            OrAssign => "|=",
            XOrAssign => "^=",
            LeftShift => "<<",
            RightShift => ">>",
            XOr => "^",
            Modulo => "%",
            ModuloAssign => "%=",
            PowerOf => "**",
            PowerOfAssign => "**=",

            Fn => "fn",
            Private => "__rawr_private",

            As => "as",

            _ => panic!("token is not a literal symbol"),
        }
    }

    /// Is this token an op-assignment operator?
    #[inline]
    #[must_use]
    pub const fn is_op_assignment(&self) -> bool {
        #[allow(clippy::enum_glob_use)]
        use Token::*;

        matches!(
            self,
            PlusAssign
                | MinusAssign
                | MultiplyAssign
                | DivideAssign
                | LeftShiftAssign
                | RightShiftAssign
                | ModuloAssign
                | PowerOfAssign
                | AndAssign
                | OrAssign
                | XOrAssign
        )
    }

    /// Get the corresponding operator of the token if it is an op-assignment operator.
    #[must_use]
    pub const fn get_base_op_from_assignment(&self) -> Option<Self> {
        #[allow(clippy::enum_glob_use)]
        use Token::*;

        Some(match self {
            PlusAssign => Plus,
            MinusAssign => Minus,
            MultiplyAssign => Multiply,
            DivideAssign => Divide,
            LeftShiftAssign => LeftShift,
            RightShiftAssign => RightShift,
            ModuloAssign => Modulo,
            PowerOfAssign => PowerOf,
            AndAssign => Ampersand,
            OrAssign => Pipe,
            XOrAssign => XOr,
            _ => return None,
        })
    }

    /// Has this token a corresponding op-assignment operator?
    #[inline]
    #[must_use]
    pub const fn has_op_assignment(&self) -> bool {
        #[allow(clippy::enum_glob_use)]
        use Token::*;

        matches!(
            self,
            Plus | Minus
                | Multiply
                | Divide
                | LeftShift
                | RightShift
                | Modulo
                | PowerOf
                | Ampersand
                | Pipe
                | XOr
        )
    }

    /// Get the corresponding op-assignment operator of the token.
    #[must_use]
    pub const fn convert_to_op_assignment(&self) -> Option<Self> {
        #[allow(clippy::enum_glob_use)]
        use Token::*;

        Some(match self {
            Plus => PlusAssign,
            Minus => MinusAssign,
            Multiply => MultiplyAssign,
            Divide => DivideAssign,
            LeftShift => LeftShiftAssign,
            RightShift => RightShiftAssign,
            Modulo => ModuloAssign,
            PowerOf => PowerOfAssign,
            Ampersand => AndAssign,
            Pipe => OrAssign,
            XOr => XOrAssign,
            _ => return None,
        })
    }

    /// Reverse lookup a symbol token from a piece of syntax.
    #[inline]
    #[must_use]
    pub fn lookup_symbol_from_syntax(syntax: &str) -> Option<Self> {
        let rawr_token = match syntax {
            "->" => Self::ThinArrow,
            "#" => Self::Hash,
            "'" => Self::Apostrophe,
            "pub" => Self::Pub,
            "struct" => Self::Struct,
            "enum" => Self::Enum,
            "impl" => Self::Impl,
            "use" => Self::Use,
            "mod" => Self::Mod,
            "self" => Self::SelfValue,
            "mut" => Self::Mut,
            _ => return Self::lookup_generated_symbol(syntax),
        };

        Some(rawr_token)
    }

    fn lookup_generated_symbol(syntax: &str) -> Option<Self> {
        // This implementation is based upon a pre-calculated table generated
        // by GNU `gperf` on the list of keywords.
        let utf8 = syntax.as_bytes();
        let len = utf8.len();

        if !(MIN_KEYWORD_LEN..=MAX_KEYWORD_LEN).contains(&len) {
            return None;
        }

        let mut hash_val = len;

        match len {
            1 => (),
            _ => hash_val += KEYWORD_ASSOC_VALUES[(utf8[1] as usize) + 1] as usize,
        }
        hash_val += KEYWORD_ASSOC_VALUES[utf8[0] as usize] as usize;

        if !(MIN_KEYWORD_HASH_VALUE..=MAX_KEYWORD_HASH_VALUE).contains(&hash_val) {
            return None;
        }

        match KEYWORDS_LIST[hash_val] {
            (_, Self::EOF) => None,
            // Fail early to avoid calling memcmp().
            // Since we are already working with bytes, mind as well check the first one.
            (s, ref t) if s.len() == len && s.as_bytes()[0] == utf8[0] && s == syntax => {
                Some(t.clone())
            }
            _ => None,
        }
    }

    /// If another operator is after these, it's probably a unary operator
    /// (not sure about `fn` name).
    #[must_use]
    pub const fn is_next_unary(&self) -> bool {
        #[allow(clippy::enum_glob_use)]
        use Token::*;

        match self {
            SemiColon        | // ; - is unary
            Colon            | // #{ foo: - is unary
            Comma            | // ( ... , -expr ) - is unary
            //Period         |
            ExclusiveRange   | // .. - is unary
            InclusiveRange   | // ..= - is unary
            LeftBrace        | // { -expr } - is unary
            // RightBrace    | // { expr } - expr not unary & is closing
            LeftParen        | // ( -expr ) - is unary
            // RightParen    | // ( expr ) - expr not unary & is closing
            LeftBracket      | // [ -expr ] - is unary
            // RightBracket  | // [ expr ] - expr not unary & is closing
            Plus             |
            PlusAssign       |
            UnaryPlus        |
            Minus            |
            MinusAssign      |
            UnaryMinus       |
            Multiply         |
            MultiplyAssign   |
            Divide           |
            DivideAssign     |
            Modulo           |
            ModuloAssign     |
            PowerOf          |
            PowerOfAssign    |
            LeftShift        |
            LeftShiftAssign  |
            RightShift       |
            RightShiftAssign |
            Equals           |
            EqualsTo         |
            NotEqualsTo      |
            LessThan         |
            GreaterThan      |
            Bang             |
            LessThanEqualsTo |
            GreaterThanEqualsTo |
            Pipe             |
            Ampersand        |
            If               |
            //Do             |
            While            |
            In               |
            And              |
            AndAssign        |
            Or               |
            OrAssign         |
            XOr              |
            XOrAssign        |
            Return           |
            Throw               => true,

            LexError(..)        => true,

            _                   => false,
        }
    }

    /// Get the precedence number of the token.
    #[must_use]
    pub const fn precedence(&self) -> Option<Precedence> {
        #[allow(clippy::enum_glob_use)]
        use Token::*;

        Precedence::new(match self {
            Or | XOr | Pipe => 30,

            And | Ampersand => 60,

            EqualsTo | NotEqualsTo => 90,

            In => 110,

            LessThan | LessThanEqualsTo | GreaterThan | GreaterThanEqualsTo => 130,

            ExclusiveRange | InclusiveRange => 140,

            Plus | Minus => 150,

            Divide | Multiply | Modulo => 180,

            PowerOf => 190,

            LeftShift | RightShift => 210,

            _ => 0,
        })
    }

    /// Does an expression bind to the right (instead of left)?
    #[must_use]
    pub const fn is_bind_right(&self) -> bool {
        #[allow(clippy::enum_glob_use)]
        use Token::*;

        match self {
            // Exponentiation binds to the right
            PowerOf => true,

            _ => false,
        }
    }

    /// Is this token a standard keyword?
    #[inline]
    #[must_use]
    pub const fn is_standard_keyword(&self) -> bool {
        #[allow(clippy::enum_glob_use)]
        use Token::*;

        matches!(
            self,
            Fn | Private
                | As
                | True
                | False
                | Let
                | Const
                | Pub
                | Struct
                | Enum
                | Impl
                | Use
                | SelfValue
                | Mut
                | If
                | Else
                | Do
                | While
                | Loop
                | For
                | In
                | Continue
                | Break
                | Return
                | Throw
                | Try
        )
    }

    /// Is this token a reserved keyword or symbol?
    #[inline(always)]
    #[must_use]
    pub const fn is_reserved(&self) -> bool {
        matches!(self, Self::Reserved(..))
    }
}

impl From<Token> for String {
    #[inline(always)]
    fn from(token: Token) -> Self {
        (&token).into()
    }
}

impl From<&Token> for String {
    #[inline(always)]
    fn from(token: &Token) -> Self {
        token.to_string()
    }
}

impl From<Token> for SmartString {
    #[inline(always)]
    fn from(token: Token) -> Self {
        (&token).into()
    }
}

impl From<&Token> for SmartString {
    #[inline(always)]
    fn from(token: &Token) -> Self {
        let mut buf = Self::new_const();
        write!(&mut buf, "{token}").unwrap();
        buf
    }
}

/// _(internals)_ State of the tokenizer.
/// Exported under the `internals` feature only.
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct TokenizeState {
    /// Maximum length of a string.
    pub max_string_len: Option<std::num::NonZeroUsize>,
    /// Can the next token be a unary operator?
    pub next_token_cannot_be_unary: bool,
    /// Shared object to allow controlling the tokenizer externally.
    pub tokenizer_control: TokenizerControl,
    /// Is the tokenizer currently inside a block comment?
    pub comment_level: usize,
    /// Include comments?
    pub include_comments: bool,
    /// What ends the string the input stopped inside, when it continues on
    /// the next input (a line at a time).
    pub is_within_text_terminated_by: Option<SmartString>,
    /// Textual syntax of the current token, if any.
    ///
    /// Set to `Some` to begin tracking this information.
    pub last_token: Option<SmartString>,
}

/// _(internals)_ Trait that encapsulates a peekable character input stream.
/// Exported under the `internals` feature only.
pub trait InputStream {
    /// Un-get a character back into the `InputStream`.
    /// The next [`get_next`][InputStream::get_next] or [`peek_next`][InputStream::peek_next]
    /// will return this character instead.
    fn unget(&mut self, ch: char);
    /// Get the next character from the `InputStream`.
    fn get_next(&mut self) -> Option<char>;
    /// Peek the next character in the `InputStream`.
    #[must_use]
    fn peek_next(&mut self) -> Option<char>;

    /// Consume the next character.
    #[inline(always)]
    fn eat_next_and_advance(&mut self, pos: &mut Position) -> Option<char> {
        pos.advance();
        self.get_next()
    }
}

/// _(internals)_ Parse a raw string literal. Exported under the `internals` feature only.
///
/// Raw string literals do not process any escapes. They start with the character `#` (`U+0023`)
/// repeated any number of times, then finally a `"` (`U+0022`, double-quote).
///
/// The raw string _body_ can contain any sequence of Unicode characters. It is terminated only by
/// another `"` (`U+0022`, double-quote) character, followed by the same number of `#` (`U+0023`)
/// characters.
///
/// All Unicode characters contained in the raw string body represent themselves, including the
/// characters `"` (`U+0022`, double-quote), except when followed by at least as many `#` (`U+0023`)
/// characters as were used to start the raw string literal, `\` (`U+005C`) etc., and do not have
/// any special meaning.
///
/// Returns the parsed string.
///
/// # Returns
///
/// | Type                      | Return Value                                                 |`state.is_within_text_terminated_by`  |
/// |---------------------------|:------------------------------------------------------------:|:------------------------------------:|
/// |`#"hello"#`                |[`StringConstant("hello")`][Token::StringConstant]            |`None`                                |
/// |`#"hello`_{EOF}_           |[`StringConstant("hello")`][Token::StringConstant]            |`Some("#")`                           |
/// |`####"hello`_{EOF}_        |[`StringConstant("hello")`][Token::StringConstant]            |`Some("####")`                        |
/// |`#" "hello" "`_{EOF}_      |[`LexError`]                                                  |`None`                                |
/// |`#""hello""#`              |[`StringConstant("\"hello\"")`][Token::StringConstant]        |`None`                                |
/// |`##"hello #"# world"##`    |[`StringConstant("hello #\"# world")`][Token::StringConstant] |`None`                                |
/// |`#"R"#`                    |[`StringConstant("R")`][Token::StringConstant]                |`None`                                |
/// |`#"\x52"#`                 |[`StringConstant("\\x52")`][Token::StringConstant]            |`None`                                |
///
/// This function does _not_ throw a [`LexError`] for an unterminated raw string at _{EOF}_
///
/// This is to facilitate using this function to parse a script line-by-line, where the end of the
/// line (i.e. _{EOF}_) is not necessarily the end of the script.
///
/// Any time a [`StringConstant`][Token::StringConstant] is returned with
/// `state.is_within_text_terminated_by` set to `Some(_)` is one of the above conditions.
pub fn parse_raw_string_literal(
    stream: &mut (impl InputStream + ?Sized),
    state: &mut TokenizeState,
    pos: &mut Position,
    mut hash_count: usize,
) -> Result<(SmartString, Position), (LexError, Position)> {
    let start = *pos;
    let mut first_char = Position::NONE;

    if hash_count == 0 {
        // Count the number of '#'s
        // Start with 1 because the first '#' is already consumed
        hash_count = 1;

        while Some('#') == stream.peek_next() {
            stream.eat_next_and_advance(pos);
            hash_count += 1;
        }

        // Match '"'
        match stream.get_next() {
            Some('"') => pos.advance(),
            Some(c) => return Err((LERR::UnexpectedInput(c.to_string()), start)),
            None => return Err((LERR::UnterminatedString, start)),
        }
    }

    let collect: SmartString = std::iter::repeat_n('#', hash_count).collect();
    if let Some(ref mut last) = state.last_token {
        last.clear();
        last.push_str(&collect);
        last.push('"');
    }
    state.is_within_text_terminated_by = Some(collect);

    // Match everything until the same number of '#'s are seen, prepended by a '"'

    // Counts the number of '#' characters seen after a quotation mark.
    // Becomes Some(0) after a quote is seen, but resets to None if a hash doesn't follow.
    let mut seen_hashes: Option<usize> = None;
    let mut result = SmartString::new_const();

    while let Some(next_char) = stream.get_next() {
        pos.advance();

        match (next_char, &mut seen_hashes) {
            // Begin attempt to close string
            ('"', None) => seen_hashes = Some(0),
            // Restart attempt to close string
            ('"', Some(count)) => {
                // result.reserve(*count as usize+c.len());
                result.push('"');
                result.extend(std::iter::repeat_n('#', *count));
                seen_hashes = Some(0);
            }
            // Continue attempt to close string
            ('#', Some(count)) => {
                *count += 1;
                if *count == hash_count {
                    state.is_within_text_terminated_by = None;
                    break;
                }
            }
            // Fail to close the string - add previous quote and hashes
            (c, Some(count)) => {
                // result.reserve(*count as usize +1+c.len());
                result.push('"');
                result.extend(std::iter::repeat_n('#', *count));
                result.push(c);
                seen_hashes = None;
            }
            // New line
            ('\n', _) => {
                result.push('\n');
                pos.new_line();
            }
            // Normal new character seen
            (c, None) => result.push(c),
        }

        // Check string length
        if let Some(max) = state.max_string_len
            && result.len() > max.get()
        {
            return Err((LexError::StringTooLong(max.get()), start));
        }

        if first_char.is_none() {
            first_char = *pos;
        }
    }

    Ok((result, first_char))
}

/// _(internals)_ Parse a string literal ended by a specified termination character.
/// Exported under the `internals` feature only.
///
/// Returns the parsed string.
///
/// A `multi_line` string spans lines as Rust's `"…"` does: a line break is
/// part of it, and `\` before one continues the line past the next line's
/// leading whitespace.
///
/// # Returns
///
/// | Type                            | Return Value                                        |`state.is_within_text_terminated_by`|
/// |---------------------------------|:---------------------------------------------------:|:----------------------------------:|
/// |`"hello"`                        |[`StringConstant("hello")`][Token::StringConstant]   |`None`                              |
/// |`"hello`_{EOF}_                  |[`LexError`]                                         |`None`                              |
/// |`"hello`_{LF}_`world"`           |[`StringConstant("hello\nworld")`][Token::StringConstant]|`None`                         |
/// |`"hello\`_{EOF}_ or _{LF}{EOF}_  |[`StringConstant("hello")`][Token::StringConstant]   |`Some('"')`                         |
///
/// This function does not throw a [`LexError`] for the following conditions:
///
/// * Unterminated literal string at _{EOF}_
///
/// * Unterminated normal string with continuation at _{EOF}_
///
/// This is to facilitate using this function to parse a script line-by-line, where the end of the
/// line (i.e. _{EOF}_) is not necessarily the end of the script.
///
/// Any time a [`StringConstant`][Token::StringConstant] is returned with
/// `state.is_within_text_terminated_by` set to `Some(_)` is one of the above conditions.
pub fn parse_string_literal(
    stream: &mut (impl InputStream + ?Sized),
    state: &mut TokenizeState,
    pos: &mut Position,
    termination_char: char,
    verbatim: bool,
    multi_line: bool,
) -> Result<(SmartString, Position), (LexError, Position)> {
    let mut result = SmartString::new_const();
    let mut escape = SmartString::new_const();

    let start = *pos;
    let mut first_char = Position::NONE;
    // After a line continuation, Rust skips the whitespace up to the next
    // character that isn't a space, a tab or a line break.
    let mut continued = false;

    state.is_within_text_terminated_by = Some(termination_char.to_string().into());
    if let Some(ref mut last) = state.last_token {
        last.clear();
        last.push(termination_char);
    }

    loop {
        debug_assert!(
            !verbatim || escape.is_empty(),
            "verbatim strings should not have any escapes"
        );

        let next_char = match stream.get_next() {
            Some(ch) => {
                pos.advance();
                ch
            }
            None if verbatim => {
                debug_assert_eq!(escape, "", "verbatim strings should not have any escapes");
                pos.advance();
                break;
            }
            None if multi_line && !escape.is_empty() => {
                debug_assert_eq!(escape, "\\", "unexpected escape {escape} at end of line");
                pos.advance();
                break;
            }
            None => {
                pos.advance();
                state.is_within_text_terminated_by = None;
                return Err((LERR::UnterminatedString, start));
            }
        };

        if let Some(ref mut last) = state.last_token {
            last.push(next_char);
        }

        // Check string length
        if let Some(max) = state.max_string_len
            && result.len() > max.get()
        {
            return Err((LexError::StringTooLong(max.get()), start));
        }

        // Close wrapper
        if termination_char == next_char && escape.is_empty() {
            // Double wrapper
            if stream.peek_next() == Some(termination_char) {
                stream.eat_next_and_advance(pos);
                if let Some(ref mut last) = state.last_token {
                    last.push(termination_char);
                }
            } else {
                state.is_within_text_terminated_by = None;
                break;
            }
        }

        if first_char.is_none() {
            first_char = *pos;
        }

        if continued {
            match next_char {
                '\n' => {
                    pos.new_line();
                    continue;
                }
                ' ' | '\t' | '\r' => continue,
                _ => continued = false,
            }
        }

        match next_char {
            // \r - ignore if followed by \n
            '\r' if stream.peek_next() == Some('\n') => (),
            // \r
            'r' if !escape.is_empty() => {
                escape.clear();
                result.push_str("\r");
            }
            // \n
            'n' if !escape.is_empty() => {
                escape.clear();
                result.push_str("\n");
            }
            // \...
            '\\' if !verbatim && escape.is_empty() => {
                escape.push_str("\\");
            }
            // \\
            '\\' if !escape.is_empty() => {
                escape.clear();
                result.push_str("\\");
            }
            // \t
            't' if !escape.is_empty() => {
                escape.clear();
                result.push_str("\t");
            }
            // \0, Rust's NUL
            '0' if !escape.is_empty() => {
                escape.clear();
                result.push('\0');
            }
            // \x??, \u????, \U????????
            ch @ ('x' | 'u' | 'U') if !escape.is_empty() => {
                let mut seq = escape.clone();
                escape.clear();
                seq.push(ch);

                let mut out_val: u32 = 0;

                // Rust's \u{1F600}: one to six hex digits in braces.
                if ch == 'u' && stream.peek_next() == Some('{') {
                    let malformed =
                        |seq: &str, pos: Position| (LERR::MalformedEscapeSequence(seq.into()), pos);
                    for digits in 0.. {
                        let c = stream.get_next().ok_or_else(|| malformed(&seq, *pos))?;
                        pos.advance();
                        seq.push(c);
                        if let Some(ref mut last) = state.last_token {
                            last.push(c);
                        }
                        match c {
                            '{' if digits == 0 => (),
                            '}' if digits > 1 => break,
                            c if digits <= 6 && c.is_ascii_hexdigit() => {
                                out_val = out_val * 16 + c.to_digit(16).unwrap();
                            }
                            _ => return Err(malformed(&seq, *pos)),
                        }
                    }
                    result.push(char::from_u32(out_val).ok_or_else(|| malformed(&seq, *pos))?);
                    continue;
                }

                let len = match ch {
                    'x' => 2,
                    'u' => 4,
                    'U' => 8,
                    c => unreachable!("x or u or U expected but gets '{}'", c),
                };

                for _ in 0..len {
                    let c = stream
                        .get_next()
                        .ok_or_else(|| (LERR::MalformedEscapeSequence(seq.to_string()), *pos))?;

                    pos.advance();
                    seq.push(c);
                    if let Some(ref mut last) = state.last_token {
                        last.push(c);
                    }

                    out_val *= 16;
                    out_val += c
                        .to_digit(16)
                        .ok_or_else(|| (LERR::MalformedEscapeSequence(seq.to_string()), *pos))?;
                }

                result.push(
                    char::from_u32(out_val)
                        .ok_or_else(|| (LERR::MalformedEscapeSequence(seq.to_string()), *pos))?,
                );
            }

            // LF - Verbatim
            '\n' if verbatim => {
                debug_assert_eq!(escape, "", "verbatim strings should not have any escapes");
                pos.new_line();
                result.push_str("\n");
            }

            // LF - Line continuation
            '\n' if multi_line && !escape.is_empty() => {
                debug_assert_eq!(escape, "\\", "unexpected escape {escape} at end of line");
                escape.clear();
                pos.new_line();
                continued = true;
            }

            // LF - A line break in a string that spans lines, as Rust's do
            '\n' if multi_line => {
                pos.new_line();
                result.push('\n');
            }

            // LF - Unterminated string
            '\n' => {
                pos.rewind();
                state.is_within_text_terminated_by = None;
                return Err((LERR::UnterminatedString, start));
            }

            // \{termination_char} - escaped termination character
            ch if termination_char == ch && !escape.is_empty() => {
                escape.clear();
                result.push(termination_char);
            }

            // Unknown escape sequence
            ch if !escape.is_empty() => {
                escape.push(ch);

                return Err((LERR::MalformedEscapeSequence(escape.to_string()), *pos));
            }

            // All other characters
            ch => {
                escape.clear();
                result.push(ch);
            }
        }
    }

    // Check string length
    if let Some(max) = state.max_string_len
        && result.len() > max.get()
    {
        return Err((LexError::StringTooLong(max.get()), start));
    }

    Ok((result, first_char))
}

/// Scan for a block comment until the end.
fn scan_block_comment(
    stream: &mut (impl InputStream + ?Sized),
    level: usize,
    pos: &mut Position,
    comment: Option<&mut String>,
) -> usize {
    let mut level = level;
    let mut comment = comment;

    while let Some(c) = stream.get_next() {
        pos.advance();

        if let Some(comment) = comment.as_mut() {
            comment.push(c);
        }

        match c {
            '/' => {
                if let Some(c2) = stream.peek_next().filter(|&ch| ch == '*') {
                    stream.eat_next_and_advance(pos);
                    if let Some(comment) = comment.as_mut() {
                        comment.push(c2);
                    }
                    level += 1;
                }
            }
            '*' => {
                if let Some(c2) = stream.peek_next().filter(|&ch| ch == '/') {
                    stream.eat_next_and_advance(pos);
                    if let Some(comment) = comment.as_mut() {
                        comment.push(c2);
                    }
                    level -= 1;
                }
            }
            '\n' => pos.new_line(),
            _ => (),
        }

        if level == 0 {
            break;
        }
    }

    level
}

/// Test if the given character is a hex character.
#[inline(always)]
const fn is_hex_digit(c: char) -> bool {
    c.is_ascii_hexdigit()
}

/// Test if the given character is a numeric digit (i.e. 0-9).
#[inline(always)]
const fn is_numeric_digit(c: char) -> bool {
    c.is_ascii_digit()
}

/// Test if the given character is an octal digit (i.e. 0-7).
#[inline(always)]
const fn is_octal_digit(c: char) -> bool {
    matches!(c, '0'..='7')
}

/// Test if the given character is a binary digit (i.e. 0 or 1).
#[inline(always)]
const fn is_binary_digit(c: char) -> bool {
    c == '0' || c == '1'
}

/// Test if the comment block is a doc-comment.
#[cfg(feature = "metadata")]
#[inline]
#[must_use]
pub fn is_doc_comment(comment: &str) -> bool {
    (comment.starts_with("///") && !comment.starts_with("////"))
        || (comment.starts_with("/**") && !comment.starts_with("/***"))
}

/// _(internals)_ Get the next token from the input stream.
/// Exported under the `internals` feature only.
#[inline(always)]
#[must_use]
pub fn get_next_token(
    stream: &mut (impl InputStream + ?Sized),
    state: &mut TokenizeState,
    pos: &mut Position,
) -> (Token, Position) {
    let result = get_next_token_inner(stream, state, pos);

    // Save the last token's state
    state.next_token_cannot_be_unary = !result.0.is_next_unary();

    result
}

/// Get the next token.
#[must_use]
fn get_next_token_inner(
    stream: &mut (impl InputStream + ?Sized),
    state: &mut TokenizeState,
    pos: &mut Position,
) -> (Token, Position) {
    state.last_token.as_mut().map(SmartString::clear);

    // Still inside a comment?
    if state.comment_level > 0 {
        let start_pos = *pos;
        let mut comment = String::new();
        let comment_buf = state.include_comments.then_some(&mut comment);

        state.comment_level = scan_block_comment(stream, state.comment_level, pos, comment_buf);

        let return_comment = state.include_comments;

        #[cfg(feature = "metadata")]
        let return_comment = return_comment || is_doc_comment(&comment);

        if return_comment {
            return (Token::Comment(comment.into()), start_pos);
        }

        // Reached EOF without ending comment block?
        if state.comment_level > 0 {
            return (Token::EOF, *pos);
        }
    }

    // Within text?
    match state.is_within_text_terminated_by.take() {
        Some(ch) if ch.starts_with('#') => {
            return parse_raw_string_literal(stream, state, pos, ch.len()).map_or_else(
                |(err, err_pos)| (Token::LexError(err.into()), err_pos),
                |(result, start_pos)| (Token::StringConstant(result.into()), start_pos),
            )
        }
        Some(ch) => {
            let c = ch.chars().next().unwrap();

            return parse_string_literal(stream, state, pos, c, true, false).map_or_else(
                |(err, err_pos)| (Token::LexError(err.into()), err_pos),
                |(result, start_pos)| (Token::StringConstant(result.into()), start_pos),
            );
        }
        None => (),
    }

    let mut negated: Option<Position> = None;

    while let Some(c) = stream.get_next() {
        pos.advance();

        let start_pos = *pos;
        let cc = stream.peek_next().unwrap_or('\0');

        // Identifiers and strings that can have non-ASCII characters
        match (c, cc) {
            // digit ...
            ('0'..='9', ..) => {
                let mut result = SmartString::new_const();
                let mut radix_base: Option<u32> = None;
                let mut valid: fn(char) -> bool = is_numeric_digit;
                let mut _has_period = false;
                let mut _has_e = false;

                result.push(c);

                while let Some(next_char) = stream.peek_next() {
                    match next_char {
                        NUMBER_SEPARATOR => {
                            stream.eat_next_and_advance(pos);
                        }
                        ch if valid(ch) => {
                            result.push(ch);
                            stream.eat_next_and_advance(pos);
                        }
                        '.' if !_has_period && radix_base.is_none() => {
                            stream.get_next().unwrap();

                            // Check if followed by digits or something that cannot start a property name
                            match stream.peek_next() {
                                // digits after period - accept the period
                                Some('0'..='9') => {
                                    result.push_str(".");
                                    pos.advance();
                                    _has_period = true;
                                }
                                // _ - cannot follow a decimal point
                                Some(NUMBER_SEPARATOR) => {
                                    stream.unget('.');
                                    break;
                                }
                                // .. - reserved symbol, not a floating-point number
                                Some('.') => {
                                    stream.unget('.');
                                    break;
                                }
                                // symbol after period - probably a float
                                Some(ch) if !is_id_first_alphabetic(ch) => {
                                    result.push_str(".");
                                    pos.advance();
                                    result.push_str("0");
                                    _has_period = true;
                                }
                                // Not a floating-point number
                                _ => {
                                    stream.unget('.');
                                    break;
                                }
                            }
                        }
                        ch @ ('e' | 'E') if !_has_e && radix_base.is_none() => {
                            stream.get_next().unwrap();

                            // Check if followed by digits or +/-
                            match stream.peek_next() {
                                // digits after e/E - accept as 'e' (no decimal points allowed)
                                Some('0'..='9') => {
                                    result.push('e');
                                    pos.advance();
                                    _has_e = true;
                                    _has_period = true;
                                }
                                // +/- after e/E - accept as 'e' and the sign (no decimal points allowed)
                                Some('+' | '-') => {
                                    result.push('e');
                                    pos.advance();
                                    result.push(stream.get_next().unwrap());
                                    pos.advance();
                                    _has_e = true;
                                    _has_period = true;
                                }
                                // Not a floating-point number
                                _ => {
                                    stream.unget(ch);
                                    break;
                                }
                            }
                        }
                        // 0x????, 0o????, 0b???? at beginning
                        ch @ ('x' | 'o' | 'b' | 'X' | 'O' | 'B')
                            if c == '0' && result.len() <= 1 =>
                        {
                            result.push(ch);
                            stream.eat_next_and_advance(pos);

                            valid = match ch {
                                'x' | 'X' => is_hex_digit,
                                'o' | 'O' => is_octal_digit,
                                'b' | 'B' => is_binary_digit,
                                c => unreachable!("x/X or o/O or b/B expected but gets '{}'", c),
                            };

                            radix_base = Some(match ch {
                                'x' | 'X' => 16,
                                'o' | 'O' => 8,
                                'b' | 'B' => 2,
                                c => unreachable!("x/X or o/O or b/B expected but gets '{}'", c),
                            });
                        }

                        _ => break,
                    }
                }

                let num_pos = negated.map_or(start_pos, |negated_pos| {
                    result.insert(0, '-');
                    negated_pos
                });

                if let Some(ref mut last) = state.last_token {
                    *last = result.clone();
                }

                // Parse number
                let token = if let Some(radix) = radix_base {
                    let digits = &result[2..];

                    // Checked Rust reads a literal past `i64::MAX` by its
                    // text, as a decimal one past it is (an `i128` or a
                    // `u128`, or a `u64`'s bits).
                    if state.tokenizer_control.borrow().rust
                        && let Ok(value) = u128::from_str_radix(digits, radix)
                        && INT::try_from(value).is_err()
                    {
                        return (
                            Token::FloatConstant(
                                (
                                    crate::types::FloatWrapper::new(value as crate::FLOAT),
                                    result,
                                )
                                    .into(),
                            ),
                            num_pos,
                        );
                    }
                    let result = digits;

                    // Deliberately allow signed integers to be created from unsigned parsing
                    #[allow(clippy::cast_possible_wrap)]
                    UNSIGNED_INT::from_str_radix(result, radix)
                        .map(|v| v as INT)
                        .map_or_else(
                            |_| Token::LexError(LERR::MalformedNumber(result.to_string()).into()),
                            Token::IntegerConstant,
                        )
                } else {
                    (|| {
                        let num = INT::from_str(&result).map(Token::IntegerConstant);

                        // If integer parsing is unnecessary, try float instead
                        if num.is_err()
                            && let Ok(v) = crate::types::FloatWrapper::from_str(&result)
                        {
                            return Token::FloatConstant((v, result).into());
                        }

                        // Then try decimal
                        #[cfg(feature = "decimal")]
                        if num.is_err()
                            && let Ok(v) = rust_decimal::Decimal::from_str(&result)
                        {
                            return Token::DecimalConstant((v, result).into());
                        }

                        // Then try decimal in scientific notation
                        #[cfg(feature = "decimal")]
                        if num.is_err()
                            && let Ok(v) = rust_decimal::Decimal::from_scientific(&result)
                        {
                            return Token::DecimalConstant((v, result).into());
                        }

                        num.unwrap_or_else(|_| {
                            Token::LexError(LERR::MalformedNumber(result.to_string()).into())
                        })
                    })()
                };

                return (token, num_pos);
            }

            // " - string literal
            ('"', ..) => {
                return parse_string_literal(stream, state, pos, c, false, true).map_or_else(
                    |(err, err_pos)| (Token::LexError(err.into()), err_pos),
                    |(result, ..)| (Token::StringConstant(result.into()), start_pos),
                );
            }
            // Rust has no backtick strings.
            ('`', ..) => {
                return (
                    Token::LexError(
                        LERR::ImproperSymbol("`".into(), "unknown start of token: `".into()).into(),
                    ),
                    start_pos,
                );
            }

            // `#"…"#` is not Rust's raw string, which starts with `r`.
            ('#', '"' | '#') if state.tokenizer_control.borrow().rust => {
                return (
                    Token::LexError(
                        LERR::ImproperSymbol("#".into(), "expected expression, found `#`".into())
                            .into(),
                    ),
                    start_pos,
                );
            }
            // r - raw string literal
            ('#', '"' | '#') => {
                return parse_raw_string_literal(stream, state, pos, 0).map_or_else(
                    |(err, err_pos)| (Token::LexError(err.into()), err_pos),
                    |(result, ..)| (Token::StringConstant(result.into()), start_pos),
                );
            }

            // ' - lifetime marker
            ('\'', next)
                if (is_id_first_alphabetic(next) || next == '_') && is_lifetime_marker(stream) =>
            {
                return (Token::Apostrophe, start_pos);
            }

            // ' - character literal
            ('\'', '\'') => {
                return (
                    Token::LexError(LERR::MalformedChar(String::new()).into()),
                    start_pos,
                )
            }
            ('\'', ..) => {
                return parse_string_literal(stream, state, pos, c, false, false).map_or_else(
                    |(err, err_pos)| (Token::LexError(err.into()), err_pos),
                    |(result, ..)| {
                        let mut chars = result.chars();
                        let first = chars.next().unwrap();

                        if chars.next().is_some() {
                            (
                                Token::LexError(LERR::MalformedChar(result.to_string()).into()),
                                start_pos,
                            )
                        } else {
                            (Token::CharConstant(first), start_pos)
                        }
                    },
                )
            }

            // Braces
            ('{', ..) => return (Token::LeftBrace, start_pos),
            ('}', ..) => return (Token::RightBrace, start_pos),

            // Unit
            ('(', ')') => {
                stream.eat_next_and_advance(pos);
                return (Token::Unit, start_pos);
            }

            // Parentheses
            ('(', ..) => return (Token::LeftParen, start_pos),
            (')', ..) => return (Token::RightParen, start_pos),

            // Indexing
            ('[', ..) => return (Token::LeftBracket, start_pos),
            (']', ..) => return (Token::RightBracket, start_pos),

            // Map literal
            ('#', '{') => {
                stream.eat_next_and_advance(pos);
                return (Token::MapStart, start_pos);
            }
            // `#!` opens an inner attribute (`#![allow(..)]`). A shebang
            // line is stepped over before the first token (`lex_raw`).
            ('#', '!') => {
                stream.eat_next_and_advance(pos);
                return (Token::Reserved(Box::new("#!".into())), start_pos);
            }

            ('#', ..) => return (Token::Hash, start_pos),

            // Operators
            ('+', '=') => {
                stream.eat_next_and_advance(pos);
                return (Token::PlusAssign, start_pos);
            }
            ('+', '+') => {
                stream.eat_next_and_advance(pos);
                return (Token::Reserved(Box::new("++".into())), start_pos);
            }
            ('+', ..) if !state.next_token_cannot_be_unary => return (Token::UnaryPlus, start_pos),
            ('+', ..) => return (Token::Plus, start_pos),

            ('-', '0'..='9') if !state.next_token_cannot_be_unary => negated = Some(start_pos),
            ('-', '0'..='9') => return (Token::Minus, start_pos),
            ('-', '=') => {
                stream.eat_next_and_advance(pos);
                return (Token::MinusAssign, start_pos);
            }
            ('-', '>') => {
                stream.eat_next_and_advance(pos);
                return (Token::ThinArrow, start_pos);
            }
            ('-', ..) if !state.next_token_cannot_be_unary => {
                return (Token::UnaryMinus, start_pos)
            }
            ('-', ..) => return (Token::Minus, start_pos),

            ('*', '=') => {
                stream.eat_next_and_advance(pos);
                return (Token::MultiplyAssign, start_pos);
            }
            // `**r` in prefix position is two derefs, as in Rust.
            ('*', '*') if !state.next_token_cannot_be_unary => return (Token::Multiply, start_pos),
            ('*', '*') => {
                stream.eat_next_and_advance(pos);

                return (
                    if stream.peek_next() == Some('=') {
                        stream.eat_next_and_advance(pos);
                        Token::PowerOfAssign
                    } else {
                        Token::PowerOf
                    },
                    start_pos,
                );
            }
            ('*', ..) => return (Token::Multiply, start_pos),

            // Comments
            ('/', '/') => {
                stream.eat_next_and_advance(pos);

                let mut comment: Option<String> = match stream.peek_next() {
                    #[cfg(feature = "metadata")]
                    Some('/') => {
                        stream.eat_next_and_advance(pos);

                        // Long streams of `///...` are not doc-comments
                        match stream.peek_next() {
                            Some('/') => None,
                            _ => Some("///".into()),
                        }
                    }
                    #[cfg(feature = "metadata")]
                    Some('!') => {
                        stream.eat_next_and_advance(pos);
                        Some("//!".into())
                    }
                    _ if state.include_comments => Some("//".into()),
                    _ => None,
                };

                while let Some(c) = stream.get_next() {
                    if c == '\r' {
                        // \r\n
                        if stream.peek_next() == Some('\n') {
                            stream.eat_next_and_advance(pos);
                        }
                        pos.new_line();
                        break;
                    }
                    if c == '\n' {
                        pos.new_line();
                        break;
                    }
                    if let Some(comment) = comment.as_mut() {
                        comment.push(c);
                    }
                    pos.advance();
                }

                match comment {
                    #[cfg(feature = "metadata")]
                    Some(comment) if comment.starts_with("//!") => {
                        let g = &mut state.tokenizer_control.borrow_mut().global_comments;
                        if !g.is_empty() {
                            *g += "\n";
                        }
                        *g += &comment;
                    }
                    Some(comment) => return (Token::Comment(comment.into()), start_pos),
                    None => (),
                }
            }
            ('/', '*') => {
                state.comment_level += 1;
                stream.eat_next_and_advance(pos);

                let mut comment: Option<String> = match stream.peek_next() {
                    #[cfg(feature = "metadata")]
                    Some('*') => {
                        stream.eat_next_and_advance(pos);

                        // Long streams of `/****...` are not doc-comments
                        match stream.peek_next() {
                            Some('*') => None,
                            _ => Some("/**".into()),
                        }
                    }
                    _ if state.include_comments => Some("/*".into()),
                    _ => None,
                };

                state.comment_level =
                    scan_block_comment(stream, state.comment_level, pos, comment.as_mut());

                if let Some(comment) = comment {
                    return (Token::Comment(comment.into()), start_pos);
                }
            }

            ('/', '=') => {
                stream.eat_next_and_advance(pos);
                return (Token::DivideAssign, start_pos);
            }
            ('/', ..) => return (Token::Divide, start_pos),

            (';', ..) => return (Token::SemiColon, start_pos),
            (',', ..) => return (Token::Comma, start_pos),

            ('.', '.') => {
                stream.eat_next_and_advance(pos);
                return (
                    match stream.peek_next() {
                        Some('.') => {
                            stream.eat_next_and_advance(pos);
                            Token::Reserved(Box::new("...".into()))
                        }
                        Some('=') => {
                            stream.eat_next_and_advance(pos);
                            Token::InclusiveRange
                        }
                        _ => Token::ExclusiveRange,
                    },
                    start_pos,
                );
            }
            ('.', ..) => return (Token::Period, start_pos),

            ('=', '=') => {
                stream.eat_next_and_advance(pos);
                return (Token::EqualsTo, start_pos);
            }
            ('=', '>') => {
                stream.eat_next_and_advance(pos);
                return (Token::DoubleArrow, start_pos);
            }
            ('=', ..) => return (Token::Equals, start_pos),

            // `::<` is Rust's turbofish: `::` then `<`.
            (':', ':') => {
                stream.eat_next_and_advance(pos);
                return (Token::DoubleColon, start_pos);
            }
            (':', ..) => return (Token::Colon, start_pos),

            ('<', '=') => {
                stream.eat_next_and_advance(pos);
                return (Token::LessThanEqualsTo, start_pos);
            }
            ('<', '<') => {
                stream.eat_next_and_advance(pos);

                return (
                    if stream.peek_next() == Some('=') {
                        stream.eat_next_and_advance(pos);
                        Token::LeftShiftAssign
                    } else {
                        Token::LeftShift
                    },
                    start_pos,
                );
            }
            ('<', '|') => {
                stream.eat_next_and_advance(pos);
                return (Token::Reserved(Box::new("<|".into())), start_pos);
            }
            ('<', ..) => return (Token::LessThan, start_pos),

            ('>', '=') => {
                stream.eat_next_and_advance(pos);
                return (Token::GreaterThanEqualsTo, start_pos);
            }
            ('>', '>') => {
                stream.eat_next_and_advance(pos);

                return (
                    if stream.peek_next() == Some('=') {
                        stream.eat_next_and_advance(pos);
                        Token::RightShiftAssign
                    } else {
                        Token::RightShift
                    },
                    start_pos,
                );
            }
            ('>', ..) => return (Token::GreaterThan, start_pos),

            ('!', '=') => {
                stream.eat_next_and_advance(pos);
                return (Token::NotEqualsTo, start_pos);
            }
            ('!', '.') => {
                stream.eat_next_and_advance(pos);
                return (Token::Reserved(Box::new("!.".into())), start_pos);
            }
            ('!', ..) => return (Token::Bang, start_pos),

            ('|', '|') => {
                stream.eat_next_and_advance(pos);
                return (Token::Or, start_pos);
            }
            ('|', '=') => {
                stream.eat_next_and_advance(pos);
                return (Token::OrAssign, start_pos);
            }
            ('|', '>') => {
                stream.eat_next_and_advance(pos);
                return (Token::Reserved(Box::new("|>".into())), start_pos);
            }
            ('|', ..) => return (Token::Pipe, start_pos),

            ('&', '&') => {
                stream.eat_next_and_advance(pos);
                return (Token::And, start_pos);
            }
            ('&', '=') => {
                stream.eat_next_and_advance(pos);
                return (Token::AndAssign, start_pos);
            }
            ('&', ..) => return (Token::Ampersand, start_pos),

            ('^', '=') => {
                stream.eat_next_and_advance(pos);
                return (Token::XOrAssign, start_pos);
            }
            ('^', ..) => return (Token::XOr, start_pos),

            ('~', ..) => return (Token::Reserved(Box::new("~".into())), start_pos),

            ('%', '=') => {
                stream.eat_next_and_advance(pos);
                return (Token::ModuloAssign, start_pos);
            }
            ('%', ..) => return (Token::Modulo, start_pos),

            ('@', ..) => return (Token::Reserved(Box::new("@".into())), start_pos),

            ('$', ..) => return (Token::Reserved(Box::new("$".into())), start_pos),

            ('?', ..) => return (Token::Reserved(Box::new("?".into())), start_pos),

            // Rust's byte literal: b'a', a `u8`.
            ('b', '\'') => {
                stream.eat_next_and_advance(pos);
                return parse_string_literal(stream, state, pos, '\'', false, false).map_or_else(
                    |(err, err_pos)| (Token::LexError(err.into()), err_pos),
                    |(result, ..)| {
                        let mut chars = result.chars();
                        match (chars.next(), chars.next()) {
                            (Some(byte), None) if (byte as u32) < 256 => {
                                (Token::ByteConstant(byte as u8), start_pos)
                            }
                            _ => (
                                Token::LexError(LERR::MalformedChar(result.to_string()).into()),
                                start_pos,
                            ),
                        }
                    },
                );
            }

            // Rust's raw strings and byte strings: r"…", r#"…"#, b"…", br#"…"#.
            ('r', '"' | '#') | ('b', '"' | 'r') => {
                if c == 'r'
                    && let Some(token) = parse_raw_identifier(stream, pos)
                {
                    return (token, start_pos);
                }
                if let Some(token) = parse_rust_string(stream, pos, c) {
                    return (token, start_pos);
                }
                return parse_identifier_token(stream, state, pos, start_pos, c);
            }

            // letter or underscore ...
            _ if is_id_first_alphabetic(c) || c == '_' => {
                return parse_identifier_token(stream, state, pos, start_pos, c);
            }

            // \n
            ('\n', ..) => pos.new_line(),

            // Whitespace - follows Rust's SPACE, TAB, CR, LF, FF which is the same as WhatWG.
            (ch, ..) if ch.is_ascii_whitespace() => (),

            _ => {
                return (
                    Token::LexError(LERR::UnexpectedInput(c.to_string()).into()),
                    start_pos,
                )
            }
        }
    }

    pos.advance();

    (Token::EOF, *pos)
}

/// Rust's raw identifier (`r#for`, `r#name`), after its `r`. `None` when the
/// `r` starts something else, with the stream as it was.
///
/// A keyword is an identifier the lowered script never reads as one, and
/// messages show it as written (`raw_names`): `r#for` is `__rawr_r_for`.
/// Any other name is the identifier it spells: `r#name` is `name`, as in
/// Rust. Rust refuses
/// `r#crate`, `r#self`, `r#super`, `r#Self` and `r#_`.
fn parse_raw_identifier(
    stream: &mut (impl InputStream + ?Sized),
    pos: &mut Position,
) -> Option<Token> {
    if stream.peek_next() != Some('#') {
        return None;
    }
    stream.get_next();
    match stream.peek_next() {
        Some(next) if is_id_first_alphabetic(next) || next == '_' => {}
        _ => {
            stream.unget('#');
            return None;
        }
    }
    pos.advance();
    let mut word = SmartString::new_const();
    while let Some(next) = stream.peek_next() {
        if !is_id_continue(next) {
            break;
        }
        stream.eat_next_and_advance(pos);
        word.push(next);
    }
    if matches!(word.as_str(), "crate" | "self" | "super" | "Self" | "_") {
        return Some(Token::LexError(
            LERR::MalformedIdentifier(format!("r#{word}")).into(),
        ));
    }
    let keyword = RUST_KEYWORDS.contains(&word.as_str())
        || Token::lookup_symbol_from_syntax(&word).is_some()
        || is_reserved_keyword_or_symbol(&word);
    if !keyword && !is_valid_identifier(&word) {
        return Some(Token::LexError(
            LERR::MalformedIdentifier(format!("r#{word}")).into(),
        ));
    }
    Some(Token::Identifier(
        if keyword {
            format!("{RAW_PREFIX}{word}").into()
        } else {
            word
        }
        .into(),
    ))
}

/// The prefix of a raw identifier whose name is a keyword: `r#for` is
/// `__rawr_r_for`.
const RAW_PREFIX: &str = "__rawr_r_";

/// Rust's strict and reserved keywords, which a raw identifier alone can
/// name. The parser reads some of them by their text (`type`, `where`,
/// `dyn`), so the lexer's own keywords are not all of them.
const RUST_KEYWORDS: [&str; 52] = [
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "async", "await", "dyn", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "typeof", "unsized", "virtual", "yield", "try", "gen",
];

/// Rust's raw strings (`r"…"`, `r#"…"#`) and byte strings (`b"…"`,
/// `br"…"`, `br#"…"#`), after their first letter `first`. `None` when the
/// letter starts a name instead, with the stream as it was.
fn parse_rust_string(
    stream: &mut (impl InputStream + ?Sized),
    pos: &mut Position,
    first: char,
) -> Option<Token> {
    let bytes = first == 'b';
    let raw = !bytes || stream.peek_next() == Some('r');
    if bytes && raw {
        stream.get_next();
        if !matches!(stream.peek_next(), Some('"' | '#')) {
            stream.unget('r');
            return None;
        }
        pos.advance();
    }
    let mut hashes = 0;
    if raw && stream.peek_next() == Some('#') {
        stream.get_next();
        if stream.peek_next() != Some('#') && stream.peek_next() != Some('"') {
            if bytes {
                return Some(Token::LexError(LERR::UnexpectedInput("#".into()).into()));
            }
            stream.unget('#');
            return None;
        }
        pos.advance();
        hashes = 1;
        while stream.peek_next() == Some('#') {
            stream.eat_next_and_advance(pos);
            hashes += 1;
        }
    }
    match stream.eat_next_and_advance(pos) {
        Some('"') => (),
        Some(c) => return Some(Token::LexError(LERR::UnexpectedInput(c.to_string()).into())),
        None => return Some(Token::LexError(LERR::UnterminatedString.into())),
    }
    let mut text = String::new();
    let mut out = Vec::new();
    loop {
        let Some(c) = stream.get_next() else {
            return Some(Token::LexError(LERR::UnterminatedString.into()));
        };
        if c == '\n' {
            pos.new_line();
        } else {
            pos.advance();
        }
        match c {
            '"' if raw => {
                let mut seen = 0;
                while seen < hashes && stream.peek_next() == Some('#') {
                    stream.eat_next_and_advance(pos);
                    seen += 1;
                }
                if seen == hashes {
                    break;
                }
                text.push('"');
                text.extend(std::iter::repeat_n('#', seen));
            }
            '"' => break,
            '\\' if !raw => {
                let escape = stream.eat_next_and_advance(pos);
                let byte = match escape {
                    Some('n') => b'\n',
                    Some('r') => b'\r',
                    Some('t') => b'\t',
                    Some('0') => 0,
                    Some(c @ ('\\' | '\'' | '"')) => c as u8,
                    Some('x') => {
                        let digits: String = (0..2)
                            .filter_map(|_| stream.eat_next_and_advance(pos))
                            .collect();
                        match u8::from_str_radix(&digits, 16) {
                            Ok(byte) if digits.len() == 2 => byte,
                            _ => {
                                return Some(Token::LexError(
                                    LERR::MalformedEscapeSequence(format!("\\x{digits}")).into(),
                                ))
                            }
                        }
                    }
                    // A line continuation: the line break and the next line's indentation.
                    Some('\n') => {
                        pos.new_line();
                        while stream.peek_next().is_some_and(char::is_whitespace) {
                            if stream.get_next() == Some('\n') {
                                pos.new_line();
                            } else {
                                pos.advance();
                            }
                        }
                        continue;
                    }
                    other => {
                        let text = other.map_or_else(String::new, String::from);
                        return Some(Token::LexError(
                            LERR::MalformedEscapeSequence(format!("\\{text}")).into(),
                        ));
                    }
                };
                out.push(byte);
            }
            c if bytes && !c.is_ascii() => {
                return Some(Token::LexError(LERR::UnexpectedInput(c.to_string()).into()));
            }
            c if bytes && !raw => out.push(c as u8),
            c => text.push(c),
        }
    }
    Some(if !bytes {
        Token::StringConstant(Box::new(text.into()))
    } else if raw {
        Token::ByteStringConstant(Box::new(text.into_bytes()))
    } else {
        Token::ByteStringConstant(Box::new(out))
    })
}

/// Get the next token, parsing it as an identifier.
fn parse_identifier_token(
    stream: &mut (impl InputStream + ?Sized),
    state: &mut TokenizeState,
    pos: &mut Position,
    start_pos: Position,
    first_char: char,
) -> (Token, Position) {
    let mut identifier = SmartString::new_const();
    identifier.push(first_char);
    if let Some(ref mut last) = state.last_token {
        last.clear();
        last.push(first_char);
    }

    while let Some(next_char) = stream.peek_next() {
        match next_char {
            x if is_id_continue(x) => {
                stream.eat_next_and_advance(pos);
                identifier.push(x);
                if let Some(ref mut last) = state.last_token {
                    last.push(x);
                }
            }
            _ => break,
        }
    }

    if let Some(token) = Token::lookup_symbol_from_syntax(&identifier) {
        return (token, start_pos);
    }

    // The lowered script's own keywords, which no Rust name spells.
    if !state.tokenizer_control.borrow().rust {
        let token = match identifier.as_str() {
            "__rawr_switch" => Some(Token::Switch),
            "__rawr_throw" => Some(Token::Throw),
            "__rawr_private" => Some(Token::Private),
            _ => None,
        };
        if let Some(token) = token {
            return (token, start_pos);
        }
    }

    if is_reserved_keyword_or_symbol(&identifier) {
        return (Token::Reserved(Box::new(identifier)), start_pos);
    }

    if !is_valid_identifier(&identifier) {
        return (
            Token::LexError(LERR::MalformedIdentifier(identifier.to_string()).into()),
            start_pos,
        );
    }

    (Token::Identifier(identifier.into()), start_pos)
}

fn is_lifetime_marker(stream: &mut (impl InputStream + ?Sized)) -> bool {
    let next = stream
        .get_next()
        .expect("peeked lifetime character must exist");
    let is_lifetime = stream.peek_next() != Some('\'');
    stream.unget(next);
    is_lifetime
}

/// The name the identifier `name` stands for: a raw identifier's keyword
/// bare, as Rust names it (`__rawr_r_type` is `type`), and any other name
/// itself.
pub(crate) fn unraw(name: &str) -> &str {
    name.strip_prefix(RAW_PREFIX).unwrap_or(name)
}

/// `text` with each raw identifier's keyword written as Rust writes it in
/// messages: `__rawr_r_for` is `r#for`.
pub(crate) fn raw_names(text: &str) -> std::borrow::Cow<'_, str> {
    keywords_written(text, "r#")
}

/// `text` with each raw identifier's keyword bare, as Rust's `Debug`
/// derive names a field: `__rawr_r_type` is `type`.
pub(crate) fn bare_names(text: &str) -> std::borrow::Cow<'_, str> {
    keywords_written(text, "")
}

fn keywords_written<'a>(text: &'a str, prefix: &str) -> std::borrow::Cow<'a, str> {
    if !text.contains(RAW_PREFIX) {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(RAW_PREFIX) {
        output.push_str(&rest[..at]);
        let raw = &rest[at + RAW_PREFIX.len()..];
        if raw.starts_with(|c: char| is_id_first_alphabetic(c) || c == '_') {
            output.push_str(prefix);
        } else {
            output.push_str(RAW_PREFIX);
        }
        rest = raw;
    }
    output.push_str(rest);
    std::borrow::Cow::Owned(output)
}

/// _(internals)_ Is a text string a valid identifier?
/// Exported under the `internals` feature only.
#[must_use]
pub fn is_valid_identifier(name: &str) -> bool {
    let mut first_alphabetic = false;

    for ch in name.chars() {
        match ch {
            '_' => (),
            _ if is_id_first_alphabetic(ch) => first_alphabetic = true,
            _ if !first_alphabetic => return false,
            _ if char::is_ascii_alphanumeric(&ch) => (),
            _ => return false,
        }
    }

    first_alphabetic
}

/// _(internals)_ Is a text string a valid script-defined function name?
/// Exported under the `internals` feature only.
#[inline(always)]
#[must_use]
pub fn is_valid_function_name(name: &str) -> bool {
    is_valid_identifier(name)
        && !is_reserved_keyword_or_symbol(name)
        && Token::lookup_symbol_from_syntax(name).is_none()
}

/// Is a character valid to start an identifier?
#[inline(always)]
#[must_use]
#[allow(clippy::missing_const_for_fn)]
pub fn is_id_first_alphabetic(x: char) -> bool {
    #[cfg(feature = "unicode-xid-ident")]
    return unicode_xid::UnicodeXID::is_xid_start(x);
    #[cfg(not(feature = "unicode-xid-ident"))]
    return x.is_ascii_alphabetic();
}

/// Is a character valid for an identifier?
#[inline(always)]
#[must_use]
#[allow(clippy::missing_const_for_fn)]
pub fn is_id_continue(x: char) -> bool {
    #[cfg(feature = "unicode-xid-ident")]
    return unicode_xid::UnicodeXID::is_xid_continue(x);
    #[cfg(not(feature = "unicode-xid-ident"))]
    return x.is_ascii_alphanumeric() || x == '_';
}

/// _(internals)_ Is `syntax` a word Rust reserves that the lexer has no
/// token of its own for, or a symbol it reads as reserved?
/// Exported under the `internals` feature only.
#[must_use]
pub fn is_reserved_keyword_or_symbol(syntax: &str) -> bool {
    matches!(
        syntax,
        "async"
            | "await"
            | "match"
            | "static"
            | "super"
            | "yield"
            | "?"
            | "#!"
            | "~"
            | "!."
            | "..."
            | "@"
            | "<|"
            | "$"
            | "|>"
            | "++"
    )
}

/// _(internals)_ A type that implements the [`InputStream`] trait.
/// Exported under the `internals` feature only.
///
/// Multiple character streams are jointed together to form one single stream.
pub struct MultiInputsStream<'a> {
    /// Buffered characters, if any.
    pub buf: [Option<char>; 2],
    /// The current stream index.
    pub index: usize,
    /// Input character streams.
    pub streams: StaticVec<Peekable<Chars<'a>>>,
}

impl InputStream for MultiInputsStream<'_> {
    #[inline]
    fn unget(&mut self, ch: char) {
        match self.buf {
            [None, ..] => self.buf[0] = Some(ch),
            [_, None] => self.buf[1] = Some(ch),
            _ => unreachable!("cannot unget more than 2 characters!"),
        }
    }
    fn get_next(&mut self) -> Option<char> {
        match self.buf {
            [None, ..] => (),
            [ch @ Some(_), None] => {
                self.buf[0] = None;
                return ch;
            }
            [_, ch @ Some(_)] => {
                self.buf[1] = None;
                return ch;
            }
        }

        loop {
            if self.index >= self.streams.len() {
                // No more streams
                return None;
            }
            if let Some(ch) = self.streams[self.index].next() {
                // Next character in main stream
                return Some(ch);
            }
            // Jump to the next stream
            self.index += 1;
        }
    }
    fn peek_next(&mut self) -> Option<char> {
        match self.buf {
            [None, ..] => (),
            [ch @ Some(_), None] => return ch,
            [_, ch @ Some(_)] => return ch,
        }

        loop {
            if self.index >= self.streams.len() {
                // No more streams
                return None;
            }
            if let Some(&ch) = self.streams[self.index].peek() {
                // Next character in main stream
                return Some(ch);
            }
            // Jump to the next stream
            self.index += 1;
        }
    }
}

/// _(internals)_ An iterator on a [`Token`] stream.
/// Exported under the `internals` feature only.
pub struct TokenIterator<'a> {
    /// Current state.
    pub state: TokenizeState,
    /// Current position.
    pub pos: Position,
    /// Input character stream.
    pub stream: MultiInputsStream<'a>,
    /// A processor function that maps a token to another.
    pub token_mapper: Option<&'a OnParseTokenCallback>,
    /// A token to give before reading the stream: the error for a
    /// frontmatter block cargo refuses.
    pub pending: Option<(Token, Position)>,
}

impl Iterator for TokenIterator<'_> {
    type Item = (Token, Position);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(pending) = self.pending.take() {
            return Some(pending);
        }
        let (within_text, compress_script) = (
            self.state.is_within_text_terminated_by.is_some(),
            self.state.tokenizer_control.borrow().compressed.is_some(),
        );

        let (token, pos) = match get_next_token(&mut self.stream, &mut self.state, &mut self.pos) {
            // {EOF}
            r @ (Token::EOF, _) => return Some(r),
            // {EOF} after unterminated string.
            // The only case where `TokenizeState.is_within_text_terminated_by` is set is when
            // a verbatim string or a string with continuation encounters {EOF}.
            // This is necessary to handle such cases for line-by-line parsing, but for an entire
            // script it is a syntax error.
            (Token::StringConstant(..), pos)
                if self.state.is_within_text_terminated_by.is_some() =>
            {
                self.state.is_within_text_terminated_by = None;
                return Some((Token::LexError(LERR::UnterminatedString.into()), pos));
            }
            // Normal symbol
            r => r,
        };

        // Run the mapper, if any
        let token = match self.token_mapper {
            Some(func) => func(token, pos, &self.state),
            None => token,
        };

        // Collect the compressed script, if needed
        if compress_script {
            let control = &mut *self.state.tokenizer_control.borrow_mut();

            if token != Token::EOF
                && let Some(ref mut compressed) = control.compressed
            {
                use std::fmt::Write;

                let last_token = self.state.last_token.as_ref().unwrap();
                let mut buf = SmartString::new_const();

                if last_token.is_empty() {
                    write!(buf, "{token}").unwrap();
                } else if within_text && matches!(token, Token::StringConstant(..)) {
                    *compressed += &last_token[1..];
                } else {
                    buf = last_token.clone();
                }

                if !buf.is_empty() && !compressed.is_empty() {
                    let cur = buf.chars().next().unwrap();
                    let prev = compressed.chars().last().unwrap();

                    // A separating space is needed when concatenating this
                    // token onto the output would change how the boundary
                    // re-tokenizes:
                    //   * two identifier characters merge into one
                    //     identifier/keyword (e.g. `let` + `x` -> `letx`);
                    //   * the two boundary characters merge into a
                    //     recognized operator or a reserved symbol (e.g.
                    //     `<` + `-` -> `<-`, which the lexer rejects, or
                    //     `<` + `=` -> `<=`).
                    // The second case is decided by the tokenizer's own
                    // symbol tables, so nothing has to be hardcoded here.
                    let is_id =
                        |c: char| c == '_' || is_id_first_alphabetic(c) || is_id_continue(c);

                    let need_space = if is_id(prev) && is_id(cur) {
                        true
                    } else {
                        let mut pair = SmartString::new_const();
                        pair.push(prev);
                        pair.push(cur);
                        Token::lookup_symbol_from_syntax(&pair).is_some()
                            || is_reserved_keyword_or_symbol(&pair)
                            // Rust reads `<-` and `--` as two tokens, but
                            // a reader takes them for one.
                            || matches!((prev, cur), ('<' | '-', '-'))
                    };

                    if need_space {
                        *compressed += " ";
                    }
                }

                *compressed += &buf;
            }
        }

        Some((token, pos))
    }
}

impl FusedIterator for TokenIterator<'_> {}

impl Engine {
    /// _(internals)_ Tokenize an input text stream.
    /// Exported under the `internals` feature only.
    #[expose_under_internals]
    #[inline(always)]
    #[must_use]
    fn lex<'a>(
        &'a self,
        inputs: impl IntoIterator<Item = &'a (impl AsRef<str> + 'a)>,
    ) -> (TokenIterator<'a>, TokenizerControl) {
        self.lex_raw(inputs, self.token_mapper.as_deref())
    }
    /// _(internals)_ Tokenize an input text stream with a mapping function.
    /// Exported under the `internals` feature only.
    #[expose_under_internals]
    #[inline(always)]
    #[must_use]
    fn lex_with_map<'a>(
        &'a self,
        inputs: impl IntoIterator<Item = &'a (impl AsRef<str> + 'a)>,
        token_mapper: &'a OnParseTokenCallback,
    ) -> (TokenIterator<'a>, TokenizerControl) {
        self.lex_raw(inputs, Some(token_mapper))
    }
    /// Tokenize an input text stream with an optional mapping function.
    #[inline]
    #[must_use]
    pub(crate) fn lex_raw<'a>(
        &'a self,
        inputs: impl IntoIterator<Item = &'a (impl AsRef<str> + 'a)>,
        token_mapper: Option<&'a OnParseTokenCallback>,
    ) -> (TokenIterator<'a>, TokenizerControl) {
        let inputs: StaticVec<&'a str> = inputs.into_iter().map(AsRef::as_ref).collect();
        let mut streams: StaticVec<_> = inputs.iter().map(|s| s.chars().peekable()).collect();
        let mut pos = Position::new(1, 0);
        let mut pending = None;
        // rustc reads a file's Rust source after its byte order mark,
        // shebang line and frontmatter block. The tokenizer steps over them
        // so positions stay the file's.
        if let (Some(text), Some(stream)) = (inputs.first(), streams.first_mut()) {
            let skip = match crate::project::frontmatter::read(text) {
                Ok(lead) => lead.source,
                Err(malformed) if !malformed.opens() => malformed.after_shebang(),
                Err(malformed) => {
                    pending = Some((
                        Token::LexError(
                            LERR::ImproperSymbol("---".into(), malformed.message.clone()).into(),
                        ),
                        position_at(text, malformed.span.start),
                    ));
                    0
                }
            };
            for ch in text[..skip].chars() {
                stream.next();
                match ch {
                    '\n' => pos.new_line(),
                    '\u{feff}' => {}
                    _ => pos.advance(),
                }
            }
        }
        self.tokens(streams, pos, pending, token_mapper)
    }

    /// Tokenize `text`, a piece of a longer generated text that starts at
    /// `at` there: each token's position is the longer text's, as if the
    /// whole were tokenized (#1877). Nothing is stepped over: a piece is
    /// never a file's start.
    #[cfg(feature = "rust-import")]
    pub(crate) fn lex_at<'a>(
        &'a self,
        text: &'a str,
        at: Position,
    ) -> (TokenIterator<'a>, TokenizerControl) {
        let mut streams = StaticVec::new();
        streams.push(text.chars().peekable());
        self.tokens(streams, at, None, self.token_mapper.as_deref())
    }

    fn tokens<'a>(
        &'a self,
        streams: StaticVec<Peekable<Chars<'a>>>,
        pos: Position,
        pending: Option<(Token, Position)>,
        token_mapper: Option<&'a OnParseTokenCallback>,
    ) -> (TokenIterator<'a>, TokenizerControl) {
        let buffer: TokenizerControl = RefCell::new(TokenizerControlBlock::new()).into();
        let buffer2 = buffer.clone();
        (
            TokenIterator {
                state: TokenizeState {
                    max_string_len: std::num::NonZeroUsize::new(self.max_string_size()),
                    next_token_cannot_be_unary: false,
                    tokenizer_control: buffer,
                    comment_level: 0,
                    include_comments: false,
                    is_within_text_terminated_by: None,
                    last_token: None,
                },
                pos,
                stream: MultiInputsStream {
                    buf: [None, None],
                    streams,
                    index: 0,
                },
                token_mapper,
                pending,
            },
            buffer2,
        )
    }
}

/// The position of the character at byte `offset` of `text`, as the
/// tokenizer counts it: a byte order mark takes no column.
fn position_at(text: &str, offset: usize) -> Position {
    let mut pos = Position::new(1, 0);
    for ch in text[..offset].chars() {
        match ch {
            '\n' => pos.new_line(),
            '\u{feff}' => {}
            _ => pos.advance(),
        }
    }
    pos.advance();
    pos
}

#[cfg(test)]
mod tests {
    use super::Token;
    use crate::Engine;

    fn tokens(source: &str) -> Vec<Token> {
        let engine = Engine::new();
        let inputs = [source];
        let (stream, _) = engine.lex_raw(&inputs, None);
        stream
            .map(|(token, _)| token)
            .take_while(|token| *token != Token::EOF)
            .collect()
    }

    #[test]
    fn tokenizer_has_no_pascal_comments() {
        use Token::{LeftParen, Multiply, Plus, RightParen};
        let id = |name: &str| Token::Identifier(Box::new(name.into()));
        assert_eq!(
            tokens("f(*s)"),
            [id("f"), LeftParen, Multiply, id("s"), RightParen]
        );
        assert_eq!(
            tokens("(*a + *b)"),
            [
                LeftParen,
                Multiply,
                id("a"),
                Plus,
                Multiply,
                id("b"),
                RightParen
            ]
        );
        assert_eq!(
            tokens("(**r)"),
            [LeftParen, Multiply, Multiply, id("r"), RightParen]
        );
        assert_eq!(
            tokens("a--b"),
            [id("a"), Token::Minus, Token::UnaryMinus, id("b")]
        );
        assert_eq!(
            tokens("a<-b"),
            [id("a"), Token::LessThan, Token::UnaryMinus, id("b")]
        );
    }

    fn positioned(source: &str) -> Vec<(Token, (usize, usize))> {
        let engine = Engine::new();
        let inputs = [source];
        let (stream, _) = engine.lex_raw(&inputs, None);
        stream
            .take_while(|(token, _)| *token != Token::EOF)
            .map(|(token, pos)| {
                (
                    token,
                    (pos.line().unwrap(), pos.position().unwrap_or_default()),
                )
            })
            .collect()
    }

    #[test]
    fn the_source_starts_after_a_shebang_and_frontmatter() {
        let id = |name: &str| Token::Identifier(Box::new(name.into()));
        let attribute = Token::Reserved(Box::new("#!".into()));
        assert_eq!(positioned("#!/usr/bin/env rawr\nx"), [(id("x"), (2, 1))]);
        assert_eq!(
            positioned("---cargo\n[dependencies]\nrand = \"0.9\"\n---\n// rawr\n  x"),
            [(id("x"), (6, 3))]
        );
        assert_eq!(
            positioned("\u{feff}#!/x\n\n---\n---\nx"),
            [(id("x"), (5, 1))]
        );
        assert_eq!(positioned("\u{feff}x"), [(id("x"), (1, 1))]);
        // An inner attribute is not a shebang, with or without a space.
        assert_eq!(positioned("#![a]")[0], (attribute.clone(), (1, 1)));
        assert_eq!(positioned("#! [a]")[0], (attribute, (1, 1)));
        // One or two dashes are Rust's.
        assert_eq!(positioned("-x")[1], (id("x"), (1, 2)));
    }

    #[test]
    fn malformed_frontmatter_is_cargo_s_error() {
        let tokens = positioned("---cargo\n[dependencies]\n");
        let (Token::LexError(error), position) = &tokens[0] else {
            panic!("{tokens:?}");
        };
        assert_eq!(error.to_string(), "unclosed frontmatter; expected `---`");
        assert_eq!(*position, (3, 1));
        let tokens = positioned("---toml\n---\n");
        let (Token::LexError(error), position) = &tokens[0] else {
            panic!("{tokens:?}");
        };
        assert_eq!(
            error.to_string(),
            "unsupported frontmatter infostring `toml`; specify `cargo` for embedding a manifest"
        );
        assert_eq!(*position, (1, 4));
    }
}
