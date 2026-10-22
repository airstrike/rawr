use std::io::{self, Write};
use std::ops::Range;

use toml::de::{DeTable, DeValue};
use toml::Spanned;
use toml_parser::parser::{RecursionGuard, ValidateWhitespace};

use crate::{fixtures, json};

pub fn case(output: &mut impl Write, case: &fixtures::Case) -> io::Result<bool> {
    let (document, errors) = DeTable::parse_recoverable(&case.source);
    let accepted = errors.is_empty();
    output.write_all(b"{\"schema\":\"rawr-toml-reference-v1\",\"id\":")?;
    json::string(output, case.id)?;
    output.write_all(b",\"local_test\":")?;
    json::string(output, case.local_test)?;
    output.write_all(b",\"purpose\":")?;
    json::string(output, case.purpose)?;
    output.write_all(b",\"source\":")?;
    json::string(output, &case.source)?;
    write!(
        output,
        ",\"expected_acceptance\":{},\"accepted\":{}",
        case.accepted, accepted
    )?;
    output.write_all(b",\"document\":[")?;
    Document { output, next: 0 }.root(&document)?;
    output.write_all(b"],\"errors\":[")?;
    for (index, error) in errors.iter().enumerate() {
        if index != 0 {
            output.write_all(b",")?;
        }
        output.write_all(b"{\"message\":")?;
        json::string(output, error.message())?;
        output.write_all(b",\"span\":")?;
        json::span(output, error.span())?;
        output.write_all(b"}")?;
    }
    output.write_all(b"],\"raw\":")?;
    raw(output, &case.source)?;
    output.write_all(b"}\n")?;
    Ok(accepted == case.accepted)
}

struct Position<'a> {
    parent: Option<usize>,
    index: usize,
    key: Option<&'a str>,
    key_span: Option<Range<usize>>,
}

struct Document<'a, W> {
    output: &'a mut W,
    next: usize,
}

impl<W: Write> Document<'_, W> {
    fn root(&mut self, table: &Spanned<DeTable<'_>>) -> io::Result<()> {
        let position = Position {
            parent: None,
            index: 0,
            key: None,
            key_span: None,
        };
        let id = self.begin(position, table.span(), "table")?;
        self.output.write_all(b"}")?;
        self.entries(id, table.get_ref())
    }

    fn begin(
        &mut self,
        position: Position<'_>,
        span: Range<usize>,
        kind: &str,
    ) -> io::Result<usize> {
        let id = self.next;
        self.next += 1;
        if id != 0 {
            self.output.write_all(b",")?;
        }
        write!(self.output, "{{\"id\":{id},\"parent\":")?;
        match position.parent {
            Some(parent) => write!(self.output, "{parent}")?,
            None => self.output.write_all(b"null")?,
        }
        write!(self.output, ",\"index\":{},\"key\":", position.index)?;
        json::optional_string(self.output, position.key)?;
        self.output.write_all(b",\"key_span\":")?;
        json::span(self.output, position.key_span)?;
        self.output.write_all(b",\"kind\":")?;
        json::string(self.output, kind)?;
        self.output.write_all(b",\"span\":")?;
        json::span(self.output, Some(span))?;
        Ok(id)
    }

    fn entries(&mut self, parent: usize, table: &DeTable<'_>) -> io::Result<()> {
        for (index, (key, value)) in table.iter().enumerate() {
            self.value(
                value,
                Position {
                    parent: Some(parent),
                    index,
                    key: Some(key.get_ref().as_ref()),
                    key_span: Some(key.span()),
                },
            )?;
        }
        Ok(())
    }

    fn value(&mut self, value: &Spanned<DeValue<'_>>, position: Position<'_>) -> io::Result<()> {
        let kind = match value.get_ref() {
            DeValue::String(_) => "string",
            DeValue::Integer(_) => "integer",
            DeValue::Float(_) => "float",
            DeValue::Boolean(_) => "boolean",
            DeValue::Datetime(_) => "datetime",
            DeValue::Array(_) => "array",
            DeValue::Table(_) => "table",
        };
        let id = self.begin(position, value.span(), kind)?;
        match value.get_ref() {
            DeValue::String(text) => {
                self.output.write_all(b",\"text\":")?;
                json::string(self.output, text)?;
            }
            DeValue::Integer(integer) => {
                self.output.write_all(b",\"text\":")?;
                json::string(self.output, integer.as_str())?;
                write!(self.output, ",\"radix\":{}", integer.radix())?;
            }
            DeValue::Float(float) => {
                self.output.write_all(b",\"text\":")?;
                json::string(self.output, float.as_str())?;
            }
            DeValue::Boolean(value) => write!(self.output, ",\"value\":{value}")?,
            DeValue::Datetime(value) => {
                self.output.write_all(b",\"text\":")?;
                json::string(self.output, &value.to_string())?;
            }
            DeValue::Array(_) | DeValue::Table(_) => {}
        }
        self.output.write_all(b"}")?;
        match value.get_ref() {
            DeValue::Array(values) => {
                for (index, value) in values.iter().enumerate() {
                    self.value(
                        value,
                        Position {
                            parent: Some(id),
                            index,
                            key: None,
                            key_span: None,
                        },
                    )?;
                }
            }
            DeValue::Table(table) => self.entries(id, table)?,
            _ => {}
        }
        Ok(())
    }
}

fn raw(output: &mut impl Write, text: &str) -> io::Result<()> {
    let source = toml_parser::Source::new(text);
    let tokens = source.lex().into_vec();
    let mut events = Vec::with_capacity(tokens.len());
    let mut errors = Vec::new();
    // Exact wrapper order and limit from pinned toml's de/parser/mod.rs.
    let mut whitespace = ValidateWhitespace::new(&mut events, source);
    let mut receiver = RecursionGuard::new(&mut whitespace, 80);
    toml_parser::parser::parse_document(&tokens, &mut receiver, &mut errors);

    output.write_all(b"{\"events\":[")?;
    for (index, event) in events.iter().enumerate() {
        if index != 0 {
            output.write_all(b",")?;
        }
        output.write_all(b"{\"kind\":")?;
        json::string(output, &format!("{:?}", event.kind()))?;
        output.write_all(b",\"encoding\":")?;
        match event.encoding() {
            Some(encoding) => json::string(output, &format!("{encoding:?}"))?,
            None => output.write_all(b"null")?,
        }
        output.write_all(b",\"span\":")?;
        json::span(output, range(Some(event.span())))?;
        output.write_all(b"}")?;
    }
    output.write_all(b"],\"errors\":[")?;
    for (index, error) in errors.iter().enumerate() {
        if index != 0 {
            output.write_all(b",")?;
        }
        output.write_all(b"{\"description\":")?;
        json::string(output, error.description())?;
        output.write_all(b",\"context\":")?;
        json::span(output, range(error.context()))?;
        output.write_all(b",\"unexpected\":")?;
        json::span(output, range(error.unexpected()))?;
        output.write_all(b",\"expected\":")?;
        match error.expected() {
            None => output.write_all(b"null")?,
            Some(expected) => {
                output.write_all(b"[")?;
                for (index, expected) in expected.iter().enumerate() {
                    if index != 0 {
                        output.write_all(b",")?;
                    }
                    let (kind, value) = match expected {
                        toml_parser::Expected::Literal(value) => ("literal", *value),
                        toml_parser::Expected::Description(value) => ("description", *value),
                        _ => return Err(io::Error::other("unexpected pinned Expected variant")),
                    };
                    output.write_all(b"{\"kind\":")?;
                    json::string(output, kind)?;
                    output.write_all(b",\"value\":")?;
                    json::string(output, value)?;
                    output.write_all(b"}")?;
                }
                output.write_all(b"]")?;
            }
        }
        output.write_all(b"}")?;
    }
    output.write_all(b"]}")
}

fn range(span: Option<toml_parser::Span>) -> Option<Range<usize>> {
    span.map(|span| span.start()..span.end())
}
