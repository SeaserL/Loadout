// SPDX-License-Identifier: Apache-2.0

use clap::Subcommand;
use loadout_platform::{Autostart, AutostartStatus};
use serde_json::json;

use crate::{Ctx, Failure, daemon_exe, output};

#[derive(Debug, Subcommand)]
pub enum AutostartCommand {
    /// Start loadoutd when you sign in.
    Enable {
        #[arg(long, hide = true)]
        value_name: Option<String>,
    },
    /// Stop starting loadoutd at sign-in.
    Disable {
        #[arg(long, hide = true)]
        value_name: Option<String>,
    },
    /// Show the current autostart registration.
    Status {
        #[arg(long, hide = true)]
        value_name: Option<String>,
    },
}

fn autostart(value_name: Option<String>) -> Autostart {
    value_name.map_or_else(Autostart::new, Autostart::with_value_name)
}

fn platform(e: loadout_platform::PlatformError) -> Failure {
    Failure::new(e.to_string())
}

pub fn run(ctx: &Ctx, cmd: AutostartCommand) -> Result<(), Failure> {
    match cmd {
        AutostartCommand::Enable { value_name } => {
            let exe = daemon_exe()?;
            autostart(value_name)
                .enable(&exe, &["--background"])
                .map_err(platform)?;
            if ctx.json {
                output::json(&json!({ "enabled": true, "executable": exe }))
            } else {
                println!("Loadout will start when you sign in ({}).", exe.display());
                Ok(())
            }
        }
        AutostartCommand::Disable { value_name } => {
            let removed = autostart(value_name).disable().map_err(platform)?;
            if ctx.json {
                output::json(&json!({ "enabled": false, "was_enabled": removed }))
            } else {
                println!(
                    "{}",
                    if removed {
                        "Autostart disabled."
                    } else {
                        "Autostart was not enabled."
                    }
                );
                Ok(())
            }
        }
        AutostartCommand::Status { value_name } => {
            match autostart(value_name).status().map_err(platform)? {
                AutostartStatus::Disabled => {
                    if ctx.json {
                        output::json(&json!({ "enabled": false }))
                    } else {
                        println!("Autostart is disabled.");
                        Ok(())
                    }
                }
                AutostartStatus::Enabled {
                    command,
                    executable_exists,
                } => {
                    if ctx.json {
                        return output::json(&json!({
                            "enabled": true,
                            "command": command,
                            "executable_exists": executable_exists,
                        }));
                    }
                    println!("Autostart is enabled: {command}");
                    if !executable_exists {
                        println!(
                            "Warning: that executable no longer exists. Run `loadout autostart enable` again."
                        );
                    }
                    Ok(())
                }
            }
        }
    }
}
