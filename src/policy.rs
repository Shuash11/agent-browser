//! User-editable autonomy policy for the agent.
//!
//! Loaded from `agent-policy.json` beside the executable. Missing or invalid
//! values fall back to safe defaults. `browser_policy {reload:true}` re-reads
//! the file at runtime without restarting the harness.

use serde::{Deserialize, Serialize};
use std::sync::RwLock;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AgentPolicy {
    /// Let the agent type into password fields. Default: off.
    pub allow_password_entry: bool,
    /// Let pointer actions target coordinates outside the page viewport.
    /// Default: on — no hard block.
    pub allow_outside_viewport: bool,
    /// Let the agent close the last remaining tab. Default: on.
    pub allow_close_last_tab: bool,
    /// Let the agent open new tabs. Default: on.
    pub allow_new_tabs: bool,
    /// Let the agent navigate to URLs. Default: on.
    pub allow_url_navigation: bool,
}

impl Default for AgentPolicy {
    fn default() -> Self {
        Self {
            allow_password_entry: false,
            allow_outside_viewport: true,
            allow_close_last_tab: true,
            allow_new_tabs: true,
            allow_url_navigation: true,
        }
    }
}

impl AgentPolicy {
    pub fn as_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
}

static POLICY: RwLock<Option<AgentPolicy>> = RwLock::new(None);

fn load_policy() -> AgentPolicy {
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
}

pub fn policy() -> AgentPolicy {
    if let Some(cached) = POLICY.read().unwrap().as_ref() {
        return cached.clone();
    }
    let loaded = load_policy();
    let mut guard = POLICY.write().unwrap();
    if let Some(cached) = guard.as_ref() {
        return cached.clone();
    }
    *guard = Some(loaded.clone());
    loaded
}

pub fn reload_policy() -> AgentPolicy {
    let loaded = load_policy();
    *POLICY.write().unwrap() = Some(loaded.clone());
    loaded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_keep_only_password_guard() {
        let p = AgentPolicy::default();
        assert!(!p.allow_password_entry);
        assert!(p.allow_outside_viewport);
        assert!(p.allow_close_last_tab);
        assert!(p.allow_new_tabs);
        assert!(p.allow_url_navigation);
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
        assert!(p.allow_outside_viewport);
    }
}
