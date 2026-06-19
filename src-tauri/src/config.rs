//! User configuration. `enabled` order MUST match the [Claude, Codex, Gemini,
//! Copilot, Cursor, z.ai] order assumed across the codebase.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub poll_seconds: u64,
    pub enabled: [bool; 6], // [claude, codex, gemini, copilot, cursor, zai]
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            poll_seconds: 300,
            enabled: [true, true, true, true, true, true],
        }
    }
}

impl AppConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.poll_seconds < 30 {
            return Err(format!(
                "poll_seconds must be >= 30, got {}",
                self.poll_seconds
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_all_enabled() {
        let c = AppConfig::default();
        assert_eq!(c.enabled, [true, true, true, true, true, true]);
        assert_eq!(c.poll_seconds, 300);
        assert!(c.validate().is_ok());
    }

    #[test]
    fn rejects_too_short_interval() {
        let c = AppConfig {
            poll_seconds: 5,
            enabled: [true; 6],
        };
        assert!(c.validate().is_err());
    }
}
