use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
    KEY_READ, REG_SZ,
};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct VivaldiTarget {
    pub exe_path: PathBuf,
    pub app_dir: PathBuf,
    pub version: String,
    pub version_dir: PathBuf,
    pub resources_dir: PathBuf,
    pub window_html: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredInstallation {
    pub exe_path: PathBuf,
    pub app_dir: PathBuf,
    pub version: String,
    pub is_snapshot: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct VersionParts(Vec<u64>);

impl VersionParts {
    fn parse(s: &str) -> Option<Self> {
        let parts: Result<Vec<u64>, _> = s.split('.').map(|p| p.parse::<u64>()).collect();
        match parts {
            Ok(vec) if !vec.is_empty() => Some(VersionParts(vec)),
            _ => None,
        }
    }
}

pub fn resolve_vivaldi(override_hint: Option<&Path>) -> Result<VivaldiTarget, String> {
    let exe_path = if let Some(hint) = override_hint {
        if hint.is_file() {
            hint.to_path_buf()
        } else if hint.is_dir() {
            if hint.join("vivaldi.exe").is_file() {
                hint.join("vivaldi.exe")
            } else if hint.join("vivaldi_snapshot.exe").is_file() {
                hint.join("vivaldi_snapshot.exe")
            } else {
                return Err(format!(
                    "Directory does not contain vivaldi.exe: {}",
                    hint.display()
                ));
            }
        } else {
            return Err(format!("Specified path does not exist: {}", hint.display()));
        }
    } else {
        let discovered = discover_all_installations();
        if let Some(first) = discovered.first() {
            first.exe_path.clone()
        } else {
            return Err("Could not locate Vivaldi executable. Please specify path via config or arguments.".to_string());
        }
    };

    let raw_canonical = exe_path
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize Vivaldi exe path {}: {}", exe_path.display(), e))?;
    let canonical_exe = strip_unc_prefix(&raw_canonical);

    let app_dir = canonical_exe
        .parent()
        .ok_or_else(|| "Could not determine Vivaldi Application directory".to_string())?
        .to_path_buf();

    // Now find the active version directory
    let (version, version_dir, window_html) = find_active_version_dir(&app_dir)?;

    let resources_dir = version_dir.join("resources").join("vivaldi");

    Ok(VivaldiTarget {
        exe_path: canonical_exe,
        app_dir,
        version,
        version_dir,
        resources_dir,
        window_html,
    })
}

pub fn discover_all_installations() -> Vec<DiscoveredInstallation> {
    let mut results = Vec::new();
    let mut seen_canonical = HashSet::new();
    let mut candidate_paths = Vec::new();

    // 1. Check registry HKCU and HKLM for vivaldi.exe and snapshot
    for exe_name in ["vivaldi.exe", "vivaldi_snapshot.exe"] {
        if let Some(path) = query_app_paths_registry(HKEY_CURRENT_USER, exe_name) {
            candidate_paths.push(path);
        }
        if let Some(path) = query_app_paths_registry(HKEY_LOCAL_MACHINE, exe_name) {
            candidate_paths.push(path);
        }
    }

    // 2. Common known installation paths in %LOCALAPPDATA% and Program Files
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let base = PathBuf::from(local_app_data);
        candidate_paths.push(base.join("Vivaldi").join("Application").join("vivaldi.exe"));
        candidate_paths.push(base.join("Vivaldi Snapshot").join("Application").join("vivaldi.exe"));
    }
    if let Ok(prog_files) = std::env::var("ProgramFiles") {
        let base = PathBuf::from(prog_files);
        candidate_paths.push(base.join("Vivaldi").join("Application").join("vivaldi.exe"));
        candidate_paths.push(base.join("Vivaldi Snapshot").join("Application").join("vivaldi.exe"));
    }
    if let Ok(prog_files_x86) = std::env::var("ProgramFiles(x86)") {
        let base = PathBuf::from(prog_files_x86);
        candidate_paths.push(base.join("Vivaldi").join("Application").join("vivaldi.exe"));
        candidate_paths.push(base.join("Vivaldi Snapshot").join("Application").join("vivaldi.exe"));
    }

    // 3. Dynamic Windows logical drive letters (scans all mounted drives A-Z)
    let drives = get_logical_drive_letters();
    for drive in drives {
        candidate_paths.push(PathBuf::from(format!(r"{}:\Vivaldi\Application\vivaldi.exe", drive)));
        candidate_paths.push(PathBuf::from(format!(r"{}:\Vivaldi Snapshot\Application\vivaldi.exe", drive)));
        candidate_paths.push(PathBuf::from(format!(r"{}:\PortableApps\Vivaldi\Application\vivaldi.exe", drive)));
        candidate_paths.push(PathBuf::from(format!(r"{}:\Program Files\Vivaldi\Application\vivaldi.exe", drive)));
    }

    for candidate in candidate_paths {
        if candidate.is_file() {
            let clean = strip_unc_prefix(&candidate.canonicalize().unwrap_or_else(|_| candidate.clone()));
            if seen_canonical.insert(clean.clone()) {
                if let Some(parent) = clean.parent() {
                    let app_dir = parent.to_path_buf();
                    if let Ok((version, _, _)) = find_active_version_dir(&app_dir) {
                        let is_snapshot = clean.to_string_lossy().to_ascii_lowercase().contains("snapshot");
                        results.push(DiscoveredInstallation {
                            exe_path: clean,
                            app_dir,
                            version,
                            is_snapshot,
                        });
                    }
                }
            }
        }
    }

    results
}

fn get_logical_drive_letters() -> Vec<char> {
    extern "system" {
        fn GetLogicalDrives() -> u32;
    }
    let mask = unsafe { GetLogicalDrives() };
    let mut letters = Vec::new();
    for i in 0..26 {
        if (mask & (1 << i)) != 0 {
            letters.push((b'A' + i as u8) as char);
        }
    }
    letters
}

fn query_app_paths_registry(root: HKEY, exe_name: &str) -> Option<PathBuf> {
    unsafe {
        let subkey_str = format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{}", exe_name);
        let subkey = encode_wide(&subkey_str);
        let mut hkey: HKEY = std::mem::zeroed();

        if RegOpenKeyExW(root, subkey.as_ptr(), 0, KEY_READ, &mut hkey) != ERROR_SUCCESS {
            return None;
        }

        let mut buf = [0u16; 1024];
        let mut buf_len: u32 = (buf.len() * std::mem::size_of::<u16>()) as u32;
        let mut val_type: u32 = 0;

        let status = RegQueryValueExW(
            hkey,
            std::ptr::null(), // default value
            std::ptr::null_mut(),
            &mut val_type,
            buf.as_mut_ptr() as *mut u8,
            &mut buf_len,
        );

        RegCloseKey(hkey);

        if status == ERROR_SUCCESS && val_type == REG_SZ {
            let u16_len = (buf_len as usize) / std::mem::size_of::<u16>();
            let s = String::from_utf16_lossy(&buf[..u16_len.saturating_sub(1)]);
            let trimmed = s.trim_matches('\0').trim_matches('"');
            let path = PathBuf::from(trimmed);
            if path.exists() {
                return Some(path);
            }
        }
        None
    }
}

pub fn find_active_version_dir(app_dir: &Path) -> Result<(String, PathBuf, PathBuf), String> {
    let entries = fs::read_dir(app_dir)
        .map_err(|e| format!("Failed to read application directory {}: {}", app_dir.display(), e))?;

    let mut version_candidates = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if let Some(parsed) = VersionParts::parse(name) {
                    let window_html = path.join("resources").join("vivaldi").join("window.html");
                    if window_html.is_file() {
                        version_candidates.push((parsed, name.to_string(), path, window_html));
                    }
                }
            }
        }
    }

    if version_candidates.is_empty() {
        return Err(format!(
            "No valid Vivaldi version directory with resources/vivaldi/window.html found in {}",
            app_dir.display()
        ));
    }

    // Sort descending by version
    version_candidates.sort_by(|a, b| b.0.cmp(&a.0));

    let (_, version_str, version_dir, window_html) = version_candidates.remove(0);
    Ok((version_str, version_dir, window_html))
}

pub fn encode_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn strip_unc_prefix(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}
