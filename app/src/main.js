const { invoke } = window.__TAURI__.core;

// ---- tiny helpers ---------------------------------------------------------
const $ = (sel) => document.querySelector(sel);
const $$ = (sel) => Array.from(document.querySelectorAll(sel));
const esc = (s) =>
  String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
const initials = (s) => (s || "?").trim().slice(0, 2).toUpperCase();

let toastTimer;
function toast(msg, isErr = false) {
  const t = $("#toast");
  t.textContent = msg;
  t.className = "toast show" + (isErr ? " err" : "");
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    t.className = "toast";
    setTimeout(() => (t.hidden = true), 250);
  }, 2600);
}

async function openExternal(url) {
  try {
    if (window.__TAURI__.opener?.openUrl) await window.__TAURI__.opener.openUrl(url);
    else if (window.__TAURI__.shell?.open) await window.__TAURI__.shell.open(url);
  } catch (_) {
    toast("Open: " + url);
  }
}

let STATE = { accounts: [], rules: [] };

// ---- navigation -----------------------------------------------------------
$("#nav").addEventListener("click", (e) => {
  const item = e.target.closest(".nav-item");
  if (!item) return;
  const view = item.dataset.view;
  $$(".nav-item").forEach((n) => n.classList.toggle("active", n === item));
  $$(".view").forEach((v) => v.classList.toggle("active", v.dataset.view === view));
  if (view === "setup") refreshSetup();
});

// ---- data -----------------------------------------------------------------
async function refresh() {
  STATE = await invoke("get_state");
  renderAccounts();
  renderRules();
  renderRuleAccountOptions();
  $("#stat-accounts").textContent = STATE.accounts.length;
  $("#stat-rules").textContent = STATE.rules.length;
  updateHealth();
}

function updateHealth() {
  const dot = $("#health-dot");
  const text = $("#health-text");
  const missing = STATE.accounts.filter((a) => !a.has_token).length;
  if (STATE.accounts.length === 0) {
    dot.className = "dot dot-idle";
    text.textContent = "No accounts yet";
  } else if (missing > 0) {
    dot.className = "dot dot-bad";
    text.textContent = `${missing} need a token`;
  } else {
    dot.className = "dot dot-ok";
    text.textContent = "All accounts ready";
  }
}

// ---- accounts -------------------------------------------------------------
function renderAccounts() {
  const list = $("#accounts-list");
  if (!STATE.accounts.length) {
    list.innerHTML = `<div class="empty-state">No accounts yet. Add one to get started.</div>`;
    return;
  }
  list.innerHTML = STATE.accounts
    .map((a) => {
      const who = a.email || a.username || "unverified";
      const pill = a.has_token
        ? `<span class="pill ok">keychain ✓</span>`
        : `<span class="pill bad">no token</span>`;
      return `
      <div class="acct">
        <div class="avatar">${esc(initials(a.label || a.id))}</div>
        <div class="acct-main">
          <div class="name">${esc(a.label)} ${pill}</div>
          <div class="meta">${esc(who)} · <span class="mono">${esc(a.id)}</span></div>
        </div>
        <div class="acct-actions">
          <button class="btn ghost sm" data-act="revalidate" data-id="${esc(a.id)}">Re-check</button>
          <button class="btn danger sm" data-act="remove" data-id="${esc(a.id)}">Remove</button>
        </div>
      </div>`;
    })
    .join("");
}

$("#accounts-list").addEventListener("click", async (e) => {
  const btn = e.target.closest("button[data-act]");
  if (!btn) return;
  const { act, id } = btn.dataset;
  if (act === "remove") {
    if (!confirm(`Remove account "${id}"? Its token will be deleted from the keychain.`)) return;
    try {
      await invoke("remove_account", { id });
      toast(`Removed ${id}`);
      await refresh();
    } catch (err) {
      toast(err, true);
    }
  } else if (act === "revalidate") {
    btn.disabled = true;
    btn.textContent = "Checking…";
    try {
      const a = await invoke("revalidate_account", { id });
      toast(`${a.label}: ${a.email || a.username || "valid"} ✓`);
      await refresh();
    } catch (err) {
      toast(`${id}: ${err}`, true);
      await refresh();
    }
  }
});

// ---- add-account modal ----------------------------------------------------
const modal = $("#account-modal");
function openModal() {
  $("#m-id").value = "";
  $("#m-label").value = "";
  $("#m-token").value = "";
  $("#m-error").hidden = true;
  modal.hidden = false;
  setTimeout(() => $("#m-id").focus(), 50);
}
function closeModal() {
  modal.hidden = true;
}
$("#add-account-btn").addEventListener("click", openModal);
$("#modal-close").addEventListener("click", closeModal);
$("#m-cancel").addEventListener("click", closeModal);
modal.addEventListener("click", (e) => {
  if (e.target === modal) closeModal();
});
$("#open-tokens").addEventListener("click", (e) => {
  e.preventDefault();
  openExternal("https://vercel.com/account/settings/tokens");
});

$("#m-save").addEventListener("click", async () => {
  const id = $("#m-id").value.trim();
  const label = $("#m-label").value.trim();
  const token = $("#m-token").value.trim();
  const errBox = $("#m-error");
  errBox.hidden = true;
  if (!id || !token) {
    errBox.textContent = "Id and token are required.";
    errBox.hidden = false;
    return;
  }
  const save = $("#m-save");
  save.disabled = true;
  save.textContent = "Validating…";
  try {
    const a = await invoke("add_account", { id, label, token });
    closeModal();
    toast(`Added ${a.label} → ${a.email || a.username || "ok"} ✓`);
    await refresh();
  } catch (err) {
    errBox.textContent = String(err);
    errBox.hidden = false;
  } finally {
    save.disabled = false;
    save.textContent = "Validate & save";
  }
});

// ---- rules ----------------------------------------------------------------
function renderRules() {
  const list = $("#rules-list");
  if (!STATE.rules.length) {
    list.innerHTML = `<div class="empty-state">No rules yet. Map a folder to an account above.</div>`;
    return;
  }
  // Show most-specific first for readability.
  const sorted = [...STATE.rules].sort((a, b) => b.path_prefix.length - a.path_prefix.length);
  list.innerHTML = sorted
    .map((r) => {
      const acct = STATE.accounts.find((a) => a.id === r.account_id);
      const label = acct ? acct.label : r.account_id;
      const warn = acct ? "" : ` <span class="pill bad">missing account</span>`;
      return `
      <div class="rule">
        <span class="path mono">${esc(r.display)}</span>
        <span class="arrow">→</span>
        <span class="to">${esc(label)}</span>${warn}
        <span class="spacer"></span>
        <button class="btn danger sm" data-path="${esc(r.path_prefix)}">Remove</button>
      </div>`;
    })
    .join("");
}

$("#rules-list").addEventListener("click", async (e) => {
  const btn = e.target.closest("button[data-path]");
  if (!btn) return;
  try {
    await invoke("remove_rule", { path: btn.dataset.path });
    await refresh();
  } catch (err) {
    toast(err, true);
  }
});

function renderRuleAccountOptions() {
  const sel = $("#rule-account");
  sel.innerHTML = STATE.accounts.map((a) => `<option value="${esc(a.id)}">${esc(a.label)}</option>`).join("");
}

$("#rule-pick").addEventListener("click", async () => {
  const picked = await invoke("pick_folder");
  if (picked) $("#rule-path").value = picked;
});

$("#rule-add-btn").addEventListener("click", async () => {
  const path = $("#rule-path").value.trim();
  const account_id = $("#rule-account").value;
  if (!path) return toast("Pick or type a folder", true);
  if (!account_id) return toast("Add an account first", true);
  try {
    await invoke("add_rule", { path, accountId: account_id });
    $("#rule-path").value = "";
    toast("Rule added");
    await refresh();
  } catch (err) {
    toast(err, true);
  }
});

// ---- overview: folder probe ----------------------------------------------
let probeTimer;
async function probe() {
  const path = $("#probe-path").value.trim();
  const box = $("#probe-result");
  if (!path) {
    box.className = "probe-result empty";
    box.textContent = "Choose a folder to preview its routing.";
    return;
  }
  try {
    const r = await invoke("resolve_dir", { path });
    if (!r) {
      box.className = "probe-result nomatch";
      box.innerHTML = `<div class="probe-badge gray">—</div><div class="probe-text"><b>No rule matches</b><div class="small">Deploys here use your default Vercel login.</div></div>`;
    } else {
      const tok = r.has_token
        ? `<span class="pill ok">token ✓</span>`
        : `<span class="pill bad">no token</span>`;
      box.className = "probe-result matched";
      box.innerHTML = `<div class="probe-badge">${esc(initials(r.label))}</div>
        <div class="probe-text"><b>${esc(r.label)}</b> ${tok}
        <div class="small">via rule <span class="mono">${esc(r.rule_display)}</span></div></div>`;
    }
  } catch (err) {
    box.className = "probe-result nomatch";
    box.textContent = String(err);
  }
}
$("#probe-path").addEventListener("input", () => {
  clearTimeout(probeTimer);
  probeTimer = setTimeout(probe, 180);
});
$("#probe-pick").addEventListener("click", async () => {
  const picked = await invoke("pick_folder");
  if (picked) {
    $("#probe-path").value = picked;
    probe();
  }
});

// ---- setup ----------------------------------------------------------------
async function refreshSetup() {
  try {
    const s = await invoke("shim_status");
    const state = $("#shim-state");
    const wrap = $("#path-line-wrap");
    if (s.installed && s.on_path) {
      state.innerHTML = `Installed and on your PATH. You're all set.`;
      wrap.hidden = true;
      $("#install-btn").textContent = "Re-install shim";
    } else if (s.installed && !s.on_path) {
      state.innerHTML = `Shim installed, but its folder isn't on your PATH yet. Add this line to the <strong>end</strong> of your <code>~/.zshrc</code> (so it wins over version managers like nvm), then open a new terminal:`;
      $("#path-line").textContent = s.path_line;
      wrap.hidden = false;
      $("#install-btn").textContent = "Re-install shim";
    } else {
      state.innerHTML = `Not installed yet. Click below to create the <code>vercel</code> shim.`;
      wrap.hidden = true;
      $("#install-btn").textContent = "Install shim";
    }
  } catch (err) {
    $("#shim-state").textContent = String(err);
  }
}

$("#install-btn").addEventListener("click", async () => {
  const btn = $("#install-btn");
  btn.disabled = true;
  try {
    const s = await invoke("install_shims");
    toast("Shim installed");
    if (!s.on_path) {
      $("#path-line").textContent = s.path_line;
      $("#path-line-wrap").hidden = false;
    }
    await refreshSetup();
  } catch (err) {
    toast(err, true);
  } finally {
    btn.disabled = false;
  }
});

$("#copy-path").addEventListener("click", async () => {
  try {
    await navigator.clipboard.writeText($("#path-line").textContent);
    toast("Copied to clipboard");
  } catch (_) {
    toast("Copy failed", true);
  }
});

$("#doctor-btn").addEventListener("click", async () => {
  const btn = $("#doctor-btn");
  btn.disabled = true;
  btn.textContent = "Running…";
  const list = $("#doctor-list");
  list.innerHTML = `<div class="muted">Checking…</div>`;
  try {
    const checks = await invoke("doctor");
    list.innerHTML = checks
      .map(
        (c) => `<div class="check">
          <span class="mark ${c.ok ? "ok" : "bad"}">${c.ok ? "✓" : "✕"}</span>
          <span class="c-label">${esc(c.label)}</span>
          <span class="c-detail mono">${esc(c.detail)}</span>
        </div>`
      )
      .join("");
  } catch (err) {
    list.innerHTML = `<div class="form-error">${esc(String(err))}</div>`;
  } finally {
    btn.disabled = false;
    btn.textContent = "Run checks";
  }
});

$("#import-btn").addEventListener("click", async () => {
  const btn = $("#import-btn");
  btn.disabled = true;
  btn.textContent = "Importing…";
  const log = $("#import-log");
  try {
    const lines = await invoke("import_direnv", { tokensDir: null });
    log.hidden = false;
    log.textContent = lines.length ? lines.join("\n") : "Nothing to import.";
    toast("Import finished");
    await refresh();
  } catch (err) {
    log.hidden = false;
    log.textContent = String(err);
  } finally {
    btn.disabled = false;
    btn.textContent = "Import existing setup";
  }
});

// ---- boot -----------------------------------------------------------------
window.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !modal.hidden) closeModal();
});

(async function boot() {
  try {
    const home = await invoke("home_dir");
    $("#probe-path").value = home;
    $("#rule-path").placeholder = home + "/Dev";
    await refresh();
    await probe();
  } catch (err) {
    toast("Failed to load: " + err, true);
  }
})();
