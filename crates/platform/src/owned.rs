// SPDX-License-Identifier: Apache-2.0

//! Child processes owned by the daemon (ExecPlan 017 §5.4).
//!
//! Unlike [`crate::spawn_detached`], a child started with [`spawn_owned`] lives inside an
//! [`OwnedJob`] created with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: when the daemon exits (or
//! crashes) the job handle closes and every process in it, including grandchildren such as
//! browser renderers, ends. The child inherits exactly the two pipe handles it is given
//! (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`) and nothing else of the parent.

#![allow(unsafe_code)]

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::path::Path;
use std::ptr;
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, HWND, LPARAM, STILL_ACTIVE, SetHandleInformation,
    WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
    GetExitCodeProcess, InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROCESS_INFORMATION, ResumeThread, STARTUPINFOEXW,
    TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
};
use windows_sys::core::BOOL;

use crate::PlatformError;
use crate::process::command_line;
use crate::wide::last_error;

/// A job object that kills its processes when the last handle closes.
#[derive(Debug)]
pub struct OwnedJob {
    handle: HANDLE,
}

// SAFETY: a job handle may be used from any thread; it is closed once on drop.
unsafe impl Send for OwnedJob {}
// SAFETY: see `Send`; the handle is only passed to thread-safe Win32 calls.
unsafe impl Sync for OwnedJob {}

impl OwnedJob {
    pub fn new() -> Result<Self, PlatformError> {
        // SAFETY: anonymous job with default security.
        let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if handle.is_null() {
            return Err(PlatformError::os("CreateJobObjectW", last_error()));
        }
        let job = Self { handle };
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `info` is a valid JOBOBJECT_EXTENDED_LIMIT_INFORMATION of the given size.
        let ok = unsafe {
            SetInformationJobObject(
                job.handle,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(PlatformError::os("SetInformationJobObject", last_error()));
        }
        Ok(job)
    }
}

impl Drop for OwnedJob {
    fn drop(&mut self) {
        // SAFETY: closed exactly once; this ends every process still in the job.
        unsafe { CloseHandle(self.handle) };
    }
}

/// A process started by [`spawn_owned`].
#[derive(Debug)]
pub struct OwnedProcess {
    handle: HANDLE,
    pid: u32,
}

// SAFETY: a process handle may be used from any thread; it is closed once on drop.
unsafe impl Send for OwnedProcess {}
// SAFETY: see `Send`; methods only query or terminate the process.
unsafe impl Sync for OwnedProcess {}

impl OwnedProcess {
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

    /// Waits up to `timeout` for the process to exit; `true` if it did.
    pub fn wait(&self, timeout: Duration) -> bool {
        let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1);
        // SAFETY: `handle` is a live process handle.
        unsafe { WaitForSingleObject(self.handle, ms) == WAIT_OBJECT_0 }
    }

    /// Terminates the process (its own children are ended by the job).
    pub fn kill(&self) {
        // SAFETY: `handle` is a live process handle; failure means it already exited.
        unsafe { TerminateProcess(self.handle, 1) };
    }
}

impl Drop for OwnedProcess {
    fn drop(&mut self) {
        // SAFETY: closed exactly once; closing does not affect the running process.
        unsafe { CloseHandle(self.handle) };
    }
}

/// The parent's ends of the two pipes given to a child.
#[derive(Debug)]
pub struct ChildPipes {
    /// Bytes written here are read by the child.
    pub to_child: File,
    /// Bytes the child writes arrive here.
    pub from_child: File,
}

/// Closes a raw handle on drop unless taken.
struct Guard(HANDLE);

impl Guard {
    fn take(mut self) -> HANDLE {
        std::mem::replace(&mut self.0, ptr::null_mut())
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: an owned, still-open handle.
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// Anonymous pipe; `(read, write)`, both inheritable.
fn pipe() -> Result<(Guard, Guard), PlatformError> {
    let sa = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: ptr::null_mut(),
        bInheritHandle: 1,
    };
    let (mut r, mut w): (HANDLE, HANDLE) = (ptr::null_mut(), ptr::null_mut());
    // SAFETY: valid out-pointers and attributes.
    if unsafe { CreatePipe(&mut r, &mut w, &sa, 1 << 20) } == 0 {
        return Err(PlatformError::os("CreatePipe", last_error()));
    }
    Ok((Guard(r), Guard(w)))
}

fn not_inheritable(h: &Guard) -> Result<(), PlatformError> {
    // SAFETY: `h` is an open handle.
    if unsafe { SetHandleInformation(h.0, HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(PlatformError::os("SetHandleInformation", last_error()));
    }
    Ok(())
}

/// Starts `program` inside `job` with two fresh pipes. `args` receives the child's handle
/// values (`read`, `write`) as decimal strings and returns the full argument list, e.g.
/// `--remote-debugging-io-pipes=<read>,<write>`.
pub fn spawn_owned(
    program: &Path,
    args: impl FnOnce(&str, &str) -> Vec<OsString>,
    job: &OwnedJob,
) -> Result<(OwnedProcess, ChildPipes), PlatformError> {
    let (child_read, parent_write) = pipe()?;
    let (parent_read, child_write) = pipe()?;
    not_inheritable(&parent_write)?;
    not_inheritable(&parent_read)?;

    let args = args(
        &(child_read.0 as usize).to_string(),
        &(child_write.0 as usize).to_string(),
    );
    let arg_refs: Vec<&OsStr> = args.iter().map(OsString::as_os_str).collect();
    let application: Vec<u16> = program.as_os_str().encode_wide().chain([0]).collect();
    let mut line = command_line(program.as_os_str(), &arg_refs);

    // Attribute list naming the only handles the child may inherit.
    let mut size = 0usize;
    // SAFETY: size query with a null list; failure with ERROR_INSUFFICIENT_BUFFER is expected.
    unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut size) };
    let mut storage = vec![0u64; size.div_ceil(8)];
    let list: LPPROC_THREAD_ATTRIBUTE_LIST = storage.as_mut_ptr().cast();
    // SAFETY: `storage` holds `size` bytes, 8-byte aligned.
    if unsafe { InitializeProcThreadAttributeList(list, 1, 0, &mut size) } == 0 {
        return Err(PlatformError::os(
            "InitializeProcThreadAttributeList",
            last_error(),
        ));
    }
    struct ListGuard(LPPROC_THREAD_ATTRIBUTE_LIST);
    impl Drop for ListGuard {
        fn drop(&mut self) {
            // SAFETY: initialized above, deleted once.
            unsafe { DeleteProcThreadAttributeList(self.0) };
        }
    }
    let _list_guard = ListGuard(list);
    let inherit: [HANDLE; 2] = [child_read.0, child_write.0];
    // SAFETY: `inherit` outlives CreateProcessW below; size matches the array.
    let ok = unsafe {
        UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            inherit.as_ptr().cast(),
            size_of_val(&inherit),
            ptr::null_mut(),
            ptr::null(),
        )
    };
    if ok == 0 {
        return Err(PlatformError::os("UpdateProcThreadAttribute", last_error()));
    }

    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.lpAttributeList = list;
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: strings are NUL-terminated; `line` is mutable as required; `startup` carries a
    // valid attribute list; inheritance is restricted to `inherit` by that list.
    let ok = unsafe {
        CreateProcessW(
            application.as_ptr(),
            line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            1,
            CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT,
            ptr::null(),
            ptr::null(),
            (&raw const startup).cast(),
            &mut info,
        )
    };
    if ok == 0 {
        return Err(PlatformError::os("CreateProcessW", last_error()));
    }
    let process = OwnedProcess {
        handle: info.hProcess,
        pid: info.dwProcessId,
    };
    let thread = Guard(info.hThread);
    // SAFETY: both handles are live; the process is suspended and has run no code yet.
    if unsafe { AssignProcessToJobObject(job.handle, process.handle) } == 0 {
        let code = last_error();
        process.kill();
        return Err(PlatformError::os("AssignProcessToJobObject", code));
    }
    // SAFETY: the primary thread handle of the suspended process.
    if unsafe { ResumeThread(thread.0) } == u32::MAX {
        let code = last_error();
        process.kill();
        return Err(PlatformError::os("ResumeThread", code));
    }
    drop(thread);
    // The child holds its own copies now.
    drop(child_read);
    drop(child_write);

    // SAFETY: each handle is owned, open and transferred exactly once.
    let pipes = unsafe {
        ChildPipes {
            to_child: File::from_raw_handle(parent_write.take() as RawHandle),
            from_child: File::from_raw_handle(parent_read.take() as RawHandle),
        }
    };
    Ok((process, pipes))
}

/// Starts `program` inside `job` without inheriting any handle (no pipes): a process the
/// daemon owns but does not talk to, e.g. a browser window the user signs in with.
pub fn spawn_owned_plain(
    program: &Path,
    args: &[OsString],
    job: &OwnedJob,
) -> Result<OwnedProcess, PlatformError> {
    let arg_refs: Vec<&OsStr> = args.iter().map(OsString::as_os_str).collect();
    let application: Vec<u16> = program.as_os_str().encode_wide().chain([0]).collect();
    let mut line = command_line(program.as_os_str(), &arg_refs);
    let startup = windows_sys::Win32::System::Threading::STARTUPINFOW {
        cb: size_of::<windows_sys::Win32::System::Threading::STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: strings are NUL-terminated; `line` is mutable as required; bInheritHandles =
    // FALSE.
    let ok = unsafe {
        CreateProcessW(
            application.as_ptr(),
            line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            CREATE_SUSPENDED,
            ptr::null(),
            ptr::null(),
            &startup,
            &mut info,
        )
    };
    if ok == 0 {
        return Err(PlatformError::os("CreateProcessW", last_error()));
    }
    let process = OwnedProcess {
        handle: info.hProcess,
        pid: info.dwProcessId,
    };
    let thread = Guard(info.hThread);
    // SAFETY: both handles are live; the process is suspended and has run no code yet.
    if unsafe { AssignProcessToJobObject(job.handle, process.handle) } == 0 {
        let code = last_error();
        process.kill();
        return Err(PlatformError::os("AssignProcessToJobObject", code));
    }
    // SAFETY: the primary thread handle of the suspended process.
    if unsafe { ResumeThread(thread.0) } == u32::MAX {
        let code = last_error();
        process.kill();
        return Err(PlatformError::os("ResumeThread", code));
    }
    Ok(process)
}

unsafe extern "system" fn close_if_owned(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam` is the `&mut (u32, usize)` passed by `close_windows` below, alive for
    // the whole EnumWindows call.
    let target = unsafe { &mut *(lparam as *mut (u32, usize)) };
    let mut pid = 0u32;
    // SAFETY: `hwnd` comes from EnumWindows; `pid` is a valid out-pointer.
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    // SAFETY: as above.
    if pid == target.0 && unsafe { IsWindowVisible(hwnd) } != 0 {
        // SAFETY: posting WM_CLOSE is what clicking the window's close button does.
        if unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) } != 0 {
            target.1 += 1;
        }
    }
    1
}

/// Asks every visible top-level window of process `pid` to close, as if the user clicked its
/// close button (the application can save its state). Returns how many were asked.
pub fn close_windows(pid: u32) -> usize {
    let mut target: (u32, usize) = (pid, 0);
    // SAFETY: the callback only reads and writes `target` during this call.
    unsafe { EnumWindows(Some(close_if_owned), (&raw mut target) as LPARAM) };
    target.1
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::time::Instant;

    use super::*;

    fn powershell() -> std::path::PathBuf {
        let system = std::env::var_os("SystemRoot").expect("SystemRoot");
        Path::new(&system)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe")
    }

    /// The child echoes one line from its inherited read handle to its inherited write handle.
    #[test]
    fn child_talks_over_the_two_given_handles() {
        let job = OwnedJob::new().expect("job");
        let script = "param($r,$w); \
            $in = New-Object System.IO.FileStream((New-Object Microsoft.Win32.SafeHandles.SafeFileHandle([IntPtr][long]$r, $false)), 'Read'); \
            $out = New-Object System.IO.FileStream((New-Object Microsoft.Win32.SafeHandles.SafeFileHandle([IntPtr][long]$w, $false)), 'Write'); \
            $buf = New-Object byte[] 5; $n = $in.Read($buf, 0, 5); $out.Write($buf, 0, $n); $out.Flush()";
        let (child, mut pipes) = spawn_owned(
            &powershell(),
            |r, w| {
                [
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    &format!("& {{ {script} }} {r} {w}"),
                ]
                .iter()
                .map(OsString::from)
                .collect()
            },
            &job,
        )
        .expect("spawn");
        pipes.to_child.write_all(b"hello").expect("write");
        let mut buf = [0u8; 5];
        pipes.from_child.read_exact(&mut buf).expect("read");
        assert_eq!(&buf, b"hello");
        assert!(child.wait(Duration::from_secs(30)), "child exits");
    }

    #[test]
    fn closing_the_job_ends_its_processes() {
        let job = OwnedJob::new().expect("job");
        let (child, _pipes) = spawn_owned(
            &powershell(),
            |_, _| {
                ["-NoProfile", "-Command", "Start-Sleep 60"]
                    .iter()
                    .map(OsString::from)
                    .collect()
            },
            &job,
        )
        .expect("spawn");
        assert_eq!(child.try_exit_code().expect("code"), None);
        let started = Instant::now();
        drop(job);
        assert!(child.wait(Duration::from_secs(10)), "job close kills");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn missing_program_is_an_error() {
        let job = OwnedJob::new().expect("job");
        assert!(spawn_owned(Path::new(r"C:\no\such.exe"), |_, _| Vec::new(), &job).is_err());
        assert!(spawn_owned_plain(Path::new(r"C:\no\such.exe"), &[], &job).is_err());
    }

    #[test]
    fn plain_children_end_with_their_job() {
        let job = OwnedJob::new().expect("job");
        let args: Vec<OsString> = ["-NoProfile", "-Command", "Start-Sleep 60"]
            .iter()
            .map(OsString::from)
            .collect();
        let child = spawn_owned_plain(&powershell(), &args, &job).expect("spawn");
        assert_eq!(child.try_exit_code().expect("code"), None);
        drop(job);
        assert!(child.wait(Duration::from_secs(10)), "job close kills");
    }
}
