// Copyright 2013-2016 The rust-url developers. MIT OR Apache-2.0.

//! Raw bulk slices preserve the pinned tab/newline and fragment boundary order.

use rawr_toml::document;

use super::{input::Input, output::Set, Parse, Parser};

impl<A: document::Admission> Parser<'_, '_, A> {
    pub(super) fn tail(
        &mut self,
        mut input: Input<'_>,
        special: bool,
    ) -> Parse<(Option<u32>, Option<u32>)> {
        let mut query = None;
        match input.next() {
            Some('#') => {}
            Some('?') => {
                query = Some(self.offset()?);
                self.output.push('?', self.session)?;
                let Some(remaining) = self.query(input, special)? else {
                    return Ok((query, None));
                };
                input = remaining;
            }
            None => return Ok((None, None)),
            _ => return self.state(),
        }
        let fragment = self.offset()?;
        self.output.push('#', self.session)?;
        self.fragment(input)?;
        Ok((query, Some(fragment)))
    }

    fn query<'i>(&mut self, mut input: Input<'i>, special: bool) -> Parse<Option<Input<'i>>> {
        let set = if special {
            Set::SpecialQuery
        } else {
            Set::Query
        };
        loop {
            let start = input.remaining();
            let mut finished = false;
            let mut part = None;
            while let Some(c) = input.next_raw() {
                if matches!(c, '\t' | '\n' | '\r' | '#') {
                    part = Some(&start[..start.len() - input.remaining().len() - 1]);
                    finished = c == '#';
                    break;
                }
            }
            let text = match part {
                Some(part) => part,
                None if start.is_empty() => return Ok(None),
                None => start,
            };
            self.output.encode(text, set, self.session)?;
            if finished {
                return Ok(Some(input));
            }
        }
    }

    fn fragment(&mut self, mut input: Input<'_>) -> Parse<()> {
        loop {
            let start = input.remaining();
            let mut part = None;
            while let Some(c) = input.next_raw() {
                if matches!(c, '\t' | '\n' | '\r') {
                    part = Some(&start[..start.len() - input.remaining().len() - 1]);
                    break;
                }
            }
            let text = match part {
                Some(part) => part,
                None if start.is_empty() => return Ok(()),
                None => start,
            };
            // NUL is encoded, despite the upstream violation's historical name.
            self.output.encode(text, Set::Fragment, self.session)?;
        }
    }
}
