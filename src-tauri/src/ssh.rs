//! SSH layer. Shells out to the system `ssh` (key auth) — same philosophy as
//! splitbox spawning `sing-box`. Reads and minimally edits the standalone
//! Hysteria2 config so the rest of the file is preserved.

use serde::Serialize;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::store::Settings;

pub const HY_CONFIG: &str = "/etc/hysteria/config.yaml";
/// x-ui writes the live xray config here; we read the VLESS REALITY inbound from it.
pub const XUI_CONFIG: &str = "/usr/local/x-ui/bin/config.json";
/// x-ui bundles the xray binary here; used to derive the REALITY public key.
pub const XRAY_BIN: &str = "/usr/local/x-ui/bin/xray-linux-amd64";

#[derive(Debug, Clone, Serialize)]
pub struct Hy2User {
    pub name: String,
    pub password: String,
}

/// One VLESS client (per-user UUID) as stored in the x-ui inbound.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct VlessClient {
    pub email: String,
    pub uuid: String,
}

/// The VLESS + REALITY inbound read from x-ui. Server params (port, sni, sid,
/// pbk, flow) are shared across users; only the per-client UUID differs.
#[derive(Debug, Clone, Serialize)]
pub struct VlessInbound {
    pub port: u16,
    pub server_name: String,
    pub short_id: String,
    /// Empty until [`derive_pubkey`] fills it (needs the xray binary).
    pub public_key: String,
    pub flow: String,
    /// The REALITY private key from the inbound — never leaves the backend.
    pub private_key: String,
    pub clients: Vec<VlessClient>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Hy2Config {
    pub port: u16,
    pub obfs_type: String,
    pub obfs_password: String,
    pub users: Vec<Hy2User>,
}

fn base_args(s: &Settings) -> Vec<String> {
    let mut a = vec![
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "ConnectTimeout=12".into(),
        "-o".into(),
        "StrictHostKeyChecking=accept-new".into(),
    ];
    if !s.key_path.trim().is_empty() {
        // `ssh` is run without a shell, so expand a leading `~/` ourselves.
        let kp = s.key_path.trim();
        let expanded = match kp.strip_prefix("~/") {
            Some(rest) => std::env::var("HOME")
                .map(|h| format!("{h}/{rest}"))
                .unwrap_or_else(|_| kp.to_string()),
            None => kp.to_string(),
        };
        a.push("-i".into());
        a.push(expanded);
    }
    a.push("-p".into());
    a.push(s.ssh_port.to_string());
    a.push(format!("{}@{}", s.ssh_user, s.host));
    a
}

/// Run a remote command, return stdout. Errors carry stderr.
pub fn run(s: &Settings, remote_cmd: &str) -> Result<String, String> {
    let mut args = base_args(s);
    args.push(remote_cmd.to_string());
    let out = Command::new("ssh")
        .args(&args)
        .output()
        .map_err(|e| format!("не удалось запустить ssh: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            format!("ssh завершился с кодом {:?}", out.status.code())
        } else {
            err
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Write `content` to a remote `path` by piping through `cat > path`.
fn write_file(s: &Settings, path: &str, content: &str) -> Result<(), String> {
    let mut args = base_args(s);
    args.push(format!("cat > {path}"));
    let mut child = Command::new("ssh")
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("не удалось запустить ssh: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("нет stdin")?
        .write_all(content.as_bytes())
        .map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

pub fn hysteria_installed(s: &Settings) -> Result<bool, String> {
    let out = run(s, &format!("test -f {HY_CONFIG} && echo OK || echo NO"))?;
    Ok(out.trim() == "OK")
}

pub fn read_config(s: &Settings) -> Result<Hy2Config, String> {
    let text = run(s, &format!("cat {HY_CONFIG}"))?;
    Ok(parse_config(&text))
}

// ── VLESS (REALITY) inbound from x-ui ────────────────────────────────────────

/// Parse the first VLESS+REALITY inbound out of an x-ui `config.json` body.
/// Pure (no SSH) so it can be unit-tested. `public_key` is left empty here —
/// x-ui only stores the private key, so the pubkey is derived separately.
pub fn parse_vless_inbound(json_text: &str) -> Option<VlessInbound> {
    let v: serde_json::Value = serde_json::from_str(json_text).ok()?;
    let inbounds = v.get("inbounds")?.as_array()?;
    for ib in inbounds {
        if ib.get("protocol").and_then(|x| x.as_str()) != Some("vless") {
            continue;
        }
        let ss = match ib.get("streamSettings") {
            Some(x) if x.is_object() => x,
            _ => continue,
        };
        if ss.get("security").and_then(|x| x.as_str()) != Some("reality") {
            continue;
        }
        let rs = match ss.get("realitySettings") {
            Some(x) => x,
            None => continue,
        };
        let port = ib.get("port").and_then(|x| x.as_u64()).unwrap_or(443) as u16;
        let first_str = |key: &str| {
            rs.get(key)
                .and_then(|x| x.as_array())
                .and_then(|a| a.first())
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string()
        };
        let server_name = first_str("serverNames");
        let short_id = first_str("shortIds");
        let private_key = rs
            .get("privateKey")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();

        let mut clients = Vec::new();
        let mut flow = "xtls-rprx-vision".to_string();
        if let Some(arr) = ib
            .get("settings")
            .and_then(|x| x.get("clients"))
            .and_then(|x| x.as_array())
        {
            for c in arr {
                let uuid = c.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                if uuid.is_empty() {
                    continue;
                }
                let email = c
                    .get("email")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                if let Some(f) = c.get("flow").and_then(|x| x.as_str()) {
                    if !f.is_empty() {
                        flow = f.to_string();
                    }
                }
                clients.push(VlessClient { email, uuid });
            }
        }

        return Some(VlessInbound {
            port,
            server_name,
            short_id,
            public_key: String::new(),
            flow,
            private_key,
            clients,
        });
    }
    None
}

/// Derive the REALITY public key from the private key using the server's xray
/// binary (`xray x25519 -i <priv>`). Output line is either `Password: X`,
/// `Password (PublicKey): X`, or the legacy `Public key: X`.
fn derive_pubkey(s: &Settings, private_key: &str) -> Result<String, String> {
    if private_key.is_empty() {
        return Ok(String::new());
    }
    // Try the x-ui-bundled xray first, then a PATH xray as a fallback.
    let out = run(
        s,
        &format!("{XRAY_BIN} x25519 -i {private_key} 2>/dev/null || xray x25519 -i {private_key}"),
    )?;
    for line in out.lines() {
        let l = line.trim();
        if let Some(idx) = l.find(':') {
            let key = l[..idx].to_ascii_lowercase();
            // Match "password"/"public" but never the "PrivateKey" line.
            if !key.contains("private") && (key.contains("password") || key.contains("public")) {
                return Ok(l[idx + 1..].trim().to_string());
            }
        }
    }
    Err("не удалось получить publicKey из `xray x25519`".into())
}

/// Read the VLESS+REALITY inbound from x-ui (if x-ui is present), with its
/// public key derived. Returns `Ok(None)` when x-ui / a matching inbound is
/// absent, so callers can treat VLESS as an optional add-on.
pub fn read_vless_inbound(s: &Settings) -> Result<Option<VlessInbound>, String> {
    let exists = run(s, &format!("test -f {XUI_CONFIG} && echo OK || echo NO"))?;
    if exists.trim() != "OK" {
        return Ok(None);
    }
    let text = run(s, &format!("cat {XUI_CONFIG}"))?;
    let mut vi = match parse_vless_inbound(&text) {
        Some(vi) => vi,
        None => return Ok(None),
    };
    vi.public_key = derive_pubkey(s, &vi.private_key)?;
    Ok(Some(vi))
}

pub fn add_user(s: &Settings, name: &str, password: &str) -> Result<(), String> {
    let text = run(s, &format!("cat {HY_CONFIG}"))?;
    let cfg = parse_config(&text);
    if cfg.users.iter().any(|u| u.name == name) {
        return Err(format!("Пользователь \"{name}\" уже существует"));
    }
    let updated = with_user_added(&text, name, password)?;
    write_and_restart(s, &updated)
}

pub fn remove_user(s: &Settings, name: &str) -> Result<(), String> {
    let text = run(s, &format!("cat {HY_CONFIG}"))?;
    let updated = with_user_removed(&text, name);
    write_and_restart(s, &updated)
}

fn write_and_restart(s: &Settings, content: &str) -> Result<(), String> {
    run(s, &format!("cp {HY_CONFIG} {HY_CONFIG}.bak-$(date +%s)"))?;
    write_file(s, HY_CONFIG, content)?;
    run(s, "systemctl restart hysteria-server")?;
    let active = run(s, "systemctl is-active hysteria-server || true")?;
    if active.trim() != "active" {
        return Err("Сервис hysteria-server не запустился после изменения".into());
    }
    Ok(())
}

// ── YAML parse / minimal edit ───────────────────────────────────────────────

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    let b = v.as_bytes();
    if b.len() >= 2
        && ((b[0] == b'"' && b[b.len() - 1] == b'"') || (b[0] == b'\'' && b[b.len() - 1] == b'\''))
    {
        return v[1..v.len() - 1].to_string();
    }
    v.to_string()
}

fn parse_config(text: &str) -> Hy2Config {
    let lines: Vec<&str> = text.lines().collect();
    let mut port: u16 = 443;
    let mut obfs_type = String::new();
    let mut obfs_password = String::new();
    let mut users = Vec::new();
    let mut in_salamander = false;

    for line in &lines {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("listen:") {
            if let Some(last) = rest.trim().rsplit(':').next() {
                if let Ok(p) = last.trim().parse::<u16>() {
                    port = p;
                }
            }
        }
        if t.starts_with("type:") && t.contains("salamander") {
            obfs_type = "salamander".into();
            in_salamander = true;
        } else if in_salamander {
            if let Some(rest) = t.strip_prefix("password:") {
                obfs_password = unquote(rest);
                in_salamander = false;
            }
        }
    }

    if let Some(up) = lines.iter().position(|l| l.trim() == "userpass:") {
        let base = indent(lines[up]);
        for line in lines.iter().skip(up + 1) {
            if line.trim().is_empty() {
                continue;
            }
            if indent(line) <= base {
                break;
            }
            let t = line.trim();
            if let Some(ci) = t.find(':') {
                if ci > 0 {
                    let name = t[..ci].trim().to_string();
                    let value = unquote(&t[ci + 1..]);
                    users.push(Hy2User {
                        name,
                        password: value,
                    });
                }
            }
        }
    }

    Hy2Config {
        port,
        obfs_type,
        obfs_password,
        users,
    }
}

fn with_user_added(text: &str, name: &str, password: &str) -> Result<String, String> {
    let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let up = lines
        .iter()
        .position(|l| l.trim() == "userpass:")
        .ok_or("В конфиге нет блока auth.userpass — несовместимый формат")?;
    let base = indent(&lines[up]);
    let mut child_indent = base + 4;
    for line in lines.iter().skip(up + 1) {
        if line.trim().is_empty() {
            continue;
        }
        if indent(line) <= base {
            break;
        }
        child_indent = indent(line);
        break;
    }
    let entry = format!("{}{}: \"{}\"", " ".repeat(child_indent), name, password);
    lines.insert(up + 1, entry);
    Ok(format!("{}\n", lines.join("\n")))
}

fn with_user_removed(text: &str, name: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let up = match lines.iter().position(|l| l.trim() == "userpass:") {
        Some(i) => i,
        None => return text.to_string(),
    };
    let base = indent(lines[up]);
    let prefix = format!("{}:", name);
    let mut out = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        if i > up && indent(line) > base && line.trim().starts_with(&prefix) {
            continue;
        }
        out.push(*line);
    }
    format!("{}\n", out.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const XUI_SAMPLE: &str = r#"{
      "inbounds": [
        { "listen": "127.0.0.1", "port": 62789, "protocol": "tunnel", "streamSettings": null },
        { "listen": "0.0.0.0", "port": 443, "protocol": "vless",
          "settings": { "clients": [
            { "email": "arti", "flow": "xtls-rprx-vision", "id": "9d5af370-e3ed-4660-8761-f2a50af78095" },
            { "email": "mama", "flow": "xtls-rprx-vision", "id": "f211303f-0aea-4042-8a94-9bf2b43607b4" }
          ], "decryption": "none" },
          "streamSettings": { "network": "tcp", "security": "reality",
            "realitySettings": { "privateKey": "iHoaW5FBSmW6vTaUJpgictnuAhyg1eahNwdTnqTvuU8",
              "serverNames": ["www.apple.com"], "shortIds": ["ab857f5e", "4eab32"], "target": "www.apple.com:443" } } }
      ]
    }"#;

    #[test]
    fn parses_vless_reality_inbound() {
        let vi = parse_vless_inbound(XUI_SAMPLE).expect("inbound");
        assert_eq!(vi.port, 443);
        assert_eq!(vi.server_name, "www.apple.com");
        assert_eq!(vi.short_id, "ab857f5e");
        assert_eq!(vi.flow, "xtls-rprx-vision");
        assert_eq!(vi.private_key, "iHoaW5FBSmW6vTaUJpgictnuAhyg1eahNwdTnqTvuU8");
        assert_eq!(vi.clients.len(), 2);
        assert_eq!(vi.clients[0], VlessClient { email: "arti".into(), uuid: "9d5af370-e3ed-4660-8761-f2a50af78095".into() });
        assert!(vi.public_key.is_empty()); // filled by derive_pubkey later
    }

    #[test]
    fn returns_none_without_reality_vless() {
        let no_vless = r#"{"inbounds":[{"port":28443,"protocol":"hysteria2"}]}"#;
        assert!(parse_vless_inbound(no_vless).is_none());
        assert!(parse_vless_inbound("not json").is_none());
    }
}
