//! The management CLI: accounts, rules, status, doctor, install, import.

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use std::io::Read;
use std::path::{Path, PathBuf};
use switchyard_core::config::{display_path, normalize_prefix};
use switchyard_core::{keychain, vercel, Account, Config};

#[derive(Parser)]
#[command(
    name = "switchyard",
    about = "Route deploys to the right cloud account, per folder — no more login/logout.",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Show which account the current folder routes to, plus all config.
    Status,
    /// Check the install, PATH, and token validity.
    Doctor,
    /// Install the provider shims (creates symlinks; prints PATH setup).
    Install,
    /// Remove the provider shims.
    Uninstall,
    /// Manage accounts.
    #[command(subcommand)]
    Account(AccountCmd),
    /// Manage folder → account rules.
    #[command(subcommand)]
    Rule(RuleCmd),
    /// Import an existing direnv + ~/.vercel-tokens setup.
    ImportDirenv(ImportArgs),
}

#[derive(Subcommand)]
enum AccountCmd {
    /// Add or replace an account. Token is read from --token or stdin.
    Add(AccountAddArgs),
    /// List configured accounts.
    List,
    /// Remove an account (and its rules + stored token).
    Rm { id: String },
    /// Re-validate stored tokens against Vercel.
    Validate {
        /// Validate just this account (default: all).
        id: Option<String>,
    },
}

#[derive(Parser)]
struct AccountAddArgs {
    /// Stable id, also the keychain key (e.g. "personal", "22grados").
    #[arg(long)]
    id: String,
    /// Human label.
    #[arg(long)]
    label: Option<String>,
    /// Token value. If omitted, read from stdin (safer — stays out of history).
    #[arg(long)]
    token: Option<String>,
    /// Skip the online validation step.
    #[arg(long)]
    no_validate: bool,
}

#[derive(Subcommand)]
enum RuleCmd {
    /// Route a folder (and everything under it) to an account.
    Add { path: String, account_id: String },
    /// List rules.
    List,
    /// Remove the rule for a folder.
    Rm { path: String },
}

#[derive(Parser)]
struct ImportArgs {
    /// Directory holding plaintext token files (default: ~/.vercel-tokens).
    #[arg(long)]
    tokens_dir: Option<PathBuf>,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Status) {
        Command::Status => status(),
        Command::Doctor => doctor(),
        Command::Install => install(),
        Command::Uninstall => uninstall(),
        Command::Account(c) => account(c),
        Command::Rule(c) => rule(c),
        Command::ImportDirenv(a) => import_direnv(a),
    }
}

// ---------------------------------------------------------------------------
// status
// ---------------------------------------------------------------------------

fn status() -> Result<()> {
    let cfg = Config::load()?;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    println!("\x1b[1mCurrent folder\x1b[0m: {}", display_path(&cwd));
    match switchyard_core::resolver::resolve(&cfg, &cwd) {
        Some(m) => {
            let has = keychain::has_token(m.account_id);
            let label = cfg
                .account(m.account_id)
                .map(|a| a.label.as_str())
                .unwrap_or(m.account_id);
            let token_state = if has {
                "token ✓"
            } else {
                "\x1b[31mno token ✗\x1b[0m"
            };
            println!(
                "  → routes to \x1b[1;32m{}\x1b[0m ({})  [{}]",
                label, m.account_id, token_state
            );
            println!(
                "    via rule {}",
                display_path(Path::new(&m.rule.path_prefix))
            );
        }
        None => {
            println!("  → \x1b[2mno rule matches — passes through to your default login\x1b[0m")
        }
    }

    println!("\n\x1b[1mAccounts\x1b[0m ({}):", cfg.accounts.len());
    for a in &cfg.accounts {
        let tok = if keychain::has_token(&a.id) {
            "🔑"
        } else {
            "\x1b[31m—\x1b[0m"
        };
        let who = a
            .email
            .as_deref()
            .or(a.username.as_deref())
            .unwrap_or("(unverified)");
        println!("  {tok} {:<14} {:<10} {}", a.id, a.label, who);
    }

    println!("\n\x1b[1mRules\x1b[0m ({}):", cfg.rules.len());
    for r in &cfg.rules {
        println!(
            "  {:<28} → {}",
            display_path(Path::new(&r.path_prefix)),
            r.account_id
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// account
// ---------------------------------------------------------------------------

fn account(cmd: AccountCmd) -> Result<()> {
    match cmd {
        AccountCmd::Add(a) => account_add(a),
        AccountCmd::List => {
            let cfg = Config::load()?;
            for a in &cfg.accounts {
                println!(
                    "{}\t{}\t{}",
                    a.id,
                    a.label,
                    a.email.as_deref().unwrap_or("-")
                );
            }
            Ok(())
        }
        AccountCmd::Rm { id } => {
            let mut cfg = Config::load()?;
            if cfg.remove_account(&id) {
                cfg.save()?;
                keychain::delete_token(&id)?;
                println!("Removed account '{id}' (token and rules cleaned up).");
            } else {
                println!("No account '{id}'.");
            }
            Ok(())
        }
        AccountCmd::Validate { id } => account_validate(id),
    }
}

fn account_add(a: AccountAddArgs) -> Result<()> {
    let token = match a.token {
        Some(t) => t,
        None => read_token_from_stdin()?,
    };
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(anyhow!("empty token"));
    }

    let label = a.label.unwrap_or_else(|| a.id.clone());
    let mut account = Account {
        id: a.id.clone(),
        label,
        provider: "vercel".to_string(),
        username: None,
        email: None,
        // A token is stored in the keychain just below, so this is always true.
        has_token: true,
    };

    if !a.no_validate {
        print!("Validating token… ");
        match vercel::validate_token(&token) {
            Ok(id) => {
                account.username = id.username.clone();
                account.email = id.email.clone();
                println!(
                    "\x1b[32mok\x1b[0m → {} ({})",
                    id.username.unwrap_or_else(|| "?".into()),
                    id.email.unwrap_or_else(|| "no email".into())
                );
            }
            Err(e) => {
                println!("\x1b[31mfailed\x1b[0m");
                return Err(e.context("token did not validate (use --no-validate to store anyway)"));
            }
        }
    }

    keychain::set_token(&a.id, &token)?;
    let mut cfg = Config::load()?;
    cfg.upsert_account(account);
    cfg.save()?;
    println!("Saved account '{}'. Token is in your OS keychain.", a.id);
    Ok(())
}

fn account_validate(id: Option<String>) -> Result<()> {
    let cfg = Config::load()?;
    let targets: Vec<&Account> = match &id {
        Some(i) => cfg.accounts.iter().filter(|a| &a.id == i).collect(),
        None => cfg.accounts.iter().collect(),
    };
    if targets.is_empty() {
        return Err(anyhow!("no matching account"));
    }
    let mut all_ok = true;
    for a in targets {
        print!("{:<14} ", a.id);
        match keychain::get_token(&a.id)? {
            None => {
                all_ok = false;
                println!("\x1b[31mno token stored\x1b[0m");
            }
            Some(tok) => match vercel::validate_token(&tok) {
                Ok(idn) => println!(
                    "\x1b[32mok\x1b[0m  {}",
                    idn.email.or(idn.username).unwrap_or_else(|| "?".into())
                ),
                Err(e) => {
                    all_ok = false;
                    println!("\x1b[31m{e}\x1b[0m");
                }
            },
        }
    }
    if !all_ok {
        return Err(anyhow!("one or more tokens need attention"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// rule
// ---------------------------------------------------------------------------

fn rule(cmd: RuleCmd) -> Result<()> {
    let mut cfg = Config::load()?;
    match cmd {
        RuleCmd::Add { path, account_id } => {
            if cfg.account(&account_id).is_none() {
                return Err(anyhow!(
                    "no account '{account_id}' — add it first with `switchyard account add`"
                ));
            }
            cfg.upsert_rule(path.clone(), account_id.clone());
            cfg.save()?;
            println!("{} → {}", normalize_prefix(&path), account_id);
        }
        RuleCmd::List => {
            for r in &cfg.rules {
                println!("{}\t{}", r.path_prefix, r.account_id);
            }
        }
        RuleCmd::Rm { path } => {
            if cfg.remove_rule(&path) {
                cfg.save()?;
                println!("Removed rule for {}", normalize_prefix(&path));
            } else {
                println!("No rule for {}", normalize_prefix(&path));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// install / uninstall
// ---------------------------------------------------------------------------

fn install() -> Result<()> {
    let shim_dir = switchyard_core::shim_dir()?;
    std::fs::create_dir_all(&shim_dir)
        .with_context(|| format!("creating {}", shim_dir.display()))?;
    let exe = std::env::current_exe().context("locating the switchyard binary")?;

    for bin in switchyard_core::SHIMMED_BINARIES {
        let link = shim_dir.join(bin);
        let _ = std::fs::remove_file(&link); // replace if present
        symlink(&exe, &link)
            .with_context(|| format!("creating shim symlink {}", link.display()))?;
        println!("shim: {} → {}", link.display(), exe.display());
    }

    let on_path = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d == shim_dir))
        .unwrap_or(false);

    println!();
    if on_path {
        println!(
            "\x1b[32m✓ {} is already on your PATH.\x1b[0m",
            shim_dir.display()
        );
    } else {
        println!("\x1b[1mOne-time PATH setup\x1b[0m — add this near the TOP of your shell rc:");
        println!(
            "  \x1b[36mexport PATH=\"{}:$PATH\"\x1b[0m",
            shim_dir.display()
        );
        println!("(it must come before the real `vercel`). Then restart your shell.");
    }
    Ok(())
}

fn uninstall() -> Result<()> {
    let shim_dir = switchyard_core::shim_dir()?;
    for bin in switchyard_core::SHIMMED_BINARIES {
        let link = shim_dir.join(bin);
        if link.exists() {
            std::fs::remove_file(&link).with_context(|| format!("removing {}", link.display()))?;
            println!("removed {}", link.display());
        }
    }
    println!("Shims removed. You can delete the PATH line from your shell rc.");
    Ok(())
}

// ---------------------------------------------------------------------------
// doctor
// ---------------------------------------------------------------------------

fn doctor() -> Result<()> {
    let mut problems = 0;
    let cfg = Config::load()?;
    let shim_dir = switchyard_core::shim_dir()?;

    // 1. shim installed?
    let vercel_shim = shim_dir.join("vercel");
    if vercel_shim.exists() {
        ok(&format!("shim present: {}", vercel_shim.display()));
    } else {
        problems += 1;
        bad("shim not installed — run `switchyard install`");
    }

    // 2. shim dir on PATH and BEFORE the real vercel?
    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let shim_idx = path_dirs.iter().position(|d| *d == shim_dir);
    let real = switchyard_core::find_real_binary("vercel", &shim_dir);
    match (shim_idx, &real) {
        (Some(_), _) => ok(&format!("{} is on PATH", shim_dir.display())),
        (None, _) => {
            problems += 1;
            bad(&format!(
                "{} is NOT on PATH — deploys won't be routed",
                shim_dir.display()
            ));
        }
    }
    match &real {
        Some(p) => ok(&format!("real vercel found: {}", p.display())),
        None => {
            problems += 1;
            bad("the real `vercel` CLI was not found on PATH");
        }
    }

    // 3. tokens valid?
    for a in &cfg.accounts {
        match keychain::get_token(&a.id)? {
            None => {
                problems += 1;
                bad(&format!("account '{}' has no stored token", a.id));
            }
            Some(tok) => match vercel::validate_token(&tok) {
                Ok(idn) => ok(&format!(
                    "account '{}' valid → {}",
                    a.id,
                    idn.email.or(idn.username).unwrap_or_else(|| "?".into())
                )),
                Err(e) => {
                    problems += 1;
                    bad(&format!("account '{}': {e}", a.id));
                }
            },
        }
    }

    println!();
    if problems == 0 {
        println!("\x1b[32mAll good. 🚂\x1b[0m");
        Ok(())
    } else {
        Err(anyhow!("{problems} problem(s) found"))
    }
}

// ---------------------------------------------------------------------------
// import-direnv
// ---------------------------------------------------------------------------

fn import_direnv(args: ImportArgs) -> Result<()> {
    let tokens_dir = args.tokens_dir.unwrap_or_else(|| {
        switchyard_core::config::home_dir()
            .unwrap_or_default()
            .join(".vercel-tokens")
    });
    if !tokens_dir.is_dir() {
        return Err(anyhow!("no tokens dir at {}", tokens_dir.display()));
    }

    let mut cfg = Config::load()?;
    let mut imported = 0;

    for entry in std::fs::read_dir(&tokens_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let id = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let token = std::fs::read_to_string(&path)?.trim().to_string();
        if token.is_empty() {
            println!("skip {id}: empty token file");
            continue;
        }
        let mut account = Account {
            id: id.clone(),
            label: id.clone(),
            provider: "vercel".to_string(),
            username: None,
            email: None,
            // Only upserted after the token is stored in the keychain below.
            has_token: true,
        };
        match vercel::validate_token(&token) {
            Ok(idn) => {
                account.username = idn.username.clone();
                account.email = idn.email.clone();
                keychain::set_token(&id, &token)?;
                cfg.upsert_account(account);
                imported += 1;
                println!(
                    "imported '{}' → {} (token moved to keychain)",
                    id,
                    idn.email.or(idn.username).unwrap_or_else(|| "?".into())
                );
            }
            Err(e) => println!("skip {id}: {e}"),
        }
    }

    cfg.save()?;
    println!(
        "\nImported {imported} account(s). Now add rules, e.g.:\n  \
         switchyard rule add ~/Dev <account-id>"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn read_token_from_stdin() -> Result<String> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("reading token from stdin")?;
    Ok(buf)
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

fn ok(msg: &str) {
    println!("\x1b[32m✓\x1b[0m {msg}");
}

fn bad(msg: &str) {
    println!("\x1b[31m✗\x1b[0m {msg}");
}
