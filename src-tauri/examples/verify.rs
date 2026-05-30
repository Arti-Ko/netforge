// End-to-end check of the Rust SSH backend against the live server.
// Run: cargo run --example verify   (from src-tauri/)
use netforge_desktop_lib::ssh;
use netforge_desktop_lib::store::Settings;

fn names(c: &ssh::Hy2Config) -> Vec<String> {
    c.users.iter().map(|u| u.name.clone()).collect()
}

fn main() {
    let home = std::env::var("HOME").unwrap();
    let s = Settings {
        host: "203.0.113.10".into(),
        ssh_user: "root".into(),
        ssh_port: 22,
        sni: "vpn.example.com".into(),
        key_path: format!("{home}/.ssh/id_ed25519"),
    };

    println!("→ installed: {:?}", ssh::hysteria_installed(&s));
    let cfg = ssh::read_config(&s).expect("read");
    println!("✓ port={} obfs={} users={:?}", cfg.port, cfg.obfs_type, names(&cfg));

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
