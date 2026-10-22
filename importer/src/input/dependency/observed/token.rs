//! Reusable decoded-token storage admitted before each replacement allocation.

use crate::input::stored::{Budget, Error, Result};

pub(super) struct Token {
    value: String,
    admitted: usize,
}

impl Token {
    pub(super) const fn new() -> Self {
        Self {
            value: String::new(),
            admitted: 0,
        }
    }

    pub(super) fn as_str(&self) -> &str {
        &self.value
    }

    pub(super) fn clear(&mut self) {
        self.value.clear();
    }

    pub(super) fn push(&mut self, character: char, budget: &mut Budget<'_>) -> Result<()> {
        budget.check()?;
        let length = self
            .value
            .len()
            .checked_add(character.len_utf8())
            .ok_or_else(|| budget.overflow())?;
        budget.limited("dep-info token bytes", budget.limits.string_bytes, length)?;
        if length > self.admitted {
            // Growth doubles independently of the string target (which only
            // records), so a long token costs amortized-linear work and
            // logarithmically many replacements, each charged once to the
            // account, never below `length`.
            let capacity = self.admitted.saturating_mul(2).max(length);
            budget.storage(1, capacity)?;
            let mut replacement = String::new();
            replacement
                .try_reserve_exact(capacity)
                .map_err(|error| budget.failure(Error::Allocation(error)))?;
            replacement.push_str(&self.value);
            self.value = replacement;
            self.admitted = capacity;
        }
        self.value.push(character);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Token;
    use crate::cargo::Limits;
    use crate::input::stored::{Budget, Error};
    use crate::input::{Allocation, Usage};
    use crate::node;

    #[test]
    fn replacement_layouts_spend_the_original_account_and_clear_reuses_storage() {
        let prior = Usage {
            items: 7,
            bytes: 31,
        };
        for floor in 0..=1 {
            let mut limits = Limits::standard();
            limits.input_proof_bytes = prior.bytes + 14 - floor;
            let mut allocation = Allocation::for_proof(prior, &limits);
            let graph = node::GraphBudget::new();
            let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
            let mut token = Token::new();
            token.push('é', &mut budget).expect("two-byte first extent");
            token
                .push('x', &mut budget)
                .expect("four-byte replacement extent");
            token.push('y', &mut budget).expect("reuse admitted extent");
            let result = token.push('z', &mut budget);
            if floor == 0 {
                result.expect("eight-byte replacement extent");
                assert_eq!(token.as_str(), "éxyz");
                assert_eq!(
                    allocation.usage(),
                    Usage {
                        items: prior.items + 3,
                        bytes: prior.bytes + 14
                    }
                );
                let before = allocation.usage();
                token.clear();
                let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
                for character in "12345678".chars() {
                    token
                        .push(character, &mut budget)
                        .expect("clear retains admission");
                }
                assert_eq!(allocation.usage(), before);
            } else {
                // The eight-byte replacement layout crosses the one-less
                // byte target; it is still charged in full to the original
                // account and recorded, and the token keeps growing.
                result.expect("accounting targets never refuse a replacement layout");
                assert_eq!(token.as_str(), "éxyz");
                assert_eq!(
                    allocation.usage(),
                    Usage {
                        items: prior.items + 3,
                        bytes: prior.bytes + 14
                    }
                );
                let record = allocation.exceedances();
                let bytes = record
                    .axis(crate::input::INPUT_BYTES_RESOURCE)
                    .expect("one less byte records the byte axis");
                assert_eq!(bytes.target, (prior.bytes + 13) as u64);
                assert_eq!(bytes.high_water, (prior.bytes + 14) as u64);
                assert_eq!(record.axes().count(), 1);
                // Only a genuine failure latches; the latched account admits
                // no further layout and the token is untouched.
                let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
                budget.refuse();
                assert!(matches!(token.push('1', &mut budget), Err(Error::Refused)));
                assert_eq!(token.as_str(), "éxyz");
            }
        }
    }

    #[test]
    fn exhausted_graph_decode_does_not_refuse_token_allocation() {
        let limits = Limits::standard();
        let prior = Usage {
            items: 7,
            bytes: 31,
        };
        let mut allocation = Allocation::for_proof(prior, &limits);
        let graph = node::GraphBudget::new();
        graph
            .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
            .expect("all decoding already spent");
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        let mut token = Token::new();
        // Token storage is a copy owned once by the input allocation.
        token
            .push('é', &mut budget)
            .expect("token storage charges no graph decode");
        assert_eq!(token.value.capacity(), 2);
        assert_eq!(token.admitted, 2);
        assert_eq!(
            graph.census_usage().expect("graph census").0,
            node::MAX_DECODED_BYTES_PER_GRAPH
        );
        assert_eq!(
            allocation.usage(),
            Usage {
                items: prior.items + 1,
                bytes: prior.bytes + 2
            }
        );
    }

    /// A token past a low string target keeps doubling: 4,096 characters under
    /// a one-byte target take 13 replacement layouts (1, 2, 4, ... 4,096),
    /// each charged once, never one per character (targets only record).
    #[test]
    fn growth_past_a_low_string_target_stays_amortized() {
        let prior = Usage {
            items: 7,
            bytes: 31,
        };
        let mut limits = Limits::standard();
        limits.string_bytes = 1;
        let mut allocation = Allocation::for_proof(prior, &limits);
        let graph = node::GraphBudget::new();
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        let mut token = Token::new();
        for _ in 0..4096 {
            token.push('a', &mut budget).expect("targets never refuse");
        }
        assert_eq!(token.as_str().len(), 4096);
        assert_eq!(token.admitted, 4096);
        assert_eq!(
            allocation.usage(),
            Usage {
                items: prior.items + 13,
                bytes: prior.bytes + 8191,
            },
            "one charge per doubling replacement"
        );
        let axis = allocation
            .exceedances()
            .axis("dep-info token bytes")
            .copied()
            .expect("the string target crossing records");
        assert_eq!((axis.target, axis.high_water), (1, 4096));
    }
}
