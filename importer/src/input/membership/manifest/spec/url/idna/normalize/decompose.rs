// Adapted from ICU4X 2.2.0. Unicode-3.0; see LICENSE.
//! Complete UTS46 decomposition, lookahead, combining expansion and stable order.

use rawr_toml::document;

use crate::input::membership::manifest::{session, storage, Step};

use super::{data, Character, Mode, Normalizer, Pending};

impl<I: Iterator<Item = char>> Normalizer<I> {
    pub(super) fn delegate_next_no_pending(&mut self) -> Option<Pending> {
        loop {
            let c = self.delegate.next()?;
            if u32::from(c) < data::DECOMPOSITION_BOUND {
                return Some(Pending {
                    character: c,
                    value: 0,
                });
            }
            let value = data::value(c);
            if value == data::IGNORABLE {
                match self.mode {
                    Mode::Map => continue,
                    Mode::Validate => {
                        return Some(Pending {
                            character: c,
                            value: u32::from(char::REPLACEMENT_CHARACTER) | data::NON_ROUND_TRIP,
                        })
                    }
                }
            }
            return Some(Pending {
                character: c,
                value,
            });
        }
    }

    fn delegate_next(&mut self) -> Option<Pending> {
        if let Some(pending) = self.pending.take() {
            Some(pending)
        } else {
            self.delegate_next_no_pending()
        }
    }

    fn push_decomposition<A: document::Admission>(
        &mut self,
        mut decomposition: impl Iterator<Item = char>,
        only_non_starters: bool,
        session: &mut session::Session<'_, A>,
    ) -> Step<(char, usize)> {
        let starter = decomposition.next().unwrap_or(char::REPLACEMENT_CHARACTER);
        let mut combining_start = 0;
        for (index, c) in decomposition.enumerate() {
            if only_non_starters {
                self.push(Character::placeholder(c), session)?;
            } else {
                let value = data::value(c);
                self.push(
                    Character::from_pending(Pending {
                        character: c,
                        value,
                    }),
                    session,
                )?;
                if !data::has_ccc(value) {
                    combining_start = index + 1;
                }
            }
        }
        Ok((starter, combining_start))
    }

    pub(super) fn decomposing_next<A: document::Admission>(
        &mut self,
        pending: Pending,
        session: &mut session::Session<'_, A>,
    ) -> Step<char> {
        let c = pending.character;
        let decomposition = pending.value;
        let (starter, combining_start) = if (decomposition
            & !(data::BACKWARD_COMBINING | data::NON_ROUND_TRIP))
            == 0
        {
            (c, 0)
        } else {
            let high_zeros = (decomposition & data::HIGH_ZEROS) == 0;
            let low_zeros = (decomposition & data::LOW_ZEROS) == 0;
            if !high_zeros && !low_zeros {
                let starter = data::character(decomposition & 0x7fff);
                let combining = data::character((decomposition >> 15) & 0x7fff);
                self.push(Character::placeholder(combining), session)?;
                (starter, 0)
            } else if high_zeros {
                let hangul_offset = u32::from(c).wrapping_sub(data::HANGUL_S_BASE);
                if hangul_offset < data::HANGUL_S_COUNT {
                    let l = hangul_offset / data::HANGUL_N_COUNT;
                    let v = (hangul_offset % data::HANGUL_N_COUNT) / data::HANGUL_T_COUNT;
                    let t = hangul_offset % data::HANGUL_T_COUNT;
                    self.push(
                        Character::starter(data::character(data::HANGUL_V_BASE + v)),
                        session,
                    )?;
                    let first = data::character(data::HANGUL_L_BASE + l);
                    if t != 0 {
                        self.push(
                            Character::starter(data::character(data::HANGUL_T_BASE + t)),
                            session,
                        )?;
                        (first, 2)
                    } else {
                        (first, 1)
                    }
                } else {
                    let singleton = decomposition as u16;
                    if singleton != 1 {
                        (data::character(u32::from(singleton)), 0)
                    } else {
                        for u in data::FDFA {
                            self.push(Character::starter(data::character(u32::from(u))), session)?;
                        }
                        ('\u{635}', 17)
                    }
                }
            } else {
                let offset = (((decomposition & !(0b11 << 30)) >> 16) as usize) - 1;
                let len_bits = decomposition & 0b1111;
                let only_non_starters = (decomposition & 0b10000) != 0;
                let bmp = data::TABLES.scalars16.len();
                let supplementary = data::TABLES.scalars24.len();
                let compatibility_bmp = data::SUPPLEMENT.scalars16.len();
                if offset < bmp {
                    self.push_decomposition(
                        data::TABLES
                            .scalars16
                            .get_subslice(offset..offset + (len_bits + 2) as usize)
                            .into_iter()
                            .flat_map(|slice| slice.iter())
                            .map(|u| data::character(u32::from(u))),
                        only_non_starters,
                        session,
                    )?
                } else if offset < bmp + supplementary {
                    let offset = offset - bmp;
                    self.push_decomposition(
                        data::TABLES
                            .scalars24
                            .get_subslice(offset..offset + (len_bits + 1) as usize)
                            .into_iter()
                            .flat_map(|slice| slice.iter()),
                        only_non_starters,
                        session,
                    )?
                } else if offset < bmp + supplementary + compatibility_bmp {
                    let offset = offset - (bmp + supplementary);
                    self.push_decomposition(
                        data::SUPPLEMENT
                            .scalars16
                            .get_subslice(offset..offset + (len_bits + 2) as usize)
                            .into_iter()
                            .flat_map(|slice| slice.iter())
                            .map(|u| data::character(u32::from(u))),
                        only_non_starters,
                        session,
                    )?
                } else {
                    let offset = offset - (bmp + supplementary + compatibility_bmp);
                    self.push_decomposition(
                        data::SUPPLEMENT
                            .scalars24
                            .get_subslice(offset..offset + (len_bits + 1) as usize)
                            .into_iter()
                            .flat_map(|slice| slice.iter()),
                        only_non_starters,
                        session,
                    )?
                }
            }
        };
        self.gather_and_sort(combining_start, session)?;
        Ok(starter)
    }

    fn gather_and_sort<A: document::Admission>(
        &mut self,
        combining_start: usize,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        while let Some(pending) = self.delegate_next() {
            if !data::has_ccc(pending.value) {
                self.pending = Some(pending);
                break;
            }
            if !data::special_non_starter(pending.value) {
                self.push(Character::from_pending(pending), session)?;
                continue;
            }
            let mapped = match pending.character {
                '\u{340}' => Character::new('\u{300}', 230),
                '\u{341}' => Character::new('\u{301}', 230),
                '\u{343}' => Character::new('\u{313}', 230),
                '\u{344}' => {
                    self.push(Character::new('\u{308}', 230), session)?;
                    Character::new('\u{301}', 230)
                }
                '\u{f73}' => {
                    self.push(Character::new('\u{f71}', 129), session)?;
                    Character::new('\u{f72}', 130)
                }
                '\u{f75}' => {
                    self.push(Character::new('\u{f71}', 129), session)?;
                    Character::new('\u{f74}', 132)
                }
                '\u{f81}' => {
                    self.push(Character::new('\u{f71}', 129), session)?;
                    Character::new('\u{f80}', 130)
                }
                '\u{ff9e}' => Character::new('\u{3099}', 8),
                '\u{ff9f}' => Character::new('\u{309a}', 8),
                _ => Character::placeholder(char::REPLACEMENT_CHARACTER),
            };
            self.push(mapped, session)?;
        }
        let slice = &mut self.buffer.as_mut_slice()[combining_start..];
        if slice.len() >= 2 {
            // Ordinals encode the precise pre-sort order, including equal-CCC
            // expansion tails. They are part of each admitted scratch record.
            for (ordinal, character) in slice.iter_mut().enumerate() {
                if character.ccc == 0xff {
                    character.ccc = data::ccc(data::value(character.character));
                }
                character.ordinal = ordinal;
            }
            storage::sort(slice, |left, right| {
                left.ccc
                    .cmp(&right.ccc)
                    .then(left.ordinal.cmp(&right.ordinal))
            });
        }
        Ok(())
    }

    pub(super) fn decomposition_next<A: document::Admission>(
        &mut self,
        session: &mut session::Session<'_, A>,
    ) -> Step<Option<char>> {
        if let Some((c, _)) = self.buffered(self.position) {
            self.position += 1;
            if self.position == self.buffer.len() {
                self.clear();
            }
            return Ok(Some(c));
        }
        let Some(pending) = self.pending.take() else {
            return Ok(None);
        };
        self.decomposing_next(pending, session).map(Some)
    }
}
