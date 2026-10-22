// Pinned toml_parser 1.1.2+spec-1.1.0; mapping and fixed cfgs: toml/provenance.json.
use core::ops::RangeInclusive;

use winnow::stream::ContainsToken as _;
use winnow::stream::Offset as _;
use winnow::stream::Stream as _;

use crate::decoder::StringBuilder;
use crate::lexer::APOSTROPHE;
use crate::lexer::ML_BASIC_STRING_DELIM;
use crate::lexer::ML_LITERAL_STRING_DELIM;
use crate::lexer::QUOTATION_MARK;
use crate::lexer::WSCHAR;
use crate::ErrorSink;
use crate::Expected;
use crate::ParseError;
use crate::Raw;
use crate::Span;

const ALLOCATION_ERROR: &str = "could not allocate for string";

/// Parse literal string
///
/// ```abnf
/// ;; Literal String
///
/// literal-string = apostrophe *literal-char apostrophe
///
/// apostrophe = %x27 ; ' apostrophe
///
/// literal-char = %x09 / %x20-26 / %x28-7E / non-ascii
/// ```
pub(crate) fn decode_literal_string<'i>(
    raw: Raw<'i>,
    output: &mut dyn StringBuilder<'i>,
    error: &mut dyn ErrorSink,
) {
    const INVALID_STRING: &str = "invalid literal string";

    output.clear();

    let s = raw.as_str();
    let s = if let Some(stripped) = s.strip_prefix(APOSTROPHE as char) {
        stripped
    } else {
        error.report_error(
            ParseError::new(INVALID_STRING)
                .with_context(Span::new_unchecked(0, raw.len()))
                .with_expected(&[Expected::Literal("'")])
                .with_unexpected(Span::new_unchecked(0, 0)),
        );
        s
    };
    let s = if let Some(stripped) = s.strip_suffix(APOSTROPHE as char) {
        stripped
    } else {
        error.report_error(
            ParseError::new(INVALID_STRING)
                .with_context(Span::new_unchecked(0, raw.len()))
                .with_expected(&[Expected::Literal("'")])
                .with_unexpected(Span::new_unchecked(raw.len(), raw.len())),
        );
        s
    };

    for (i, b) in s.as_bytes().iter().enumerate() {
        if !LITERAL_CHAR.contains_token(b) {
            let offset = (&s.as_bytes()[i..]).offset_from(&raw.as_bytes());
            error.report_error(
                ParseError::new(INVALID_STRING)
                    .with_context(Span::new_unchecked(0, raw.len()))
                    .with_expected(&[Expected::Description("non-single-quote visible characters")])
                    .with_unexpected(Span::new_unchecked(offset, offset)),
            );
        }
    }

    if !output.push_str(s) {
        error.report_error(
            ParseError::new(ALLOCATION_ERROR).with_unexpected(Span::new_unchecked(0, raw.len())),
        );
    }
}

/// ```abnf
/// literal-char = %x09 / %x20-26 / %x28-7E / non-ascii
/// ```
const LITERAL_CHAR: (
    u8,
    RangeInclusive<u8>,
    RangeInclusive<u8>,
    RangeInclusive<u8>,
) = (0x9, 0x20..=0x26, 0x28..=0x7E, NON_ASCII);

/// ```abnf
/// non-ascii = %x80-D7FF / %xE000-10FFFF
/// ```
/// - ASCII is 0xxxxxxx
/// - First byte for UTF-8 is 11xxxxxx
/// - Subsequent UTF-8 bytes are 10xxxxxx
const NON_ASCII: RangeInclusive<u8> = 0x80..=0xff;

/// Parse multi-line literal string
///
/// ```abnf
/// ;; Multiline Literal String
///
/// ml-literal-string = ml-literal-string-delim [ newline ] ml-literal-body
///                     ml-literal-string-delim
/// ml-literal-string-delim = 3apostrophe
/// ml-literal-body = *mll-content *( mll-quotes 1*mll-content ) [ mll-quotes ]
///
/// mll-content = literal-char / newline
/// mll-quotes = 1*2apostrophe
/// ```
pub(crate) fn decode_ml_literal_string<'i>(
    raw: Raw<'i>,
    output: &mut dyn StringBuilder<'i>,
    error: &mut dyn ErrorSink,
) {
    const INVALID_STRING: &str = "invalid multi-line literal string";
    output.clear();

    let s = raw.as_str();
    let s = if let Some(stripped) = s.strip_prefix(ML_LITERAL_STRING_DELIM) {
        stripped
    } else {
        error.report_error(
            ParseError::new(INVALID_STRING)
                .with_context(Span::new_unchecked(0, raw.len()))
                .with_expected(&[Expected::Literal("'")])
                .with_unexpected(Span::new_unchecked(0, 0)),
        );
        s
    };
    let s = strip_start_newline(s);
    let s = if let Some(stripped) = s.strip_suffix(ML_LITERAL_STRING_DELIM) {
        stripped
    } else {
        error.report_error(
            ParseError::new(INVALID_STRING)
                .with_context(Span::new_unchecked(0, raw.len()))
                .with_expected(&[Expected::Literal("'")])
                .with_unexpected(Span::new_unchecked(raw.len(), raw.len())),
        );
        s.trim_end_matches('\'')
    };

    for (i, b) in s.as_bytes().iter().enumerate() {
        if *b == b'\'' || *b == b'\n' {
        } else if *b == b'\r' {
            if s.as_bytes().get(i + 1) != Some(&b'\n') {
                let offset = (&s.as_bytes()[i + 1..]).offset_from(&raw.as_bytes());
                error.report_error(
                    ParseError::new("carriage return must be followed by newline")
                        .with_context(Span::new_unchecked(0, raw.len()))
                        .with_expected(&[Expected::Literal("\n")])
                        .with_unexpected(Span::new_unchecked(offset, offset)),
                );
            }
        } else if !LITERAL_CHAR.contains_token(b) {
            let offset = (&s.as_bytes()[i..]).offset_from(&raw.as_bytes());
            error.report_error(
                ParseError::new(INVALID_STRING)
                    .with_context(Span::new_unchecked(0, raw.len()))
                    .with_expected(&[Expected::Description("non-single-quote characters")])
                    .with_unexpected(Span::new_unchecked(offset, offset)),
            );
        }
    }

    if !output.push_str(s) {
        error.report_error(
            ParseError::new(ALLOCATION_ERROR).with_unexpected(Span::new_unchecked(0, raw.len())),
        );
    }
}

/// Parse basic string
///
/// ```abnf
/// ;; Basic String
///
/// basic-string = quotation-mark *basic-char quotation-mark
///
/// basic-char = basic-unescaped / escaped
///
/// escaped = escape escape-seq-char
/// ```
pub(crate) fn decode_basic_string<'i>(
    raw: Raw<'i>,
    output: &mut dyn StringBuilder<'i>,
    error: &mut dyn ErrorSink,
) {
    const INVALID_STRING: &str = "invalid basic string";
    output.clear();

    let s = raw.as_str();
    let s = if let Some(stripped) = s.strip_prefix(QUOTATION_MARK as char) {
        stripped
    } else {
        error.report_error(
            ParseError::new(INVALID_STRING)
                .with_context(Span::new_unchecked(0, raw.len()))
                .with_expected(&[Expected::Literal("\"")])
                .with_unexpected(Span::new_unchecked(0, 0)),
        );
        s
    };
    let mut s = if let Some(stripped) = s.strip_suffix(QUOTATION_MARK as char) {
        stripped
    } else {
        error.report_error(
            ParseError::new(INVALID_STRING)
                .with_context(Span::new_unchecked(0, raw.len()))
                .with_expected(&[Expected::Literal("\"")])
                .with_unexpected(Span::new_unchecked(raw.len(), raw.len())),
        );
        s
    };

    let segment = basic_unescaped(&mut s);
    if !output.push_str(segment) {
        error.report_error(
            ParseError::new(ALLOCATION_ERROR).with_unexpected(Span::new_unchecked(0, raw.len())),
        );
    }
    while !s.is_empty() {
        if s.starts_with("\\") {
            let _ = s.next_token();

            let c = escape_seq_char(&mut s, raw, error);
            if !output.push_char(c) {
                error.report_error(
                    ParseError::new(ALLOCATION_ERROR)
                        .with_unexpected(Span::new_unchecked(0, raw.len())),
                );
            }
        } else {
            let invalid = basic_invalid(&mut s);
            let start = invalid.offset_from(&raw.as_str());
            let end = start + invalid.len();
            error.report_error(
                ParseError::new(INVALID_STRING)
                    .with_context(Span::new_unchecked(0, raw.len()))
                    .with_expected(&[
                        Expected::Description("non-double-quote visible characters"),
                        Expected::Literal("\\"),
                    ])
                    .with_unexpected(Span::new_unchecked(start, end)),
            );
            let _ = output.push_str(invalid);
        }

        let segment = basic_unescaped(&mut s);
        if !output.push_str(segment) {
            let start = segment.offset_from(&raw.as_str());
            let end = start + segment.len();
            error.report_error(
                ParseError::new(ALLOCATION_ERROR).with_unexpected(Span::new_unchecked(start, end)),
            );
        }
    }
}

/// ```abnf
/// basic-unescaped = wschar / %x21 / %x23-5B / %x5D-7E / non-ascii
/// ```
fn basic_unescaped<'i>(stream: &mut &'i str) -> &'i str {
    let offset = stream
        .as_bytes()
        .offset_for(|b| !BASIC_UNESCAPED.contains_token(b))
        .unwrap_or(stream.len());
    #[cfg(any())] // SAFETY: BASIC_UNESCAPED ensure `offset` is along UTF-8 boundary
    unsafe {
        stream.next_slice_unchecked(offset)
    }
    #[cfg(not(any()))]
    stream.next_slice(offset)
}

fn basic_invalid<'i>(stream: &mut &'i str) -> &'i str {
    let offset = stream
        .as_bytes()
        .offset_for(|b| (BASIC_UNESCAPED, ESCAPE).contains_token(b))
        .unwrap_or(stream.len());
    #[cfg(any())] // SAFETY: BASIC_UNESCAPED ensure `offset` is along UTF-8 boundary
    unsafe {
        stream.next_slice_unchecked(offset)
    }
    #[cfg(not(any()))]
    stream.next_slice(offset)
}

/// ```abnf
/// basic-unescaped = wschar / %x21 / %x23-5B / %x5D-7E / non-ascii
/// ```
#[allow(clippy::type_complexity)]
const BASIC_UNESCAPED: (
    (u8, u8),
    u8,
    RangeInclusive<u8>,
    RangeInclusive<u8>,
    RangeInclusive<u8>,
) = (WSCHAR, 0x21, 0x23..=0x5B, 0x5D..=0x7E, NON_ASCII);

/// ```abnf
/// escape = %x5C                    ; \
/// ```
const ESCAPE: u8 = b'\\';

/// ```abnf
/// escape-seq-char =  %x22         ; "    quotation mark  U+0022
/// escape-seq-char =/ %x5C         ; \    reverse solidus U+005C
/// escape-seq-char =/ %x62         ; b    backspace       U+0008
/// escape-seq-char =/ %x65         ; e    escape          U+001B
/// escape-seq-char =/ %x66         ; f    form feed       U+000C
/// escape-seq-char =/ %x6E         ; n    line feed       U+000A
/// escape-seq-char =/ %x72         ; r    carriage return U+000D
/// escape-seq-char =/ %x74         ; t    tab             U+0009
/// escape-seq-char =/ %x78 2HEXDIG ; xHH                  U+00HH
/// escape-seq-char =/ %x75 4HEXDIG ; uHHHH                U+HHHH
/// escape-seq-char =/ %x55 8HEXDIG ; UHHHHHHHH            U+HHHHHHHH
/// ```
fn escape_seq_char(stream: &mut &str, raw: Raw<'_>, error: &mut dyn ErrorSink) -> char {
    const EXPECTED_ESCAPES: &[Expected] = &[
        Expected::Literal("b"),
        Expected::Literal("e"),
        Expected::Literal("f"),
        Expected::Literal("n"),
        Expected::Literal("r"),
        Expected::Literal("\\"),
        Expected::Literal("\""),
        Expected::Literal("x"),
        Expected::Literal("u"),
        Expected::Literal("U"),
    ];

    let start = stream.checkpoint();
    let Some(id) = stream.next_token() else {
        let offset = stream.offset_from(&raw.as_str());
        error.report_error(
            ParseError::new("missing escaped value")
                .with_context(Span::new_unchecked(0, raw.len()))
                .with_expected(EXPECTED_ESCAPES)
                .with_unexpected(Span::new_unchecked(offset, offset)),
        );
        return '\\';
    };
    match id {
        'b' => '\u{8}',
        'e' => '\u{1b}',
        'f' => '\u{c}',
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        'x' => hexescape(stream, 2, raw, error),
        'u' => hexescape(stream, 4, raw, error),
        'U' => hexescape(stream, 8, raw, error),
        '\\' => '\\',
        '"' => '"',
        _ => {
            stream.reset(&start);
            let offset = stream.offset_from(&raw.as_str());
            error.report_error(
                ParseError::new("missing escaped value")
                    .with_context(Span::new_unchecked(0, raw.len()))
                    .with_expected(EXPECTED_ESCAPES)
                    .with_unexpected(Span::new_unchecked(offset, offset)),
            );
            '\\'
        }
    }
}

fn hexescape(
    stream: &mut &str,
    num_digits: usize,
    raw: Raw<'_>,
    error: &mut dyn ErrorSink,
) -> char {
    let offset = stream
        .as_bytes()
        .offset_for(|b| !HEXDIG.contains_token(b))
        .unwrap_or_else(|| stream.eof_offset())
        .min(num_digits);
    #[cfg(any())] // SAFETY: HEXDIG ensure `offset` is along UTF-8 boundary
    let value = unsafe { stream.next_slice_unchecked(offset) };
    #[cfg(not(any()))]
    let value = stream.next_slice(offset);

    if value.len() != num_digits {
        let offset = stream.offset_from(&raw.as_str());
        error.report_error(
            ParseError::new("too few unicode value digits")
                .with_context(Span::new_unchecked(0, raw.len()))
                .with_expected(&[Expected::Description("unicode hexadecimal value")])
                .with_unexpected(Span::new_unchecked(offset, offset)),
        );
        return '�';
    }

    let Some(value) = u32::from_str_radix(value, 16).ok().and_then(char::from_u32) else {
        let offset = value.offset_from(&raw.as_str());
        error.report_error(
            ParseError::new("invalid value")
                .with_context(Span::new_unchecked(0, raw.len()))
                .with_expected(&[Expected::Description("unicode hexadecimal value")])
                .with_unexpected(Span::new_unchecked(offset, offset)),
        );
        return '�';
    };

    value
}

/// ```abnf
/// HEXDIG = DIGIT / "A" / "B" / "C" / "D" / "E" / "F"
/// ```
const HEXDIG: (RangeInclusive<u8>, RangeInclusive<u8>, RangeInclusive<u8>) =
    (DIGIT, b'A'..=b'F', b'a'..=b'f');

/// ```abnf
/// DIGIT = %x30-39 ; 0-9
/// ```
const DIGIT: RangeInclusive<u8> = b'0'..=b'9';

fn strip_start_newline(s: &str) -> &str {
    s.strip_prefix('\n')
        .or_else(|| s.strip_prefix("\r\n"))
        .unwrap_or(s)
}

/// Parse multi-line basic string
///
/// ```abnf
/// ;; Multiline Basic String
///
/// ml-basic-string = ml-basic-string-delim [ newline ] ml-basic-body
///                   ml-basic-string-delim
/// ml-basic-string-delim = 3quotation-mark
/// ml-basic-body = *mlb-content *( mlb-quotes 1*mlb-content ) [ mlb-quotes ]
///
/// mlb-content = basic-char / newline / mlb-escaped-nl
/// mlb-quotes = 1*2quotation-mark
/// ```
pub(crate) fn decode_ml_basic_string<'i>(
    raw: Raw<'i>,
    output: &mut dyn StringBuilder<'i>,
    error: &mut dyn ErrorSink,
) {
    const INVALID_STRING: &str = "invalid multi-line basic string";

    let s = raw.as_str();
    let s = if let Some(stripped) = s.strip_prefix(ML_BASIC_STRING_DELIM) {
        stripped
    } else {
        error.report_error(
            ParseError::new(INVALID_STRING)
                .with_context(Span::new_unchecked(0, raw.len()))
                .with_expected(&[Expected::Literal("\"")])
                .with_unexpected(Span::new_unchecked(0, 0)),
        );
        s
    };
    let s = strip_start_newline(s);
    let mut s = if let Some(stripped) = s.strip_suffix(ML_BASIC_STRING_DELIM) {
        stripped
    } else {
        error.report_error(
            ParseError::new(INVALID_STRING)
                .with_context(Span::new_unchecked(0, raw.len()))
                .with_expected(&[Expected::Literal("\"")])
                .with_unexpected(Span::new_unchecked(raw.len(), raw.len())),
        );
        s
    };

    let segment = mlb_unescaped(&mut s);
    if !output.push_str(segment) {
        error.report_error(
            ParseError::new(ALLOCATION_ERROR).with_unexpected(Span::new_unchecked(0, raw.len())),
        );
    }
    while !s.is_empty() {
        if s.starts_with("\\") {
            let _ = s.next_token();

            if s.as_bytes()
                .first()
                .map(|b| (WSCHAR, b'\r', b'\n').contains_token(b))
                .unwrap_or(false)
            {
                mlb_escaped_nl(&mut s, raw, error);
            } else {
                let c = escape_seq_char(&mut s, raw, error);
                if !output.push_char(c) {
                    error.report_error(
                        ParseError::new(ALLOCATION_ERROR)
                            .with_unexpected(Span::new_unchecked(0, raw.len())),
                    );
                }
            }
        } else if s.starts_with("\r") {
            let offset = if s.starts_with("\r\n") {
                "\r\n".len()
            } else {
                let start = s.offset_from(&raw.as_str()) + 1;
                error.report_error(
                    ParseError::new("carriage return must be followed by newline")
                        .with_context(Span::new_unchecked(0, raw.len()))
                        .with_expected(&[Expected::Literal("\n")])
                        .with_unexpected(Span::new_unchecked(start, start)),
                );
                "\r".len()
            };
            #[cfg(any())]
            // SAFETY: Newlines ensure `offset` is along UTF-8 boundary
            let newline = unsafe { s.next_slice_unchecked(offset) };
            #[cfg(not(any()))]
            let newline = s.next_slice(offset);
            if !output.push_str(newline) {
                let start = newline.offset_from(&raw.as_str());
                let end = start + newline.len();
                error.report_error(
                    ParseError::new(ALLOCATION_ERROR)
                        .with_unexpected(Span::new_unchecked(start, end)),
                );
            }
        } else {
            let invalid = mlb_invalid(&mut s);
            let start = invalid.offset_from(&raw.as_str());
            let end = start + invalid.len();
            error.report_error(
                ParseError::new(INVALID_STRING)
                    .with_context(Span::new_unchecked(0, raw.len()))
                    .with_expected(&[Expected::Literal("\\"), Expected::Description("characters")])
                    .with_unexpected(Span::new_unchecked(start, end)),
            );
            let _ = output.push_str(invalid);
        }

        let segment = mlb_unescaped(&mut s);
        if !output.push_str(segment) {
            let start = segment.offset_from(&raw.as_str());
            let end = start + segment.len();
            error.report_error(
                ParseError::new(ALLOCATION_ERROR).with_unexpected(Span::new_unchecked(start, end)),
            );
        }
    }
}

/// ```abnf
/// mlb-escaped-nl = escape ws newline *( wschar / newline )
/// ```
fn mlb_escaped_nl(stream: &mut &str, raw: Raw<'_>, error: &mut dyn ErrorSink) {
    const INVALID_STRING: &str = "invalid multi-line basic string";
    let ws_offset = stream
        .as_bytes()
        .offset_for(|b| !WSCHAR.contains_token(b))
        .unwrap_or(stream.len());
    #[cfg(any())] // SAFETY: WSCHAR ensure `offset` is along UTF-8 boundary
    unsafe {
        stream.next_slice_unchecked(ws_offset);
    }
    #[cfg(not(any()))]
    stream.next_slice(ws_offset);

    let start = stream.checkpoint();
    match stream.next_token() {
        Some('\n') => {}
        Some('\r') => {
            if stream.as_bytes().first() == Some(&b'\n') {
                let _ = stream.next_token();
            } else {
                let start = stream.offset_from(&raw.as_str());
                let end = start;
                error.report_error(
                    ParseError::new("carriage return must be followed by newline")
                        .with_context(Span::new_unchecked(0, raw.len()))
                        .with_expected(&[Expected::Literal("\n")])
                        .with_unexpected(Span::new_unchecked(start, end)),
                );
            }
        }
        _ => {
            stream.reset(&start);

            let start = stream.offset_from(&raw.as_str());
            let end = start;
            error.report_error(
                ParseError::new(INVALID_STRING)
                    .with_context(Span::new_unchecked(0, raw.len()))
                    .with_expected(&[Expected::Literal("\n")])
                    .with_unexpected(Span::new_unchecked(start, end)),
            );
        }
    }

    loop {
        let start_offset = stream.offset_from(&raw.as_str());

        let offset = stream
            .as_bytes()
            .offset_for(|b| !(WSCHAR, b'\n').contains_token(b))
            .unwrap_or(stream.len());
        #[cfg(any())] // SAFETY: WSCHAR ensure `offset` is along UTF-8 boundary
        unsafe {
            stream.next_slice_unchecked(offset);
        }
        #[cfg(not(any()))]
        stream.next_slice(offset);

        if stream.starts_with("\r") {
            let offset = if stream.starts_with("\r\n") {
                "\r\n".len()
            } else {
                let start = stream.offset_from(&raw.as_str()) + 1;
                error.report_error(
                    ParseError::new("carriage return must be followed by newline")
                        .with_context(Span::new_unchecked(0, raw.len()))
                        .with_expected(&[Expected::Literal("\n")])
                        .with_unexpected(Span::new_unchecked(start, start)),
                );
                "\r".len()
            };
            #[cfg(any())]
            // SAFETY: Newlines ensure `offset` is along UTF-8 boundary
            let _ = unsafe { stream.next_slice_unchecked(offset) };
            #[cfg(not(any()))]
            let _ = stream.next_slice(offset);
        }

        let end_offset = stream.offset_from(&raw.as_str());
        if start_offset == end_offset {
            break;
        }
    }
}

/// `mlb-unescaped` extended with `mlb-quotes` and `LF`
///
/// This is a specialization of [`basic_unescaped`] to help with multi-line basic strings
///
/// **warning:** `newline` is not validated
///
/// ```abnf
/// ml-basic-body = *mlb-content *( mlb-quotes 1*mlb-content ) [ mlb-quotes ]
///
/// mlb-content = basic-cha / newline / mlb-escaped-nl
/// mlb-quotes = 1*2quotation-mark
/// ```
fn mlb_unescaped<'i>(stream: &mut &'i str) -> &'i str {
    let offset = stream
        .as_bytes()
        .offset_for(|b| !(BASIC_UNESCAPED, b'"', b'\n').contains_token(b))
        .unwrap_or(stream.len());
    #[cfg(any())] // SAFETY: BASIC_UNESCAPED ensure `offset` is along UTF-8 boundary
    unsafe {
        stream.next_slice_unchecked(offset)
    }
    #[cfg(not(any()))]
    stream.next_slice(offset)
}

fn mlb_invalid<'i>(stream: &mut &'i str) -> &'i str {
    let offset = stream
        .as_bytes()
        .offset_for(|b| (BASIC_UNESCAPED, b'"', b'\n', ESCAPE, '\r').contains_token(b))
        .unwrap_or(stream.len());
    #[cfg(any())] // SAFETY: BASIC_UNESCAPED ensure `offset` is along UTF-8 boundary
    unsafe {
        stream.next_slice_unchecked(offset)
    }
    #[cfg(not(any()))]
    stream.next_slice(offset)
}

/// Parse unquoted key
///
/// ```abnf
/// unquoted-key = 1*( ALPHA / DIGIT / %x2D / %x5F ) ; A-Z / a-z / 0-9 / - / _
/// ```
pub(crate) fn decode_unquoted_key<'i>(
    raw: Raw<'i>,
    output: &mut dyn StringBuilder<'i>,
    error: &mut dyn ErrorSink,
) {
    let s = raw.as_str();

    if s.is_empty() {
        error.report_error(
            ParseError::new("unquoted keys cannot be empty")
                .with_context(Span::new_unchecked(0, s.len()))
                .with_expected(&[
                    Expected::Description("letters"),
                    Expected::Description("numbers"),
                    Expected::Literal("-"),
                    Expected::Literal("_"),
                ])
                .with_unexpected(Span::new_unchecked(0, s.len())),
        );
    }

    let mut span = None;
    for (i, _b) in s
        .as_bytes()
        .iter()
        .enumerate()
        .filter(|(_, b)| !UNQUOTED_CHAR.contains_token(*b))
    {
        if let Some((start, end)) = span {
            if i == end {
                span = Some((start, i + 1));
            } else {
                error.report_error(
                    ParseError::new("invalid unquoted key")
                        .with_context(Span::new_unchecked(0, s.len()))
                        .with_expected(&[
                            Expected::Description("letters"),
                            Expected::Description("numbers"),
                            Expected::Literal("-"),
                            Expected::Literal("_"),
                        ])
                        .with_unexpected(Span::new_unchecked(start, end)),
                );
                span = Some((i, i + 1));
            }
        } else {
            span = Some((i, i + 1));
        }
    }
    if let Some((start, end)) = span {
        error.report_error(
            ParseError::new("invalid unquoted key")
                .with_context(Span::new_unchecked(0, s.len()))
                .with_expected(&[
                    Expected::Description("letters"),
                    Expected::Description("numbers"),
                    Expected::Literal("-"),
                    Expected::Literal("_"),
                ])
                .with_unexpected(Span::new_unchecked(start, end)),
        );
    }

    if !output.push_str(s) {
        error.report_error(
            ParseError::new(ALLOCATION_ERROR).with_unexpected(Span::new_unchecked(0, raw.len())),
        );
    }
}

/// ```abnf
/// unquoted-key = 1*( ALPHA / DIGIT / %x2D / %x5F ) ; A-Z / a-z / 0-9 / - / _
/// ```
const UNQUOTED_CHAR: (
    RangeInclusive<u8>,
    RangeInclusive<u8>,
    RangeInclusive<u8>,
    u8,
    u8,
) = (b'A'..=b'Z', b'a'..=b'z', b'0'..=b'9', b'-', b'_');
