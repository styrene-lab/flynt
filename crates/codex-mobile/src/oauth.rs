//! FFI bridge to the Swift GitHub OAuth layer and token-based git clone.
//!
//! On iOS, the Swift code handles ASWebAuthenticationSession and Keychain storage.
//! This module provides safe Rust wrappers around the C-callable FFI functions.
//! On non-iOS targets, the functions return None / no-op (desktop uses SSH/credential helpers).

use anyhow::Result;
use std::path::Path;

#[cfg(target_os = "ios")]
extern "C" {
    fn github_oauth_start(client_id: *const std::ffi::c_char, callback_scheme: *const std::ffi::c_char);
    fn github_oauth_get_token() -> *mut std::ffi::c_char;
    fn github_oauth_clear_token();
}

/// Trigger the GitHub OAuth flow (iOS only). Opens a browser session.
#[allow(unused_variables)]
pub fn start_oauth(client_id: &str, callback_scheme: &str) {
    #[cfg(target_os = "ios")]
    {
        let cid = std::ffi::CString::new(client_id).unwrap();
        let scheme = std::ffi::CString::new(callback_scheme).unwrap();
        unsafe { github_oauth_start(cid.as_ptr(), scheme.as_ptr()) }
    }
}

/// Retrieve the stored GitHub token. Returns None if not authenticated.
pub fn get_token() -> Option<String> {
    #[cfg(target_os = "ios")]
    {
        let ptr = unsafe { github_oauth_get_token() };
        if ptr.is_null() {
            return None;
        }
        let token = unsafe { std::ffi::CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        unsafe { libc::free(ptr as *mut std::ffi::c_void) };
        Some(token)
    }
    #[cfg(not(target_os = "ios"))]
    {
        None
    }
}

/// Clear the stored GitHub token (sign out).
pub fn clear_token() {
    #[cfg(target_os = "ios")]
    unsafe { github_oauth_clear_token() }
}

/// Clone a remote repository using a GitHub token for HTTPS authentication.
pub fn clone_with_token(
    url: &str,
    branch: &str,
    dest: &Path,
    token: &str,
) -> Result<()> {
    codex_store::sync::GitSync::clone_repo_with_token(url, branch, dest, token)?;
    Ok(())
}
