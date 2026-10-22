// Adapted from ICU4X 2.2.0. Unicode-3.0; see LICENSE.
//! Exact borrowed tables and fixed-format trie operations; no provider loading.

use icu_collections::char16trie::TrieResult;
use icu_normalizer::provider;

pub(super) const IGNORABLE: u32 = 0xffffffff;
pub(super) const NON_ROUND_TRIP: u32 = 1 << 30;
pub(super) const BACKWARD_COMBINING: u32 = 1 << 31;
pub(super) const HIGH_ZEROS: u32 = 0x3fff0000;
pub(super) const LOW_ZEROS: u32 = 0xffe0;
pub(super) const HANGUL_S_BASE: u32 = 0xac00;
pub(super) const HANGUL_L_BASE: u32 = 0x1100;
pub(super) const HANGUL_V_BASE: u32 = 0x1161;
pub(super) const HANGUL_T_BASE: u32 = 0x11a7;
pub(super) const HANGUL_T_COUNT: u32 = 28;
pub(super) const HANGUL_N_COUNT: u32 = 588;
pub(super) const HANGUL_S_COUNT: u32 = 11172;

pub(super) const DECOMPOSITIONS: &provider::DecompositionData<'static> =
    provider::Baked::SINGLETON_NORMALIZER_UTS46_DATA_V1;
pub(super) const TABLES: &provider::DecompositionTables<'static> =
    provider::Baked::SINGLETON_NORMALIZER_NFD_TABLES_V1;
pub(super) const SUPPLEMENT: &provider::DecompositionTables<'static> =
    provider::Baked::SINGLETON_NORMALIZER_NFKD_TABLES_V1;

const _: () = assert!(
    TABLES.scalars16.const_len()
        + TABLES.scalars24.const_len()
        + SUPPLEMENT.scalars16.const_len()
        + SUPPLEMENT.scalars24.const_len()
        <= 0xfff
);
const _: () = assert!(DECOMPOSITIONS.passthrough_cap <= 0x300);

pub(super) const DECOMPOSITION_BOUND: u32 = if DECOMPOSITIONS.passthrough_cap < 0xc0 {
    DECOMPOSITIONS.passthrough_cap as u32
} else {
    0xc0
};
pub(super) const COMPOSITION_BOUND: u32 = if DECOMPOSITIONS.passthrough_cap < 0x300 {
    DECOMPOSITIONS.passthrough_cap as u32
} else {
    0x300
};

pub(super) const FDFA: [u16; 17] = [
    0x644, 0x649, 0x20, 0x627, 0x644, 0x644, 0x647, 0x20, 0x639, 0x644, 0x64a, 0x647, 0x20, 0x648,
    0x633, 0x644, 0x645,
];

pub(super) fn value(c: char) -> u32 {
    DECOMPOSITIONS.trie.get(c)
}

pub(super) fn has_ccc(value: u32) -> bool {
    (value & 0x3ffffe00) == 0xd800
}

pub(super) fn special_non_starter(value: u32) -> bool {
    (value & 0x3fffff00) == 0xd900
}

pub(super) fn ccc(value: u32) -> u8 {
    if has_ccc(value) {
        value as u8
    } else {
        0
    }
}

/// The pinned provider owns valid scalar data; replacement retains GIGO fallback.
pub(super) fn character(value: u32) -> char {
    char::from_u32(value).unwrap_or(char::REPLACEMENT_CHARACTER)
}

pub(super) fn compose(starter: char, second: char) -> Option<char> {
    let v = u32::from(second).wrapping_sub(HANGUL_V_BASE);
    if v >= 0x1200 - HANGUL_V_BASE {
        return compose_non_hangul(starter, second);
    }
    if v < 21 {
        let l = u32::from(starter).wrapping_sub(HANGUL_L_BASE);
        if l < 19 {
            let lv = l * HANGUL_N_COUNT + v * HANGUL_T_COUNT;
            return Some(character(HANGUL_S_BASE + lv));
        }
        return None;
    }
    if u32::from(second).wrapping_sub(0x11a8) <= 0x11c2 - 0x11a8 {
        let lv = u32::from(starter).wrapping_sub(HANGUL_S_BASE);
        if lv < HANGUL_S_COUNT && lv % HANGUL_T_COUNT == 0 {
            let lvt = lv + (u32::from(second) - HANGUL_T_BASE);
            return Some(character(HANGUL_S_BASE + lvt));
        }
    }
    None
}

pub(super) fn compose_non_hangul(starter: char, second: char) -> Option<char> {
    let mut iter = provider::Baked::SINGLETON_NORMALIZER_NFC_V1
        .canonical_compositions
        .iter();
    match iter.next(second) {
        TrieResult::NoValue => match iter.next(starter) {
            TrieResult::FinalValue(value) => char::from_u32(value as u32),
            TrieResult::NoMatch | TrieResult::NoValue | TrieResult::Intermediate(_) => None,
        },
        TrieResult::NoMatch | TrieResult::FinalValue(_) | TrieResult::Intermediate(_) => None,
    }
}
