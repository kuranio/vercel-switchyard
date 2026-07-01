//! Secret storage backed by the OS keychain (macOS Keychain, Windows Credential
//! Manager, or Secret Service on Linux). Tokens never touch disk in plaintext.

use anyhow::{Context, Result};
use keyring::Entry;

const SERVICE: &str = "switchyard";

fn entry(account_id: &str) -> Result<Entry> {
    Entry::new(SERVICE, account_id)
        .with_context(|| format!("opening keychain entry for account '{account_id}'"))
}

/// Store (or replace) the token for an account.
pub fn set_token(account_id: &str, token: &str) -> Result<()> {
    entry(account_id)?
        .set_password(token)
        .with_context(|| format!("saving token for account '{account_id}' to keychain"))
}

/// Fetch the token for an account, if present.
pub fn get_token(account_id: &str) -> Result<Option<String>> {
    match entry(account_id)?.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading token for account '{account_id}'")),
    }
}

/// Remove the token for an account. Succeeds even if there was nothing to remove.
pub fn delete_token(account_id: &str) -> Result<()> {
    match entry(account_id)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e).with_context(|| format!("deleting token for account '{account_id}'")),
    }
}

/// Does this account have a stored token?
pub fn has_token(account_id: &str) -> bool {
    matches!(get_token(account_id), Ok(Some(_)))
}
