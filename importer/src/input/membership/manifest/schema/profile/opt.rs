//! Cargo preserves every i64 as normalized decimal text, plus exactly s and z.

use rawr_toml::document;

use super::super::super::{session, value, Diagnostic, Step, Tag};

const EXPECTED: &str = "an optimization level";

/// Converted optimization text. The longest signed i64 occupies twenty bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super::super) struct Opt {
    bytes: [u8; 20],
    start: u8,
}

impl Opt {
    pub(in super::super::super) fn interpret<A: document::Admission>(
        input: value::Input<'_, '_>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        let view = input.view(session)?;
        if let value::View::Scalar(value::Kind::String(text)) = view {
            let byte = match text.as_str() {
                "s" => b's',
                "z" => b'z',
                _ => {
                    return session.reject(Diagnostic {
                        tag: Tag::Variant,
                        span: input.span,
                        expected: Some(EXPECTED),
                    });
                }
            };
            // Input::view already admitted this exact one-byte string length.
            let mut bytes = [0; 20];
            bytes[19] = byte;
            return Ok(Self { bytes, start: 19 });
        }
        if let value::View::Scalar(kind) = view
            && let Some(value) = super::signed(kind)
        {
            return Self::decimal(value, session);
        }
        session.reject(Diagnostic {
            tag: Tag::Type,
            span: input.span,
            expected: Some(EXPECTED),
        })
    }

    pub(in super::super::super) fn as_str(&self) -> &str {
        // The private constructors write only decimal ASCII or s/z.
        std::str::from_utf8(&self.bytes[usize::from(self.start)..])
            .expect("optimization text is ASCII")
    }

    fn decimal<A: document::Admission>(
        value: i64,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        let mut magnitude = value.unsigned_abs();
        let mut count = magnitude;
        let mut digits = 1;
        while count >= 10 {
            count /= 10;
            digits += 1;
        }
        session.string(digits + usize::from(value < 0))?;

        let mut bytes = [0; 20];
        let mut start = bytes.len();
        loop {
            start -= 1;
            bytes[start] = b'0' + (magnitude % 10) as u8;
            magnitude /= 10;
            if magnitude == 0 {
                break;
            }
        }
        if value < 0 {
            start -= 1;
            bytes[start] = b'-';
        }
        Ok(Self {
            bytes,
            start: start as u8,
        })
    }
}
