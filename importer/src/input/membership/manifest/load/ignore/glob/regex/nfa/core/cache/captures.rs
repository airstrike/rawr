// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../../LICENSE-regex-syntax-MIT.
//! Captures::all owns its actual GroupInfo share and all absent offset slots.
use super::super::finalize;
use super::{Admission, Session, Step};
use std::{num::NonZeroUsize, sync::Arc};

pub(super) struct Captures {
    pub(super) group_info: Arc<finalize::captures::Groups>,
    pub(super) pid: Option<u32>,
    // NonMaxUsize uses the same NonZeroUsize representation as the other
    // contained capture slots. This lifecycle does not interpret offsets.
    pub(super) slots: Vec<Option<NonZeroUsize>>,
}

impl Captures {
    pub(super) fn all<A: Admission>(
        groups: &Arc<finalize::captures::Groups>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.items(1)?;
        let group_info = Arc::clone(groups);
        session.items(1)?;
        let length = group_info.slot_len();
        let slots = finalize::storage::repeat(None, length, session)?;
        Ok(Self {
            group_info,
            pid: None,
            slots,
        })
    }
}
