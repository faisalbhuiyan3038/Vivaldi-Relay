use std::fs;
use std::path::PathBuf;

// Include module sources or link as needed for tests
// Since we have a binary crate, let's test via module declaration or integration
#[path = "../src/patcher.rs"]
mod patcher;

#[path = "../src/config.rs"]
mod config;

#[path = "../src/bundler.rs"]
mod bundler;

use config::{ModConfig, ModItem, ModType};

fn setup_temp_dir(test_name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push("vivaldi_interceptor_test");
    p.push(test_name);
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn test_html_patch_idempotency() {
    let temp_dir = setup_temp_dir("idempotency");
    let html_file = temp_dir.join("window.html");

    let original_html = r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="UTF-8" />
  <title>Vivaldi</title>
  <link rel="stylesheet" href="style/common.css" />
</head>
<body>
  <div id="app"></div>
</body>
</html>"#;

    fs::write(&html_file, original_html).unwrap();

    // 1. Initial state: not patched
    assert!(!patcher::is_patched(&html_file).unwrap());

    // 2. First patch: should succeed and modify file
    let newly_patched = patcher::patch_window_html(&html_file).unwrap();
    assert!(newly_patched);
    assert!(patcher::is_patched(&html_file).unwrap());

    let patched_content = fs::read_to_string(&html_file).unwrap();
    assert!(patched_content.contains("inject_bundle.js"));
    assert!(patched_content.contains("<!-- VIVALDI-JIT-HOOK -->"));
    assert!(patched_content.contains("</body>"));

    // Check backup was created
    let backup_file = temp_dir.join("window.html.orig");
    assert!(backup_file.exists());
    assert_eq!(fs::read_to_string(&backup_file).unwrap(), original_html);

    // Count occurrences of inject_bundle.js: must be exactly 1
    let count = patched_content.matches("inject_bundle.js").count();
    assert_eq!(count, 1);

    // 3. Second patch: idempotent, should NOT modify or duplicate
    let second_patch = patcher::patch_window_html(&html_file).unwrap();
    assert!(!second_patch); // was already patched

    let second_content = fs::read_to_string(&html_file).unwrap();
    assert_eq!(second_content.matches("inject_bundle.js").count(), 1);

    // 4. Unpatch: restores original
    let unpatched = patcher::unpatch_window_html(&html_file).unwrap();
    assert!(unpatched);
    assert!(!patcher::is_patched(&html_file).unwrap());
    assert_eq!(fs::read_to_string(&html_file).unwrap(), original_html);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_html_patch_without_body_tag() {
    let temp_dir = setup_temp_dir("no_body_tag");
    let html_file = temp_dir.join("window.html");

    let minimal_html = "<div>Minimal content</div>";
    fs::write(&html_file, minimal_html).unwrap();

    let patched = patcher::patch_window_html(&html_file).unwrap();
    assert!(patched);

    let content = fs::read_to_string(&html_file).unwrap();
    assert!(content.contains("inject_bundle.js"));
    assert!(content.starts_with(minimal_html));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_config_serialization_and_toggle() {
    let mut config = ModConfig {
        version: 1,
        vivaldi_path: Some(r"C:\Vivaldi\Application\vivaldi.exe".to_string()),
        auto_patch: true,
        mods: vec![
            ModItem {
                name: "CustomTheme.css".to_string(),
                mod_type: ModType::Css,
                enabled: true,
                order: 10,
                description: "Test theme".to_string(),
            },
            ModItem {
                name: "CustomNav.js".to_string(),
                mod_type: ModType::Js,
                enabled: false,
                order: 5,
                description: "Test nav".to_string(),
            },
        ],
        ..ModConfig::default()
    };

    let json = serde_json::to_string_pretty(&config).unwrap();
    let deserialized: ModConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.version, 1);
    assert_eq!(deserialized.mods.len(), 2);
    assert!(deserialized.mods[0].enabled);
    assert_eq!(deserialized.mods[0].order, 10);
    assert!(!deserialized.mods[1].enabled);
    assert_eq!(deserialized.ui_ready_selector, "#browser");

    // Toggle mod in memory
    let found = config.mods.iter_mut().find(|m| m.name == "CustomTheme.css").unwrap();
    found.enabled = !found.enabled;
    assert!(!found.enabled);
}

#[test]
fn test_bundler_compilation() {
    let temp_dir = setup_temp_dir("bundler");
    let resources_dir = temp_dir.join("resources").join("vivaldi");
    fs::create_dir_all(&resources_dir).unwrap();

    let css_file = temp_dir.join("test_style.css");
    fs::write(&css_file, "body { background: red !important; }").unwrap();

    let js_file = temp_dir.join("test_script.js");
    fs::write(&js_file, "console.log('hello world mod');").unwrap();

    let mut config = ModConfig {
        version: 1,
        vivaldi_path: None,
        auto_patch: true,
        mods: Vec::new(),
        ..ModConfig::default()
    };

    let test_app_dir = temp_dir.join("app_data");
    std::env::set_var("VIVALDI_MOD_MANAGER_DIR", &test_app_dir);
    let _ = ModConfig::ensure_directories();

    let css_imported = config.import_mod(&css_file).unwrap();
    let js_imported = config.import_mod(&js_file).unwrap();
    assert_eq!(css_imported, "test_style.css");
    assert_eq!(js_imported, "test_script.js");

    let result = bundler::compile_bundle(&config, &resources_dir).unwrap();
    assert!(result.css_count >= 1);
    assert!(result.js_count >= 1);

    let bundle_css = fs::read_to_string(&result.css_path).unwrap();
    assert!(bundle_css.contains("background: red !important"));
    assert!(bundle_css.contains("test_style.css"));

    let bundle_js = fs::read_to_string(&result.js_path).unwrap();
    assert!(bundle_js.contains("hello world mod"));
    assert!(bundle_js.contains("test_script.js"));
    assert!(bundle_js.contains("waitForUI"));
    assert!(bundle_js.contains("inject_bundle.css"));

    std::env::remove_var("VIVALDI_MOD_MANAGER_DIR");
    let _ = fs::remove_dir_all(&temp_dir);
}
