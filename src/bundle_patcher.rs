use std::fs;
use std::path::Path;

const OLD_MAX_WIDTH: &str = "=180";
const NEW_MAX_WIDTH: &str = "=5200";
const DRAG_PINNED_MARKER: &str = "isDraggingPinnedTab";
const LOOKAHEAD_LIMIT: usize = 1000;
const FORBIDDEN_PRECEDING: &str = "-+!<>=*/%&^|?";

pub fn is_bundle_js_patched(bundle_js_path: &Path) -> Result<bool, String> {
    if !bundle_js_path.exists() {
        return Err(format!(
            "File does not exist: {}",
            bundle_js_path.display()
        ));
    }
    let content = fs::read_to_string(bundle_js_path)
        .map_err(|e| format!("Failed to read {}: {}", bundle_js_path.display(), e))?;
    Ok(content.contains(NEW_MAX_WIDTH))
}

pub fn patch_bundle_js(bundle_js_path: &Path) -> Result<bool, String> {
    if !bundle_js_path.exists() {
        return Err(format!(
            "File does not exist: {}",
            bundle_js_path.display()
        ));
    }

    let content = fs::read_to_string(bundle_js_path)
        .map_err(|e| format!("Failed to read {}: {}", bundle_js_path.display(), e))?;

    if content.contains(NEW_MAX_WIDTH) {
        return Ok(false);
    }

    let match_pos = find_max_tab_width_pattern(&content).ok_or_else(|| {
        "Could not find max tab width pattern (=180 followed by isDraggingPinnedTab) in bundle.js. \
         This version of Vivaldi may have changed its internal code structure."
            .to_string()
    })?;

    let orig_backup = bundle_js_path.with_extension("js.orig");
    if !orig_backup.exists() {
        fs::copy(bundle_js_path, &orig_backup).map_err(|e| {
            format!(
                "Failed to create backup {}: {}",
                orig_backup.display(),
                e
            )
        })?;
    }

    let mut patched = String::with_capacity(content.len() + NEW_MAX_WIDTH.len());
    patched.push_str(&content[..match_pos]);
    patched.push_str(NEW_MAX_WIDTH);
    patched.push_str(&content[match_pos + OLD_MAX_WIDTH.len()..]);

    let tmp_path = bundle_js_path.with_extension("js.tmp");
    fs::write(&tmp_path, &patched)
        .map_err(|e| format!("Failed to write temporary file {}: {}", tmp_path.display(), e))?;

    if let Err(e) = fs::rename(&tmp_path, bundle_js_path) {
        fs::copy(&tmp_path, bundle_js_path).map_err(|e2| {
            format!(
                "Failed to replace {}: {} (rename error: {})",
                bundle_js_path.display(),
                e2,
                e
            )
        })?;
        let _ = fs::remove_file(&tmp_path);
    }

    Ok(true)
}

pub fn unpatch_bundle_js(bundle_js_path: &Path) -> Result<bool, String> {
    if !bundle_js_path.exists() {
        return Err(format!(
            "File does not exist: {}",
            bundle_js_path.display()
        ));
    }

    let orig_backup = bundle_js_path.with_extension("js.orig");
    if orig_backup.exists() {
        fs::copy(&orig_backup, bundle_js_path).map_err(|e| {
            format!(
                "Failed to restore backup from {}: {}",
                orig_backup.display(),
                e
            )
        })?;
        return Ok(true);
    }

    let content = fs::read_to_string(bundle_js_path)
        .map_err(|e| format!("Failed to read {}: {}", bundle_js_path.display(), e))?;

    if !content.contains(NEW_MAX_WIDTH) {
        return Ok(false);
    }

    let restored = content.replace(NEW_MAX_WIDTH, OLD_MAX_WIDTH);
    fs::write(bundle_js_path, &restored)
        .map_err(|e| format!("Failed to write restored file: {}", e))?;

    Ok(true)
}

pub fn find_max_tab_width_pattern(content: &str) -> Option<usize> {
    let bytes = content.as_bytes();
    let target = OLD_MAX_WIDTH.as_bytes();
    let marker = DRAG_PINNED_MARKER.as_bytes();
    let target_len = target.len();

    if bytes.len() < target_len {
        return None;
    }

    for i in 0..=(bytes.len() - target_len) {
        if &bytes[i..i + target_len] != target {
            continue;
        }

        if i > 0 {
            let prev = bytes[i - 1] as char;
            if FORBIDDEN_PRECEDING.contains(prev) {
                continue;
            }
        }

        let after_idx = i + target_len;
        if after_idx >= bytes.len() {
            continue;
        }
        let after = bytes[after_idx] as char;
        if after != ';' && after != ',' {
            continue;
        }

        let search_start = after_idx + 1;
        let search_end = (search_start + LOOKAHEAD_LIMIT).min(bytes.len());
        if search_end <= search_start {
            continue;
        }

        let region = &bytes[search_start..search_end];
        if region.windows(marker.len()).any(|w| w == marker) {
            return Some(i);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_pattern_basic_match() {
        let content = r#"foo=180,isDraggingPinnedTab"#;
        assert_eq!(find_max_tab_width_pattern(content), Some(3));
    }

    #[test]
    fn test_find_pattern_semicolon_separator() {
        let content = r#"foo=180;isDraggingPinnedTab"#;
        assert_eq!(find_max_tab_width_pattern(content), Some(3));
    }

    #[test]
    fn test_find_pattern_within_1000_chars() {
        let content = format!(
            "foo=180;{}isDraggingPinnedTab",
            "x".repeat(500)
        );
        assert_eq!(find_max_tab_width_pattern(&content), Some(3));
    }

    #[test]
    fn test_find_pattern_beyond_1000_chars_fails() {
        let content = format!(
            "foo=180;{}isDraggingPinnedTab",
            "x".repeat(1001)
        );
        assert_eq!(find_max_tab_width_pattern(&content), None);
    }

    #[test]
    fn test_find_pattern_rejects_operator_preceding() {
        let content = r#"foo==180,isDraggingPinnedTab"#;
        assert_eq!(find_max_tab_width_pattern(content), None);
    }

    #[test]
    fn test_find_pattern_rejects_minus_preceding() {
        let content = r#"foo-180,isDraggingPinnedTab"#;
        assert_eq!(find_max_tab_width_pattern(content), None);
    }

    #[test]
    fn test_find_pattern_rejects_plus_preceding() {
        let content = r#"foo+180,isDraggingPinnedTab"#;
        assert_eq!(find_max_tab_width_pattern(content), None);
    }

    #[test]
    fn test_find_pattern_requires_semicolon_or_comma_after() {
        let content = r#"foo=180 isDraggingPinnedTab"#;
        assert_eq!(find_max_tab_width_pattern(content), None);
    }

    #[test]
    fn test_find_pattern_no_marker_fails() {
        let content = r#"foo=180,somethingElse"#;
        assert_eq!(find_max_tab_width_pattern(content), None);
    }

    #[test]
    fn test_find_pattern_at_start_of_string() {
        let content = r#"=180,isDraggingPinnedTab"#;
        assert_eq!(find_max_tab_width_pattern(content), Some(0));
    }

    #[test]
    fn test_find_pattern_empty_string() {
        assert_eq!(find_max_tab_width_pattern(""), None);
    }

    #[test]
    fn test_find_pattern_realistic_bundle_content() {
        let content = r#"const maxTabWidth=180,tabState={isDraggingPinnedTab:!1,maxWidth:200};"#;
        let pos = find_max_tab_width_pattern(content);
        assert!(pos.is_some());
        assert_eq!(&content[pos.unwrap()..pos.unwrap() + 4], "=180");
    }
}
