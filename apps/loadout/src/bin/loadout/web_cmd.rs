// SPDX-License-Identifier: Apache-2.0

//! `loadout account web …`: browser-session accounts (ADR-014, ExecPlan 017 Stage 6).

use clap::Subcommand;
use loadout_ipc::api::{
    self, AccountRef, WebAddParams, WebBindParams, WebConfigParams, WebSessionStatus,
};

use crate::account_cmd::ask;
use crate::{Ctx, Failure, output};

#[derive(Debug, Subcommand)]
pub enum WebCommand {
    /// Add a browser-session account (e.g. `web-chatgpt`). Shows the risks, asks for two
    /// confirmations, then opens a dedicated Edge window where you sign in yourself.
    Add {
        provider: String,
        alias: String,
        /// Non-interactive: accept the risk notice.
        #[arg(long)]
        i_understand_the_risk: bool,
        /// Non-interactive: the account alias, as the second confirmation.
        #[arg(long)]
        confirm: Option<String>,
        #[arg(long)]
        name: Option<String>,
    },
    /// State of the session: ready, busy, needs attention, cooling down…
    Status { provider: String, alias: String },
    /// Bring the account's browser window to the front (sign in, solve a verification page,
    /// or watch the conversation).
    Show { provider: String, alias: String },
    /// Check the page again now, e.g. after signing in or completing a verification page.
    Recheck { provider: String, alias: String },
    /// Use this conversation when a request names none (paste its URL), or `--clear` to let
    /// the next request open a new one.
    Bind {
        provider: String,
        alias: String,
        url: Option<String>,
        #[arg(long, conflicts_with = "url")]
        clear: bool,
    },
    /// Conversation mode and limits.
    Config {
        provider: String,
        alias: String,
        /// `persistent` (continue one conversation) or `stateless` (a temporary
        /// conversation per request, holding the whole history).
        #[arg(long, value_parser = ["persistent", "stateless"])]
        mode: Option<String>,
        /// Minimum seconds between two requests.
        #[arg(long, value_name = "SECONDS")]
        min_interval: Option<u32>,
        /// Requests per day (UTC) after which the account is skipped; 0 = no cap.
        #[arg(long, value_name = "N")]
        daily_cap: Option<u32>,
    },
}

pub(crate) const RISK_NOTICE: &str = "\
WARNING — browser session (ADR-014)
  * Loadout types your requests into the site's own web page in a dedicated Edge window
    and reads the page's answers. This is not an API.
  * The site's terms may forbid automated use. OpenAI's Terms of Use say you may not
    \"automatically or programmatically extract data or Output\". The site can limit,
    suspend or close the account, without notice. You take that risk; Loadout cannot
    undo it.
  * Loadout uses an unmodified browser, does not hide automation, never solves
    verification pages and never reads your cookies. Some sites may therefore not work.
  * Conversations are saved in the site's history like any you type yourself.
  * One request at a time per account, a minimum interval and a daily cap apply; the
    account is never part of loadout/default.
  Details: docs/providers/web-chatgpt.md";

/// The two confirmation steps (ADR-014 §8); returns the confirmed alias.
pub(crate) fn confirm_web(
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
        None => ask(&format!("Type the account alias ({alias}) to confirm: "))?,
    };
    if typed != alias {
        return Err(Failure::new("the alias did not match; nothing changed"));
    }
    Ok(typed)
}

fn describe_state(s: &WebSessionStatus) -> String {
    match s.state.as_str() {
        "ready" => "ready".into(),
        "busy" => "busy (answering, or the window is in use)".into(),
        "starting" => "starting the browser".into(),
        "stopped" if !s.enabled_globally => "off (web_session.enabled is false)".into(),
        "stopped" => "not running (the daemon starts it shortly)".into(),
        "cooling" => format!(
            "cooling down until {}",
            output::timestamp(s.next_allowed_ms)
        ),
        "needs_attention" => format!(
            "needs you: {}",
            s.attention.as_deref().unwrap_or("open the browser window")
        ),
        other => other.to_owned(),
    }
}

pub(crate) fn print_status(s: &WebSessionStatus) {
    println!("{}/{}: {}", s.provider, s.alias, describe_state(s));
    println!(
        "  mode {}, minimum interval {} s, daily cap {}, {} request(s) today{}",
        s.mode,
        s.min_interval_secs,
        if s.daily_soft_cap == 0 {
            "none".to_owned()
        } else {
            s.daily_soft_cap.to_string()
        },
        s.today_count,
        if s.headless { ", headless" } else { "" }
    );
    match &s.default_conversation {
        Some(url) => println!(
            "  default conversation: {url}{}",
            match s.bound_by.as_deref() {
                Some("auto") => " (opened by the first request)",
                _ => "",
            }
        ),
        None => println!("  default conversation: none yet (the next request opens one)"),
    }
}

pub async fn run(ctx: &Ctx, cmd: WebCommand) -> Result<(), Failure> {
    match cmd {
        WebCommand::Add {
            provider,
            alias,
            i_understand_the_risk,
            confirm,
            name,
        } => {
            let confirm = confirm_web(&alias, i_understand_the_risk, confirm)?;
            let mut ipc = ctx.connect().await?;
            let info = ipc
                .call::<api::AccountWebAdd>(&WebAddParams {
                    provider: provider.clone(),
                    alias: alias.clone(),
                    confirm,
                    display_name: name,
                })
                .await?;
            if ctx.json {
                return output::json(&info);
            }
            println!("Added {}/{}.", info.provider, info.alias);
            println!(
                "A dedicated Edge window opens in a few seconds: sign in there yourself. Then \
                 `loadout account web status {provider} {alias}` should say \"ready\"."
            );
            println!(
                "Use it as {}/{}/auto (the page's default model) or another model from \
                 `loadout model list`.",
                info.provider, info.alias
            );
            Ok(())
        }
        WebCommand::Status { provider, alias } => {
            let mut ipc = ctx.connect().await?;
            let s = ipc
                .call::<api::AccountWebStatus>(&AccountRef { provider, alias })
                .await?;
            if ctx.json {
                return output::json(&s);
            }
            print_status(&s);
            Ok(())
        }
        WebCommand::Show { provider, alias } => {
            let mut ipc = ctx.connect().await?;
            let s = ipc
                .call::<api::AccountWebShow>(&AccountRef { provider, alias })
                .await?;
            if ctx.json {
                return output::json(&s);
            }
            print_status(&s);
            Ok(())
        }
        WebCommand::Recheck { provider, alias } => {
            let mut ipc = ctx.connect().await?;
            let s = ipc
                .call::<api::AccountWebRecheck>(&AccountRef { provider, alias })
                .await?;
            if ctx.json {
                return output::json(&s);
            }
            print_status(&s);
            Ok(())
        }
        WebCommand::Bind {
            provider,
            alias,
            url,
            clear,
        } => {
            if url.is_none() && !clear {
                return Err(Failure::new("give a conversation URL, or --clear"));
            }
            let mut ipc = ctx.connect().await?;
            let s = ipc
                .call::<api::AccountWebBind>(&WebBindParams {
                    provider,
                    alias,
                    conversation_url: url,
                })
                .await?;
            if ctx.json {
                return output::json(&s);
            }
            print_status(&s);
            Ok(())
        }
        WebCommand::Config {
            provider,
            alias,
            mode,
            min_interval,
            daily_cap,
        } => {
            let mut ipc = ctx.connect().await?;
            let s = ipc
                .call::<api::AccountWebConfig>(&WebConfigParams {
                    provider,
                    alias,
                    mode,
                    min_interval_secs: min_interval,
                    daily_soft_cap: daily_cap,
                })
                .await?;
            if ctx.json {
                return output::json(&s);
            }
            print_status(&s);
            Ok(())
        }
    }
}
