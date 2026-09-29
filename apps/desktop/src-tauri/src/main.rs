// SPDX-License-Identifier: Apache-2.0

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    loadout_desktop::run();
}
