// SPDX-License-Identifier: Apache-2.0

use clap::Subcommand;
use loadout_ipc::api::{self, ProviderAddParams, SlugParams};
use serde_json::{Map, Value};

use crate::{Ctx, Failure, output};

#[derive(Debug, Subcommand)]
pub enum ProviderCommand {
    /// List the provider types this build supports.
    Kinds,
    /// Add a provider.
    ///
    /// Example: loadout provider add deepseek --kind openai-compatible --base-url https://api.deepseek.com/v1
    Add {
        /// Short name used in model names, e.g. `deepseek` in `deepseek/main/deepseek-chat`.
        slug: String,
        /// Provider type (see `loadout provider kinds`).
        #[arg(long)]
        kind: String,
        /// API base URL including the version path, e.g. https://api.example.com/v1
        #[arg(long)]
        base_url: Option<String>,
        /// Provider setting as key=value (repeatable). JSON values are accepted.
        #[arg(long = "setting", value_name = "KEY=VALUE")]
        settings: Vec<String>,
        /// Allow plain http to a non-local host.
        #[arg(long)]
        allow_insecure_http: bool,
        /// Display name.
        #[arg(long)]
        name: Option<String>,
    },
    /// List providers.
    List,
    /// Remove a provider with all its accounts, keys and models.
    Remove { slug: String },
}

fn parse_settings(pairs: &[String], allow_insecure_http: bool) -> Result<Value, Failure> {
    let mut map = Map::new();
    for pair in pairs {
        let (k, v) = pair
            .split_once('=')
            .ok_or_else(|| Failure::new(format!("setting {pair:?} must be KEY=VALUE")))?;
        let value = serde_json::from_str(v).unwrap_or_else(|_| Value::String(v.to_owned()));
        map.insert(k.trim().to_owned(), value);
    }
    if allow_insecure_http {
        map.insert("allow_insecure_http".into(), Value::Bool(true));
    }
    Ok(Value::Object(map))
}

pub async fn run(ctx: &Ctx, cmd: ProviderCommand) -> Result<(), Failure> {
    let mut ipc = ctx.connect().await?;
    match cmd {
        ProviderCommand::Kinds => {
            let kinds = ipc.call::<api::ProviderKinds>(&()).await?;
            if ctx.json {
                return output::json(&kinds);
            }
            let rows: Vec<Vec<String>> = kinds
                .iter()
                .map(|k| {
                    vec![
                        k.kind.clone(),
                        k.display_name.clone(),
                        k.protocols.join(","),
                        k.auth.clone(),
                        k.default_base_url
                            .clone()
                            .unwrap_or_else(|| "(required)".into()),
                    ]
                })
                .collect();
            output::table(&["KIND", "NAME", "PROTOCOLS", "AUTH", "BASE URL"], &rows);
            Ok(())
        }
        ProviderCommand::Add {
            slug,
            kind,
            base_url,
            settings,
            allow_insecure_http,
            name,
        } => {
            let info = ipc
                .call::<api::ProviderAdd>(&ProviderAddParams {
                    slug,
                    kind,
                    base_url,
                    settings: parse_settings(&settings, allow_insecure_http)?,
                    display_name: name,
                })
                .await?;
            if ctx.json {
                return output::json(&info);
            }
            println!("Added provider {} ({}).", info.slug, info.kind);
            println!("Next: loadout account add {} <alias>", info.slug);
            Ok(())
        }
        ProviderCommand::List => {
            let list = ipc.call::<api::ProviderList>(&()).await?;
            if ctx.json {
                return output::json(&list);
            }
            if list.is_empty() {
                println!("No providers. Add one with `loadout provider add`.");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = list
                .iter()
                .map(|p| {
                    vec![
                        p.slug.clone(),
                        p.kind.clone(),
                        p.base_url.clone().unwrap_or_default(),
                        p.accounts.to_string(),
                    ]
                })
                .collect();
            output::table(&["SLUG", "KIND", "BASE URL", "ACCOUNTS"], &rows);
            Ok(())
        }
        ProviderCommand::Remove { slug } => {
            let r = ipc
                .call::<api::ProviderRemove>(&SlugParams { slug: slug.clone() })
                .await?;
            if ctx.json {
                return output::json(&r);
            }
            if r.revoked {
                println!("Removed provider {slug} and its accounts, keys and models.");
                Ok(())
            } else {
                Err(Failure::new(format!("no provider named {slug}")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_accept_json_and_strings() {
        let v = parse_settings(
            &[
                "max_tokens_field=max_completion_tokens".into(),
                r#"model_protocols=["chat","responses"]"#.into(),
                "default_max_tokens=32000".into(),
            ],
            true,
        )
        .expect("settings");
        assert_eq!(v["max_tokens_field"], "max_completion_tokens");
        assert_eq!(v["model_protocols"][1], "responses");
        assert_eq!(v["default_max_tokens"], 32000);
        assert_eq!(v["allow_insecure_http"], true);
        assert!(parse_settings(&["novalue".into()], false).is_err());
    }
}
