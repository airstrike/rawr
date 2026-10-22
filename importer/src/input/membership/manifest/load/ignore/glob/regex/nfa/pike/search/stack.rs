//! Prospective epsilon continuation ownership, including exact batch extension.
use super::super::storage::Array;
use super::{cache::Frame, Admission, Id, Session, Step};

impl Array<Frame> {
    pub(super) fn extend_reverse<A: Admission>(
        &mut self,
        alternatives: &[Id],
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        // The source extend iterator is exact-sized: reserve its incoming
        // extent before transferring its first reversed alternative.
        self.reserve(alternatives.len(), session)?;
        let mut alternatives = alternatives.iter().rev();
        loop {
            session.items(1)?;
            let Some(&id) = alternatives.next() else {
                break;
            };
            self.values.push(Frame::Explore(id));
        }
        Ok(())
    }
}
