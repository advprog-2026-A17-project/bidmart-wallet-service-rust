#[test]
fn runtime_database_layer_supports_postgresql_urls_from_compose() {
    let manifest = std::fs::read_to_string("Cargo.toml").expect("read Cargo.toml");
    let server = std::fs::read_to_string("src/server.rs").expect("read server");
    let main = std::fs::read_to_string("src/main.rs").expect("read main");
    let repositories =
        std::fs::read_to_string("src/persistence/repositories.rs").expect("read repositories");
    let migration =
        std::fs::read_to_string("migrations/20260429000000_init.sql").expect("read migration");

    assert!(manifest.contains("\"postgres\""));
    assert!(manifest.contains("\"any\""));
    assert!(server.contains("AnyPoolOptions"));
    assert!(server.contains("install_default_drivers"));
    assert!(server.contains("postgresql://"));
    assert!(main.contains("default_database_url()"));
    assert!(repositories.contains("AnyPool"));
    assert!(repositories.contains("$1"));
    assert!(!repositories.contains("datetime('now')"));
    assert!(!repositories.contains("rowid"));
    assert!(!server.contains("SqliteConnectOptions"));
    assert!(!migration.contains("datetime('now')"));
}
