//! `switchyard` — manage per-folder cloud accounts, and act as a transparent
//! shim for provider CLIs (currently `vercel`).
//!
//! Dual personality (busybox-style): if the binary is invoked under the name of
//! a shimmed provider (e.g. via a `vercel` symlink), it injects the right token
//! and execs the real provider CLI. Otherwise it's the management tool.

mod commands;

use std::path::Path;

fn main() {
    // Shim path: decide based on the name we were invoked as.
    let arg0 = std::env::args().next().unwrap_or_default();
    let invoked = Path::new(&arg0)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .trim_end_matches(".exe");

    if switchyard_core::SHIMMED_BINARIES.contains(&invoked) {
        commands::shim::run(invoked); // never returns on success
        return;
    }

    if let Err(e) = commands::cli::run() {
        eprintln!("switchyard: {e:#}");
        std::process::exit(1);
    }
}
