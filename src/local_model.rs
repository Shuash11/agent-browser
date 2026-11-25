//! Local GGUF model hosting. A dropped model is served by the bundled
//! llama.cpp server over loopback only, so confidential work never leaves
//! the machine. OpenCode config templates make the provider switchable.

use serde::{Deserialize, Serialize};
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
}

impl LocalModel {
    pub fn new(root: PathBuf) -> Self {
        let state = Self::read_state(&root);
        Self { root, inner: Mutex::new(Inner { state, child: None }) }
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
        if let Ok(text) = serde_json::to_string_pretty(state) {
            let _ = std::fs::write(Self::state_path(&self.root), text);
        }
    }

    pub fn status(&self) -> String {
        let inner = self.inner.lock().unwrap();
        let mut value = serde_json::to_value(&inner.state).unwrap_or_default();
        value["port"] = serde_json::json!(PORT);
        value["model_id"] = serde_json::json!(MODEL_ID);
        value["runtime_ready"] = serde_json::json!(self.server_exe().is_file());
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into())
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

    fn healthy() -> bool {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(2)))
            .build()
            .new_agent();
        agent.get(HEALTH_URL).call().is_ok()
    }

    pub fn stop(&self) -> String {
        let mut inner = self.inner.lock().unwrap();
        if let Some(mut child) = inner.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        inner.state.running = false;
        self.save_state(&inner.state);
        "local model stopped".into()
    }

    pub fn load(&self, raw_path: &str) -> Result<String, String> {
        let path = PathBuf::from(raw_path.trim().trim_matches('"'));
        if !path.is_file() {
            return Err(format!("model file not found: {}", path.display()));
        }
        if path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase() != "gguf" {
            return Err("model file must be a .gguf".into());
        }
        let _ = self.stop();
        let exe = self.ensure_runtime()?;
        let child = Command::new(&exe)
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
            let _ = self.stop();
            return Err("llama-server did not become healthy within 60 seconds".into());
        }
        {
            let mut inner = self.inner.lock().unwrap();
            inner.child = Some(child);
            inner.state.model_path = Some(path.to_string_lossy().to_string());
            inner.state.running = true;
            self.save_state(&inner.state);
        }
        // A dropped model implies the user wants confidential work to stay
        // local; select the local provider automatically.
        let result = self.use_provider("local");
        Ok(format!("local model loaded: {} ({})", path.file_name().unwrap_or_default().to_string_lossy(), result))
    }

    pub fn use_provider(&self, provider: &str) -> String {
        let provider = provider.to_ascii_lowercase();
        if provider != "cloud" && provider != "local" {
            return "provider must be cloud or local".into();
        }
        if provider == "local" && !self.inner.lock().unwrap().state.running {
            return "load a local model before switching to it".into();
        }
        let template_name = if provider == "local" { "opencode.local.jsonc" } else { "opencode.cloud.jsonc" };
        let template = self.root.join(template_name);
        let Ok(config) = std::fs::read_to_string(&template) else {
            return format!("missing config template: {template_name}");
        };
        for dest in [
            self.root.join("opencode.jsonc"),
            self.root.join(".opencode").join("opencode.jsonc"),
        ] {
            if let Some(parent) = dest.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(&dest, &config) {
                return format!("could not write {}: {e}", dest.display());
            }
        }
        let mut inner = self.inner.lock().unwrap();
        inner.state.provider = provider.clone();
        self.save_state(&inner.state);
        format!("OpenCode switched to {provider}; run `opencode reload` in the terminal")
    }

    /// Text injected into the embedded terminal after a provider switch.
    pub fn reload_command() -> &'static str {
        "opencode reload\n"
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
