//! Manual update check against GitHub releases. No agent access, no background polling.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use semver::Version;
use sha2::{Digest, Sha256};

const RELEASES_API: &str =
    "https://api.github.com/repos/Shuash11/agent-browser/releases/latest";

#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub tag: String,
    pub version: String,
    pub notes: String,
    pub asset_name: String,
    pub download_url: String,
    pub sha256: Option<String>,
}

pub fn check_latest() -> Result<Option<ReleaseInfo>, String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .build()
        .new_agent();
    let body = agent
        .get(RELEASES_API)
        .header("User-Agent", "agent-browser-updater")
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("update check failed: {e}"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("read release JSON failed: {e}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("invalid release JSON: {e}"))?;

    let tag = json.get("tag_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let version = tag.strip_prefix('v').unwrap_or(&tag).to_string();
    let latest = Version::parse(&version).map_err(|e| format!("bad release version: {e}"))?;
    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|e| format!("bad current version: {e}"))?;
    if latest <= current {
        return Ok(None);
    }

    let mut asset_name = String::new();
    let mut download_url = String::new();
    let mut sha256 = None;
    if let Some(assets) = json.get("assets").and_then(|a| a.as_array()) {
        for asset in assets {
            let name = asset.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if name.ends_with(".exe") && name.contains("Setup") {
                asset_name = name.to_string();
                download_url = asset
                    .get("browser_download_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                sha256 = asset
                    .get("digest")
                    .and_then(|v| v.as_str())
                    .map(|d| d.strip_prefix("sha256:").unwrap_or(d).to_string());
                break;
            }
        }
    }
    if download_url.is_empty() {
        return Err("release has no installer asset".to_string());
    }

    Ok(Some(ReleaseInfo {
        tag,
        version,
        notes: json.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        asset_name,
        download_url,
        sha256,
    }))
}

pub fn download_verified(info: &ReleaseInfo, path: &Path) -> Result<(), String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(120)))
        .build()
        .new_agent();
    let mut response = agent
        .get(&info.download_url)
        .header("User-Agent", "agent-browser-updater")
        .call()
        .map_err(|e| format!("download failed: {e}"))?;
    let mut file = fs::File::create(path).map_err(|e| format!("create installer failed: {e}"))?;
    let mut reader = response.body_mut().as_reader();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("download read failed: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| format!("download write failed: {e}"))?;
    }
    drop(file);

    if let Some(expected) = &info.sha256 {
        let bytes = fs::read(path).map_err(|e| format!("hash read failed: {e}"))?;
        let actual = hex(Sha256::digest(&bytes).to_vec());
        if !actual.eq_ignore_ascii_case(expected) {
            let _ = fs::remove_file(path);
            return Err(format!("SHA-256 mismatch: {actual} != {expected}"));
        }
    }
    Ok(())
}

fn hex(bytes: Vec<u8>) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02X}"));
    }
    out
}

fn state_path(root: &Path) -> PathBuf {
    root.join("data").join("update-state.json")
}

pub fn is_skipped(root: &Path, tag: &str) -> bool {
    let Ok(text) = fs::read_to_string(state_path(root)) else { return false };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else { return false };
    json.get("skipped_tag").and_then(|v| v.as_str()) == Some(tag)
}

pub fn skip(root: &Path, tag: &str) {
    let path = state_path(root);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let text = serde_json::json!({ "skipped_tag": tag }).to_string();
    let _ = fs::write(path, text);
}
