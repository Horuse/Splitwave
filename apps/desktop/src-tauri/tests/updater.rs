use serde_json::Value;
use splitwave_lib::commands::UpdateMetadata;
use std::fs;
use std::path::Path;
use tauri::Resource;

struct MockUpdateResource {
    pub version: String,
}

impl Resource for MockUpdateResource {}

#[test]
fn test_webview_resource_table_isolation_and_lookup() {
    let mut webview_resources = tauri::ResourceTable::default();
    let mut app_resources = tauri::ResourceTable::default();

    let update = MockUpdateResource {
        version: "1.2.0".to_string(),
    };

    let webview_rid = webview_resources.add(update);
    let retrieved = webview_resources.get::<MockUpdateResource>(webview_rid);
    assert!(retrieved.is_ok());
    assert_eq!(retrieved.unwrap().version, "1.2.0");

    let app_update = MockUpdateResource {
        version: "1.2.0".to_string(),
    };
    let app_rid = app_resources.add(app_update);

    let failed_lookup = webview_resources.get::<MockUpdateResource>(app_rid);
    assert!(failed_lookup.is_err());
}

#[test]
fn test_update_metadata_serialization_contract() {
    let meta = UpdateMetadata {
        rid: 42,
        current_version: "1.1.0".to_string(),
        version: "1.2.0".to_string(),
        date: Some("2026-09-11T18:00:00Z".to_string()),
        body: Some("Release notes".to_string()),
        raw_json: serde_json::json!({ "version": "1.2.0" }),
    };

    let serialized = serde_json::to_string(&meta).expect("must serialize");
    let v: Value = serde_json::from_str(&serialized).expect("must parse json");

    assert_eq!(v["rid"], 42);
    assert_eq!(v["currentVersion"], "1.1.0");
    assert_eq!(v["version"], "1.2.0");
    assert_eq!(v["date"], "2026-09-11T18:00:00Z");
    assert_eq!(v["body"], "Release notes");
    assert!(v["rawJson"].is_object());

    assert!(v.get("current_version").is_none());
    assert!(v.get("raw_json").is_none());
}

#[test]
fn test_tauri_conf_updater_configuration() {
    let conf_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
    let content = fs::read_to_string(&conf_path).expect("failed to read tauri.conf.json");
    let conf: Value = serde_json::from_str(&content).expect("invalid json in tauri.conf.json");

    let pkg_version = env!("CARGO_PKG_VERSION");
    let conf_version = conf["version"].as_str().expect("version string missing");
    assert_eq!(conf_version, pkg_version);

    assert_eq!(conf["bundle"]["createUpdaterArtifacts"], true);

    let endpoints = conf["plugins"]["updater"]["endpoints"]
        .as_array()
        .expect("plugins.updater.endpoints must be an array");
    assert!(!endpoints.is_empty());

    for ep in endpoints {
        let ep_str = ep.as_str().expect("endpoint must be a string");
        assert!(ep_str.starts_with("https://"));
        assert!(ep_str.ends_with("/latest.json"));
        assert!(url::Url::parse(ep_str).is_ok());
    }

    let pubkey = conf["plugins"]["updater"]["pubkey"]
        .as_str()
        .expect("plugins.updater.pubkey must be a string");
    assert!(!pubkey.is_empty());
    assert!(pubkey.starts_with("dW50cnVzdGVk"));
    assert!(base64::Engine::decode(&base64::prelude::BASE64_STANDARD, pubkey).is_ok());
}
