// Adapted from ICU4X 2.2.0. Unicode-3.0; see LICENSE.
//! Pinned composition continuation, including backward-combining starters.

use rawr_toml::document;

use crate::input::membership::manifest::{session, Step};

use super::{data, Normalizer, Pending};

impl<I: Iterator<Item = char>> Normalizer<I> {
    pub(super) fn composition_next<A: document::Admission>(
        &mut self,
        session: &mut session::Session<'_, A>,
    ) -> Step<Option<char>> {
        let mut undecomposed_starter = Pending {
            character: '\0',
            value: 0,
        };
        if self.unprocessed_starter.is_none() {
            // One forward jump preserves ICU's distinct pending and buffered
            // starter paths without recursion or an owned continuation stack.
            'starter: {
                if let Some((character, ccc)) = self.buffered(self.position) {
                    self.position += 1;
                    if self.position == self.buffer.len() {
                        self.clear();
                    }
                    if ccc == 0 {
                        self.unprocessed_starter = Some(character);
                        break 'starter;
                    }
                    return Ok(Some(character));
                }
                let Some(pending) = self.pending.take() else {
                    return Ok(None);
                };
                undecomposed_starter = pending;
                if u32::from(undecomposed_starter.character) < data::COMPOSITION_BOUND
                    || (undecomposed_starter.value & data::NON_ROUND_TRIP) == 0
                {
                    if let Some(upcoming) = self.delegate_next_no_pending() {
                        let cannot_combine_backwards = u32::from(upcoming.character)
                            < data::COMPOSITION_BOUND
                            || (upcoming.value & data::BACKWARD_COMBINING) == 0;
                        self.pending = Some(upcoming);
                        if cannot_combine_backwards {
                            return Ok(Some(undecomposed_starter.character));
                        }
                    } else {
                        return Ok(Some(undecomposed_starter.character));
                    }
                }
            }
        }
        let mut starter = '\0';
        let mut attempt_composition = false;
        loop {
            if let Some(unprocessed) = self.unprocessed_starter.take() {
                starter = unprocessed;
            } else {
                let next_starter = self.decomposing_next(undecomposed_starter, session)?;
                if !attempt_composition {
                    starter = next_starter;
                } else if let Some(composed) = data::compose(starter, next_starter) {
                    starter = composed;
                } else {
                    self.unprocessed_starter = Some(next_starter);
                    return Ok(Some(starter));
                }
            }
            loop {
                let (character, ccc) = if let Some(value) = self.buffered(self.position) {
                    value
                } else {
                    self.clear();
                    break;
                };
                if let Some(composed) = data::compose(starter, character) {
                    starter = composed;
                    self.position += 1;
                    continue;
                }
                let mut most_recent_skipped_ccc = ccc;
                self.drain_prefix();
                if most_recent_skipped_ccc == 0 {
                    return Ok(Some(starter));
                }
                let mut index = 1;
                while let Some((character, ccc)) = self.buffered(index) {
                    if ccc == 0 {
                        return Ok(Some(starter));
                    }
                    if ccc != most_recent_skipped_ccc
                        && let Some(composed) = data::compose_non_hangul(starter, character)
                    {
                        self.remove(index);
                        starter = composed;
                        continue;
                    }
                    most_recent_skipped_ccc = ccc;
                    index += 1;
                }
                break;
            }
            if self.buffer.len() != 0 {
                return Ok(Some(starter));
            }
            if let Some(pending) = self.pending.take() {
                if u32::from(pending.character) < data::COMPOSITION_BOUND
                    || (pending.value & data::BACKWARD_COMBINING) == 0
                {
                    self.pending = Some(pending);
                    return Ok(Some(starter));
                }
                undecomposed_starter = pending;
                attempt_composition = true;
                continue;
            }
            return Ok(Some(starter));
        }
    }
}
