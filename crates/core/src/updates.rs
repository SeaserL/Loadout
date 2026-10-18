// SPDX-License-Identifier: Apache-2.0
//! Stable-release delivery, outside the inference path. No remote scripts are executed.
use loadout_ipc::{ErrorBody, api::UpdateStatus};
use loadout_storage::Database;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

const API: &str = "https://api.github.com/repos/SeaserL/Loadout/releases/latest";
const MAX_PACKAGE: usize = 384 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
    #[serde(default)]
    url: String,
}
#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

fn version(text: &str) -> Option<[u64; 3]> {
    let parts: Vec<_> = text.strip_prefix('v').unwrap_or(text).split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut result = [0; 3];
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty()
            || !part.bytes().all(|b| b.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return None;
        }
        result[i] = part.parse().ok()?;
    }
    Some(result)
}

fn digest_text(raw: &[u8], name: &str) -> Result<String, String> {
    let text = std::str::from_utf8(raw).map_err(|_| "Invalid checksum encoding")?;
    let words: Vec<_> = text.split_whitespace().collect();
    if words.len() != 2
        || words[1] != name
        || words[0].len() != 64
        || !words[0].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("Invalid release checksum".into());
    }
    Ok(words[0].to_ascii_lowercase())
}

fn asset<'a>(release: &'a Release, name: &str) -> Result<&'a Asset, String> {
    let items: Vec<_> = release.assets.iter().filter(|a| a.name == name).collect();
    if items.len() != 1 {
        return Err("Missing or duplicate Windows release asset".into());
    }
    let item = items[0];
    let expected = format!(
        "https://github.com/SeaserL/Loadout/releases/download/{}/{}",
        release.tag_name, name
    );
    if item.browser_download_url != expected || item.size == 0 || item.size > MAX_PACKAGE as u64 {
        return Err("Untrusted release asset".into());
    }
    Ok(item)
}

fn install_dir() -> Option<PathBuf> {
    let expected = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Programs/Loadout");
    let actual = std::env::current_exe()
        .ok()?
        .parent()?
        .canonicalize()
        .ok()?;
    if actual != expected.canonicalize().ok()? {
        return None;
    }
    // PowerShell 5.1 compares ordinary Win32 paths; canonicalize adds \\?\.
    Some(expected)
}

#[derive(Debug)]
pub(crate) struct Updates {
    root: PathBuf,
    state: Mutex<UpdateStatus>,
    operation: tokio::sync::Mutex<()>,
    prepared: Mutex<Option<(PathBuf, String)>>,
}

impl Updates {
    pub(crate) fn new(data: PathBuf) -> Self {
        Self {
            root: data.join("updates"),
            state: Mutex::new(UpdateStatus {
                current_version: crate::VERSION.into(),
                phase: "idle".into(),
                install_supported: install_dir().is_some(),
                ..Default::default()
            }),
            operation: tokio::sync::Mutex::new(()),
            prepared: Mutex::new(None),
        }
    }
    pub(crate) fn status(&self) -> UpdateStatus {
        let mut state = self.state.lock().expect("update state");
        if matches!(state.phase.as_str(), "installing" | "idle")
            && let Ok(raw) = std::fs::read(self.root.join("result.json"))
            && let Ok(result) = serde_json::from_slice::<serde_json::Value>(
                raw.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&raw),
            )
            && result.get("status").and_then(|v| v.as_str()) == Some("failed")
        {
            state.phase = "error".into();
            state.error = Some("Installation failed; inspect the local update transaction. Previous programs are restored if replacement began.".into());
        }
        state.clone()
    }
    fn phase(&self, phase: &str, error: Option<String>) {
        let mut state = self.state.lock().expect("update state");
        state.phase = phase.into();
        state.error = error;
    }
    fn automatic_install_blocked(&self) -> bool {
        let failed = std::fs::read(self.root.join("result.json"))
            .ok()
            .and_then(|raw| {
                serde_json::from_slice::<serde_json::Value>(
                    raw.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&raw),
                )
                .ok()
            })
            .is_some_and(|result| result.get("status").and_then(|v| v.as_str()) == Some("failed"));
        if failed {
            self.state.lock().expect("update state").error = Some("Previous installation failed. Automatic installation is paused; use Install and restart to retry manually.".into());
        }
        failed
    }
    pub(crate) fn start(self: &Arc<Self>, download: bool) {
        let this = self.clone();
        tokio::spawn(async move {
            let Ok(_guard) = this.operation.try_lock() else {
                return;
            };
            if this.status().phase == "installing" {
                return;
            }
            this.phase("checking", None);
            *this.prepared.lock().expect("prepared update") = None;
            if let Err(error) = this.check(download).await {
                this.phase("error", Some(error));
            }
        });
    }
    async fn check(&self, download: bool) -> Result<(), String> {
        self.check_for_version(
            download,
            version(crate::VERSION).ok_or("Invalid application version")?,
        )
        .await
    }
    async fn check_for_version(&self, download: bool, current: [u64; 3]) -> Result<(), String> {
        let credential = tokio::task::spawn_blocking(loadout_secrets::github_git_credential)
            .await
            .map_err(|_| "Cannot read GitHub login")?;
        let client = reqwest::Client::builder()
            .user_agent(concat!("Loadout/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(600))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                let host = attempt.url().host_str().unwrap_or("");
                if attempt.previous().len() < 5
                    && attempt.url().scheme() == "https"
                    && matches!(
                        host,
                        "github.com"
                            | "release-assets.githubusercontent.com"
                            | "objects.githubusercontent.com"
                    )
                {
                    attempt.follow()
                } else {
                    attempt.error("Untrusted update redirect")
                }
            }))
            .build()
            .map_err(|_| "Cannot create update HTTP client")?;
        let bytes = read_response(
            request(&client, API, credential.as_ref(), false),
            2 * 1024 * 1024,
        )
        .await?;
        let release: Release =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid GitHub release metadata")?;
        if release.draft || release.prerelease {
            return Err("Expected a stable published release".into());
        }
        let latest = version(&release.tag_name).ok_or("Invalid stable version")?;
        {
            let mut state = self.state.lock().expect("update state");
            state.checked_at_ms = Some(loadout_storage::now_ms());
            state.latest_version = Some(release.tag_name.clone());
        }
        if latest <= current {
            self.phase("current", None);
            return Ok(());
        }
        let name = format!(
            "Loadout-{}.{}.{}-windows-x64.zip",
            latest[0], latest[1], latest[2]
        );
        let package = asset(&release, &name)?;
        let checksum = asset(&release, &format!("{name}.sha256"))?;
        if !download {
            self.phase("available", None);
            return Ok(());
        }
        self.phase("downloading", None);
        let expected = digest_text(
            &read_response(
                request(
                    &client,
                    asset_download(checksum, credential.is_some())?,
                    credential.as_ref(),
                    true,
                ),
                1024,
            )
            .await?,
            &name,
        )?;
        tokio::fs::create_dir_all(&self.root)
            .await
            .map_err(|_| "Cannot create update cache")?;
        loadout_platform::restrict_dir_to_current_user(&self.root)
            .map_err(|_| "Cannot protect update cache")?;
        let path = self.root.join(&name);
        let partial = self.root.join(format!("{name}.part"));
        let result = async {
            let mut response = request(
                &client,
                asset_download(package, credential.is_some())
                    .map_err(|_| "Invalid asset API URL")?,
                credential.as_ref(),
                true,
            )
            .send()
            .await
            .map_err(|_| "Update download failed")?
            .error_for_status()
            .map_err(|_| "Update asset unavailable")?;
            let mut file = tokio::fs::File::create(&partial)
                .await
                .map_err(|_| "Cannot write update cache")?;
            let mut total = 0usize;
            let mut digest = Sha256::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| "Update download interrupted")?
            {
                total += chunk.len();
                if total > MAX_PACKAGE || total as u64 > package.size {
                    return Err("Update exceeds declared size");
                }
                digest.update(&chunk);
                file.write_all(&chunk)
                    .await
                    .map_err(|_| "Cannot write update cache")?;
            }
            let actual: String = digest
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if total as u64 != package.size || actual != expected {
                return Err("Update checksum or size mismatch");
            }
            file.sync_all()
                .await
                .map_err(|_| "Cannot flush update cache")?;
            drop(file);
            if path.exists() {
                tokio::fs::remove_file(&path)
                    .await
                    .map_err(|_| "Cannot replace update cache")?;
            }
            tokio::fs::rename(&partial, &path)
                .await
                .map_err(|_| "Cannot finalize update cache")?;
            Ok(())
        }
        .await;
        if let Err(error) = result {
            let _ = tokio::fs::remove_file(&partial).await;
            return Err(error.into());
        }
        *self.prepared.lock().expect("prepared update") = Some((path, expected));
        self.phase("ready", None);
        Ok(())
    }
    pub(crate) async fn install(&self) -> Result<UpdateStatus, ErrorBody> {
        let _guard = self
            .operation
            .try_lock()
            .map_err(|_| ErrorBody::conflict("Update operation already running"))?;
        if self.status().phase != "ready" {
            return Err(ErrorBody::conflict("No verified update is ready"));
        }
        let target = install_dir().ok_or_else(|| ErrorBody::invalid_params("Automatic installation requires %LOCALAPPDATA%/Programs/Loadout; development builds only download"))?;
        let prepared = self
            .prepared
            .lock()
            .expect("prepared update")
            .clone()
            .ok_or_else(|| ErrorBody::conflict("Update cache missing"))?;
        let helper = self.root.join("apply-update.ps1");
        let manifest = self.root.join("apply-update.json");
        let _ = tokio::fs::remove_file(self.root.join("result.json")).await;
        tokio::fs::write(&helper, include_str!("../../../packaging/update.ps1"))
            .await
            .map_err(|_| ErrorBody::internal("Cannot prepare update helper"))?;
        let plan = serde_json::json!({"zip":prepared.0,"sha256":prepared.1,"install_dir":target,"version":self.status().latest_version});
        tokio::fs::write(&manifest, serde_json::to_vec(&plan).expect("manifest"))
            .await
            .map_err(|_| ErrorBody::internal("Cannot prepare update manifest"))?;
        let shell =
            PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into()))
                .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        use std::ffi::OsStr;
        loadout_platform::spawn_detached(
            &shell,
            &[
                OsStr::new("-NoProfile"),
                OsStr::new("-ExecutionPolicy"),
                OsStr::new("Bypass"),
                OsStr::new("-File"),
                helper.as_os_str(),
                OsStr::new("-Manifest"),
                manifest.as_os_str(),
            ],
        )
        .map_err(|_| ErrorBody::internal("Cannot launch update helper"))?;
        self.phase("installing", None);
        Ok(self.status())
    }
}

fn asset_download(asset: &Asset, authenticated: bool) -> Result<&str, String> {
    if !authenticated {
        return Ok(&asset.browser_download_url);
    }
    let prefix = "https://api.github.com/repos/SeaserL/Loadout/releases/assets/";
    if !asset
        .url
        .strip_prefix(prefix)
        .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("Invalid GitHub asset API URL".into());
    }
    Ok(&asset.url)
}

fn request(
    client: &reqwest::Client,
    url: &str,
    credential: Option<&loadout_secrets::Secret>,
    binary: bool,
) -> reqwest::RequestBuilder {
    let mut request = client.get(url);
    // Only the fixed GitHub API receives credentials. Reqwest strips Authorization on
    // cross-host redirects to the signed asset CDN; never put it in default headers.
    if url.starts_with("https://api.github.com/repos/SeaserL/Loadout/") {
        if let Some(secret) = credential {
            request = request.bearer_auth(secret.expose());
        }
        request = request.header(
            reqwest::header::ACCEPT,
            if binary {
                "application/octet-stream"
            } else {
                "application/vnd.github+json"
            },
        );
    }
    request
}

async fn read_response(request: reqwest::RequestBuilder, limit: usize) -> Result<Vec<u8>, String> {
    let mut response = request
        .send()
        .await
        .map_err(|_| "GitHub update request failed")?
        .error_for_status()
        .map_err(|_| "GitHub release unavailable. For private repositories, sign in with Git Credential Manager using an account with access; otherwise retry later.")?;
    let mut data = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "GitHub update response interrupted")?
    {
        if data.len() + chunk.len() > limit {
            return Err("Update metadata too large".into());
        }
        data.extend_from_slice(&chunk);
    }
    Ok(data)
}

pub(crate) async fn run(updates: Arc<Updates>, db: Arc<Database>, stop: CancellationToken) {
    let mut next = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        tokio::select! { _ = stop.cancelled() => return, _ = tokio::time::sleep(Duration::from_secs(30)) => {} }
        let enabled = |key| {
            crate::settings::value(&db, key)
                .ok()
                .is_some_and(|v| v == "true")
        };
        if enabled("updates.enabled") && tokio::time::Instant::now() >= next {
            updates.start(enabled("updates.auto_download"));
            next = tokio::time::Instant::now() + Duration::from_secs(6 * 3600);
        }
        if enabled("updates.enabled")
            && enabled("updates.auto_install")
            && updates.status().install_supported
            && updates.status().phase == "ready"
            && !updates.automatic_install_blocked()
            && let Err(error) = updates.install().await
        {
            updates.phase("error", Some(error.message));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rollback_receipt_handles_powershell_bom_after_restart() {
        let temp = tempfile::tempdir().expect("temporary cache");
        let updates = Updates::new(temp.path().into());
        std::fs::create_dir(&updates.root).expect("cache");
        std::fs::write(
            updates.root.join("result.json"),
            b"\xef\xbb\xbf{\"status\":\"failed\"}",
        )
        .expect("receipt");
        assert_eq!(updates.status().phase, "error");
        updates.phase("ready", None);
        assert!(updates.automatic_install_blocked());
        assert_eq!(updates.status().phase, "ready");
    }
    #[tokio::test]
    #[ignore = "explicit GitHub access; downloads a release into a temporary cache only"]
    async fn live_release_download_verifies_existing_asset() {
        assert_eq!(
            std::env::var("LOADOUT_UPDATE_LIVE_TESTS").as_deref(),
            Ok("1")
        );
        let temp = tempfile::tempdir().expect("temporary cache");
        let updates = Updates::new(temp.path().into());
        updates
            .check_for_version(true, [0, 0, 0])
            .await
            .expect("download and verify");
        assert_eq!(updates.status().phase, "ready");
        assert!(!updates.status().install_supported);
        let prepared = updates.prepared.lock().expect("prepared");
        let (path, hash) = prepared.as_ref().expect("verified asset");
        assert!(path.starts_with(temp.path()));
        assert_eq!(hash.len(), 64);
        assert!(path.metadata().expect("asset").len() > 0);
    }

    #[test]
    fn authorization_is_scoped_and_redacted() {
        let client = reqwest::Client::new();
        let credential = loadout_secrets::Secret::new("synthetic-private-token");
        let authorized = request(&client, API, Some(&credential), false)
            .build()
            .expect("request");
        assert!(
            authorized
                .headers()
                .get(reqwest::header::AUTHORIZATION)
                .expect("auth")
                .is_sensitive()
        );
        let unrelated = request(
            &client,
            "https://release-assets.githubusercontent.com/example",
            Some(&credential),
            true,
        )
        .build()
        .expect("request");
        assert!(
            !unrelated
                .headers()
                .contains_key(reqwest::header::AUTHORIZATION)
        );
        assert!(!format!("{authorized:?}").contains(credential.expose()));
    }
    #[tokio::test]
    async fn metadata_reader_rejects_http_errors_and_oversize() {
        use axum::{Router, routing::get as route_get};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let addr = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/ok", route_get(|| async { "synthetic" }))
                    .route(
                        "/bad",
                        route_get(|| async { axum::http::StatusCode::TOO_MANY_REQUESTS }),
                    ),
            )
            .await
            .expect("server");
        });
        let client = reqwest::Client::new();
        assert_eq!(
            read_response(client.get(format!("http://{addr}/ok")), 100)
                .await
                .expect("body"),
            b"synthetic"
        );
        assert!(
            read_response(client.get(format!("http://{addr}/ok")), 2)
                .await
                .is_err()
        );
        assert!(
            read_response(client.get(format!("http://{addr}/bad")), 100)
                .await
                .is_err()
        );
        server.abort();
    }
    #[test]
    fn versions_and_checksums_reject_ambiguous_inputs() {
        assert!(version("v0.10.0") > version("0.9.9"));
        for bad in [
            "1.2",
            "1.2.3-beta",
            "v1.02.3",
            "../1.2.3",
            "1.2.3/",
            "1.2.-3",
        ] {
            assert!(version(bad).is_none());
        }
        assert!(digest_text(format!("{}  a.zip", "a".repeat(64)).as_bytes(), "a.zip").is_ok());
        assert!(digest_text(format!("{}  b.zip", "a".repeat(64)).as_bytes(), "a.zip").is_err());
        assert!(digest_text(b"bad a.zip", "a.zip").is_err());
    }
    #[test]
    fn release_asset_cannot_redirect_to_another_repository() {
        let mut release = Release {
            tag_name: "v0.4.0".into(),
            draft: false,
            prerelease: false,
            assets: vec![Asset {
                name: "a.zip".into(),
                browser_download_url:
                    "https://github.com/SeaserL/Loadout/releases/download/v0.4.0/a.zip".into(),
                size: 100,
                url: "https://api.github.com/repos/SeaserL/Loadout/releases/assets/123".into(),
            }],
        };
        assert!(asset(&release, "a.zip").is_ok());
        assert!(asset_download(&release.assets[0], true).is_ok());
        release.assets[0].url.push_str("?unexpected");
        assert!(asset_download(&release.assets[0], true).is_err());
        release.assets[0].browser_download_url.push_str("?evil=1");
        assert!(asset(&release, "a.zip").is_err());
        release.assets.clear();
        assert!(asset(&release, "a.zip").is_err());
    }
}
