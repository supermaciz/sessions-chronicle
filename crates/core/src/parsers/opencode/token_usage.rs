//! Aggregate observed usage without conflating missing usage with zero usage.

use crate::models::TokenUsage;

use super::MessageTokens;

#[derive(Default)]
pub(super) struct TokenUsageAccumulator {
    input: i64,
    output: i64,
    reasoning: i64,
    cache_read: i64,
    cache_write: i64,
    observed: bool,
}

impl TokenUsageAccumulator {
    pub(super) fn push(&mut self, tokens: MessageTokens) {
        self.observed = true;
        self.input += tokens.input;
        self.output += tokens.output;
        self.reasoning += tokens.reasoning.unwrap_or(0);
        self.cache_read += tokens.cache_read.unwrap_or(0);
        self.cache_write += tokens.cache_write.unwrap_or(0);
    }

    pub(super) fn finish(self) -> Option<TokenUsage> {
        self.observed.then_some(TokenUsage {
            input_tokens: self.input,
            output_tokens: self.output,
            reasoning_tokens: (self.reasoning > 0).then_some(self.reasoning),
            cache_read_tokens: (self.cache_read > 0).then_some(self.cache_read),
            cache_write_tokens: (self.cache_write > 0).then_some(self.cache_write),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{models::TokenUsage, parsers::opencode::MessageTokens};

    #[test]
    fn unobserved_usage_is_unknown() {
        assert_eq!(TokenUsageAccumulator::default().finish(), None);
    }

    #[test]
    fn observed_zero_usage_is_not_unknown() {
        let mut accumulator = TokenUsageAccumulator::default();
        accumulator.push(MessageTokens {
            input: 0,
            output: 0,
            reasoning: Some(0),
            cache_read: None,
            cache_write: Some(0),
        });
        assert_eq!(
            accumulator.finish(),
            Some(TokenUsage {
                input_tokens: 0,
                output_tokens: 0,
                reasoning_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None
            })
        );
    }

    #[test]
    fn sums_usage_and_preserves_optional_counters() {
        let mut accumulator = TokenUsageAccumulator::default();
        accumulator.push(MessageTokens {
            input: 10,
            output: 20,
            reasoning: Some(3),
            cache_read: None,
            cache_write: Some(5),
        });
        accumulator.push(MessageTokens {
            input: 30,
            output: 40,
            reasoning: None,
            cache_read: Some(7),
            cache_write: Some(11),
        });
        accumulator.push(MessageTokens {
            input: 50,
            output: 60,
            reasoning: Some(13),
            cache_read: Some(17),
            cache_write: None,
        });
        assert_eq!(
            accumulator.finish(),
            Some(TokenUsage {
                input_tokens: 90,
                output_tokens: 120,
                reasoning_tokens: Some(16),
                cache_read_tokens: Some(24),
                cache_write_tokens: Some(16)
            })
        );
    }
}
