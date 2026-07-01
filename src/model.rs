use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Usage {
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
}

impl Usage {
    pub fn uncached_input_tokens(&self) -> i64 {
        (self.input_tokens - self.cached_input_tokens).max(0)
    }

    pub fn add(&self, other: &Usage) -> Usage {
        Usage {
            input_tokens: self.input_tokens + other.input_tokens,
            cached_input_tokens: self.cached_input_tokens + other.cached_input_tokens,
            output_tokens: self.output_tokens + other.output_tokens,
            reasoning_output_tokens: self.reasoning_output_tokens + other.reasoning_output_tokens,
            total_tokens: self.total_tokens + other.total_tokens,
        }
    }

    pub fn delta_from(&self, previous: Option<&Usage>) -> Usage {
        let previous = previous.cloned().unwrap_or_default();
        Usage {
            input_tokens: (self.input_tokens - previous.input_tokens).max(0),
            cached_input_tokens: (self.cached_input_tokens - previous.cached_input_tokens).max(0),
            output_tokens: (self.output_tokens - previous.output_tokens).max(0),
            reasoning_output_tokens: (self.reasoning_output_tokens
                - previous.reasoning_output_tokens)
                .max(0),
            total_tokens: (self.total_tokens - previous.total_tokens).max(0),
        }
    }

    pub fn key(&self) -> (i64, i64, i64, i64, i64) {
        (
            self.input_tokens,
            self.cached_input_tokens,
            self.output_tokens,
            self.reasoning_output_tokens,
            self.total_tokens,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallStats {
    pub timestamp: Option<DateTime<Utc>>,
    pub usage: Usage,
    pub running_total: Usage,
    pub context_window: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionStats {
    pub path: PathBuf,
    pub session_id: Option<String>,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub cli_version: Option<String>,
    pub context_window: Option<i64>,
    pub usage: Usage,
    pub calls: Vec<CallStats>,
    pub call_count_override: Option<usize>,
    pub malformed_lines: usize,
}

impl SessionStats {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            session_id: None,
            start: None,
            end: None,
            cwd: None,
            model: None,
            provider: None,
            cli_version: None,
            context_window: None,
            usage: Usage::default(),
            calls: Vec::new(),
            call_count_override: None,
            malformed_lines: 0,
        }
    }

    pub fn call_count(&self) -> usize {
        self.call_count_override.unwrap_or(self.calls.len())
    }
}
