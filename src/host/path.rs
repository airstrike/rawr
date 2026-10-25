//! Canonical paths inside a static host facade.

use crate::tokenizer::is_valid_identifier;
use crate::{host, ImmutableString};
use std::fmt;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct Path(Box<[ImmutableString]>);

impl Path {
    #[cfg(feature = "rust-import")]
    pub(super) fn take(&mut self) -> Self {
        Self(std::mem::take(&mut self.0))
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn try_clone(&self) -> Result<Self, rawr_import::surface::Error> {
        let mut segments = Vec::new();
        segments
            .try_reserve_exact(self.0.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        segments.extend(self.0.iter().cloned());
        Ok(Self(segments.into_boxed_slice()))
    }

    pub(crate) fn parse(path: impl Into<ImmutableString>) -> Result<Self, host::Error> {
        let path = path.into();
        let count = path.split("::").count();
        let mut segments = Vec::new();
        segments
            .try_reserve_exact(count)
            .map_err(|_| host::Error::Allocation)?;
        segments.extend(path.split("::").map(Into::into));

        // A tuple field's reader ends in its index: `touch::Finger::0`.
        let tuple_field = |segment: &ImmutableString| {
            !segment.is_empty() && segment.bytes().all(|byte| byte.is_ascii_digit())
        };
        if path.is_empty()
            || segments
                .iter()
                .enumerate()
                .any(|(index, segment): (usize, &ImmutableString)| {
                    !is_valid_identifier(segment)
                        && !(index + 1 == segments.len() && index > 0 && tuple_field(segment))
                })
        {
            return Err(host::Error::InvalidFacadePath(path));
        }

        Ok(Self(segments.into_boxed_slice()))
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn parse_imported<A: host::ImportAllocation + ?Sized>(
        path: &str,
        allocation: &mut A,
    ) -> Result<Self, host::Error> {
        let count = path.split("::").count();
        let string_bytes = path
            .split("::")
            .try_fold(0_usize, |bytes, segment| bytes.checked_add(segment.len()));
        let bytes = count
            .checked_mul(std::mem::size_of::<ImmutableString>())
            .and_then(|bytes| bytes.checked_add(string_bytes?));
        allocation
            .retain_checked(Some(1), bytes)
            .map_err(|_| host::Error::Allocation)?;
        Self::parse(path)
    }

    pub(crate) fn qualify(root: &Self, relative: &Self) -> Self {
        let mut segments = Vec::with_capacity(root.0.len() + relative.0.len());
        segments.extend(root.0.iter().cloned());
        segments.extend(relative.0.iter().cloned());
        Self(segments.into_boxed_slice())
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn qualify_imported<A: host::ImportAllocation + ?Sized>(
        root: &Self,
        relative: Self,
        allocation: &mut A,
    ) -> Result<Self, host::Error> {
        let segment_count = root
            .0
            .len()
            .checked_add(relative.0.len())
            .ok_or(host::Error::Allocation)?;
        let bytes = segment_count
            .checked_mul(std::mem::size_of::<ImmutableString>())
            .and_then(|bytes| {
                root.0
                    .iter()
                    .try_fold(bytes, |bytes, segment| bytes.checked_add(segment.len()))
            })
            .ok_or(host::Error::Allocation)?;
        allocation
            .retain_checked(Some(1), Some(bytes))
            .map_err(|_| host::Error::Allocation)?;
        let mut segments = Vec::new();
        segments
            .try_reserve_exact(segment_count)
            .map_err(|_| host::Error::Allocation)?;
        segments.extend(root.0.iter().cloned());
        segments.extend(relative.0.into_vec());
        Ok(Self(segments.into_boxed_slice()))
    }

    /// This path with its leading `from` segments replaced by `to`, when it
    /// starts with them: a re-exported path's canonical spelling.
    pub(crate) fn rebase(&self, from: &Self, to: &Self) -> Option<Self> {
        let rest = self.0.strip_prefix(&*from.0)?;
        Some(Self(to.0.iter().chain(rest).cloned().collect()))
    }

    pub(crate) fn parent(&self) -> Option<Self> {
        (self.0.len() > 1).then(|| Self(self.0[..self.0.len() - 1].into()))
    }

    pub(crate) fn name(&self) -> &str {
        self.0
            .last()
            .expect("validated host paths contain one segment")
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn name_owned(&self) -> crate::ImmutableString {
        self.0
            .last()
            .expect("validated host paths contain one segment")
            .clone()
    }

    pub(crate) fn segments(&self) -> &[ImmutableString] {
        &self.0
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        std::mem::size_of::<Self>()
            .checked_add(
                self.0
                    .len()
                    .checked_mul(std::mem::size_of::<ImmutableString>())?,
            )?
            .checked_add(
                self.0
                    .iter()
                    .try_fold(0_usize, |bytes, segment| bytes.checked_add(segment.len()))?,
            )
    }

    pub(crate) fn display_len(&self) -> Option<usize> {
        self.0
            .iter()
            .enumerate()
            .try_fold(0_usize, |bytes, (index, segment)| {
                bytes
                    .checked_add(usize::from(index != 0).checked_mul(2)?)?
                    .checked_add(segment.len())
            })
    }
}

impl fmt::Display for Path {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut segments = self.0.iter();
        if let Some(first) = segments.next() {
            formatter.write_str(first)?;
        }
        for segment in segments {
            write!(formatter, "::{segment}")?;
        }
        Ok(())
    }
}
