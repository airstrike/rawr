//! Cargo's finite BTreeMap<String, BTreeMap<String, String>> visitor. Conversion
//! follows input order; retained maps expose string order after completion.

use rawr_toml::document;

use super::super::super::{session, storage, value, Diagnostic, Span, Step, Tag};

pub(in super::super::super) struct Badges<'d> {
    entries: storage::Buffer<Badge<'d>>,
}
pub(in super::super::super) struct Badge<'d> {
    key: value::Key<'d>,
    values: storage::Buffer<Pair<'d>>,
    order: usize,
}

pub(in super::super::super) struct Pair<'d> {
    key: value::Key<'d>,
    text: value::Text<'d>,
    order: usize,
}

impl<'d> Badges<'d> {
    pub(super) fn interpret<A: document::Admission>(
        mut cursor: value::Cursor<'_, 'd>,
        span: Option<Span>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        let mut entries = storage::Buffer::new();
        while let Some((key, input)) = cursor.next(session)? {
            let Some(key) = key else {
                return session.fail(super::super::super::Error::State);
            };
            let value::View::Map(mut inner) = input.view(session)? else {
                return session.reject(Diagnostic::at(Tag::Type, input.span.or(span)));
            };
            let mut values = storage::Buffer::new();
            while let Some((key, input)) = inner.next(session)? {
                let Some(key) = key else {
                    return session.fail(super::super::super::Error::State);
                };
                let value::View::Scalar(value::Kind::String(text)) = input.view(session)? else {
                    return session.reject(Diagnostic::at(Tag::Type, input.span.or(span)));
                };
                values.push(
                    Pair {
                        key,
                        text,
                        order: values.len(),
                    },
                    session,
                )?;
            }
            storage::sort(values.as_mut_slice(), |left, right| {
                left.key
                    .text
                    .as_str()
                    .cmp(right.key.text.as_str())
                    .then(left.order.cmp(&right.order))
            });
            let mut retained = 0;
            for index in 0..values.len() {
                if retained != 0
                    && values.as_slice()[retained - 1].key.text.as_str()
                        == values.as_slice()[index].key.text.as_str()
                {
                    // Sorted encounter order preserves BTreeMap's first key
                    // while replacing its value with the last occurrence.
                    values.as_mut_slice()[retained - 1].text = values.as_slice()[index].text;
                } else {
                    values.as_mut_slice().swap(retained, index);
                    retained += 1;
                }
            }
            values.truncate(retained);
            entries.push(
                Badge {
                    key,
                    values,
                    order: entries.len(),
                },
                session,
            )?;
        }
        storage::sort(entries.as_mut_slice(), |left, right| {
            left.key
                .text
                .as_str()
                .cmp(right.key.text.as_str())
                .then(left.order.cmp(&right.order))
        });
        let mut retained = 0;
        for index in 0..entries.len() {
            if retained != 0
                && entries.as_slice()[retained - 1].key.text.as_str()
                    == entries.as_slice()[index].key.text.as_str()
            {
                let entries = entries.as_mut_slice();
                let (left, right) = entries.split_at_mut(index);
                std::mem::swap(&mut left[retained - 1].values, &mut right[0].values);
            } else {
                entries.as_mut_slice().swap(retained, index);
                retained += 1;
            }
        }
        entries.truncate(retained);
        Ok(Self { entries })
    }

    pub(in super::super::super) fn entries(&self) -> &[Badge<'d>] {
        self.entries.as_slice()
    }
}

impl<'d> Badge<'d> {
    pub(in super::super::super) fn key(&self) -> &value::Key<'d> {
        &self.key
    }
    pub(in super::super::super) fn values(&self) -> &[Pair<'d>] {
        self.values.as_slice()
    }
}

impl<'d> Pair<'d> {
    pub(in super::super::super) fn key(&self) -> &value::Key<'d> {
        &self.key
    }
    pub(in super::super::super) fn text(&self) -> &value::Text<'d> {
        &self.text
    }
}
