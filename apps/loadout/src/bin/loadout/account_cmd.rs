// SPDX-License-Identifier: Apache-2.0

use std::io::Read;

use clap::{Args, Subcommand};
use loadout_ipc::api::{
    self, AccountAddParams, AccountEnableParams, AccountInfo, AccountKeyParams, AccountListParams,
    AccountRef, AccountTransportParams, ApiKey, OAuthStartParams, OAuthStatusParams, SyncResult,
};

use crate::{Ctx, Failure, output};

#[derive(Debug, Args)]
pub struct KeyInput {
    /// Read the API key from standard input instead of prompting.
    #[arg(long)]
    api_key_stdin: bool,
}

#[derive(Debug, Subcommand)]
pub enum AccountCommand {
    /// Add an account (you are prompted for its API key; it is stored in Windows Credential
    /// Manager, never in the database).
    Add {
        provider: String,
        /// Short name used in model names, e.g. `main` in `deepseek/main/deepseek-chat`.
        alias: String,
        #[command(flatten)]
        key: KeyInput,
        /// The account has no API key (e.g. a local server).
        #[arg(long, conflicts_with = "api_key_stdin")]
        no_key: bool,
        /// Do not fetch the model list now.
        #[arg(long)]
        no_sync: bool,
        #[arg(long)]
        name: Option<String>,
    },
    /// List accounts.
    List { provider: Option<String> },
    /// Route requests to this account again.
    Enable { provider: String, alias: String },
    /// Stop routing requests to this account.
    Disable { provider: String, alias: String },
    /// Replace the account's API key.
    SetKey {
        provider: String,
        alias: String,
        #[command(flatten)]
        key: KeyInput,
    },
    /// Remove the account, its key and its models.
    Remove { provider: String, alias: String },
    /// Sign in to an account that uses a Loadout-managed login (e.g. `codex-sub`): opens a
    /// device-code sign-in; Loadout keeps and refreshes the tokens.
    Login { provider: String, alias: String },
    /// Allow or stop routing through a lower-trust transport (e.g. a ChatGPT subscription,
    /// ADR-012). Turning it on asks for two confirmations.
    Transport {
        provider: String,
        alias: String,
        #[arg(value_parser = ["on", "off"])]
        state: String,
        /// Non-interactive: accept the risk notice.
        #[arg(long)]
        i_understand_the_risk: bool,
        /// Non-interactive: the account alias, as the second confirmation.
        #[arg(long)]
        confirm: Option<String>,
    },
    /// Record the subscription plan, monthly price and next renewal date (for the usage
    /// view and subscription value statistics).
    Plan {
        provider: String,
        alias: String,
        /// Plan name, e.g. "Pro".
        #[arg(long)]
        name: Option<String>,
        /// Monthly price in USD.
        #[arg(long)]
        price: Option<f64>,
        /// Next renewal date, YYYY-MM-DD.
        #[arg(long)]
        renews: Option<String>,
    },
    /// Test the account's key with the provider's model list call: ok, invalid key, account
    /// disabled, rate limited, no credit or unreachable. A successful check also lifts an
    /// `AUTH_FAILED` state.
    Check {
        provider: String,
        alias: String,
        /// Also send a 1-token request to every enabled model (uses a little quota) and
        /// check that its response stream ends cleanly.
        #[arg(long)]
        models: bool,
    },
    /// Keep a quota reserve for this account: when a usage window has less than N percent
    /// left, requests stop going to it until the window resets. Overrides the global
    /// `routing.quota_reserve_percent`. Without options, shows the current reserve.
    Reserve {
        provider: String,
        alias: String,
        /// Reserve for every window without its own value.
        #[arg(long, value_name = "PERCENT")]
        default: Option<f64>,
        /// Reserve for one window, e.g. `--window 5h=10 --window weekly=20`.
        #[arg(long = "window", value_name = "SCOPE=PERCENT")]
        windows: Vec<String>,
        /// Remove the account's reserve (the global setting applies again).
        #[arg(long, conflicts_with_all = ["default", "windows"])]
        clear: bool,
    },
}

fn check_summary(status: &str) -> &'static str {
    match status {
        "ok" => "the key works",
        "invalid_key" => "the key was rejected",
        "account_disabled" => "the account is disabled, suspended or banned",
        "rate_limited" => "rate limited right now",
        "no_credit" => "out of credit or quota",
        "unreachable" => "the provider could not be reached",
        "unsupported" => "this provider has no model list call",
        "missing_key" => "no API key is stored",
        _ => "the provider returned an error",
    }
}

async fn check(ctx: &Ctx, provider: String, alias: String, models: bool) -> Result<(), Failure> {
    let mut ipc = ctx.connect().await?;
    let r = ipc
        .call::<api::AccountCheck>(&api::AccountCheckParams {
            provider,
            alias,
            models,
        })
        .await?;
    if ctx.json {
        return output::json(&r);
    }
    let detail = match (&r.http_status, &r.message) {
        (Some(code), Some(m)) => format!(" (HTTP {code}: {m})"),
        (None, Some(m)) => format!(" ({m})"),
        _ => String::new(),
    };
    println!(
        "{}: {} — {}{detail}",
        r.account,
        r.status,
        check_summary(&r.status)
    );
    if let Some(n) = r.models_listed {
        println!("The provider lists {n} models.");
    }
    if r.cleared {
        println!("Cleared the account's AUTH_FAILED state; requests are routed to it again.");
    }
    if models {
        if r.models.is_empty() {
            println!("No enabled models are routed through this account.");
        } else {
            let rows: Vec<Vec<String>> = r
                .models
                .iter()
                .map(|m| {
                    vec![
                        m.name.clone(),
                        m.protocol.clone(),
                        if m.ok { "ok" } else { "error" }.to_owned(),
                        m.ttft_ms.map_or_else(|| "-".into(), |t| format!("{t} ms")),
                        m.stream.clone().unwrap_or_else(|| "-".into()),
                        m.error.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            output::table(
                &["MODEL", "PROTOCOL", "RESULT", "TTFT", "STREAM", "ERROR"],
                &rows,
            );
        }
    }
    let models_ok = r.models.iter().all(|m| m.ok);
    if (r.status == "ok" || r.status == "unsupported") && models_ok {
        Ok(())
    } else {
        Err(Failure::new("the check found a problem"))
    }
}


/// The `account.set_reserve` value for the CLI options: `Ok(None)` = nothing to set (show),
/// `Ok(Some(None))` = clear.
fn reserve_value(
    default: Option<f64>,
    windows: &[String],
    clear: bool,
) -> Result<Option<Option<serde_json::Value>>, Failure> {
    if clear {
        return Ok(Some(None));
    }
    if default.is_none() && windows.is_empty() {
        return Ok(None);
    }
    let mut map = serde_json::Map::new();
    if let Some(d) = default {
        map.insert("default".into(), d.into());
    }
    for w in windows {
        let (scope, n) = w
            .split_once('=')
            .ok_or_else(|| Failure::new(format!("expected SCOPE=PERCENT, not {w:?}")))?;
        let n: f64 = n
            .trim()
            .parse()
            .map_err(|_| Failure::new(format!("invalid percentage in {w:?}")))?;
        map.insert(scope.trim().to_owned(), n.into());
    }
    Ok(Some(Some(serde_json::Value::Object(map))))
}

fn describe_reserve(info: &AccountInfo) -> String {
    match info.quota_reserve.as_ref().and_then(|v| v.as_object()) {
        Some(map) if !map.is_empty() => map
            .iter()
            .map(|(k, v)| format!("{k}={}%", v.as_f64().unwrap_or_default()))
            .collect::<Vec<_>>()
            .join(", "),
        _ => "global setting (routing.quota_reserve_percent)".to_owned(),
    }
}

fn read_key(input: &KeyInput, what: &str) -> Result<Option<String>, Failure> {
    let raw = if input.api_key_stdin {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| Failure::new(format!("reading stdin: {e}")))?;
        s
    } else {
        loadout_platform::read_hidden_line(&format!("API key for {what} (input hidden): "))
            .map_err(|e| Failure::new(format!("reading the key: {e}")))?
    };
    let key = raw.trim().to_owned();
    Ok((!key.is_empty()).then_some(key))
}

const RISK_NOTICE: &str = "\
WARNING — subscription transport (ADR-012)
  * This is not an official API. Requests go to an undocumented endpoint of the
    subscription service with a login Loadout obtained for you.
  * The provider's terms may not allow it. The provider can limit, suspend or close
    the account, without notice. You take that risk; Loadout cannot undo it.
  * Loadout identifies itself honestly, limits each account to a few concurrent
    requests, and keeps it out of loadout/default — but cannot make this safe.
  Details: docs/providers/codex-subscription.md";

fn ask(prompt: &str) -> Result<String, Failure> {
    use std::io::Write;
    print!("{prompt}");
    std::io::stdout()
        .flush()
        .map_err(|e| Failure::new(format!("writing to the console: {e}")))?;
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| Failure::new(format!("reading the answer: {e}")))?;
    Ok(line.trim().to_owned())
}

/// The two confirmation steps (ADR-012 §1); returns the confirmed alias.
fn confirm_transport(
    alias: &str,
    accepted: bool,
    confirm: Option<String>,
) -> Result<String, Failure> {
    eprintln!("{RISK_NOTICE}\n");
    if !accepted && ask("Type \"I understand\" to continue: ")? != "I understand" {
        return Err(Failure::new("not confirmed; nothing changed"));
    }
    let typed = match confirm {
        Some(c) => c,
        None => ask(&format!("Type the account alias ({alias}) to turn it on: "))?,
    };
    if typed != alias {
        return Err(Failure::new("the alias did not match; nothing changed"));
    }
    Ok(typed)
}

async fn login(ctx: &Ctx, provider: String, alias: String) -> Result<(), Failure> {
    let mut ipc = ctx.connect().await?;
    let start = ipc
        .call::<api::AccountOAuthStart>(&OAuthStartParams {
            provider: provider.clone(),
            alias: alias.clone(),
        })
        .await?;
    if !ctx.json {
        println!("Open {} in your browser", start.verification_url);
        println!("and enter the code:  {}", start.user_code);
        println!(
            "Waiting for the sign-in (up to {} minutes)…",
            start.expires_in_secs / 60
        );
    }
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let status = ipc
            .call::<api::AccountOAuthStatus>(&OAuthStatusParams {
                login_id: start.login_id.clone(),
            })
            .await?;
        match status.state.as_str() {
            "pending" => continue,
            "done" => {
                if ctx.json {
                    return output::json(&status);
                }
                let a = status.account.ok_or_else(|| Failure::new("no account"))?;
                println!(
                    "Signed in: {}/{}{} ({} models).",
                    a.provider,
                    a.alias,
                    a.display_name
                        .map(|n| format!(" — {n}"))
                        .unwrap_or_default(),
                    a.models
                );
                if !a.transport_opt_in {
                    println!(
                        "Requests are not routed through it yet. To allow that: loadout account transport {} {} on",
                        a.provider, a.alias
                    );
                }
                return Ok(());
            }
            _ => {
                return Err(Failure::new(format!(
                    "sign-in failed: {}",
                    status.message.unwrap_or_default()
                )));
            }
        }
    }
}

pub fn print_sync(sync: &SyncResult) {
    println!(
        "Models: {} total ({} new, {} updated, {} no longer offered).",
        sync.total, sync.added, sync.updated, sync.disabled
    );
    if !sync.ambiguous.is_empty() {
        println!(
            "Warning: these model ids start with an account alias and may be ambiguous: {}",
            sync.ambiguous.join(", ")
        );
    }
}

fn print_accounts(list: &[AccountInfo]) {
    if list.is_empty() {
        println!("No accounts. Add one with `loadout account add <provider> <alias>`.");
        return;
    }
    let rows: Vec<Vec<String>> = list
        .iter()
        .map(|a| {
            vec![
                format!("{}/{}", a.provider, a.alias),
                if a.transport_opt_in {
                    format!("{} (transport on)", a.account_type)
                } else {
                    a.account_type.clone()
                },
                if a.enabled { "enabled" } else { "disabled" }.to_owned(),
                if a.has_key { "yes" } else { "no" }.to_owned(),
                format!("{}/{}", a.enabled_models, a.models),
            ]
        })
        .collect();
    output::table(&["ACCOUNT", "TYPE", "STATUS", "KEY", "MODELS"], &rows);
}

pub async fn run(ctx: &Ctx, cmd: AccountCommand) -> Result<(), Failure> {
    // Read secrets before connecting so a slow prompt does not hold a pipe instance.
    match cmd {
        AccountCommand::Add {
            provider,
            alias,
            key,
            no_key,
            no_sync,
            name,
        } => {
            let api_key = if no_key {
                None
            } else {
                read_key(&key, &format!("{provider}/{alias}"))?
            };
            let mut ipc = ctx.connect().await?;
            let added = ipc
                .call::<api::AccountAdd>(&AccountAddParams {
                    provider,
                    alias,
                    api_key: api_key.map(ApiKey),
                    display_name: name,
                    sync_models: !no_sync,
                })
                .await?;
            if ctx.json {
                return output::json(&added);
            }
            println!(
                "Added account {}/{}.",
                added.account.provider, added.account.alias
            );
            if let Some(sync) = &added.sync {
                print_sync(sync);
            }
            if let Some(err) = &added.sync_error {
                println!("Could not fetch models: {err}");
                println!(
                    "Add models manually with `loadout model add {} {} <model>`.",
                    added.account.provider, added.account.alias
                );
            }
            Ok(())
        }
        AccountCommand::SetKey {
            provider,
            alias,
            key,
        } => {
            let api_key = read_key(&key, &format!("{provider}/{alias}"))?
                .ok_or_else(|| Failure::new("no API key given"))?;
            let mut ipc = ctx.connect().await?;
            let info = ipc
                .call::<api::AccountSetKey>(&AccountKeyParams {
                    provider,
                    alias,
                    api_key: ApiKey(api_key),
                })
                .await?;
            if ctx.json {
                return output::json(&info);
            }
            println!("Updated the API key of {}/{}.", info.provider, info.alias);
            Ok(())
        }
        AccountCommand::List { provider } => {
            let mut ipc = ctx.connect().await?;
            let list = ipc
                .call::<api::AccountList>(&AccountListParams { provider })
                .await?;
            if ctx.json {
                return output::json(&list);
            }
            print_accounts(&list);
            Ok(())
        }
        AccountCommand::Enable { provider, alias } => set_enabled(ctx, provider, alias, true).await,
        AccountCommand::Disable { provider, alias } => {
            set_enabled(ctx, provider, alias, false).await
        }
        AccountCommand::Plan {
            provider,
            alias,
            name,
            price,
            renews,
        } => {
            let renews_at_ms =
                match renews {
                    Some(d) => Some(output::parse_date(&d).ok_or_else(|| {
                        Failure::new(format!("invalid date {d:?}; use YYYY-MM-DD"))
                    })?),
                    None => None,
                };
            let mut ipc = ctx.connect().await?;
            let info = ipc
                .call::<api::AccountSetPlan>(&api::AccountPlanParams {
                    provider,
                    alias,
                    name,
                    price_usd_monthly: price,
                    renews_at_ms,
                })
                .await?;
            if ctx.json {
                return output::json(&info);
            }
            println!("Saved the plan of {}/{}.", info.provider, info.alias);
            Ok(())
        }
        AccountCommand::Reserve {
            provider,
            alias,
            default,
            windows,
            clear,
        } => {
            let value = reserve_value(default, &windows, clear)?;
            let mut ipc = ctx.connect().await?;
            let info = match value {
                Some(reserve) => {
                    ipc.call::<api::AccountSetReserve>(&api::AccountReserveParams {
                        provider,
                        alias,
                        reserve,
                    })
                    .await?
                }
                None => ipc
                    .call::<api::AccountList>(&AccountListParams {
                        provider: Some(provider.clone()),
                    })
                    .await?
                    .into_iter()
                    .find(|a| a.alias == alias)
                    .ok_or_else(|| Failure::new(format!("account {provider}/{alias} not found")))?,
            };
            if ctx.json {
                return output::json(&info);
            }
            println!(
                "Quota reserve of {}/{}: {}",
                info.provider,
                info.alias,
                describe_reserve(&info)
            );
            Ok(())
        }
        AccountCommand::Login { provider, alias } => login(ctx, provider, alias).await,
        AccountCommand::Check {
            provider,
            alias,
            models,
        } => check(ctx, provider, alias, models).await,
        AccountCommand::Transport {
            provider,
            alias,
            state,
            i_understand_the_risk,
            confirm,
        } => {
            let enabled = state == "on";
            let confirm = if enabled {
                Some(confirm_transport(&alias, i_understand_the_risk, confirm)?)
            } else {
                None
            };
            let mut ipc = ctx.connect().await?;
            let info = ipc
                .call::<api::AccountSetTransport>(&AccountTransportParams {
                    provider,
                    alias,
                    enabled,
                    confirm,
                })
                .await?;
            if ctx.json {
                return output::json(&info);
            }
            if info.transport_opt_in {
                println!(
                    "Routing through {}/{} is on. Use exact names such as {}/{}/<model>; it is not part of loadout/default.",
                    info.provider, info.alias, info.provider, info.alias
                );
            } else {
                println!("Routing through {}/{} is off.", info.provider, info.alias);
            }
            Ok(())
        }
        AccountCommand::Remove { provider, alias } => {
            let mut ipc = ctx.connect().await?;
            let r = ipc
                .call::<api::AccountRemove>(&AccountRef {
                    provider: provider.clone(),
                    alias: alias.clone(),
                })
                .await?;
            if ctx.json {
                return output::json(&r);
            }
            if r.revoked {
                println!("Removed {provider}/{alias} with its key and models.");
                Ok(())
            } else {
                Err(Failure::new(format!("no account {provider}/{alias}")))
            }
        }
    }
}

async fn set_enabled(
    ctx: &Ctx,
    provider: String,
    alias: String,
    enabled: bool,
) -> Result<(), Failure> {
    let mut ipc = ctx.connect().await?;
    let info = ipc
        .call::<api::AccountSetEnabled>(&AccountEnableParams {
            provider,
            alias,
            enabled,
        })
        .await?;
    if ctx.json {
        return output::json(&info);
    }
    println!(
        "{}/{} is now {}.",
        info.provider,
        info.alias,
        if info.enabled { "enabled" } else { "disabled" }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserve_options_become_ipc_values() {
        assert!(reserve_value(None, &[], false).expect("show").is_none());
        assert_eq!(reserve_value(None, &[], true).expect("clear"), Some(None));
        assert_eq!(
            reserve_value(Some(10.0), &["5h=5".into(), " weekly = 20 ".into()], false)
                .expect("set"),
            Some(Some(
                serde_json::json!({"default": 10.0, "5h": 5.0, "weekly": 20.0})
            ))
        );
        assert!(reserve_value(None, &["weekly".into()], false).is_err());
        assert!(reserve_value(None, &["weekly=x".into()], false).is_err());
    }
}
