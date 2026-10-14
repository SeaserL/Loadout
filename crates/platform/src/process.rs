// SPDX-License-Identifier: Apache-2.0

//! Starting `loadoutd` fully detached from the caller.
//!
//! `std::process::Command` creates children with handle inheritance enabled, so a daemon
//! started from `loadout daemon start | more` would inherit the pipe feeding `more` and keep
//! it open forever — the caller's pipeline never finishes. [`spawn_detached`] calls
//! `CreateProcessW` with inheritance disabled, so the daemon holds no handle of its parent.

#![allow(unsafe_code)]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, HANDLE, STILL_ACTIVE};
use windows_sys::Win32::System::Threading::{
    CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CreateProcessW, DETACHED_PROCESS,
    GetExitCodeProcess, PROCESS_INFORMATION, STARTUPINFOW,
};

use crate::PlatformError;
use crate::wide::last_error;

/// Handle to a detached child; used only to notice an early exit.
#[derive(Debug)]
pub struct DetachedProcess {
    handle: HANDLE,
    pid: u32,
}

// SAFETY: a process handle may be used from any thread; it is closed once on drop.
unsafe impl Send for DetachedProcess {}
// SAFETY: see `Send`; methods only query the handle.
unsafe impl Sync for DetachedProcess {}

impl DetachedProcess {
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// `Some(code)` once the process has exited.
    pub fn try_exit_code(&self) -> Result<Option<u32>, PlatformError> {
        let mut code = 0u32;
        // SAFETY: `handle` is a live process handle; `code` is a valid out-pointer.
        if unsafe { GetExitCodeProcess(self.handle, &mut code) } == 0 {
            return Err(PlatformError::os("GetExitCodeProcess", last_error()));
        }
        Ok((code != STILL_ACTIVE as u32).then_some(code))
    }
}

impl Drop for DetachedProcess {
    fn drop(&mut self) {
        // SAFETY: closed exactly once; closing does not affect the running process.
        unsafe { CloseHandle(self.handle) };
    }
}

/// Starts `program` with `args` in a new process group, without inheriting any handle, and
/// outside the caller's job object when the job allows it.
pub fn spawn_detached(program: &Path, args: &[&OsStr]) -> Result<DetachedProcess, PlatformError> {
    let base = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    match create(program, args, base | CREATE_BREAKAWAY_FROM_JOB) {
        // The caller's job forbids breakaway; the daemon then lives as long as that job.
        Err(PlatformError::Os { code, .. }) if code == ERROR_ACCESS_DENIED => {
            create(program, args, base)
        }
        other => other,
    }
}

fn create(program: &Path, args: &[&OsStr], flags: u32) -> Result<DetachedProcess, PlatformError> {
    let application: Vec<u16> = program.as_os_str().encode_wide().chain([0]).collect();
    let mut command_line = command_line(program.as_os_str(), args);
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut info = PROCESS_INFORMATION::default();

    // SAFETY: strings are NUL-terminated; `command_line` is mutable as CreateProcessW
    // requires; `startup` and `info` are valid for the call. bInheritHandles = FALSE.
    let ok = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            flags,
            ptr::null(),
            ptr::null(),
            &startup,
            &mut info,
        )
    };
    if ok == 0 {
        return Err(PlatformError::os("CreateProcessW", last_error()));
    }
    // SAFETY: the thread handle is valid and not needed.
    unsafe { CloseHandle(info.hThread) };
    Ok(DetachedProcess {
        handle: info.hProcess,
        pid: info.dwProcessId,
    })
}

/// Builds a NUL-terminated command line quoted per the MSVC CRT rules, so the child's
/// `argv` matches `args` exactly (including spaces, quotes and trailing backslashes).
pub(crate) fn command_line(program: &OsStr, args: &[&OsStr]) -> Vec<u16> {
    let mut line = Vec::new();
    quote(&mut line, program);
    for arg in args {
        line.push(u16::from(b' '));
        quote(&mut line, arg);
    }
    line.push(0);
    line
}

fn quote(out: &mut Vec<u16>, arg: &OsStr) {
    let wide: Vec<u16> = arg.encode_wide().collect();
    let needs_quotes = wide.is_empty()
        || wide
            .iter()
            .any(|&c| c == u16::from(b' ') || c == u16::from(b'\t') || c == u16::from(b'"'));
    if !needs_quotes {
        out.extend_from_slice(&wide);
        return;
    }
    let backslash = u16::from(b'\\');
    let quote_char = u16::from(b'"');
    out.push(quote_char);
    let mut backslashes = 0usize;
    for &c in &wide {
        if c == backslash {
            backslashes += 1;
            continue;
        }
        if c == quote_char {
            // Backslashes before a quote are doubled, and the quote itself escaped.
            out.extend(std::iter::repeat_n(backslash, backslashes * 2 + 1));
        } else {
            out.extend(std::iter::repeat_n(backslash, backslashes));
        }
        backslashes = 0;
        out.push(c);
    }
    // Trailing backslashes are doubled so they do not escape the closing quote.
    out.extend(std::iter::repeat_n(backslash, backslashes * 2));
    out.push(quote_char);
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    fn line(program: &str, args: &[&str]) -> String {
        let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        let mut wide = command_line(OsStr::new(program), &args);
        wide.pop();
        String::from_utf16(&wide).expect("utf16")
    }

    #[test]
    fn quoting_follows_crt_rules() {
        assert_eq!(line(r"C:\a\b.exe", &["--x"]), r"C:\a\b.exe --x");
        assert_eq!(
            line(
                r"C:\Program Files\L\loadoutd.exe",
                &["--data-dir", r"C:\My Data\"]
            ),
            r#""C:\Program Files\L\loadoutd.exe" --data-dir "C:\My Data\\""#
        );
        assert_eq!(line("p", &[""]), r#"p """#);
        assert_eq!(line("p", &[r#"say "hi""#]), r#"p "say \"hi\"""#);
        assert_eq!(line("p", &[r#"a\"b c"#]), r#"p "a\\\"b c""#);
        assert_eq!(line("p", &[r"\\server\share"]), r"p \\server\share");
    }

    #[test]
    fn spawned_process_reports_exit_code() {
        let system = std::env::var_os("SystemRoot").expect("SystemRoot");
        let cmd = Path::new(&system).join("System32").join("cmd.exe");
        let child = spawn_detached(&cmd, &[OsStr::new("/c"), OsStr::new("exit 7")]).expect("spawn");
        assert!(child.pid() > 0);

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(code) = child.try_exit_code().expect("exit code") {
                assert_eq!(code, 7);
                break;
            }
            assert!(Instant::now() < deadline, "child did not exit");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn missing_program_is_an_error() {
        assert!(spawn_detached(Path::new(r"C:\no\such\program.exe"), &[]).is_err());
    }
}
