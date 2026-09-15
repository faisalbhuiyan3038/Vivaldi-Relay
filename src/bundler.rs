use crate::config::{ModConfig, ModType};
use std::fs;
use std::path::{Path, PathBuf};

#[allow(dead_code)]
pub struct BundleResult {
    pub css_count: usize,
    pub js_count: usize,
    pub css_path: PathBuf,
    pub js_path: PathBuf,
}

pub fn compile_bundle(config: &ModConfig, target_resources_dir: &Path) -> Result<BundleResult, String> {
    if !target_resources_dir.exists() {
        return Err(format!(
            "Target resources directory does not exist: {}",
            target_resources_dir.display()
        ));
    }

    let css_dir = ModConfig::get_css_dir();
    let js_dir = ModConfig::get_js_dir();

    // 1. Bundle CSS with order sorting
    let mut bundled_css = String::new();
    bundled_css.push_str("/* [Vivaldi JIT Mod Interceptor - Compiled Stylesheet] */\n\n");

    let mut css_mods: Vec<_> = config
        .mods
        .iter()
        .filter(|m| m.enabled && m.mod_type == ModType::Css)
        .collect();
    css_mods.sort_by(|a, b| {
        a.order
            .cmp(&b.order)
            .then_with(|| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()))
    });

    let mut css_count = 0;
    for mod_item in css_mods {
        let file_path = css_dir.join(&mod_item.name);
        if file_path.is_file() {
            if let Ok(content) = fs::read_to_string(&file_path) {
                bundled_css.push_str(&format!("/* === [Mod: {} (order: {})] === */\n", mod_item.name, mod_item.order));
                bundled_css.push_str(&content);
                bundled_css.push_str("\n\n");
                css_count += 1;
            }
        }
    }

    let css_dest = target_resources_dir.join("inject_bundle.css");
    fs::write(&css_dest, bundled_css)
        .map_err(|e| format!("Failed to write {}: {}", css_dest.display(), e))?;

    // 2. Bundle JS with order sorting
    let mut bundled_js = String::new();
    bundled_js.push_str(
r#"// [Vivaldi JIT Mod Interceptor - JavaScript Bootstrap Payload]
(function() {
    'use strict';
    console.info('[VivaldiModInterceptor] Initializing mod loader...');

    // 1. Inject compiled stylesheet into <head>
    function injectStylesheets() {
        if (!document.getElementById('vivaldi-jit-bundle-css')) {
            const link = document.createElement('link');
            link.id = 'vivaldi-jit-bundle-css';
            link.rel = 'stylesheet';
            link.type = 'text/css';
            link.href = 'inject_bundle.css';
            (document.head || document.documentElement).appendChild(link);
        }
    }

    injectStylesheets();

    // 2. Mod execution routine with isolated scopes & error traps
    function executeMods() {
"#);

    let mut js_mods: Vec<_> = config
        .mods
        .iter()
        .filter(|m| m.enabled && m.mod_type == ModType::Js)
        .collect();
    js_mods.sort_by(|a, b| {
        a.order
            .cmp(&b.order)
            .then_with(|| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()))
    });

    let mut js_count = 0;
    for mod_item in js_mods {
        let file_path = js_dir.join(&mod_item.name);
        if file_path.is_file() {
            if let Ok(content) = fs::read_to_string(&file_path) {
                bundled_js.push_str(&format!(
                    "        // --- [Mod: {} (order: {})] ---\n        try {{\n            (function() {{\n",
                    mod_item.name, mod_item.order
                ));
                bundled_js.push_str(&content);
                bundled_js.push_str(&format!(
                    "\n            }})();\n        }} catch (err) {{\n            console.error('[VivaldiModInterceptor] Error executing mod \"{}\":', err);\n        }}\n\n",
                    mod_item.name
                ));
                js_count += 1;
            }
        }
    }

    bundled_js.push_str(&format!(
        "        console.info('[VivaldiModInterceptor] Successfully injected {} CSS and {} JS mods.');\n    }}\n\n",
        css_count, js_count
    ));

    // Dynamic UI readiness selector and delay
    let selector = &config.ui_ready_selector;
    let delay_ms = config.init_delay_ms;

    bundled_js.push_str(&format!(
r#"    // 3. Wait for configured root UI readiness container before executing DOM mods
    function waitForUI() {{
        if (document.querySelector('{}')) {{
            setTimeout(executeMods, {});
        }} else {{
            setTimeout(waitForUI, 200);
        }}
    }}

    if (document.readyState === 'loading') {{
        document.addEventListener('DOMContentLoaded', waitForUI);
    }} else {{
        waitForUI();
    }}
}})();
"#, selector, delay_ms));

    let js_dest = target_resources_dir.join("inject_bundle.js");
    fs::write(&js_dest, bundled_js)
        .map_err(|e| format!("Failed to write {}: {}", js_dest.display(), e))?;

    // Also mirror files into target_resources_dir/user_mods for backwards compatibility
    sync_user_mods_mirror(config, target_resources_dir)?;

    Ok(BundleResult {
        css_count,
        js_count,
        css_path: css_dest,
        js_path: js_dest,
    })
}

fn sync_user_mods_mirror(config: &ModConfig, target_resources_dir: &Path) -> Result<(), String> {
    let mirror_css = target_resources_dir.join("user_mods").join("css");
    let mirror_js = target_resources_dir.join("user_mods").join("js");
    let _ = fs::create_dir_all(&mirror_css);
    let _ = fs::create_dir_all(&mirror_js);

    let src_css = ModConfig::get_css_dir();
    let src_js = ModConfig::get_js_dir();

    for mod_item in &config.mods {
        match mod_item.mod_type {
            ModType::Css => {
                let s = src_css.join(&mod_item.name);
                let d = mirror_css.join(&mod_item.name);
                if s.is_file() {
                    let _ = fs::copy(s, d);
                }
            }
            ModType::Js => {
                let s = src_js.join(&mod_item.name);
                let d = mirror_js.join(&mod_item.name);
                if s.is_file() {
                    let _ = fs::copy(s, d);
                }
            }
        }
    }
    Ok(())
}
