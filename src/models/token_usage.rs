use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
}

impl TokenUsage {
    #[allow(dead_code)] // Will be used in UI token display
    pub fn display_total_tokens(&self) -> i64 {
        self.input_tokens + self.output_tokens + self.reasoning_tokens.unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_total_tokens_sums_input_output_reasoning() {
        let usage = TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: Some(200),
            cache_write_tokens: Some(50),
            reasoning_tokens: Some(300),
        };
        assert_eq!(usage.display_total_tokens(), 1800);
    }

    #[test]
    fn display_total_tokens_without_reasoning() {
        let usage = TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
        };
        assert_eq!(usage.display_total_tokens(), 1500);
    }
}
