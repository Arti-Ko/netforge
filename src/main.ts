import { invoke } from "@tauri-apps/api/core";

// ── Bundle helpers — coffee://bundle?w=<base64url(wifi)>&m=<base64url(mobile)> ──
function generateBundle(wifi: string, mobile?: string): string {
  const encode = (s: string) =>
    btoa(unescape(encodeURIComponent(s)))
      .replace(/\+/g, "-")
      .replace(/\//g, "_")
      .replace(/=/g, "");
  const q = `w=${encode(wifi)}${mobile ? `&m=${encode(mobile)}` : ""}`;
  return `coffee://bundle?${q}`;
}

// ── Types (mirror Rust) ──────────────────────────────────────────────────────
interface Settings {
  host: string;
  ssh_user: string;
  ssh_port: number;
  sni: string;
  key_path: string;
  vless_link: string; // if set, every new config is auto-bundled as coffee://bundle
}
interface ConfigEntry {
  name: string;
  link: string;
  description: string;
  created_ms: number;
  expires_ms: number | null;
  expired: boolean;
}
interface ServerInfo {
  host: string;
  port: number;
  sni: string;
  obfs: string;
  count: number;
}
interface LoadResult {
  info: ServerInfo;
  entries: ConfigEntry[];
}

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const DAY = 86_400_000;

let settings: Settings = { host: "", ssh_user: "root", ssh_port: 22, sni: "", key_path: "", vless_link: "" };
let armedDelete: string | null = null;

// ── Boot ─────────────────────────────────────────────────────────────────────
async function init() {
  settings = await invoke<Settings>("get_settings");
  wire();
  if (!settings.host.trim()) {
    openSettings(true);
  } else {
    loadConfigs();
  }
}

function wire() {
  $("btnCreate").onclick = () => openCreate();
  $("btnSettings").onclick = () => openSettings(false);
  $("cCancel").onclick = () => closeModal("createModal");
  $("cSubmit").onclick = submitCreate;
  $("setCancel").onclick = () => {
    if (settings.host.trim()) closeModal("settingsModal");
  };
  $("setSubmit").onclick = submitSettings;
  wireBundlePanel();
}

// ── Bundle panel ──────────────────────────────────────────────────────────────
function openBundlePanel(prefillWifi?: string) {
  const panel = $("bundlePanel");
  if (prefillWifi !== undefined) {
    ($<HTMLInputElement>("bWifi")).value = prefillWifi;
  }
  panel.style.display = "flex";
  // trigger live update after prefill
  panel.dispatchEvent(new CustomEvent("bundle:refresh"));
}

function wireBundlePanel() {
  const panel = $("bundlePanel");
  const wifiInput = $<HTMLInputElement>("bWifi");
  const mobileInput = $<HTMLInputElement>("bMobile");
  const result = $("bResult");
  const copyBtn = $<HTMLButtonElement>("bCopy");

  $("btnBundle").onclick = () => {
    const isOpen = panel.style.display !== "none";
    panel.style.display = isOpen ? "none" : "flex";
    if (!isOpen) updateResult();
  };
  $("bundleClose").onclick = () => { panel.style.display = "none"; };
  panel.addEventListener("bundle:refresh", updateResult);

  function updateResult() {
    const wifi = wifiInput.value.trim();
    if (!wifi) {
      result.innerHTML = `<span class="bundle-result__placeholder">Введите WiFi link чтобы сгенерировать…</span>`;
      copyBtn.disabled = true;
      return;
    }
    const mobile = mobileInput.value.trim() || undefined;
    const bundle = generateBundle(wifi, mobile);
    result.textContent = bundle;
    copyBtn.disabled = false;
  }

  $("bGenerate").onclick = updateResult;
  wifiInput.oninput = updateResult;
  mobileInput.oninput = updateResult;

  copyBtn.onclick = async () => {
    const text = result.textContent ?? "";
    if (!text || text.includes("Введите")) return;
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      const ta = document.createElement("textarea");
      ta.value = text;
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      ta.remove();
    }
    toast("Bundle скопирован");
  };
}

// ── Load + render ─────────────────────────────────────────────────────────────
async function loadConfigs() {
  renderCenter(`<div class="spinner"></div><div class="muted">Подключаюсь по SSH…</div>`);
  try {
    const res = await invoke<LoadResult>("load_configs");
    render(res);
  } catch (e) {
    const msg = String(e);
    if (msg === "NO_SERVER") {
      openSettings(true);
      return;
    }
    renderCenter(
      `<div class="label">Ошибка</div><div class="muted">${esc(msg)}</div>
       <div style="margin-top:18px"><button class="btn accent" id="retry">Повторить</button></div>`
    );
    $("retry").onclick = loadConfigs;
  }
}

function render(res: LoadResult) {
  $("serverPill").textContent = `${res.info.host} · :${res.info.port}`;
  $("listLabel").textContent = `Конфиги · ${res.info.count}`;

  if (res.entries.length === 0) {
    renderCenter(
      `<div class="label">Пусто</div><div class="muted">Нажми «Создать», чтобы выдать первую персональную ссылку</div>`
    );
    return;
  }

  const list = $("list");
  list.innerHTML = "";
  for (const e of res.entries) list.appendChild(rowEl(e));
}

function rowEl(e: ConfigEntry): HTMLElement {
  const row = document.createElement("div");
  row.className = "row";
  const badge = expiryBadge(e);
  row.innerHTML = `
    <div class="row__avatar">${esc((e.name[0] || "?").toUpperCase())}</div>
    <div class="row__body">
      <div class="row__top">
        <span class="row__name">${esc(e.name)}</span>
        <span class="badge ${badge.cls}">${badge.text}</span>
      </div>
      ${e.description ? `<div class="row__desc">${esc(e.description)}</div>` : ""}
      <div class="row__link" title="${esc(e.link)}">${esc(e.link)}</div>
    </div>
    <div class="row__actions">
      <button class="btn mini" data-act="copy" title="Копировать ссылку">⧉</button>
      <button class="btn mini" data-act="del" title="Удалить">✕</button>
    </div>`;

  row.querySelector<HTMLButtonElement>('[data-act="copy"]')!.onclick = () => {
    copy(e.link);
    toast("Ссылка скопирована");
  };
  const del = row.querySelector<HTMLButtonElement>('[data-act="del"]')!;
  del.onclick = () => onDelete(e.name, del);
  return row;
}

function expiryBadge(e: ConfigEntry): { text: string; cls: string } {
  if (e.expires_ms == null) return { text: "БЕССРОЧНО", cls: "live" };
  const left = e.expires_ms - Date.now();
  if (left <= 0) return { text: "ИСТЁК", cls: "dead" };
  const days = Math.ceil(left / DAY);
  return { text: `ЕЩЁ ${days} ДН.`, cls: days <= 3 ? "soon" : "live" };
}

// ── Actions ────────────────────────────────────────────────────────────────
function onDelete(name: string, btn: HTMLButtonElement) {
  if (armedDelete === name) {
    armedDelete = null;
    doDelete(name);
    return;
  }
  armedDelete = name;
  btn.textContent = "точно?";
  btn.classList.add("accent");
  setTimeout(() => {
    if (armedDelete === name) {
      armedDelete = null;
      btn.textContent = "✕";
      btn.classList.remove("accent");
    }
  }, 2500);
}

async function doDelete(name: string) {
  try {
    await invoke("delete_config", { name });
    toast(`Удалён: ${name}`);
    loadConfigs();
  } catch (e) {
    toast(String(e), true);
  }
}

function openCreate() {
  $("createErr").textContent = "";
  ($("cName") as HTMLInputElement).value = "";
  ($("cDesc") as HTMLTextAreaElement).value = "";
  ($("cTtl") as HTMLSelectElement).value = "0";
  openModal("createModal");
  ($("cName") as HTMLInputElement).focus();
}

async function submitCreate() {
  const name = ($("cName") as HTMLInputElement).value.trim();
  const description = ($("cDesc") as HTMLTextAreaElement).value.trim();
  const ttlDays = parseInt(($("cTtl") as HTMLSelectElement).value, 10) || 0;
  if (!name) {
    $("createErr").textContent = "Укажи имя";
    return;
  }
  const btn = $("cSubmit") as HTMLButtonElement;
  btn.disabled = true;
  btn.textContent = "Создаю…";
  try {
    const entry = await invoke<ConfigEntry>("create_config", { name, description, ttlDays });
    closeModal("createModal");
    if (entry.link.startsWith("coffee://bundle")) {
      // vless_link was configured — bundle was auto-generated, just copy it
      await copy(entry.link);
      toast(`Создан ${entry.name} — bundle скопирован`);
    } else {
      // no vless_link set — open bundle panel so user can add mobile link manually
      openBundlePanel(entry.link);
      toast(`Создан ${entry.name} — добавь VLESS link в настройках для авто-bundle`);
    }
    loadConfigs();
  } catch (e) {
    $("createErr").textContent = String(e);
  } finally {
    btn.disabled = false;
    btn.textContent = "Создать";
  }
}

function openSettings(forced: boolean) {
  $("setErr").textContent = forced ? "Сначала укажи сервер" : "";
  ($("sHost") as HTMLInputElement).value = settings.host;
  ($("sSni") as HTMLInputElement).value = settings.sni;
  ($("sUser") as HTMLInputElement).value = settings.ssh_user || "root";
  ($("sPort") as HTMLInputElement).value = String(settings.ssh_port || 22);
  ($("sKey") as HTMLInputElement).value = settings.key_path;
  ($("sVless") as HTMLInputElement).value = settings.vless_link || "";
  openModal("settingsModal");
}

async function submitSettings() {
  const next: Settings = {
    host: ($("sHost") as HTMLInputElement).value.trim(),
    sni: ($("sSni") as HTMLInputElement).value.trim(),
    ssh_user: ($("sUser") as HTMLInputElement).value.trim() || "root",
    ssh_port: parseInt(($("sPort") as HTMLInputElement).value, 10) || 22,
    key_path: ($("sKey") as HTMLInputElement).value.trim(),
    vless_link: ($("sVless") as HTMLInputElement).value.trim(),
  };
  if (!next.host) {
    $("setErr").textContent = "Укажи хост";
    return;
  }
  try {
    await invoke("save_settings", { settings: next });
    settings = next;
    closeModal("settingsModal");
    loadConfigs();
  } catch (e) {
    $("setErr").textContent = String(e);
  }
}

// ── Helpers ────────────────────────────────────────────────────────────────
async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const ta = document.createElement("textarea");
    ta.value = text;
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    ta.remove();
  }
}

function renderCenter(html: string) {
  $("list").innerHTML = `<div class="center"><div>${html}</div></div>`;
}
function openModal(id: string) { $(id).classList.add("open"); }
function closeModal(id: string) { $(id).classList.remove("open"); }

let toastTimer: number | undefined;
function toast(msg: string, isErr = false) {
  const t = $("toast");
  t.textContent = msg;
  t.className = `toast show${isErr ? " err" : ""}`;
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (t.className = "toast"), 2200);
}

function esc(s: string): string {
  return s.replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!
  );
}

window.addEventListener("DOMContentLoaded", init);
