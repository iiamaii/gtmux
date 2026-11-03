use std::{io, path::Path, ptr::{null, null_mut}, os::windows::ffi::OsStrExt};
use windows_sys::Win32::{Foundation::*, Security::{*, Authorization::*}, System::Threading::*};
fn wide(value: &std::ffi::OsStr) -> Vec<u16> { value.encode_wide().chain(Some(0)).collect() }
fn error() -> io::Error { io::Error::last_os_error() }
struct Handle(HANDLE);
impl Drop for Handle { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }
struct Local(*mut core::ffi::c_void);
impl Drop for Local { fn drop(&mut self) { unsafe { LocalFree(self.0); } } }
fn user_sid() -> io::Result<String> {
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 { return Err(error()); }
        let token = Handle(token); let mut length = 0;
        GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut length);
        let mut buffer = vec![0usize; (length as usize + size_of::<usize>() - 1) / size_of::<usize>()];
        if GetTokenInformation(token.0, TokenUser, buffer.as_mut_ptr().cast(), length, &mut length) == 0 { return Err(error()); }
        let user = &*(buffer.as_ptr().cast::<TOKEN_USER>()); let mut text = null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 { return Err(error()); }
        let _allocation = Local(text.cast()); let mut n = 0; while *text.add(n) != 0 { n += 1; }
        Ok(String::from_utf16_lossy(std::slice::from_raw_parts(text, n)))
    }
}
/// Protect the owned directory before writing any credentials. Children inherit
/// only the current account and SYSTEM. Never grant Everyone/Users access.
pub fn private_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    let sddl = wide(std::ffi::OsStr::new(&format!("D:P(A;OICI;FA;;;{})(A;OICI;FA;;;SY)", user_sid()?)));
    unsafe {
        let mut descriptor = null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), 1, &mut descriptor, null_mut()) == 0 { return Err(error()); }
        let _descriptor = Local(descriptor); let mut present = 0; let mut defaulted = 0; let mut acl = null_mut();
        if GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted) == 0 { return Err(error()); }
        let name = wide(path.as_os_str());
        let code = SetNamedSecurityInfoW(name.as_ptr(), SE_FILE_OBJECT, DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION, null_mut(), null_mut(), acl, null_mut());
        if code != 0 { return Err(io::Error::from_raw_os_error(code as i32)); }
    }
    check_private(path, 0o700)
}
pub fn check_private(path: &Path, _expected: u32) -> io::Result<()> {
    unsafe {
        let mut acl = null_mut(); let mut descriptor = null_mut();
        let code = GetNamedSecurityInfoW(wide(path.as_os_str()).as_ptr(), SE_FILE_OBJECT, DACL_SECURITY_INFORMATION, null_mut(), null_mut(), &mut acl, null_mut(), &mut descriptor);
        if code != 0 { return Err(io::Error::from_raw_os_error(code as i32)); }
        let _descriptor = Local(descriptor);
        if acl.is_null() || (*acl).AceCount == 0 { return Err(io::Error::new(io::ErrorKind::PermissionDenied, "private DACL required")); }
        let user = user_sid()?;
        for index in 0..(*acl).AceCount {
            let mut ace = null_mut();
            if GetAce(acl, index as u32, &mut ace) == 0 { return Err(error()); }
            let header = &*(ace.cast::<ACE_HEADER>());
            if header.AceType != 0 { return Err(io::Error::new(io::ErrorKind::PermissionDenied, "unsupported private DACL entry")); }
            let allowed = &*(ace.cast::<ACCESS_ALLOWED_ACE>()); let mut text = null_mut();
            if ConvertSidToStringSidW((&allowed.SidStart as *const u32).cast_mut().cast(), &mut text) == 0 { return Err(error()); }
            let _text = Local(text.cast()); let mut n = 0; while *text.add(n) != 0 { n += 1; }
            let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, n));
            if sid != user && sid != "S-1-5-18" { return Err(io::Error::new(io::ErrorKind::PermissionDenied, "credential readable by another account")); }
        }
        Ok(())
    }
}
pub fn pid_alive(pid: i32) -> bool {
    if pid <= 0 { return false; }
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid as u32);
        if handle.is_null() { return GetLastError() == ERROR_ACCESS_DENIED; }
        let handle = Handle(handle); let mut code = 0;
        GetExitCodeProcess(handle.0, &mut code) != 0 && code == STILL_ACTIVE as u32
    }
}
fn event_name(pid: i32) -> Vec<u16> { wide(std::ffi::OsStr::new(&format!("Local\\gtmux-stop-{pid}"))) }
/// Named event uses the creating user's default DACL. It never signals other
/// consoles, terminates WSL, or kills a process merely because its name matches.
pub fn request_stop(pid: i32) -> io::Result<()> {
    unsafe {
        let event = OpenEventW(EVENT_MODIFY_STATE, 0, event_name(pid).as_ptr());
        if event.is_null() { return Err(error()); }
        let event = Handle(event);
        if SetEvent(event.0) == 0 { return Err(error()); }
    }
    Ok(())
}
pub fn wait_stop() -> io::Result<()> {
    unsafe {
        let event = CreateEventW(null(), 1, 0, event_name(std::process::id() as i32).as_ptr());
        if event.is_null() { return Err(error()); }
        let event = Handle(event);
        if WaitForSingleObject(event.0, INFINITE) == WAIT_FAILED { return Err(error()); }
    }
    Ok(())
}

/// A kernel-owned descendant group. Closing the job kills only this pane tree.
#[derive(Debug)]
pub struct ChildJob(HANDLE);
// Windows job handles may be accessed/closed from any thread. The sole owner
// closes the handle after the PTY handle is no longer available to callers.
unsafe impl Send for ChildJob {}
unsafe impl Sync for ChildJob {}
impl ChildJob {
    pub fn attach(process: std::os::windows::io::RawHandle) -> io::Result<Self> {
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let raw = CreateJobObjectW(null(), null());
            if raw.is_null() { return Err(error()); }
            let job = Self(raw);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(raw, JobObjectExtendedLimitInformation, (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(), size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32) == 0 { return Err(error()); }
            if AssignProcessToJobObject(raw, process) == 0 { return Err(error()); }
            Ok(job)
        }
    }
}
impl Drop for ChildJob { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }

// Lock a reserved byte beyond the diagnostic JSON. Windows byte-range locks
// are mandatory: locking the entire body would prevent even diagnostic reads.
// Every ownership operation (including cleanup) locks this same stable byte.
pub fn lock(file: &std::fs::File, exclusive: bool) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{Storage::FileSystem::*, System::IO::OVERLAPPED};
    unsafe {
        let mut overlapped: OVERLAPPED = std::mem::zeroed();
        overlapped.Anonymous.Anonymous.Offset = 0xffff_fffe;
        overlapped.Anonymous.Anonymous.OffsetHigh = 0x7fff_ffff;
        let flags = LOCKFILE_FAIL_IMMEDIATELY | if exclusive { LOCKFILE_EXCLUSIVE_LOCK } else { 0 };
        if LockFileEx(file.as_raw_handle(), flags, 0, 1, 0, &mut overlapped) == 0 {
            let e = error();
            if e.raw_os_error() == Some(ERROR_LOCK_VIOLATION as i32) { return Err(io::Error::new(io::ErrorKind::WouldBlock, e)); }
            return Err(e);
        }
    }
    Ok(())
}
pub fn unlock(file: &std::fs::File) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{Storage::FileSystem::UnlockFileEx, System::IO::OVERLAPPED};
    unsafe {
        let mut overlapped: OVERLAPPED = std::mem::zeroed();
        overlapped.Anonymous.Anonymous.Offset = 0xffff_fffe;
        overlapped.Anonymous.Anonymous.OffsetHigh = 0x7fff_ffff;
        if UnlockFileEx(file.as_raw_handle(), 0, 1, 0, &mut overlapped) == 0 { return Err(error()); }
    }
    Ok(())
}

/// Shell association with a literal filename; no cmd.exe parsing or expansion.
pub fn open_path(path: &Path) -> io::Result<()> {
    unsafe {
        let result = windows_sys::Win32::UI::Shell::ShellExecuteW(null_mut(), wide(std::ffi::OsStr::new("open")).as_ptr(), wide(path.as_os_str()).as_ptr(), null(), null(), windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL);
        if result as isize <= 32 { return Err(io::Error::other(format!("Windows file association failed ({})", result as isize))); }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temporary() -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("gtmux-private-{}-{nonce}", std::process::id()))
    }
    #[test]
    fn credentials_inherit_only_user_and_system_permissions() {
        let root = temporary(); private_dir(&root).unwrap();
        let token = root.join("token"); std::fs::write(&token, "test-only").unwrap();
        check_private(&root, 0o700).unwrap(); check_private(&token, 0o600).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn permissive_acl_is_rejected_and_private_directory_repairs_it() {
        let root = temporary(); std::fs::create_dir_all(&root).unwrap();
        // Deliberately grant Everyone on this newly-created, empty test folder.
        unsafe {
            let mut descriptor = null_mut();
            let sddl = wide(std::ffi::OsStr::new("D:P(A;OICI;FA;;;WD)"));
            assert_ne!(ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), 1, &mut descriptor, null_mut()), 0);
            let _allocation = Local(descriptor); let mut acl = null_mut(); let mut present = 0; let mut defaulted = 0;
            assert_ne!(GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted), 0);
            assert_eq!(SetNamedSecurityInfoW(wide(root.as_os_str()).as_ptr(), SE_FILE_OBJECT, DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION, null_mut(), null_mut(), acl, null_mut()), 0);
        }
        assert_eq!(check_private(&root, 0o700).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        private_dir(&root).unwrap(); check_private(&root, 0o700).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
