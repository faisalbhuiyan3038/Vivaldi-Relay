// src/webui/mod.rs
// Embedded HTTP server for the Vivaldi Mod Manager WebUI.
// Launched via `interceptor webui [--port PORT] [--open]`.

use crate::bundler;
use crate::config::ModConfig;
use crate::discovery;
use crate::os_hook;
use crate::patcher;
use serde_json::Value;
#[allow(unused_imports)]
use std::io::Read; // used via request.as_reader().read_to_end()
use std::path::Path;
use tiny_http::{Method, Response, Server};

static UI_HTML: &str = include_str!("ui.html");

// ─────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────
pub fn run(port: u16, open_browser: bool) {
    let addr = format!("127.0.0.1:{}", port);
    let server = match Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[WebUI] Failed to bind to {}: {}", addr, e);
            eprintln!("[WebUI] Is port {} already in use?", port);
            std::process::exit(1);
        }
    };

    let url = format!("http://127.0.0.1:{}", port);
    println!("╔══════════════════════════════════════════════╗");
    println!("║   Vivaldi Mod Manager  —  Web Interface      ║");
    println!("║                                              ║");
    println!("║   {}   ║", pad_to(&url, 42));
    println!("║                                              ║");
    println!("║   Press Ctrl+C or use the UI to stop.        ║");
    println!("╚══════════════════════════════════════════════╝");

    if open_browser {
        open_in_browser(&url);
    } else {
        println!("[WebUI] Pass --open to auto-open in browser.");
    }

    for mut request in server.incoming_requests() {
        let method = request.method().clone();
        let url_str = request.url().to_string();
        let path = url_str.split('?').next().unwrap_or("/").to_string();
        let query = url_str.split('?').nth(1).unwrap_or("").to_string();

        // Read the body up-front (consumes the reader inside Request)
        let mut body_bytes: Vec<u8> = Vec::new();
        let _ = request.as_reader().read_to_end(&mut body_bytes);
        let body_str = String::from_utf8_lossy(&body_bytes).to_string();

        // Collect headers before routing (clone what we need)
        let content_type = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("Content-Type"))
            .map(|h| h.value.to_string())
            .unwrap_or_default();

        let (status, resp_body, is_html, do_shutdown) = route(
            &method,
            &path,
            &query,
            &body_str,
            &body_bytes,
            &content_type,
        );

        if is_html {
            let header: tiny_http::Header = "Content-Type: text/html; charset=utf-8".parse().unwrap();
            let resp = Response::from_data(resp_body.into_bytes())
                .with_status_code(200)
                .with_header(header);
            let _ = request.respond(resp);
        } else {
            let resp = Response::from_data(resp_body.into_bytes())
                .with_status_code(status)
                .with_header("Content-Type: application/json".parse::<tiny_http::Header>().unwrap())
                .with_header("Access-Control-Allow-Origin: *".parse::<tiny_http::Header>().unwrap());
            let _ = request.respond(resp);
        }

        if do_shutdown {
            println!("[WebUI] Shutdown requested via API. Stopping server.");
            break;
        }
    }

    println!("[WebUI] Server stopped.");
}

fn pad_to(s: &str, width: usize) -> String {
    if s.len() >= width { s.to_string() }
    else { format!("{}{}", s, " ".repeat(width - s.len())) }
}

fn open_in_browser(url: &str) {
    use crate::discovery::encode_wide;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let verb = encode_wide("open");
    let file = encode_wide(url);
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL as i32,
        );
    }
}

// ─────────────────────────────────────────────────────────────
// Router — returns (status, body, is_html, shutdown)
// ─────────────────────────────────────────────────────────────
fn route(
    method: &Method,
    path: &str,
    query: &str,
    body_str: &str,
    body_bytes: &[u8],
    content_type: &str,
) -> (u16, String, bool, bool) {
    match (method, path) {
        (Method::Get, "/") | (Method::Get, "/index.html") => {
            (200, UI_HTML.to_string(), true, false)
        }
        (Method::Get, "/api/status") => {
            let (s, b) = api_status();
            (s, b, false, false)
        }
        (Method::Get, "/api/mods") => {
            let rescan = query.contains("rescan=true");
            let (s, b) = api_list_mods(rescan);
            (s, b, false, false)
        }
        (Method::Get, "/api/config") => {
            let (s, b) = api_get_config();
            (s, b, false, false)
        }
        (Method::Post, "/api/toggle") => {
            let (s, b) = api_toggle(body_str);
            (s, b, false, false)
        }
        (Method::Post, "/api/set-order") => {
            let (s, b) = api_set_order(body_str);
            (s, b, false, false)
        }
        (Method::Post, "/api/import") => {
            let (s, b) = api_import(body_bytes, content_type);
            (s, b, false, false)
        }
        (Method::Post, "/api/config") => {
            let (s, b) = api_save_config(body_str);
            (s, b, false, false)
        }
        (Method::Post, "/api/patch") => {
            let (s, b) = api_patch();
            (s, b, false, false)
        }
        (Method::Post, "/api/unpatch") => {
            let clean = query.contains("clean=true");
            let (s, b) = api_unpatch(clean);
            (s, b, false, false)
        }
        (Method::Post, "/api/install-hook") => {
            let (s, b) = api_install_hook();
            (s, b, false, false)
        }
        (Method::Post, "/api/uninstall-hook") => {
            let (s, b) = api_uninstall_hook();
            (s, b, false, false)
        }
        (Method::Post, "/api/shutdown") => {
            (200, r#"{"message":"Shutting down"}"#.to_string(), false, true)
        }
        _ => {
            (404, r#"{"error":"Not found"}"#.to_string(), false, false)
        }
    }
}

// ─────────────────────────────────────────────────────────────
// API Handlers
// ─────────────────────────────────────────────────────────────

fn api_status() -> (u16, String) {
    let config = ModConfig::load().unwrap_or_default();
    let exe_name = &config.exe_name;
    let hook_status = os_hook::get_hook_status(exe_name);

    use os_hook::HookStatus;
    let (hook_active, debugger_path, matches_self) = match &hook_status {
        HookStatus::Installed { debugger_path, matches_current_exe } => {
            (true, debugger_path.clone(), *matches_current_exe)
        }
        HookStatus::NotInstalled => (false, String::new(), false),
    };

    let vivaldi_hint = config.vivaldi_path.as_deref().map(Path::new);
    let target_result = discovery::resolve_vivaldi(vivaldi_hint);
    let (vivaldi_found, vivaldi_exe, version, patched) = match &target_result {
        Ok(t) => {
            let is_p = patcher::is_patched(&t.window_html).unwrap_or(false);
            (true, t.exe_path.to_string_lossy().to_string(), t.version.clone(), is_p)
        }
        Err(_) => (false, String::new(), String::new(), false),
    };

    let total_mods = config.mods.len();
    let enabled_css = config.mods.iter().filter(|m| m.enabled && m.mod_type == crate::config::ModType::Css).count();
    let enabled_js  = config.mods.iter().filter(|m| m.enabled && m.mod_type == crate::config::ModType::Js).count();

    let json = serde_json::json!({
        "storage": {
            "mode": if ModConfig::is_portable() { "portable" } else { "standard" },
            "app_dir": ModConfig::get_app_dir().to_string_lossy()
        },
        "hook": {
            "active": hook_active,
            "target_image": exe_name,
            "debugger_path": debugger_path,
            "matches_interceptor": matches_self
        },
        "vivaldi": {
            "found": vivaldi_found,
            "path": vivaldi_exe,
            "version": version,
            "patched": patched
        },
        "settings": {
            "ui_ready_selector": config.ui_ready_selector,
            "init_delay_ms": config.init_delay_ms,
            "detach_timeout_ms": config.detach_timeout_ms,
            "exe_name": config.exe_name,
            "auto_patch": config.auto_patch
        },
        "mods": {
            "total": total_mods,
            "enabled_css": enabled_css,
            "enabled_js": enabled_js
        }
    });
    (200, serde_json::to_string(&json).unwrap_or_default())
}

fn api_list_mods(rescan: bool) -> (u16, String) {
    let mut config = match ModConfig::load_or_require_setup() {
        Ok(c) => c,
        Err(e) => return err_json(400, &e),
    };
    if rescan {
        if let Ok(n) = config.rescan_mods() {
            if n > 0 { let _ = config.save(); }
        }
    }
    (200, serde_json::to_string(&config.mods).unwrap_or_default())
}

fn api_get_config() -> (u16, String) {
    let config = match ModConfig::load_or_require_setup() {
        Ok(c) => c,
        Err(e) => return err_json(400, &e),
    };
    (200, serde_json::to_string(&config).unwrap_or_default())
}

fn api_toggle(body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return err_json(400, "Invalid JSON body"),
    };
    let name = match v["name"].as_str() {
        Some(n) => n.to_string(),
        None => return err_json(400, "Missing 'name' field"),
    };

    let mut config = match ModConfig::load_or_require_setup() {
        Ok(c) => c,
        Err(e) => return err_json(400, &e),
    };

    match config.toggle_mod(&name) {
        Ok(new_state) => {
            if let Ok(target) = discovery::resolve_vivaldi(config.vivaldi_path.as_deref().map(Path::new)) {
                let _ = bundler::compile_bundle(&config, &target.resources_dir);
            }
            let json = serde_json::json!({
                "name": name,
                "enabled": new_state,
                "message": format!("Mod '{}' is now {}.", name, if new_state { "enabled" } else { "disabled" })
            });
            (200, serde_json::to_string(&json).unwrap_or_default())
        }
        Err(e) => err_json(400, &e),
    }
}

fn api_set_order(body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return err_json(400, "Invalid JSON body"),
    };
    let name = match v["name"].as_str() {
        Some(n) => n.to_string(),
        None => return err_json(400, "Missing 'name' field"),
    };
    let order = match v["order"].as_i64() {
        Some(o) => o as i32,
        None => return err_json(400, "Missing or invalid 'order' field"),
    };

    let mut config = match ModConfig::load_or_require_setup() {
        Ok(c) => c,
        Err(e) => return err_json(400, &e),
    };

    if let Some(m) = config.mods.iter_mut().find(|m| m.name.eq_ignore_ascii_case(&name)) {
        m.order = order;
    } else {
        return err_json(404, &format!("Mod '{}' not found.", name));
    }

    config.mods.sort_by(|a, b| {
        a.order.cmp(&b.order).then_with(|| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()))
    });
    if let Err(e) = config.save() {
        return err_json(500, &e);
    }
    if let Ok(target) = discovery::resolve_vivaldi(config.vivaldi_path.as_deref().map(Path::new)) {
        let _ = bundler::compile_bundle(&config, &target.resources_dir);
    }
    let json = serde_json::json!({ "name": name, "order": order, "message": "Order updated and bundle recompiled." });
    (200, serde_json::to_string(&json).unwrap_or_default())
}

fn api_import(body_bytes: &[u8], content_type: &str) -> (u16, String) {
    let boundary = extract_boundary(content_type);
    if boundary.is_empty() {
        return err_json(400, "Missing multipart boundary in Content-Type");
    }

    let parts = parse_multipart(body_bytes, &boundary);
    if parts.is_empty() {
        return err_json(400, "No file parts found in multipart body");
    }

    let mut config = match ModConfig::load_or_require_setup() {
        Ok(c) => c,
        Err(e) => return err_json(400, &e),
    };

    let mut imported = Vec::new();
    for (filename, file_bytes) in parts {
        let ext = filename.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        if ext != "css" && ext != "js" {
            continue;
        }
        let tmp_path = std::env::temp_dir().join(&filename);
        if std::fs::write(&tmp_path, &file_bytes).is_err() {
            return err_json(500, &format!("Failed to write temp file for {}", filename));
        }
        match config.import_mod(&tmp_path) {
            Ok(name) => {
                imported.push(name);
                if let Ok(target) = discovery::resolve_vivaldi(config.vivaldi_path.as_deref().map(Path::new)) {
                    let _ = bundler::compile_bundle(&config, &target.resources_dir);
                }
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp_path);
                return err_json(400, &format!("Import failed for {}: {}", filename, e));
            }
        }
        let _ = std::fs::remove_file(&tmp_path);
    }

    if imported.is_empty() {
        return err_json(400, "No valid .css or .js files found in upload");
    }

    let json = serde_json::json!({
        "imported": imported,
        "message": format!("Successfully imported {} mod(s).", imported.len())
    });
    (200, serde_json::to_string(&json).unwrap_or_default())
}

fn api_save_config(body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return err_json(400, "Invalid JSON body"),
    };

    let mut config = ModConfig::load().unwrap_or_default();

    if v["vivaldi_path"].is_null() {
        config.vivaldi_path = None;
    } else if let Some(path) = v["vivaldi_path"].as_str() {
        config.vivaldi_path = if path.is_empty() { None } else { Some(path.to_string()) };
    }
    if let Some(s) = v["ui_ready_selector"].as_str() { config.ui_ready_selector = s.to_string(); }
    if let Some(n) = v["init_delay_ms"].as_u64()     { config.init_delay_ms = n; }
    if let Some(n) = v["detach_timeout_ms"].as_u64() { config.detach_timeout_ms = n as u32; }
    if let Some(s) = v["exe_name"].as_str()          { config.exe_name = s.to_string(); }
    if let Some(b) = v["auto_patch"].as_bool()       { config.auto_patch = b; }

    match config.save() {
        Ok(_) => {
            let json = serde_json::json!({ "message": "Configuration saved successfully." });
            (200, serde_json::to_string(&json).unwrap_or_default())
        }
        Err(e) => err_json(500, &e),
    }
}

fn api_patch() -> (u16, String) {
    let config = match ModConfig::load_or_require_setup() {
        Ok(c) => c,
        Err(e) => return err_json(400, &e),
    };
    if config.vivaldi_path.is_none() {
        return err_json(400, "Vivaldi path not configured. Run setup or configure via the Config tab.");
    }
    match discovery::resolve_vivaldi(config.vivaldi_path.as_deref().map(Path::new)) {
        Ok(target) => {
            let b = match bundler::compile_bundle(&config, &target.resources_dir) {
                Ok(b) => b,
                Err(e) => return err_json(500, &format!("Bundle compilation failed: {}", e)),
            };
            match patcher::patch_window_html(&target.window_html) {
                Ok(newly_patched) => {
                    let msg = if newly_patched {
                        format!("Patched window.html. Bundle: {} CSS, {} JS mods.", b.css_count, b.js_count)
                    } else {
                        format!("window.html already patched. Bundle recompiled: {} CSS, {} JS mods.", b.css_count, b.js_count)
                    };
                    let json = serde_json::json!({ "message": msg });
                    (200, serde_json::to_string(&json).unwrap_or_default())
                }
                Err(e) => err_json(500, &format!("Patch failed: {}", e)),
            }
        }
        Err(e) => err_json(400, &format!("Vivaldi discovery failed: {}", e)),
    }
}

fn api_unpatch(clean: bool) -> (u16, String) {
    let config = match ModConfig::load_or_require_setup() {
        Ok(c) => c,
        Err(e) => return err_json(400, &e),
    };
    if config.vivaldi_path.is_none() {
        return err_json(400, "Vivaldi path not configured.");
    }
    match discovery::resolve_vivaldi(config.vivaldi_path.as_deref().map(Path::new)) {
        Ok(target) => {
            let mut msgs = Vec::new();
            match crate::launcher::remove_real_executable(&target.exe_path) {
                Ok(true)  => msgs.push("Removed hardlink launcher.".to_string()),
                Ok(false) => {}
                Err(e)    => msgs.push(format!("Notice: {}", e)),
            }
            for fname in &["inject_bundle.css", "inject_bundle.js"] {
                let p = target.resources_dir.join(fname);
                if p.exists() { let _ = std::fs::remove_file(&p); msgs.push(format!("Removed {}.", fname)); }
            }
            match patcher::unpatch_window_html(&target.window_html) {
                Ok(true)  => msgs.push("Restored original window.html.".to_string()),
                Ok(false) => msgs.push("window.html was not patched.".to_string()),
                Err(e)    => return err_json(500, &format!("Unpatch failed: {}", e)),
            }
            let orig = target.resources_dir.join("window.html.orig");
            if orig.exists() {
                if clean { let _ = std::fs::remove_file(&orig); msgs.push("Removed window.html.orig backup.".to_string()); }
                else { msgs.push("window.html.orig backup preserved.".to_string()); }
            }
            let json = serde_json::json!({ "message": msgs.join(" ") });
            (200, serde_json::to_string(&json).unwrap_or_default())
        }
        Err(e) => err_json(400, &format!("Vivaldi discovery failed: {}", e)),
    }
}

fn api_install_hook() -> (u16, String) {
    let config = match ModConfig::load_or_require_setup() {
        Ok(c) => c,
        Err(e) => return err_json(400, &e),
    };
    match os_hook::install_hook(&config.exe_name, None) {
        Ok(exe) => {
            let json = serde_json::json!({
                "message": format!("IFEO hook installed for '{}'. Debugger: {}", config.exe_name, exe.display())
            });
            (200, serde_json::to_string(&json).unwrap_or_default())
        }
        Err(e) => err_json(500, &format!("Hook installation failed: {}", e)),
    }
}

fn api_uninstall_hook() -> (u16, String) {
    let config = ModConfig::load().unwrap_or_default();
    if let Ok(target) = discovery::resolve_vivaldi(config.vivaldi_path.as_deref().map(Path::new)) {
        let _ = crate::launcher::remove_real_executable(&target.exe_path);
    }
    match os_hook::uninstall_hook(&config.exe_name) {
        Ok(_) => {
            let json = serde_json::json!({
                "message": format!("IFEO hook removed for '{}'.", config.exe_name)
            });
            (200, serde_json::to_string(&json).unwrap_or_default())
        }
        Err(e) => err_json(500, &format!("Hook removal failed: {}", e)),
    }
}

// ─────────────────────────────────────────────────────────────
// Multipart parser
// ─────────────────────────────────────────────────────────────

fn extract_boundary(content_type: &str) -> String {
    for part in content_type.split(';') {
        let part = part.trim();
        if let Some(val) = part.strip_prefix("boundary=") {
            return val.trim_matches('"').to_string();
        }
    }
    String::new()
}

fn parse_multipart(body: &[u8], boundary: &str) -> Vec<(String, Vec<u8>)> {
    let delimiter = format!("--{}", boundary);
    let delim_bytes = delimiter.as_bytes();
    let mut results = Vec::new();
    let mut cursor = 0usize;

    while cursor < body.len() {
        // Find next boundary
        let Some(found) = find_bytes(&body[cursor..], delim_bytes) else { break };
        cursor += found + delim_bytes.len();

        // Skip \r\n after boundary
        if body.get(cursor..cursor + 2) == Some(b"\r\n") { cursor += 2; }
        // End boundary --
        if body.get(cursor..cursor + 2) == Some(b"--") { break; }

        // Find next boundary to determine this part's end
        let Some(next_found) = find_bytes(&body[cursor..], delim_bytes) else { break };
        let part_end = cursor + next_found;
        // Part bytes (trim trailing \r\n before next delimiter)
        let part_end = if part_end >= 2 && &body[part_end - 2..part_end] == b"\r\n" {
            part_end - 2
        } else { part_end };

        let part = &body[cursor..part_end];

        // Split headers / body at \r\n\r\n
        if let Some(sep) = find_bytes(part, b"\r\n\r\n") {
            let header_str = std::str::from_utf8(&part[..sep]).unwrap_or("");
            let file_bytes = part[sep + 4..].to_vec();
            if let Some(filename) = extract_filename(header_str) {
                results.push((filename, file_bytes));
            }
        }
        cursor += next_found;
    }
    results
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() { return None; }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn extract_filename(headers: &str) -> Option<String> {
    for line in headers.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("content-disposition") {
            for part in line.split(';') {
                let part = part.trim();
                let key = part.to_ascii_lowercase();
                if key.starts_with("filename=") {
                    let val = &part["filename=".len()..];
                    return Some(val.trim_matches('"').to_string());
                }
            }
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────

fn err_json(code: u16, msg: &str) -> (u16, String) {
    let safe = msg.replace('"', "'");
    (code, format!(r#"{{"error":"{}"}}"#, safe))
}
