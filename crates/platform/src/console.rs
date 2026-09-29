// SPDX-License-Identifier: Apache-2.0

#![allow(unsafe_code)]

use std::io::{self, BufRead, Write};

use windows_sys::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AttachConsole, CONSOLE_MODE, ENABLE_ECHO_INPUT, GetConsoleMode,
    GetStdHandle, STD_INPUT_HANDLE, SetConsoleMode,
};

/// Attaches a GUI-subsystem process (such as `loadoutd`, which must not flash a console
/// window at login) to the console of the process that started it, so `--foreground` output
/// is visible in a terminal. Returns `false` when there is no parent console.
pub fn attach_parent_console() -> bool {
    // SAFETY: AttachConsole has no memory-safety preconditions; failure is reported by the
    // return value.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) != 0 }
}

/// Reads one line without echoing it when stdin is a console (for API keys). When stdin is
/// redirected, reads the line normally. The prompt goes to stderr.
pub fn read_hidden_line(prompt: &str) -> io::Result<String> {
    // SAFETY: GetStdHandle has no preconditions.
    let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    let mut mode: CONSOLE_MODE = 0;
    // SAFETY: `mode` is a valid out-pointer; failure means stdin is not a console.
    let is_console = !handle.is_null() && unsafe { GetConsoleMode(handle, &mut mode) } != 0;

    eprint!("{prompt}");
    io::stderr().flush()?;
    if is_console {
        // SAFETY: `handle` is the console input handle whose mode we just read.
        unsafe { SetConsoleMode(handle, mode & !ENABLE_ECHO_INPUT) };
    }
    let mut line = String::new();
    let result = io::stdin().lock().read_line(&mut line);
    if is_console {
        // SAFETY: restores the mode read above.
        unsafe { SetConsoleMode(handle, mode) };
        eprintln!();
    }
    result?;
    Ok(line.trim_end_matches(['\r', '\n']).to_owned())
}
