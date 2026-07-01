//! Minimal Vercel REST client — just enough to validate a token and read back
//! the identity it belongs to, so the UI can confirm "this is the right account".

use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::time::Duration;

const USER_ENDPOINT: &str = "https://api.vercel.com/v2/user";

/// The identity behind a token.
#[derive(Debug, Clone)]
pub struct Identity {
    pub username: Option<String>,
    pub email: Option<String>,
    pub name: Option<String>,
}

#[derive(Deserialize)]
struct UserEnvelope {
    user: Option<UserBody>,
    // Some responses return the user fields at the top level.
    username: Option<String>,
    email: Option<String>,
    name: Option<String>,
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct UserBody {
    username: Option<String>,
    email: Option<String>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct ApiError {
    code: Option<String>,
    message: Option<String>,
}

/// Validate `token` against Vercel and return the identity it maps to.
///
/// Returns `Err` with a friendly message for expired/revoked tokens so the UI
/// can surface "this token no longer works" rather than a raw HTTP error.
pub fn validate_token(token: &str) -> Result<Identity> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .build();

    let resp = agent
        .get(USER_ENDPOINT)
        .set("Authorization", &format!("Bearer {token}"))
        .call();

    match resp {
        Ok(r) => {
            let env: UserEnvelope = r
                .into_json()
                .map_err(|e| anyhow!("could not parse Vercel response: {e}"))?;
            if let Some(err) = env.error {
                return Err(anyhow!(
                    "Vercel rejected the token: {} ({})",
                    err.message.unwrap_or_else(|| "unknown".into()),
                    err.code.unwrap_or_else(|| "?".into())
                ));
            }
            let (username, email, name) = if let Some(u) = env.user {
                (u.username, u.email, u.name)
            } else {
                (env.username, env.email, env.name)
            };
            Ok(Identity {
                username,
                email,
                name,
            })
        }
        Err(ureq::Error::Status(401 | 403, _)) => Err(anyhow!(
            "token is expired or revoked (Vercel returned 'not authorized')"
        )),
        Err(ureq::Error::Status(code, _)) => {
            Err(anyhow!("Vercel returned an unexpected status: {code}"))
        }
        Err(ureq::Error::Transport(t)) => {
            Err(anyhow!("could not reach Vercel (network problem): {t}"))
        }
    }
}
