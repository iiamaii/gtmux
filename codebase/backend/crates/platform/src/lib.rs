//! OS boundaries for private files and cooperative server shutdown.
use std::{io, path::Path};
pub fn home() -> Option<std::ffi::OsString> {
    std::env::var_os("HOME").or_else(|| if cfg!(windows) { std::env::var_os("USERPROFILE") } else { None })
}
#[cfg(unix)]
pub fn private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}
#[cfg(unix)]
pub fn check_private(path: &Path, expected: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if std::fs::metadata(path)?.permissions().mode() & 0o777 != expected {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "private file permissions required"));
    }
    Ok(())
}
#[cfg(unix)]
pub fn sync_dir(path: &Path) -> io::Result<()> { std::fs::File::open(path)?.sync_all() }
#[cfg(windows)]
pub fn sync_dir(_path: &Path) -> io::Result<()> { Ok(()) } // Atomic replacement flushes the file; Windows cannot fsync directories.
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{private_dir, check_private, pid_alive, request_stop, wait_stop, ChildJob, open_path};

#[cfg(unix)]
pub fn lock(file: &std::fs::File, exclusive: bool) -> io::Result<()> {
    if exclusive { fs2::FileExt::try_lock_exclusive(file) } else { fs2::FileExt::try_lock_shared(file) }
}
#[cfg(unix)]
pub fn unlock(file: &std::fs::File) -> io::Result<()> { fs2::FileExt::unlock(file) }
#[cfg(windows)]
pub use windows::{lock, unlock};
