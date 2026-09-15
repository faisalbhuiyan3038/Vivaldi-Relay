use std::fs;
use std::path::Path;

pub const HOOK_SCRIPT_TAG: &str = r#"  <!-- VIVALDI-JIT-HOOK -->
  <script src="inject_bundle.js"></script>"#;

pub const HOOK_IDENTIFIER: &str = "inject_bundle.js";

pub fn is_patched(window_html_path: &Path) -> Result<bool, String> {
    if !window_html_path.exists() {
        return Err(format!("File does not exist: {}", window_html_path.display()));
    }
    let content = fs::read_to_string(window_html_path)
        .map_err(|e| format!("Failed to read {}: {}", window_html_path.display(), e))?;

    Ok(content.contains(HOOK_IDENTIFIER))
}

pub fn patch_window_html(window_html_path: &Path) -> Result<bool, String> {
    if !window_html_path.exists() {
        return Err(format!("File does not exist: {}", window_html_path.display()));
    }

    let content = fs::read_to_string(window_html_path)
        .map_err(|e| format!("Failed to read {}: {}", window_html_path.display(), e))?;

    if content.contains(HOOK_IDENTIFIER) {
        // Already patched
        return Ok(false);
    }

    // Preserve original file if not already backed up
    let orig_backup = window_html_path.with_extension("html.orig");
    if !orig_backup.exists() {
        let _ = fs::copy(window_html_path, &orig_backup);
    }

    let patched_content = inject_into_html(&content);

    // Atomic write
    let tmp_path = window_html_path.with_extension("html.tmp");
    fs::write(&tmp_path, &patched_content)
        .map_err(|e| format!("Failed to write temporary file {}: {}", tmp_path.display(), e))?;

    if let Err(e) = fs::rename(&tmp_path, window_html_path) {
        // Fallback to copy + remove if cross-volume or lock overwrite occurs
        fs::copy(&tmp_path, window_html_path)
            .map_err(|e2| format!("Failed to replace {}: {} (rename error: {})", window_html_path.display(), e2, e))?;
        let _ = fs::remove_file(&tmp_path);
    }

    Ok(true)
}

pub fn unpatch_window_html(window_html_path: &Path) -> Result<bool, String> {
    if !window_html_path.exists() {
        return Err(format!("File does not exist: {}", window_html_path.display()));
    }

    let orig_backup = window_html_path.with_extension("html.orig");
    if orig_backup.exists() {
        fs::copy(&orig_backup, window_html_path)
            .map_err(|e| format!("Failed to restore backup from {}: {}", orig_backup.display(), e))?;
        return Ok(true);
    }

    // Otherwise remove injected lines manually without altering other user scripts
    let content = fs::read_to_string(window_html_path)
        .map_err(|e| format!("Failed to read {}: {}", window_html_path.display(), e))?;

    if !content.contains(HOOK_IDENTIFIER) {
        return Ok(false);
    }

    let cleaned = content
        .lines()
        .filter(|line| !line.contains(HOOK_IDENTIFIER) && !line.contains("VIVALDI-JIT-HOOK"))
        .collect::<Vec<_>>()
        .join("\n");

    fs::write(window_html_path, cleaned)
        .map_err(|e| format!("Failed to write cleaned file: {}", e))?;

    Ok(true)
}

pub fn inject_into_html(html: &str) -> String {
    // Pure injection: keep all existing user code untouched, only inject our hook before </body>
    let lower = html.to_ascii_lowercase();
    if let Some(pos) = lower.rfind("</body>") {
        let mut result = String::with_capacity(html.len() + HOOK_SCRIPT_TAG.len() + 10);
        result.push_str(&html[..pos]);
        result.push_str(HOOK_SCRIPT_TAG);
        result.push('\n');
        result.push_str(&html[pos..]);
        result
    } else {
        // Fallback: append at the end
        format!("{}\n{}\n", html, HOOK_SCRIPT_TAG)
    }
}
