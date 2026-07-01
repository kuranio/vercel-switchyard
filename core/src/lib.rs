//! switchyard-core — the engine shared by the CLI/shim and the desktop app.
//!
//! Responsibilities:
//!   * model + persist account/rule configuration ([`config`])
//!   * keep tokens in the OS keychain ([`keychain`])
//!   * validate tokens against Vercel ([`vercel`])
//!   * resolve a directory to an account ([`resolver`])
//!
//! Everything here is synchronous and side-effect-light so it can be driven by
//! a tiny shim binary on every `vercel` invocation without noticeable latency.

pub mod config;
pub mod keychain;
pub mod resolver;
pub mod vercel;

use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};

pub use config::{Account, Config, Rule};
pub use resolver::Match;

/// The environment variable each provider expects its token in.
pub fn token_env_var(provider: &str) -> &'static str {
    match provider {
        // Only Vercel for now; this is the seam for future providers.
        _ => "VERCEL_TOKEN",
    }
}

/// Result of resolving the current directory: which account, and its token.
pub struct Resolution {
    pub account_id: String,
    pub provider: String,
    pub token: String,
    pub env_var: &'static str,
}

/// Resolve `dir` to a ready-to-use token, reading config + keychain.
///
/// Returns `Ok(None)` when no rule matches (caller should pass through
/// transparently). Returns `Err` when a rule matches but its token is missing —
/// that's a misconfiguration worth surfacing.
pub fn resolve_token(dir: &Path) -> Result<Option<Resolution>> {
    let cfg = Config::load()?;
    let Some(m) = resolver::resolve(&cfg, dir) else {
        return Ok(None);
    };
    let account_id = m.account_id.to_string();
    let provider = cfg
        .account(&account_id)
        .map(|a| a.provider.clone())
        .unwrap_or_else(|| "vercel".to_string());

    let token = keychain::get_token(&account_id)?.ok_or_else(|| {
        anyhow!(
            "folder is routed to account '{}' but no token is stored for it \
             (add one in the Switchyard app or run `switchyard account add`)",
            account_id
        )
    })?;

    Ok(Some(Resolution {
        env_var: token_env_var(&provider),
        account_id,
        provider,
        token,
    }))
}

/// The directory where shim symlinks (e.g. `vercel`) are installed. Kept on a
/// fixed, conventional path so both install and resolution agree without
/// relying on argv[0], which shells don't always pass as a full path.
pub fn shim_dir() -> Result<PathBuf> {
    let home = config::home_dir().ok_or_else(|| anyhow!("could not determine home directory"))?;
    Ok(home.join(".local").join("switchyard").join("bin"))
}

/// Provider binaries we shim. Extend this as providers are added.
pub const SHIMMED_BINARIES: &[&str] = &["vercel"];

/// Locate the *real* provider binary on PATH, skipping our own shim directory.
///
/// The shim is installed as a symlink (e.g. `<shim_dir>/vercel`); without this
/// guard the shim would re-invoke itself forever.
///
/// Tries the process PATH first (the fast path when the shim runs inside a
/// terminal). If that fails — e.g. the desktop app was launched from Finder and
/// didn't inherit the shell's PATH, so version-manager binaries like nvm's
/// `vercel` are invisible — it falls back to asking the user's login shell.
pub fn find_real_binary(name: &str, shim_dir: &Path) -> Option<PathBuf> {
    scan_path(std::env::var_os("PATH"), name, shim_dir)
        .or_else(|| scan_path(login_shell_path(), name, shim_dir))
}

/// Scan a `PATH`-style variable for an executable `name`, skipping `shim_dir`.
fn scan_path(path_var: Option<std::ffi::OsString>, name: &str, shim_dir: &Path) -> Option<PathBuf> {
    let path_var = path_var?;
    for dir in std::env::split_paths(&path_var) {
        if dir == shim_dir {
            continue;
        }
        let candidate = dir.join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Ask the user's interactive login shell for its `PATH`, so we pick up nvm /
/// asdf / Homebrew locations that a GUI-launched process never inherits.
fn login_shell_path() -> Option<std::ffi::OsString> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let out = std::process::Command::new(shell)
        .args(["-lic", "printf %s \"$PATH\""])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(std::ffi::OsString::from(path))
    }
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}
