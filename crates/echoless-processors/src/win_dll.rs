//! Windows DLL search-path guard shared by the dynamically loaded backends.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

use anyhow::{Context, Result};
use windows_sys::Win32::System::LibraryLoader::{
    AddDllDirectory, RemoveDllDirectory, SetDefaultDllDirectories,
    LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_USER_DIRS,
};

/// Keeps directories on the process DLL search path until dropped. Store it next
/// to the loaded `Library` so delay-loaded companion DLLs can still be found.
pub(crate) struct DllDirectoryGuard {
    cookies: Vec<*mut c_void>,
}

// Cookies are process-wide handles; RemoveDllDirectory may run on any thread.
unsafe impl Send for DllDirectoryGuard {}

impl DllDirectoryGuard {
    pub(crate) fn new(paths: &[PathBuf]) -> Result<Self> {
        let ok = unsafe {
            SetDefaultDllDirectories(
                LOAD_LIBRARY_SEARCH_DEFAULT_DIRS | LOAD_LIBRARY_SEARCH_USER_DIRS,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error())
                .context("failed to set the Windows DLL default search path");
        }

        // Collect into the guard so a failure on a later path releases earlier cookies.
        let mut guard = Self {
            cookies: Vec::with_capacity(paths.len()),
        };
        for path in paths {
            let wide: Vec<u16> = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let cookie = unsafe { AddDllDirectory(wide.as_ptr()) };
            if cookie.is_null() {
                return Err(std::io::Error::last_os_error())
                    .with_context(|| format!("failed to add DLL search path: {}", path.display()));
            }
            guard.cookies.push(cookie);
        }
        Ok(guard)
    }
}

impl Drop for DllDirectoryGuard {
    fn drop(&mut self) {
        for cookie in self.cookies.drain(..) {
            unsafe {
                RemoveDllDirectory(cookie);
            }
        }
    }
}
