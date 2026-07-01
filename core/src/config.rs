//! Configuration model and persistence.
//!
//! Tokens are NEVER stored here — only account metadata and folder rules.
//! Secrets live in the OS keychain (see [`crate::keychain`]).

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const CONFIG_VERSION: u32 = 1;

/// A cloud account the user can deploy to (e.g. a Vercel login).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    /// Stable identifier, also the keychain key. e.g. "personal", "22grados".
    pub id: String,
    /// Human label shown in the UI.
    pub label: String,
    /// Provider slug. Only "vercel" for now.
    #[serde(default = "default_provider")]
    pub provider: String,
    /// Cached identity from the last successful validation (display only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Whether a token is currently stored in the keychain for this account.
    /// Cached here so the UI can render status without probing the keychain on
    /// every refresh — each probe pops an OS password prompt on unsigned builds.
    #[serde(default)]
    pub has_token: bool,
}

fn default_provider() -> String {
    "vercel".to_string()
}

/// A folder → account mapping. The most specific (longest) prefix wins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// Absolute path prefix. Any cwd at or under this path matches.
    pub path_prefix: String,
    /// The account id this folder routes to.
    pub account_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

fn default_version() -> u32 {
    CONFIG_VERSION
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: CONFIG_VERSION,
            accounts: Vec::new(),
            rules: Vec::new(),
        }
    }
}

impl Config {
    /// Resolve the directory holding `config.json` (and create it if missing).
    pub fn dir() -> Result<PathBuf> {
        // Allow override for tests / power users.
        if let Ok(custom) = std::env::var("SWITCHYARD_CONFIG_DIR") {
            return Ok(PathBuf::from(custom));
        }
        let pd = ProjectDirs::from("dev", "switchyard", "switchyard")
            .context("could not determine a config directory for this OS")?;
        Ok(pd.config_dir().to_path_buf())
    }

    pub fn path() -> Result<PathBuf> {
        Ok(Self::dir()?.join("config.json"))
    }

    /// Load config, returning a default (empty) config if the file is absent.
    pub fn load() -> Result<Config> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Config::default());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("reading config at {}", path.display()))?;
        let cfg: Config = serde_json::from_str(&raw)
            .with_context(|| format!("parsing config at {}", path.display()))?;
        Ok(cfg)
    }

    /// Persist config atomically (write to temp then rename).
    pub fn save(&self) -> Result<()> {
        let dir = Self::dir()?;
        fs::create_dir_all(&dir)
            .with_context(|| format!("creating config dir {}", dir.display()))?;
        let path = Self::path()?;
        let tmp = dir.join("config.json.tmp");
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&tmp, json).with_context(|| format!("writing {}", tmp.display()))?;
        fs::rename(&tmp, &path).with_context(|| format!("renaming into {}", path.display()))?;
        Ok(())
    }

    pub fn account(&self, id: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == id)
    }

    pub fn upsert_account(&mut self, account: Account) {
        if let Some(existing) = self.accounts.iter_mut().find(|a| a.id == account.id) {
            *existing = account;
        } else {
            self.accounts.push(account);
        }
    }

    pub fn remove_account(&mut self, id: &str) -> bool {
        let before = self.accounts.len();
        self.accounts.retain(|a| a.id != id);
        // Also drop any rules that pointed at it.
        self.rules.retain(|r| r.account_id != id);
        self.accounts.len() != before
    }

    /// Add or replace the rule for an exact path prefix.
    pub fn upsert_rule(&mut self, path_prefix: String, account_id: String) {
        let normalized = normalize_prefix(&path_prefix);
        if let Some(existing) = self.rules.iter_mut().find(|r| r.path_prefix == normalized) {
            existing.account_id = account_id;
        } else {
            self.rules.push(Rule {
                path_prefix: normalized,
                account_id,
            });
        }
    }

    pub fn remove_rule(&mut self, path_prefix: &str) -> bool {
        let normalized = normalize_prefix(path_prefix);
        let before = self.rules.len();
        self.rules.retain(|r| r.path_prefix != normalized);
        self.rules.len() != before
    }
}

/// Canonicalize a path prefix for stable comparison: expand `~`, strip a
/// trailing slash. We deliberately do NOT hit the filesystem so rules can
/// reference folders that don't exist yet.
pub fn normalize_prefix(input: &str) -> String {
    let expanded = if let Some(rest) = input.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            home.join(rest).to_string_lossy().into_owned()
        } else {
            input.to_string()
        }
    } else if input == "~" {
        home_dir()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|| input.to_string())
    } else {
        input.to_string()
    };
    let trimmed = expanded.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())
}

/// Shorten an absolute path under $HOME to a `~`-prefixed display form.
pub fn display_path(p: &Path) -> String {
    if let Some(home) = home_dir() {
        if let Ok(rest) = p.strip_prefix(&home) {
            if rest.as_os_str().is_empty() {
                return "~".to_string();
            }
            return format!("~/{}", rest.display());
        }
    }
    p.display().to_string()
}
