// SPDX-License-Identifier: Apache-2.0

#![allow(unsafe_code)]

use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};

/// Attaches a GUI-subsystem process (such as `loadoutd`, which must not flash a console
/// window at login) to the console of the process that started it, so `--foreground` output
/// is visible in a terminal. Returns `false` when there is no parent console.
pub fn attach_parent_console() -> bool {
    // SAFETY: AttachConsole has no memory-safety preconditions; failure is reported by the
    // return value.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) != 0 }
}
