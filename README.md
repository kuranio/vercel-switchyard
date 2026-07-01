# 🚂 Switchyard

**Route every deploy to the right cloud account, automatically — based on which folder you're in. No more `login` / `logout`.**

![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)
![Platform: macOS · Linux](https://img.shields.io/badge/platform-macOS%20%C2%B7%20Linux-blue)

If you have two Vercel accounts (say a personal one and a work one), the CLI only
lets you be logged into one at a time — so you end up doing `vercel logout` /
`vercel login` all day, and one distracted deploy ends up in the wrong account.

Switchyard fixes that. You tell it once:

> "Everything under `~/Dev` is my **personal** account, everything under `~/work`
> is my **work** account."

…and from then on it transparently injects the correct token whenever you run
`vercel`, based on the folder you're standing in. New projects inside those
folders are routed automatically — nothing to configure per project.

- 🔐 **Tokens live in your OS keychain**, never in plaintext files.
- 🪄 **Transparent shim** — you keep typing `vercel deploy` exactly like before.
- 📁 **Per-folder rules** with longest-prefix matching (nested folders override).
- 🩺 **`doctor`** tells you exactly what's wrong (expired token? shim not on PATH?).
- 🖥️ **Desktop app** (Tauri) to manage accounts and rules with a clean UI.

> Status: early but usable. Vercel only for now — the architecture is
> provider-agnostic, so more providers can be added.

## How it works

Switchyard installs a tiny `vercel` shim early on your `PATH`. When you run
`vercel`, the shim:

1. looks at your current directory,
2. finds the matching folder rule → account,
3. pulls that account's token from the keychain,
4. sets `VERCEL_TOKEN`, and
5. `exec`s the real `vercel` (which then behaves 100% normally).

If no rule matches, it passes straight through to your default login — so it's
safe to have on your PATH everywhere.

## Install (from source)

Requires [Rust](https://rustup.rs).

```bash
git clone https://github.com/kuranio/vercel-switchyard
cd vercel-switchyard
cargo install --path cli      # builds the `switchyard` binary
switchyard install            # creates the shims, prints the PATH line
```

Add the printed line to the **end** of your `~/.zshrc` — it must end up *first*
on your `PATH`, ahead of the real `vercel` (version managers like nvm prepend to
PATH too, so putting it last is what guarantees it wins):

```bash
export PATH="$HOME/.local/switchyard/bin:$PATH"
```

Then open a new terminal and check `which vercel` points at
`~/.local/switchyard/bin/vercel`.

## Usage

```bash
# Add an account (token is read from stdin, so it never hits your shell history)
printf '%s' 'YOUR_VERCEL_TOKEN' | switchyard account add --id personal --label Personal

# Route folders to accounts
switchyard rule add ~/Dev     personal
switchyard rule add ~/work    acme

# See what's going on
switchyard status     # what does the current folder route to?
switchyard doctor     # is everything healthy? are tokens still valid?
```

Create Vercel tokens at <https://vercel.com/account/settings/tokens> (pick the
longest expiry available so they don't silently die on you).

Already using the `direnv` + `~/.vercel-tokens` trick? Import it in one shot:

```bash
switchyard import-direnv
```

## Desktop app

Prefer a UI? The Tauri app lets you add accounts, draw folder rules, and run the
health checks without touching the terminal.

```bash
cd app
npm install
npm run tauri dev      # run in dev
npm run tauri build    # produce a native app bundle (target/release/bundle)
```

The app and the CLI share the exact same config file and keychain entries, so
you can mix and match freely.

> **Note on unsigned builds:** if you build it yourself without a code-signing
> certificate, macOS will ask for keychain permission the first time an account
> is added or re-checked. Click **"Always Allow"**. A signed/notarized release
> removes this entirely.

## Layout

```
core/   reusable engine: config, keychain, Vercel API, folder resolver
cli/    the `switchyard` binary (management tool + busybox-style shim)
app/    Tauri desktop app (uses core)
```

## Contributing

Issues and PRs welcome. To hack on it:

```bash
cargo test            # run the test suite
cargo run -p switchyard-cli -- status   # run the CLI without installing
```

## License

[MIT](LICENSE) © Raúl García
