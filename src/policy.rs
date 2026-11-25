//! User-editable autonomy policy for the agent.
//!
//! Loaded once from `agent-policy.json` beside the executable. A missing or
//! malformed file falls back to safe defaults, so the harness never becomes
//! less safe because of a bad config. The user controls flexibility; no
//! protection is removed silently.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AgentPolicy {
    /// Let the agent type into password fields. Default: off.
    pub allow_password_entry: bool,
    /// Let pointer actions target coordinates outside the page viewport.
    /// Default: off.
    pub allow_outside_viewport: bool,
    /// Let the agent close the last remaining tab. Default: off.
    pub allow_close_last_tab: bool,
    /// Let the agent open new tabs. Default: on.
    pub allow_new_tabs: bool,
    /// Let the agent navigate to http(s) URLs. Default: on.
    pub allow_url_navigation: bool,
    /// Total tab WebViews created at startup. Extras start hidden and are
    /// reused by `browser_new_tab`. Default: 6 (4 open + 2 spare).
    pub max_tabs: usize,
}

impl Default for AgentPolicy {
    fn default() -> Self {
        Self {
            allow_password_entry: false,
            allow_outside_viewport: false,
            allow_close_last_tab: false,
            allow_new_tabs: true,
            allow_url_navigation: true,
            max_tabs: 6,
        }
    }
}

impl AgentPolicy {
    pub fn as_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
}

static POLICY: OnceLock<AgentPolicy> = OnceLock::new();

pub fn policy() -> &'static AgentPolicy {
    POLICY.get_or_init(|| {
        let path = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("agent-policy.json")));
        let Some(path) = path else {
            return AgentPolicy::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<AgentPolicy>(text.trim_start_matches('\u{feff}')) {
                Ok(parsed) => parsed,
                Err(e) => {
                    eprintln!("agent-browser: invalid agent-policy.json ({e}); using defaults");
                    AgentPolicy::default()
                }
            },
            Err(_) => AgentPolicy::default(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_keep_protections() {
        let p = AgentPolicy::default();
        assert!(!p.allow_password_entry);
        assert!(!p.allow_outside_viewport);
        assert!(!p.allow_close_last_tab);
        assert!(p.allow_new_tabs);
        assert!(p.allow_url_navigation);
        assert_eq!(p.max_tabs, 6);
    }

    #[test]
    fn utf8_bom_is_tolerated() {
        let with_bom = format!("\u{feff}{}", r#"{"allow_password_entry":true}"#);
        let p: AgentPolicy = serde_json::from_str(with_bom.trim_start_matches('\u{feff}')).unwrap();
        assert!(p.allow_password_entry);
    }

    #[test]
    fn partial_config_uses_defaults_for_missing_keys() {
        let p: AgentPolicy = serde_json::from_str(r#"{"allow_password_entry":true}"#).unwrap();
        assert!(p.allow_password_entry);
        assert!(!p.allow_outside_viewport);
        assert_eq!(p.max_tabs, 6);
    }
}
