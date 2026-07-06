pub mod ssh;
pub mod store;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Serialize;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

use ssh::{Hy2Config, Hy2User, VlessInbound};
use store::{Meta, Settings};

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    store::ensure_dir(&dir);
    Ok(dir)
}

#[derive(Serialize)]
struct ConfigEntry {
    name: String,
    link: String,
    description: String,
    created_ms: i64,
    expires_ms: Option<i64>,
    expired: bool,
}

#[derive(Serialize)]
struct ServerInfo {
    host: String,
    port: u16,
    sni: String,
    obfs: String,
    count: usize,
    /// VLESS Reality port from x-ui, or 0 when no such inbound is present.
    vless_port: u16,
    /// How many hysteria2 users were auto-paired with a per-user VLESS UUID.
    vless_matched: usize,
}

#[derive(Serialize)]
struct LoadResult {
    info: ServerInfo,
    entries: Vec<ConfigEntry>,
}

fn effective_sni(s: &Settings) -> String {
    if s.sni.trim().is_empty() {
        s.host.clone()
    } else {
        s.sni.trim().to_string()
    }
}

fn build_hy2_link(s: &Settings, cfg: &Hy2Config, u: &Hy2User) -> String {
    let mut query = format!("insecure=1&sni={}", effective_sni(s));
    if !cfg.obfs_type.is_empty() {
        query.push_str(&format!(
            "&obfs={}&obfs-password={}",
            cfg.obfs_type, cfg.obfs_password
        ));
    }
    format!(
        "hysteria2://{}:{}@{}:{}/?{}#{}",
        u.name, u.password, s.host, cfg.port, query, u.name
    )
}

/// Build the per-user VLESS Reality link from the x-ui inbound + this user's UUID.
fn build_vless_link(host: &str, vi: &VlessInbound, uuid: &str, name: &str) -> String {
    format!(
        "vless://{uuid}@{host}:{port}?type=tcp&security=reality&flow={flow}&pbk={pbk}&fp=chrome&sni={sni}&sid={sid}#{name}-vless",
        uuid = uuid,
        host = host,
        port = vi.port,
        flow = vi.flow,
        pbk = vi.public_key,
        sni = vi.server_name,
        sid = vi.short_id,
        name = name,
    )
}

/// Resolve the mobile (VLESS) leg for a hysteria2 user:
///  1. per-user VLESS from x-ui, matched to the user by name (case-insensitive);
///  2. otherwise the static `vless_link` from settings as a manual fallback.
fn mobile_leg(s: &Settings, u: &Hy2User, vless: Option<&VlessInbound>) -> Option<String> {
    if let Some(vi) = vless {
        if !vi.public_key.is_empty() {
            if let Some(c) = vi.clients.iter().find(|c| c.email.eq_ignore_ascii_case(&u.name)) {
                return Some(build_vless_link(&s.host, vi, &c.uuid, &u.name));
            }
        }
    }
    let manual = s.vless_link.trim();
    if manual.is_empty() {
        None
    } else {
        Some(manual.to_string())
    }
}

/// hysteria2 is the WiFi leg (`w=`); VLESS the mobile leg (`m=`). When a mobile
/// leg exists we emit a coffee://bundle so the client auto-switches by network;
/// otherwise a plain hysteria2 link.
fn build_link(s: &Settings, cfg: &Hy2Config, u: &Hy2User, vless: Option<&VlessInbound>) -> String {
    let hy2 = build_hy2_link(s, cfg, u);
    match mobile_leg(s, u, vless) {
        Some(vless_link) => {
            let w = URL_SAFE_NO_PAD.encode(hy2.as_bytes());
            let m = URL_SAFE_NO_PAD.encode(vless_link.as_bytes());
            format!("coffee://bundle?w={}&m={}", w, m)
        }
        None => hy2,
    }
}

// ── Commands ────────────────────────────────────────────────────────────────

#[tauri::command]
fn get_settings(app: AppHandle) -> Result<Settings, String> {
    Ok(store::load_settings(&data_dir(&app)?))
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    store::save_settings(&data_dir(&app)?, &settings)
}

/// Connect, prune expired users, return the live list with metadata.
#[tauri::command]
fn load_configs(app: AppHandle) -> Result<LoadResult, String> {
    let dir = data_dir(&app)?;
    let settings = store::load_settings(&dir);
    if settings.host.trim().is_empty() {
        return Err("NO_SERVER".into());
    }
    if !ssh::hysteria_installed(&settings)? {
        return Err(format!(
            "На сервере не найден {} — Hysteria2 не установлен",
            ssh::HY_CONFIG
        ));
    }

    let mut meta = store::load_meta(&dir);
    let mut cfg = ssh::read_config(&settings)?;
    // VLESS is an optional add-on: failing to read it must not break the list.
    let vless = ssh::read_vless_inbound(&settings).unwrap_or(None);
    let now = now_ms();

    let expired: Vec<String> = cfg
        .users
        .iter()
        .filter(|u| {
            meta.get(&u.name)
                .and_then(|m| m.expires_ms)
                .map(|e| e <= now)
                .unwrap_or(false)
        })
        .map(|u| u.name.clone())
        .collect();

    if !expired.is_empty() {
        for name in &expired {
            ssh::remove_user(&settings, name)?;
            meta.remove(name);
        }
        store::save_meta(&dir, &meta)?;
        cfg = ssh::read_config(&settings)?;
    }

    let entries: Vec<ConfigEntry> = cfg
        .users
        .iter()
        .map(|u| {
            let m = meta.get(&u.name).cloned().unwrap_or_default();
            ConfigEntry {
                name: u.name.clone(),
                link: build_link(&settings, &cfg, u, vless.as_ref()),
                description: m.description,
                created_ms: m.created_ms,
                expires_ms: m.expires_ms,
                expired: false,
            }
        })
        .collect();

    let vless_matched = vless
        .as_ref()
        .map(|vi| {
            cfg.users
                .iter()
                .filter(|u| vi.clients.iter().any(|c| c.email.eq_ignore_ascii_case(&u.name)))
                .count()
        })
        .unwrap_or(0);

    Ok(LoadResult {
        info: ServerInfo {
            host: settings.host.clone(),
            port: cfg.port,
            sni: effective_sni(&settings),
            obfs: cfg.obfs_type.clone(),
            count: cfg.users.len(),
            vless_port: vless.as_ref().map(|vi| vi.port).unwrap_or(0),
            vless_matched,
        },
        entries,
    })
}

/// Create a new config: random password, add on server, store metadata.
#[tauri::command]
fn create_config(
    app: AppHandle,
    name: String,
    description: String,
    ttl_days: i64,
) -> Result<ConfigEntry, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Укажи имя".into());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("Имя: только латиница, цифры, _ и -".into());
    }

    let dir = data_dir(&app)?;
    let settings = store::load_settings(&dir);
    let password = random_password(16);
    ssh::add_user(&settings, &name, &password)?;

    let now = now_ms();
    let expires_ms = if ttl_days > 0 {
        Some(now + ttl_days * 86_400_000)
    } else {
        None
    };

    let mut meta = store::load_meta(&dir);
    meta.insert(
        name.clone(),
        Meta {
            description: description.trim().to_string(),
            created_ms: now,
            expires_ms,
        },
    );
    store::save_meta(&dir, &meta)?;

    let cfg = ssh::read_config(&settings)?;
    let vless = ssh::read_vless_inbound(&settings).unwrap_or(None);
    let user = Hy2User {
        name: name.clone(),
        password,
    };
    Ok(ConfigEntry {
        link: build_link(&settings, &cfg, &user, vless.as_ref()),
        name,
        description: description.trim().to_string(),
        created_ms: now,
        expires_ms,
        expired: false,
    })
}

#[tauri::command]
fn delete_config(app: AppHandle, name: String) -> Result<(), String> {
    let dir = data_dir(&app)?;
    let settings = store::load_settings(&dir);
    ssh::remove_user(&settings, &name)?;
    let mut meta = store::load_meta(&dir);
    meta.remove(&name);
    store::save_meta(&dir, &meta)
}

fn random_password(len: usize) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut buf = vec![0u8; len];
    getrandom::getrandom(&mut buf).expect("OS RNG unavailable");
    buf.iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            load_configs,
            create_config,
            delete_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
