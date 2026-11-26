//! Local GGUF model hosting. A dropped model is served by the bundled
//! llama.cpp server over loopback only, so confidential work never leaves
//! the machine. OpenCode config is merged, not replaced, when switching.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

const PORT: u16 = 8080;
const MODEL_ID: &str = "local-model";
const HEALTH_URL: &str = "http://127.0.0.1:8080/health";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelState {
    pub model_path: Option<String>,
    pub provider: String,
    /// Persisted only for compatibility; always written as false. Runtime
    /// status is derived from the owned child process.
    pub running: bool,
}

impl Default for ModelState {
    fn default() -> Self {
        Self { model_path: None, provider: "cloud".into(), running: false }
    }
}

struct Inner {
    state: ModelState,
    child: Option<Child>,
}

pub struct LocalModel {
    root: PathBuf,
    inner: Mutex<Inner>,
    /// Serializes load/stop/switch operations across GUI and MCP callers.
    op: Mutex<()>,
}

impl Drop for LocalModel {
    fn drop(&mut self) {
        self.stop_locked();
    }
}

impl LocalModel {
    pub fn new(root: PathBuf) -> Self {
        let mut state = Self::read_state(&root);
        // A process from a previous session cannot still be ours.
        state.running = false;
        Self { root, inner: Mutex::new(Inner { state, child: None }), op: Mutex::new(()) }
    }

    fn state_path(root: &Path) -> PathBuf {
        root.join("local-model.json")
    }

    fn read_state(root: &Path) -> ModelState {
        let path = Self::state_path(root);
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(text.trim_start_matches('\u{feff}'))
                .unwrap_or_default(),
            Err(_) => ModelState::default(),
        }
    }

    fn save_state(&self, state: &ModelState) {
        let mut persisted = state.clone();
        persisted.running = false;
        if let Ok(text) = serde_json::to_string_pretty(&persisted) {
            let _ = std::fs::write(Self::state_path(&self.root), text);
        }
    }

    fn child_alive(&self) -> bool {
        let mut inner = self.inner.lock().unwrap();
        match inner.child.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        }
    }

    fn healthy() -> bool {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(2)))
            .build()
            .new_agent();
        agent.get(HEALTH_URL).call().is_ok()
    }

    pub fn status(&self) -> String {
        let state = self.state();
        let child_alive = self.inner.lock().unwrap().child.is_some();
        let alive = child_alive && Self::healthy();
        let mut value = serde_json::to_value(&state).unwrap_or_default();
        value["port"] = json!(PORT);
        value["model_id"] = json!(MODEL_ID);
        value["runtime_ready"] = json!(self.server_exe().is_file());
        value["running"] = json!(alive);
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
    }

    fn state(&self) -> ModelState {
        self.inner.lock().unwrap().state.clone()
    }

    fn server_exe(&self) -> PathBuf {
        self.root.join("models").join("llama").join("llama-server.exe")
    }

    fn ensure_runtime(&self) -> Result<PathBuf, String> {
        let exe = self.server_exe();
        if exe.is_file() {
            return Ok(exe);
        }
        let zip = self.root.join("runtime").join("llama-cpu.zip");
        if !zip.is_file() {
            return Err("bundled llama.cpp runtime is missing".into());
        }
        let dir = exe.parent().unwrap();
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let script = format!(
            "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
            zip.display(),
            dir.display()
        );
        let out = Command::new("powershell.exe")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() || !exe.is_file() {
            return Err("could not extract llama.cpp runtime".into());
        }
        Ok(exe)
    }

    /// Kill and wait outside the state lock so a stuck child cannot freeze
    /// the GUI event loop.
    fn stop_locked(&self) -> String {
        let child = {
            let mut inner = self.inner.lock().unwrap();
            inner.state.running = false;
            inner.child.take()
        };
        if let Some(mut child) = child {
            let _ = child.kill();
            let _ = child.wait();
        }
        let state = self.state();
        self.save_state(&state);
        "local model stopped".into()
    }

    pub fn stop(&self) -> String {
        let _guard = self.op.lock().unwrap();
        self.stop_locked()
    }

    pub fn load(&self, raw_path: &str) -> Result<String, String> {
        let _guard = self.op.lock().unwrap();
        let path = PathBuf::from(raw_path.trim().trim_matches('"'));
        if !path.is_file() {
            return Err(format!("model file not found: {}", path.display()));
        }
        if path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase() != "gguf" {
            return Err("model file must be a .gguf".into());
        }
        let _ = self.stop_locked();
        let exe = self.ensure_runtime()?;
        let mut child = Command::new(&exe)
            .args([
                "-m",
                &path.to_string_lossy(),
                "--host", "127.0.0.1",
                "--port", &PORT.to_string(),
                "-c", "4096",
                "-a", MODEL_ID,
                "--no-webui",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("could not start llama-server: {e}"))?;
        let mut ready = false;
        for _ in 0..120 {
            std::thread::sleep(Duration::from_millis(500));
            if !matches!(child.try_wait(), Ok(None)) {
                let _ = self.stop_locked();
                return Err("llama-server exited during startup".into());
            }
            if Self::healthy() {
                ready = true;
                break;
            }
        }
        if !ready {
            {
                let mut inner = self.inner.lock().unwrap();
                inner.child = Some(child);
            }
            let _ = self.stop_locked();
            return Err("llama-server did not become healthy within 60 seconds".into());
        }
        {
            let mut inner = self.inner.lock().unwrap();
            inner.child = Some(child);
            inner.state.model_path = Some(path.to_string_lossy().to_string());
        }
        let state = self.state();
        self.save_state(&state);
        let result = self.use_provider_inner("local")?;
        Ok(format!(
            "local model loaded: {} ({result})",
            path.file_name().unwrap_or_default().to_string_lossy()
        ))
    }

    /// Merge only the provider/model fields into the active configs.
    fn merge_config(&self, provider: &str) -> Result<(), String> {
        let local_template = self.root.join("opencode.local.jsonc");
        let cloud_template = self.root.join("opencode.cloud.jsonc");
        let template = if provider == "local" { &local_template } else { &cloud_template };
        let template_text = std::fs::read_to_string(template)
            .map_err(|e| format!("missing config template {}: {e}", template.display()))?;
        let template_json: Value = serde_json::from_str(&template_text)
            .map_err(|e| format!("invalid config template: {e}"))?;
        let local_provider = template_json
            .get("providers")
            .and_then(|p| p.get("local"))
            .cloned()
            .ok_or_else(|| "local provider template missing".to_string())?;
        let model = if provider == "local" {
            "local/local-model".to_string()
        } else {
            "nvidia-glm/glm-5.3-flash".to_string()
        };
        let targets = [
            self.root.join("opencode.jsonc"),
            self.root.join(".opencode").join("opencode.jsonc"),
        ];
        for dest in targets {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut config: Value = std::fs::read_to_string(&dest)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_else(|| template_json.clone());
            config["model"] = json!(model);
            config
                .as_object_mut()
                .ok_or_else(|| "config is not an object".to_string())?
                .entry("providers")
                .or_insert_with(|| json!({}))["local"] = local_provider.clone();
            let tmp = dest.with_extension("jsonc.tmp");
            std::fs::write(&tmp, serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, &dest).map_err(|e| format!("could not install config: {e}"))?;
        }
        Ok(())
    }

    fn use_provider_inner(&self, provider: &str) -> Result<String, String> {
        let provider = provider.to_ascii_lowercase();
        if provider != "cloud" && provider != "local" {
            return Err("provider must be cloud or local".into());
        }
        if provider == "local" && !(self.child_alive() && Self::healthy()) {
            return Err("load a running local model before switching to it".into());
        }
        self.merge_config(&provider)?;
        let mut inner = self.inner.lock().unwrap();
        inner.state.provider = provider.clone();
        self.save_state(&inner.state);
        Ok(format!("OpenCode switched to {provider}; configuration reloaded on next start"))
    }

    pub fn use_provider(&self, provider: &str) -> Result<String, String> {
        let _guard = self.op.lock().unwrap();
        self.use_provider_inner(provider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_cloud_and_stopped() {
        let s = ModelState::default();
        assert_eq!(s.provider, "cloud");
        assert!(!s.running);
        assert!(s.model_path.is_none());
    }

    #[test]
    fn rejects_non_gguf_paths() {
        let dir = std::env::temp_dir();
        let m = LocalModel::new(dir);
        assert!(m.load("C:\\definitely\\missing.gguf").is_err());
    }
}
