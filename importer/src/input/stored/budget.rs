//! Prospective ownership through the original importer accounts.

use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

use super::{Error, Result};
use crate::input::Allocation;
use crate::{cargo, node};

pub(crate) struct Budget<'account> {
    pub(in crate::input) limits: cargo::Limits,
    allocation: &'account mut Allocation,
    graph: &'account node::GraphBudget,
    /// Address range of the enclosing record whose wire this budget already
    /// charged, so a nested section decoded from it is not charged again.
    framed: Option<(usize, usize)>,
    #[cfg(test)]
    allocations: usize,
}

fn input_refusal(mut error: &crate::Error) -> bool {
    loop {
        match error {
            crate::Error::Limit { .. }
            | crate::Error::Allocation { .. }
            | crate::Error::InputAccountRefused => return true,
            crate::Error::Node { source, .. } => match source.as_ref() {
                node::Error::Limit { .. } | node::Error::Allocation => return true,
                node::Error::Importer(inner) => error = inner,
                _ => return false,
            },
            _ => return false,
        }
    }
}

fn bundle_refusal(error: &node::Error) -> bool {
    match error {
        node::Error::Limit { .. } | node::Error::Allocation => true,
        node::Error::Importer(error) => input_refusal(error),
        _ => false,
    }
}

impl<'account> Budget<'account> {
    pub(in crate::input) fn workspace(
        &mut self,
        authority: &[cargo::Input],
        metadata: &cargo::unit::Metadata,
    ) -> Result<crate::input::workspace::Image> {
        self.check()?;
        let result = {
            let mut account =
                crate::input::workspace::Account::borrow(self.allocation, self.graph, &self.limits);
            crate::input::workspace::roots(authority, metadata, &mut account)
                .and_then(|roots| crate::input::workspace::capture(roots, &mut account))
        };
        result.map_err(|error| self.failure(Error::Input(error)))
    }

    pub(crate) fn graph(&self) -> &node::GraphBudget {
        self.graph
    }

    pub(crate) fn borrow(
        limits: &cargo::Limits,
        allocation: &'account mut Allocation,
        graph: &'account node::GraphBudget,
    ) -> Self {
        let mut limits = *limits;
        limits.items = limits.items.min(allocation.item_limit);
        limits.input_proof_bytes = limits.input_proof_bytes.min(allocation.byte_limit);
        limits.string_bytes = limits.string_bytes.min(allocation.string_limit);
        Self {
            limits,
            allocation,
            graph,
            framed: None,
            #[cfg(test)]
            allocations: 0,
        }
    }

    pub(crate) fn check(&self) -> Result<()> {
        if self.allocation.refused {
            Err(Error::Refused)
        } else {
            Ok(())
        }
    }

    pub(crate) fn failure(&mut self, error: Error) -> Error {
        let refused = match &error {
            Error::Input(error) => input_refusal(error),
            Error::Bundle(error) => bundle_refusal(error),
            Error::Graph(_)
            | Error::Allocation(_)
            | Error::Overflow
            | Error::Refused
            | Error::Observation(super::observe::Error::Allocation) => true,
            _ => false,
        };
        if refused {
            self.allocation.refused = true;
        }
        error
    }

    /// An accounting target is a measured comparison, never a refusal.
    /// Crossing it records the axis on the original input account and the
    /// work continues; a latched account still does no further work.
    pub(in crate::input) fn limited(
        &mut self,
        resource: &'static str,
        limit: usize,
        actual: usize,
    ) -> Result<()> {
        self.check()?;
        self.allocation.exceeded(resource, limit, actual);
        Ok(())
    }

    /// A physical bound (recursion depth, a read that grew past its admitted
    /// buffer) still refuses and latches the account; it is not an
    /// accounting target and is never recorded as an exceedance.
    pub(in crate::input) fn physical(
        &mut self,
        resource: &'static str,
        limit: usize,
        actual: usize,
    ) -> Result<()> {
        self.check()?;
        if actual > limit {
            self.allocation.refused = true;
            return Err(crate::Error::Limit {
                resource,
                limit,
                actual,
            }
            .into());
        }
        Ok(())
    }

    pub(in crate::input) fn overflow(&mut self) -> Error {
        self.allocation.refused = true;
        Error::Overflow
    }

    pub(super) fn admit(&mut self, items: usize, bytes: usize) -> Result<()> {
        self.check()?;
        let admitted = self
            .allocation
            .items(items)
            .and_then(|()| self.allocation.bytes(bytes));
        if let Err(error) = admitted {
            self.allocation.refused = true;
            return Err(error.into());
        }
        Ok(())
    }

    #[track_caller]
    pub(super) fn wire(&mut self, bytes: usize) -> Result<()> {
        self.check()?;
        self.limited(
            "stored baseline bytes",
            self.limits.input_proof_bytes,
            bytes,
        )?;
        self.graph.decode(bytes).map_err(|error| {
            self.allocation.refused = true;
            error.into()
        })
    }

    /// Charge an enclosing record's wire once and remember its extent, so the
    /// sections decoded from it within this budget are framed by that charge.
    #[track_caller]
    pub(in crate::input) fn wire_record(&mut self, bytes: &[u8]) -> Result<()> {
        self.wire(bytes.len())?;
        let start = bytes.as_ptr() as usize;
        self.framed = Some((start, start.saturating_add(bytes.len())));
        Ok(())
    }

    /// Charge a section's wire unless it lies inside the record this budget
    /// already framed; a standalone or repeated decode charges in full.
    #[track_caller]
    pub(in crate::input) fn wire_section(&mut self, bytes: &[u8]) -> Result<()> {
        let start = bytes.as_ptr() as usize;
        let end = start.saturating_add(bytes.len());
        if self
            .framed
            .is_some_and(|(framed_start, framed_end)| start >= framed_start && end <= framed_end)
        {
            return self.check();
        }
        self.wire(bytes.len())
    }

    pub(crate) fn digest(&mut self, bytes: &[u8]) -> Result<cargo::Digest> {
        self.hash(bytes.len())?;
        Ok(cargo::digest(bytes))
    }

    pub(in crate::input) fn hash(&mut self, bytes: usize) -> Result<()> {
        self.check()?;
        self.graph.hash(bytes).map_err(|error| {
            self.allocation.refused = true;
            error.into()
        })
    }

    /// Actual filesystem attempts, including failed probes, retries and EOF.
    pub(super) fn paths(&mut self, count: usize) -> Result<()> {
        self.check()?;
        self.graph.paths(count).map_err(|error| {
            self.allocation.refused = true;
            error.into()
        })
    }

    /// New filesystem-observation ownership is owned once by the original
    /// allocation account. Codec framing has its own wire admission; the
    /// graph's decode axis measures decoding, not copies of admitted bytes.
    #[track_caller]
    pub(in crate::input) fn storage(&mut self, items: usize, bytes: usize) -> Result<()> {
        self.admit(items, bytes)
    }

    #[track_caller]
    pub(in crate::input) fn decode(&mut self, bytes: usize) -> Result<()> {
        self.check()?;
        self.graph.decode(bytes).map_err(|error| {
            self.allocation.refused = true;
            error.into()
        })
    }

    #[track_caller]
    pub(super) fn owned(&mut self, length: usize) -> Result<Vec<u8>> {
        self.storage(1, length)?;
        let mut output = Vec::new();
        output
            .try_reserve_exact(length)
            .map_err(|error| self.failure(Error::Allocation(error)))?;
        Ok(output)
    }

    #[track_caller]
    pub(super) fn owned_path(&mut self, bytes: &[u8]) -> Result<PathBuf> {
        let mut output = self.owned(bytes.len())?;
        output.extend_from_slice(bytes);
        Ok(PathBuf::from(std::ffi::OsString::from_vec(output)))
    }

    #[track_caller]
    pub(in crate::input) fn owned_text(&mut self, text: &str) -> Result<Box<str>> {
        let mut output = self.owned(text.len())?;
        output.extend_from_slice(text.as_bytes());
        if output.capacity() != output.len() {
            self.storage(0, output.len())?;
        }
        let output = String::from_utf8(output)
            .map_err(|_| Error::Encoding("owned text changed encoding"))?;
        Ok(output.into_boxed_str())
    }

    #[track_caller]
    pub(in crate::input) fn owned_box<T>(&mut self, values: Vec<T>) -> Result<Box<[T]>> {
        self.check()?;
        if values.capacity() != values.len() {
            let bytes = values
                .len()
                .checked_mul(std::mem::size_of::<T>())
                .ok_or_else(|| self.overflow())?;
            self.storage(0, bytes)?;
        }
        Ok(values.into_boxed_slice())
    }

    #[track_caller]
    pub(super) fn items<T>(&mut self, count: usize) -> Result<Vec<T>> {
        self.check()?;
        let bytes = count
            .checked_mul(std::mem::size_of::<T>())
            .ok_or_else(|| self.overflow())?;
        self.admit(count, bytes)?;
        let mut output = Vec::new();
        output.try_reserve_exact(count).map_err(|error| {
            self.allocation.refused = true;
            Error::Allocation(error)
        })?;
        #[cfg(test)]
        if count != 0 {
            self.allocations += 1;
        }
        Ok(output)
    }

    pub(super) fn buffer(&mut self, length: usize) -> Result<Vec<u8>> {
        self.admit(1, length)?;
        let mut output = Vec::new();
        output.try_reserve_exact(length).map_err(|error| {
            self.allocation.refused = true;
            Error::Allocation(error)
        })?;
        #[cfg(test)]
        if length != 0 {
            self.allocations += 1;
        }
        Ok(output)
    }

    pub(super) fn bytes(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        let mut output = self.buffer(bytes.len())?;
        output.extend_from_slice(bytes);
        Ok(output)
    }

    pub(super) fn boxed<T>(&mut self, value: Vec<T>) -> Result<Box<[T]>> {
        self.check()?;
        if value.capacity() != value.len() {
            let bytes = value
                .len()
                .checked_mul(std::mem::size_of::<T>())
                .ok_or_else(|| self.overflow())?;
            self.admit(0, bytes)?;
        }
        Ok(value.into_boxed_slice())
    }

    pub(super) fn text(&mut self, bytes: &[u8], scalar: bool) -> Result<Box<str>> {
        self.check()?;
        std::str::from_utf8(bytes).map_err(|_| Error::Encoding("text is not UTF-8"))?;
        if scalar {
            self.limited(
                "stored baseline string bytes",
                self.limits.string_bytes.min(self.allocation.string_limit),
                bytes.len(),
            )?;
        }
        let value = String::from_utf8(self.bytes(bytes)?)
            .map_err(|_| Error::Encoding("text encoding changed"))?;
        if value.capacity() != value.len() {
            self.admit(0, value.len())?;
        }
        Ok(value.into_boxed_str())
    }

    pub(super) fn path(&mut self, bytes: &[u8]) -> Result<PathBuf> {
        Ok(PathBuf::from(std::ffi::OsString::from_vec(
            self.bytes(bytes)?,
        )))
    }

    #[cfg(test)]
    pub(super) fn allocations(&self) -> usize {
        self.allocations
    }

    #[cfg(test)]
    pub(super) fn usage(&self) -> crate::input::Usage {
        self.allocation.usage
    }

    /// The input account's recorded exceedances.
    #[cfg(test)]
    pub(crate) fn exceedances(&self) -> crate::exceedance::Record {
        self.allocation.exceedances()
    }

    /// Latch the input account on a genuine failure (test-only cause injection).
    #[cfg(test)]
    pub(crate) fn refuse(&mut self) {
        self.allocation.refused = true;
    }
}
