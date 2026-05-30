//! SSH layer. Shells out to the system `ssh` (key auth) — same philosophy as
//! splitbox spawning `sing-box`. Reads and minimally edits the standalone
//! Hysteria2 config so the rest of the file is preserved.

use serde::Serialize;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::store::Settings;

pub const HY_CONFIG: &str = "/etc/hysteria/config.yaml";

#[derive(Debug, Clone, Serialize)]
pub struct Hy2User {
    pub name: String,
    pub password: String,
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
        a.push("-i".into());
        a.push(s.key_path.trim().into());
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
