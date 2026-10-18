// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(windows, windows_subsystem = "windows")]

//! Launch a user-selected local helper with the same handle isolation as the daemon.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let program = args.next().ok_or("expected executable path")?;
    let args: Vec<_> = args.collect();
    let refs: Vec<_> = args.iter().map(std::ffi::OsString::as_os_str).collect();
    let child = loadout_platform::spawn_detached(std::path::Path::new(&program), &refs)?;
    println!("{}", child.pid());
    Ok(())
}
