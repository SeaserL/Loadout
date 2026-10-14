// SPDX-License-Identifier: Apache-2.0

use clap::Subcommand;
use loadout_ipc::api::{self, Setting, SettingKeyParams, SettingSetParams};

use crate::{Ctx, Failure, output};

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// List all settings.
    List,
    /// Show one setting.
    Get { key: String },
    /// Change a setting.
    Set { key: String, value: String },
    /// Restore a setting's default.
    Reset { key: String },
}

pub async fn run(ctx: &Ctx, cmd: ConfigCommand) -> Result<(), Failure> {
    let mut ipc = ctx.connect().await?;
    match cmd {
        ConfigCommand::List => {
            let all = ipc.call::<api::SettingsList>(&()).await?;
            if ctx.json {
                return output::json(&all);
            }
            let rows: Vec<Vec<String>> = all
                .iter()
                .map(|s| {
                    vec![
                        s.key.clone(),
                        display_value(&s.value),
                        if s.is_default { "default" } else { "custom" }.to_owned(),
                        s.description.clone(),
                    ]
                })
                .collect();
            output::table(&["KEY", "VALUE", "SOURCE", "DESCRIPTION"], &rows);
            Ok(())
        }
        ConfigCommand::Get { key } => {
            let all = ipc.call::<api::SettingsList>(&()).await?;
            let setting = all
                .into_iter()
                .find(|s| s.key == key)
                .ok_or_else(|| Failure::new(format!("unknown setting {key}")))?;
            if ctx.json {
                output::json(&setting)
            } else {
                println!("{}", setting.value);
                Ok(())
            }
        }
        ConfigCommand::Set { key, value } => {
            if key == "web_session.enabled" && value.trim().eq_ignore_ascii_case("true") {
                eprintln!("{}\n", crate::web_cmd::RISK_NOTICE);
            }
            let setting = ipc
                .call::<api::SettingsSet>(&SettingSetParams { key, value })
                .await?;
            report(ctx, &setting)
        }
        ConfigCommand::Reset { key } => {
            let setting = ipc
                .call::<api::SettingsReset>(&SettingKeyParams { key })
                .await?;
            report(ctx, &setting)
        }
    }
}

fn display_value(value: &str) -> String {
    if value.is_empty() {
        "(empty)".into()
    } else {
        value.to_owned()
    }
}

fn report(ctx: &Ctx, setting: &Setting) -> Result<(), Failure> {
    if ctx.json {
        return output::json(setting);
    }
    println!("{} = {}", setting.key, display_value(&setting.value));
    if setting.restart_required {
        println!("Takes effect after `loadout daemon stop` and `loadout daemon start`.");
    }
    Ok(())
}
