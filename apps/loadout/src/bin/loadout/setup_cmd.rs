// SPDX-License-Identifier: Apache-2.0

//! One-click client configuration (docs/research/2026-10-04-feature-landscape.md §1, demo).
//!
//! Points Claude Code, Codex or OpenCode at the local gateway with a freshly minted client
//! key. Only Loadout-owned keys are changed, the original file is backed up next to it
//! first (`<file>.loadout-backup-<unix secs>`), and `--restore` puts the newest backup back.
//! A file that cannot be parsed is never rewritten.

use std::path::{Path, PathBuf};

use clap::{Subcommand, ValueEnum};
use loadout_ipc::api::{self, ClientCreateParams};
use serde_json::{Map, Value, json};

use crate::{Ctx, Failure};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Client {
    /// Claude Code CLI (`%USERPROFILE%\.claude\settings.json`).
    ClaudeCode,
    /// Codex CLI / IDE (`%USERPROFILE%\.codex\config.toml`; `auth.json` is never touched).
    Codex,
    /// OpenCode (`%USERPROFILE%\.config\opencode\opencode.json`).
    Opencode,
}

impl Client {
    fn name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Opencode => "opencode",
        }
    }

    fn path(self, home: &Path) -> PathBuf {
        match self {
            Self::ClaudeCode => std::env::var_os("CLAUDE_CONFIG_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".claude"))
                .join("settings.json"),
            Self::Codex => std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex"))
                .join("config.toml"),
            Self::Opencode => home.join(".config").join("opencode").join("opencode.json"),
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum SetupCommand {
    /// Point a client at Loadout (creates a client key named after the client).
    Apply {
        client: Client,
        /// Default model, e.g. `loadout/default` or `openrouter/main/anthropic/claude-sonnet-4.5`.
        #[arg(long)]
        model: Option<String>,
    },
    /// Put back the newest backup taken by `apply`.
    Restore { client: Client },
    /// Show where each client's configuration lives and whether it points at Loadout.
    Status,
}

fn home() -> Result<PathBuf, Failure> {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .ok_or_else(|| Failure::new("USERPROFILE is not set"))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn io(e: std::io::Error, what: &Path) -> Failure {
    Failure::new(format!("{}: {e}", what.display()))
}

/// Copies the current file (or records that there was none) before the first change.
fn backup(path: &Path) -> Result<PathBuf, Failure> {
    let stamp = now_secs();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let target = if path.exists() {
        let b = path.with_file_name(format!("{name}.loadout-backup-{stamp}"));
        std::fs::copy(path, &b).map_err(|e| io(e, &b))?;
        b
    } else {
        let b = path.with_file_name(format!("{name}.loadout-backup-{stamp}.absent"));
        std::fs::write(&b, b"").map_err(|e| io(e, &b))?;
        b
    };
    Ok(target)
}

/// Writes through a temporary file in the same directory, then replaces the target.
fn write_atomic(path: &Path, text: &str) -> Result<(), Failure> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| io(e, dir))?;
    }
    let tmp = path.with_extension(format!("loadout-tmp-{}", std::process::id()));
    std::fs::write(&tmp, text).map_err(|e| io(e, &tmp))?;
    std::fs::rename(&tmp, path).map_err(|e| io(e, path))
}

fn read_json(path: &Path) -> Result<Map<String, Value>, Failure> {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(io(e, path)),
        Ok(text) if text.trim().is_empty() => Ok(Map::new()),
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(m)) => Ok(m),
            _ => Err(Failure::new(format!(
                "{} is not plain JSON (comments or a syntax error?); not changing it. Configure it by hand with the values from `loadout setup status`.",
                path.display()
            ))),
        },
    }
}

pub(crate) fn claude_settings(
    mut doc: Map<String, Value>,
    base: &str,
    key: &str,
    model: Option<&str>,
) -> Map<String, Value> {
    let env = doc
        .entry("env")
        .or_insert_with(|| Value::Object(Map::new()));
    if !env.is_object() {
        *env = Value::Object(Map::new());
    }
    let env = env.as_object_mut().expect("object");
    env.remove("ANTHROPIC_API_KEY");
    env.insert("ANTHROPIC_BASE_URL".into(), json!(base));
    env.insert("ANTHROPIC_AUTH_TOKEN".into(), json!(key));
    if let Some(m) = model {
        env.insert("ANTHROPIC_MODEL".into(), json!(m));
    }
    doc
}

pub(crate) fn opencode_config(
    mut doc: Map<String, Value>,
    base: &str,
    key: &str,
    model: Option<&str>,
) -> Map<String, Value> {
    let providers = doc
        .entry("provider")
        .or_insert_with(|| Value::Object(Map::new()));
    if !providers.is_object() {
        *providers = Value::Object(Map::new());
    }
    let mut models = Map::new();
    for m in ["loadout/default", "loadout/fast", "loadout/realtime"]
        .into_iter()
        .chain(model)
    {
        models.insert(m.to_owned(), json!({ "name": m }));
    }
    providers.as_object_mut().expect("object").insert(
        "loadout".into(),
        json!({
            "npm": "@ai-sdk/openai-compatible",
            "name": "Loadout",
            "options": { "baseURL": format!("{base}/v1"), "apiKey": key },
            "models": models,
        }),
    );
    if let Some(m) = model {
        doc.insert("model".into(), json!(format!("loadout/{m}")));
    }
    doc
}

/// Line-based TOML edit (demo): replaces top-level `model_provider` / `model`, and the whole
/// `[model_providers.loadout]` table; everything else, comments included, is kept.
pub(crate) fn codex_config(text: &str, base: &str, key: &str, model: Option<&str>) -> String {
    let mut top: Vec<String> = Vec::new();
    let mut rest: Vec<String> = Vec::new();
    let mut in_tables = false;
    let mut skipping = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with('[') {
            in_tables = true;
            skipping = t.starts_with("[model_providers.loadout]")
                || t.starts_with("[model_providers.loadout.");
        }
        if skipping {
            continue;
        }
        if !in_tables {
            let k = t.split('=').next().unwrap_or("").trim();
            if k == "model_provider" || (model.is_some() && k == "model") {
                continue;
            }
            top.push(line.to_owned());
        } else {
            rest.push(line.to_owned());
        }
    }
    let mut out = vec!["model_provider = \"loadout\"".to_owned()];
    if let Some(m) = model {
        out.push(format!("model = {}", json!(m)));
    }
    out.extend(top);
    out.extend(rest);
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    out.push(String::new());
    out.push("[model_providers.loadout]".into());
    out.push("name = \"Loadout\"".into());
    out.push(format!("base_url = {}", json!(format!("{base}/v1"))));
    out.push("wire_api = \"responses\"".into());
    out.push(format!("experimental_bearer_token = {}", json!(key)));
    out.join("\n") + "\n"
}

fn points_at_loadout(client: Client, path: &Path) -> &'static str {
    let Ok(text) = std::fs::read_to_string(path) else {
        return "not configured (no file)";
    };
    let hit = match client {
        Client::ClaudeCode => text.contains("ANTHROPIC_BASE_URL") && text.contains("127.0.0.1"),
        Client::Codex => text.contains("[model_providers.loadout]"),
        Client::Opencode => text.contains("\"loadout\""),
    };
    if hit {
        "points at Loadout"
    } else {
        "not configured"
    }
}

pub async fn run(ctx: &Ctx, cmd: SetupCommand) -> Result<(), Failure> {
    let home = home()?;
    match cmd {
        SetupCommand::Status => {
            for c in [Client::ClaudeCode, Client::Codex, Client::Opencode] {
                let p = c.path(&home);
                println!(
                    "{:<12} {:<26} {}",
                    c.name(),
                    points_at_loadout(c, &p),
                    p.display()
                );
            }
            println!(
                "\nOther apps (Continue, Zed, Aider, SDKs, Cline): OpenAI base URL http://127.0.0.1:8317/v1, Anthropic base URL http://127.0.0.1:8317, API key = a client key (`loadout client create <name>`)."
            );
            println!(
                "Cursor cannot reach a local gateway (its requests come from Cursor's servers)."
            );
            Ok(())
        }
        SetupCommand::Restore { client } => {
            let path = client.path(&home);
            let dir = path.parent().ok_or_else(|| Failure::new("bad path"))?;
            let prefix = format!(
                "{}.loadout-backup-",
                path.file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default()
            );
            let newest = std::fs::read_dir(dir)
                .map_err(|e| io(e, dir))?
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with(&prefix))
                })
                .max()
                .ok_or_else(|| {
                    Failure::new(format!("no Loadout backup next to {}", path.display()))
                })?;
            if newest.to_string_lossy().ends_with(".absent") {
                let _ = std::fs::remove_file(&path);
                println!(
                    "Removed {} (it did not exist before Loadout configured it).",
                    path.display()
                );
            } else {
                std::fs::copy(&newest, &path).map_err(|e| io(e, &path))?;
                println!("Restored {} from {}.", path.display(), newest.display());
            }
            Ok(())
        }
        SetupCommand::Apply { client, model } => {
            let path = client.path(&home);
            // Validate before minting a key: never rewrite a file we cannot parse.
            let json_doc = match client {
                Client::ClaudeCode | Client::Opencode => Some(read_json(&path)?),
                Client::Codex => None,
            };
            let mut ipc = ctx.connect().await?;
            let status = ipc.call::<api::Status>(&()).await?;
            let base = format!("http://{}", status.gateway.addr);
            let created = ipc
                .call::<api::ClientCreate>(&ClientCreateParams {
                    name: format!("{}-{}", client.name(), now_secs()),
                })
                .await?;
            let key = created.key.clone();
            let text = match (client, json_doc) {
                (Client::ClaudeCode, Some(doc)) => serde_json::to_string_pretty(&Value::Object(
                    claude_settings(doc, &base, &key, model.as_deref()),
                ))
                .map_err(|e| Failure::new(e.to_string()))?,
                (Client::Opencode, Some(mut doc)) => {
                    doc.entry("$schema")
                        .or_insert_with(|| json!("https://opencode.ai/config.json"));
                    serde_json::to_string_pretty(&Value::Object(opencode_config(
                        doc,
                        &base,
                        &key,
                        model.as_deref(),
                    )))
                    .map_err(|e| Failure::new(e.to_string()))?
                }
                _ => {
                    let old = std::fs::read_to_string(&path).unwrap_or_default();
                    codex_config(&old, &base, &key, model.as_deref())
                }
            };
            let backup = backup(&path)?;
            write_atomic(&path, &text)?;
            println!(
                "Configured {} → {base} (client key \"{}\").",
                client.name(),
                created.client.name
            );
            println!("File:   {}", path.display());
            println!("Backup: {}", backup.display());
            println!("Undo:   loadout setup restore {}", client.name());
            if matches!(client, Client::Codex | Client::Opencode) {
                println!("Restart {} if it is running.", client.name());
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_settings_keep_other_keys_and_drop_api_key() {
        let doc: Map<String, Value> =
            serde_json::from_str(r#"{"theme":"dark","env":{"FOO":"1","ANTHROPIC_API_KEY":"old"}}"#)
                .expect("json");
        let out = claude_settings(doc, "http://127.0.0.1:8317", "lo-key", Some("loadout/fast"));
        assert_eq!(out["theme"], "dark");
        assert_eq!(out["env"]["FOO"], "1");
        assert!(out["env"].get("ANTHROPIC_API_KEY").is_none());
        assert_eq!(out["env"]["ANTHROPIC_BASE_URL"], "http://127.0.0.1:8317");
        assert_eq!(out["env"]["ANTHROPIC_AUTH_TOKEN"], "lo-key");
        assert_eq!(out["env"]["ANTHROPIC_MODEL"], "loadout/fast");
    }

    #[test]
    fn codex_config_keeps_comments_and_replaces_our_table() {
        let old = "# my settings\nmodel_provider = \"openai\"\nmodel = \"gpt-x\"\n\n[model_providers.loadout]\nbase_url = \"old\"\n\n[mcp_servers.x]\ncommand = \"y\"\n";
        let out = codex_config(old, "http://127.0.0.1:8317", "k", None);
        assert!(out.starts_with("model_provider = \"loadout\"\n# my settings\n"));
        assert!(
            out.contains("model = \"gpt-x\""),
            "model kept when not given"
        );
        assert!(!out.contains("model_provider = \"openai\""));
        assert!(!out.contains("base_url = \"old\""));
        assert!(out.contains("[mcp_servers.x]\ncommand = \"y\""));
        assert_eq!(out.matches("[model_providers.loadout]").count(), 1);
        assert!(out.contains("base_url = \"http://127.0.0.1:8317/v1\""));
        assert!(out.contains("wire_api = \"responses\""));
        let again = codex_config(&out, "http://127.0.0.1:8317", "k2", Some("loadout/default"));
        assert_eq!(again.matches("[model_providers.loadout]").count(), 1);
        assert!(again.contains("model = \"loadout/default\""));
        assert!(!again.contains("gpt-x"));
    }

    #[test]
    fn opencode_adds_provider() {
        let out = opencode_config(
            Map::new(),
            "http://127.0.0.1:8317",
            "k",
            Some("loadout/fast"),
        );
        assert_eq!(
            out["provider"]["loadout"]["options"]["baseURL"],
            "http://127.0.0.1:8317/v1"
        );
        assert_eq!(out["model"], "loadout/loadout/fast");
    }
}
