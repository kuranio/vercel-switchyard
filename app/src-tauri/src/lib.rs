//! Tauri bridge for the Switchyard desktop app.
//!
//! Every command is a thin wrapper over `switchyard-core`, so the GUI and the
//! CLI share exactly the same engine, config file, and keychain entries.

use serde::Serialize;
use std::path::{Path, PathBuf};
use switchyard_core::config::display_path;
use switchyard_core::{keychain, resolver, vercel, Account, Config};
use tauri_plugin_dialog::DialogExt;

// ---------------------------------------------------------------------------
// DTOs sent to the frontend
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct AccountDto {
    id: String,
    label: String,
    provider: String,
    email: Option<String>,
    username: Option<String>,
    has_token: bool,
}

#[derive(Serialize)]
struct RuleDto {
    path_prefix: String,
    display: String,
    account_id: String,
}

#[derive(Serialize)]
struct StateDto {
    accounts: Vec<AccountDto>,
    rules: Vec<RuleDto>,
}

#[derive(Serialize)]
struct ResolveDto {
    account_id: String,
    label: String,
    has_token: bool,
    rule_display: String,
}

#[derive(Serialize)]
struct DoctorCheck {
    ok: bool,
    label: String,
    detail: String,
}

#[derive(Serialize)]
struct ShimStatus {
    installed: bool,
    on_path: bool,
    shim_dir: String,
    path_line: String,
}

fn account_dto(a: &Account) -> AccountDto {
    AccountDto {
        has_token: a.has_token,
        id: a.id.clone(),
        label: a.label.clone(),
        provider: a.provider.clone(),
        email: a.email.clone(),
        username: a.username.clone(),
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_state() -> Result<StateDto, String> {
    let cfg = Config::load().map_err(estr)?;
    Ok(StateDto {
        accounts: cfg.accounts.iter().map(account_dto).collect(),
        rules: cfg
            .rules
            .iter()
            .map(|r| RuleDto {
                display: display_path(Path::new(&r.path_prefix)),
                path_prefix: r.path_prefix.clone(),
                account_id: r.account_id.clone(),
            })
            .collect(),
    })
}

#[tauri::command]
fn resolve_dir(path: String) -> Result<Option<ResolveDto>, String> {
    let cfg = Config::load().map_err(estr)?;
    Ok(resolver::resolve(&cfg, Path::new(&path)).map(|m| {
        let label = cfg
            .account(m.account_id)
            .map(|a| a.label.clone())
            .unwrap_or_else(|| m.account_id.to_string());
        ResolveDto {
            account_id: m.account_id.to_string(),
            label,
            has_token: cfg.account(m.account_id).map(|a| a.has_token).unwrap_or(false),
            rule_display: display_path(Path::new(&m.rule.path_prefix)),
        }
    }))
}

/// Add or replace an account. Validates the token online, then stores it in the
/// keychain. The token is never persisted to config or returned to the UI.
#[tauri::command]
async fn add_account(id: String, label: String, token: String) -> Result<AccountDto, String> {
    run_blocking(move || add_account_impl(id, label, token)).await
}

fn add_account_impl(id: String, label: String, token: String) -> Result<AccountDto, String> {
    let id = id.trim().to_string();
    let token = token.trim().to_string();
    if id.is_empty() {
        return Err("Account id can't be empty".into());
    }
    if token.is_empty() {
        return Err("Token can't be empty".into());
    }
    let identity = vercel::validate_token(&token).map_err(estr)?;
    keychain::set_token(&id, &token).map_err(estr)?;

    let account = Account {
        id: id.clone(),
        label: if label.trim().is_empty() {
            id.clone()
        } else {
            label.trim().to_string()
        },
        provider: "vercel".to_string(),
        username: identity.username,
        email: identity.email,
        has_token: true,
    };
    let mut cfg = Config::load().map_err(estr)?;
    cfg.upsert_account(account.clone());
    cfg.save().map_err(estr)?;
    Ok(account_dto(&account))
}

#[tauri::command]
fn remove_account(id: String) -> Result<(), String> {
    let mut cfg = Config::load().map_err(estr)?;
    cfg.remove_account(&id);
    cfg.save().map_err(estr)?;
    keychain::delete_token(&id).map_err(estr)?;
    Ok(())
}

/// Re-check an account's stored token and refresh its cached identity.
#[tauri::command]
async fn revalidate_account(id: String) -> Result<AccountDto, String> {
    run_blocking(move || revalidate_account_impl(id)).await
}

fn revalidate_account_impl(id: String) -> Result<AccountDto, String> {
    let token = keychain::get_token(&id)
        .map_err(estr)?
        .ok_or_else(|| "No token stored for this account".to_string())?;
    let identity = vercel::validate_token(&token).map_err(estr)?;
    let mut cfg = Config::load().map_err(estr)?;
    let mut updated = cfg
        .account(&id)
        .cloned()
        .ok_or_else(|| "Unknown account".to_string())?;
    updated.username = identity.username;
    updated.email = identity.email;
    updated.has_token = true;
    cfg.upsert_account(updated.clone());
    cfg.save().map_err(estr)?;
    Ok(account_dto(&updated))
}

#[tauri::command]
fn add_rule(path: String, account_id: String) -> Result<(), String> {
    let mut cfg = Config::load().map_err(estr)?;
    if cfg.account(&account_id).is_none() {
        return Err(format!("No account '{account_id}'"));
    }
    cfg.upsert_rule(path, account_id);
    cfg.save().map_err(estr)
}

#[tauri::command]
fn remove_rule(path: String) -> Result<(), String> {
    let mut cfg = Config::load().map_err(estr)?;
    cfg.remove_rule(&path);
    cfg.save().map_err(estr)
}

#[tauri::command]
fn pick_folder(app: tauri::AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .blocking_pick_folder()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
fn home_dir() -> String {
    switchyard_core::config::home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .to_string_lossy()
        .into_owned()
}

#[tauri::command]
fn shim_status() -> Result<ShimStatus, String> {
    let shim_dir = switchyard_core::shim_dir().map_err(estr)?;
    let installed = shim_dir.join("vercel").exists();
    let on_path = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d == shim_dir))
        .unwrap_or(false);
    Ok(ShimStatus {
        installed,
        on_path,
        path_line: format!("export PATH=\"{}:$PATH\"", shim_dir.display()),
        shim_dir: shim_dir.to_string_lossy().into_owned(),
    })
}

/// Create the shim symlinks. Returns the PATH line the user still needs to add.
#[tauri::command]
fn install_shims() -> Result<ShimStatus, String> {
    let shim_dir = switchyard_core::shim_dir().map_err(estr)?;
    std::fs::create_dir_all(&shim_dir).map_err(|e| e.to_string())?;
    let target = which_switchyard().ok_or_else(|| {
        "Could not find the `switchyard` CLI. Install it with `cargo install --path cli`.".to_string()
    })?;
    for bin in switchyard_core::SHIMMED_BINARIES {
        let link = shim_dir.join(bin);
        let _ = std::fs::remove_file(&link);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).map_err(|e| e.to_string())?;
    }
    shim_status()
}

#[tauri::command]
async fn doctor() -> Result<Vec<DoctorCheck>, String> {
    run_blocking(doctor_impl).await
}

fn doctor_impl() -> Result<Vec<DoctorCheck>, String> {
    let mut checks = Vec::new();
    let cfg = Config::load().map_err(estr)?;
    let shim_dir = switchyard_core::shim_dir().map_err(estr)?;

    let vercel_shim = shim_dir.join("vercel");
    checks.push(DoctorCheck {
        ok: vercel_shim.exists(),
        label: "Shim installed".into(),
        detail: vercel_shim.to_string_lossy().into_owned(),
    });

    let on_path = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d == shim_dir))
        .unwrap_or(false);
    checks.push(DoctorCheck {
        ok: on_path,
        label: "Shim dir on PATH".into(),
        detail: if on_path {
            "ready".into()
        } else {
            "add the PATH line and restart your shell".into()
        },
    });

    match switchyard_core::find_real_binary("vercel", &shim_dir) {
        Some(p) => checks.push(DoctorCheck {
            ok: true,
            label: "Real vercel CLI".into(),
            detail: p.to_string_lossy().into_owned(),
        }),
        None => checks.push(DoctorCheck {
            ok: false,
            label: "Real vercel CLI".into(),
            detail: "not found on PATH".into(),
        }),
    }

    for a in &cfg.accounts {
        let (ok, detail) = match keychain::get_token(&a.id) {
            Ok(Some(tok)) => match vercel::validate_token(&tok) {
                Ok(id) => (
                    true,
                    id.email.or(id.username).unwrap_or_else(|| "valid".into()),
                ),
                Err(e) => (false, e.to_string()),
            },
            Ok(None) => (false, "no token stored".into()),
            Err(e) => (false, e.to_string()),
        };
        checks.push(DoctorCheck {
            ok,
            label: format!("Account · {}", a.label),
            detail,
        });
    }

    Ok(checks)
}

#[tauri::command]
async fn import_direnv(tokens_dir: Option<String>) -> Result<Vec<String>, String> {
    run_blocking(move || import_direnv_impl(tokens_dir)).await
}

fn import_direnv_impl(tokens_dir: Option<String>) -> Result<Vec<String>, String> {
    let dir = tokens_dir.map(PathBuf::from).unwrap_or_else(|| {
        switchyard_core::config::home_dir()
            .unwrap_or_default()
            .join(".vercel-tokens")
    });
    if !dir.is_dir() {
        return Err(format!("No folder at {}", dir.display()));
    }
    let mut cfg = Config::load().map_err(estr)?;
    let mut log = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(id) = path.file_name().and_then(|s| s.to_str()).map(String::from) else {
            continue;
        };
        let token = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .trim()
            .to_string();
        if token.is_empty() {
            log.push(format!("skipped {id}: empty"));
            continue;
        }
        match vercel::validate_token(&token) {
            Ok(identity) => {
                keychain::set_token(&id, &token).map_err(estr)?;
                cfg.upsert_account(Account {
                    id: id.clone(),
                    label: id.clone(),
                    provider: "vercel".into(),
                    username: identity.username.clone(),
                    email: identity.email.clone(),
                    has_token: true,
                });
                log.push(format!(
                    "imported {id} → {}",
                    identity
                        .email
                        .or(identity.username)
                        .unwrap_or_else(|| "?".into())
                ));
            }
            Err(e) => log.push(format!("skipped {id}: {e}")),
        }
    }
    cfg.save().map_err(estr)?;
    Ok(log)
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn estr<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Run blocking work (network calls, keychain access) off the webview's main
/// thread so the UI never freezes and the IPC promise resolves cleanly.
async fn run_blocking<T, F>(f: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("background task failed: {e}"))?
}

/// Best-effort lookup of the `switchyard` CLI for the shim target.
fn which_switchyard() -> Option<PathBuf> {
    if let Some(home) = switchyard_core::config::home_dir() {
        let cargo_bin = home.join(".cargo").join("bin").join("switchyard");
        if cargo_bin.exists() {
            return Some(cargo_bin);
        }
        let local_bin = home.join(".local").join("bin").join("switchyard");
        if local_bin.exists() {
            return Some(local_bin);
        }
    }
    switchyard_core::find_real_binary("switchyard", Path::new("/nonexistent"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_state,
            resolve_dir,
            add_account,
            remove_account,
            revalidate_account,
            add_rule,
            remove_rule,
            pick_folder,
            home_dir,
            shim_status,
            install_shims,
            doctor,
            import_direnv,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
