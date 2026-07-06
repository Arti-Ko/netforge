// End-to-end check of the Rust SSH backend against the live server.
// Run: cargo run --example verify   (from src-tauri/)
use netforge_desktop_lib::ssh;
use netforge_desktop_lib::store::Settings;

fn names(c: &ssh::Hy2Config) -> Vec<String> {
    c.users.iter().map(|u| u.name.clone()).collect()
}

fn main() {
    let home = std::env::var("HOME").unwrap();
    // Point this at your own server via env vars, e.g.:
    //   NF_HOST=203.0.113.10 NF_SNI=vpn.example.com cargo run --example verify
    let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.into());
    let s = Settings {
        host: env("NF_HOST", "203.0.113.10"),
        ssh_user: env("NF_SSH_USER", "root"),
        ssh_port: env("NF_SSH_PORT", "22").parse().unwrap_or(22),
        sni: env("NF_SNI", "vpn.example.com"),
        key_path: env("NF_KEY", &format!("{home}/.ssh/id_ed25519")),
        vless_link: String::new(),
    };

    println!("→ installed: {:?}", ssh::hysteria_installed(&s));
    let cfg = ssh::read_config(&s).expect("read");
    println!("✓ port={} obfs={} users={:?}", cfg.port, cfg.obfs_type, names(&cfg));

    match ssh::read_vless_inbound(&s) {
        Ok(Some(vi)) => println!(
            "✓ vless :{} sni={} sid={} pbk={}… clients={:?}",
            vi.port,
            vi.server_name,
            vi.short_id,
            vi.public_key.chars().take(8).collect::<String>(),
            vi.clients.iter().map(|c| c.email.clone()).collect::<Vec<_>>()
        ),
        Ok(None) => println!("· vless: x-ui inbound not found"),
        Err(e) => println!("✗ vless read failed: {e}"),
    }

    const TMP: &str = "nf_rust_tmp";
    if names(&cfg).iter().any(|n| n == TMP) {
        ssh::remove_user(&s, TMP).ok();
    }

    println!("→ add {TMP}");
    ssh::add_user(&s, TMP, "TestPass123abc").expect("add");
    let c2 = ssh::read_config(&s).expect("read2");
    let added = names(&c2).iter().any(|n| n == TMP);
    println!("{} after add: {:?}", if added { "✓" } else { "✗" }, names(&c2));

    println!("→ remove {TMP}");
    ssh::remove_user(&s, TMP).expect("remove");
    let c3 = ssh::read_config(&s).expect("read3");
    let removed = !names(&c3).iter().any(|n| n == TMP);
    println!("{} after remove: {:?}", if removed { "✓" } else { "✗" }, names(&c3));

    println!("{}", if added && removed { "\n✅ RUST BACKEND OK" } else { "\n❌ FAILED" });
}
