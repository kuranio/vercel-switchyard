//! The shim: inject the right token for the current folder, then exec the real
//! provider CLI. Designed to be invisible and fast — if anything is unclear we
//! fall through to the real binary unchanged rather than block the user.

use std::path::PathBuf;
use std::process::Command;

/// Run as a shim for `provider_bin` (e.g. "vercel"). Does not return on success.
pub fn run(provider_bin: &str) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Inject the token for whichever account this folder maps to.
    match switchyard_core::resolve_token(&cwd) {
        Ok(Some(res)) => {
            std::env::set_var(res.env_var, &res.token);
            // One quiet line to stderr so the user knows which account is active.
            if std::env::var_os("SWITCHYARD_QUIET").is_none() {
                eprintln!(
                    "\x1b[2mswitchyard → {} ({})\x1b[0m",
                    res.account_id, res.provider
                );
            }
        }
        Ok(None) => {
            // No rule for this folder: pass through untouched.
        }
        Err(e) => {
            // Rule matched but token missing/broken: warn, but still run so the
            // user isn't hard-blocked (the real CLI will use its own login).
            eprintln!("\x1b[33mswitchyard warning:\x1b[0m {e:#}");
        }
    }

    let shim_dir = match switchyard_core::shim_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("switchyard: {e:#}");
            std::process::exit(1);
        }
    };

    let real = switchyard_core::find_real_binary(provider_bin, &shim_dir).unwrap_or_else(|| {
        eprintln!(
            "switchyard: could not find the real `{provider_bin}` on PATH \
             (is it installed?)"
        );
        std::process::exit(127);
    });

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cmd = Command::new(&real);
    cmd.args(&args);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec(); // replaces this process on success
        eprintln!("switchyard: failed to exec {}: {err}", real.display());
        std::process::exit(126);
    }

    #[cfg(not(unix))]
    {
        match cmd.status() {
            Ok(status) => std::process::exit(status.code().unwrap_or(1)),
            Err(err) => {
                eprintln!("switchyard: failed to run {}: {err}", real.display());
                std::process::exit(126);
            }
        }
    }
}
