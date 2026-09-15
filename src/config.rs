use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ModType {
    #[serde(rename = "css")]
    Css,
    #[serde(rename = "js")]
    Js,
}

impl ModType {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "css" => Some(ModType::Css),
            "js" => Some(ModType::Js),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ModType::Css => "css",
            ModType::Js => "js",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModItem {
    pub name: String,
    pub mod_type: ModType,
    pub enabled: bool,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModConfig {
    pub version: u32,
    pub vivaldi_path: Option<String>,
    pub auto_patch: bool,
    pub mods: Vec<ModItem>,
}

impl Default for ModConfig {
    fn default() -> Self {
        Self {
            version: 1,
            vivaldi_path: None,
            auto_patch: true,
            mods: Vec::new(),
        }
    }
}

impl ModConfig {
    pub fn get_app_dir() -> PathBuf {
        if let Ok(custom) = std::env::var("VIVALDI_MOD_MANAGER_DIR") {
            PathBuf::from(custom)
        } else if let Ok(appdata) = std::env::var("APPDATA") {
            PathBuf::from(appdata).join("VivaldiModManager")
        } else {
            PathBuf::from(".").join(".vivaldi_mod_manager")
        }
    }

    pub fn get_config_path() -> PathBuf {
        Self::get_app_dir().join("mods_config.json")
    }

    pub fn get_mods_dir() -> PathBuf {
        Self::get_app_dir().join("user_mods")
    }

    pub fn get_css_dir() -> PathBuf {
        Self::get_mods_dir().join("css")
    }

    pub fn get_js_dir() -> PathBuf {
        Self::get_mods_dir().join("js")
    }

    pub fn ensure_directories() -> Result<(), String> {
        let app_dir = Self::get_app_dir();
        fs::create_dir_all(Self::get_css_dir())
            .map_err(|e| format!("Failed to create css mods directory: {}", e))?;
        fs::create_dir_all(Self::get_js_dir())
            .map_err(|e| format!("Failed to create js mods directory: {}", e))?;
        if !app_dir.exists() {
            fs::create_dir_all(&app_dir)
                .map_err(|e| format!("Failed to create app directory: {}", e))?;
        }
        Ok(())
    }

    pub fn load() -> Result<Self, String> {
        let path = Self::get_config_path();
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read config from {}: {}", path.display(), e))?;

        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse config JSON: {}", e))
    }

    pub fn save(&self) -> Result<(), String> {
        Self::ensure_directories()?;
        let path = Self::get_config_path();
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        fs::write(&path, content)
            .map_err(|e| format!("Failed to write config to {}: {}", path.display(), e))?;

        Ok(())
    }

    pub fn toggle_mod(&mut self, name: &str) -> Result<bool, String> {
        let name_lower = name.to_ascii_lowercase();
        let found = self.mods.iter_mut().find(|m| {
            m.name.to_ascii_lowercase() == name_lower
                || m.name.to_ascii_lowercase() == format!("{}.css", name_lower)
                || m.name.to_ascii_lowercase() == format!("{}.js", name_lower)
        });

        match found {
            Some(m) => {
                m.enabled = !m.enabled;
                let new_state = m.enabled;
                self.save()?;
                Ok(new_state)
            }
            None => Err(format!("Mod '{}' not found in configuration", name)),
        }
    }

    #[allow(dead_code)]
    pub fn set_mod_enabled(&mut self, name: &str, enabled: bool) -> Result<(), String> {
        let name_lower = name.to_ascii_lowercase();
        let found = self.mods.iter_mut().find(|m| {
            m.name.to_ascii_lowercase() == name_lower
                || m.name.to_ascii_lowercase() == format!("{}.css", name_lower)
                || m.name.to_ascii_lowercase() == format!("{}.js", name_lower)
        });

        match found {
            Some(m) => {
                m.enabled = enabled;
                self.save()?;
                Ok(())
            }
            None => Err(format!("Mod '{}' not found in configuration", name)),
        }
    }

    pub fn import_mod(&mut self, file_path: &Path) -> Result<String, String> {
        if !file_path.exists() {
            return Err(format!("File does not exist: {}", file_path.display()));
        }

        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| "Invalid file name".to_string())?
            .to_string();

        let ext = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let mod_type = ModType::from_extension(&ext)
            .ok_or_else(|| format!("Unsupported file extension '.{}'. Only .css and .js supported.", ext))?;

        Self::ensure_directories()?;

        let dest_dir = match mod_type {
            ModType::Css => Self::get_css_dir(),
            ModType::Js => Self::get_js_dir(),
        };

        let dest_path = dest_dir.join(&file_name);
        fs::copy(file_path, &dest_path)
            .map_err(|e| format!("Failed to copy mod to {}: {}", dest_path.display(), e))?;

        // Update or register in config
        if let Some(existing) = self.mods.iter_mut().find(|m| m.name.eq_ignore_ascii_case(&file_name)) {
            existing.mod_type = mod_type;
        } else {
            self.mods.push(ModItem {
                name: file_name.clone(),
                mod_type,
                enabled: true,
                description: String::new(),
            });
        }

        self.mods.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
        self.save()?;
        Ok(file_name)
    }

    pub fn auto_import_from_dir(&mut self, source_dir: &Path) -> Result<usize, String> {
        if !source_dir.exists() {
            return Ok(0);
        }

        let mut imported_count = 0;
        let mut visit_stack = vec![source_dir.to_path_buf()];

        while let Some(current_dir) = visit_stack.pop() {
            let entries = match fs::read_dir(&current_dir) {
                Ok(e) => e,
                Err(_) => continue,
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    visit_stack.push(path);
                } else if path.is_file() {
                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        if ModType::from_extension(ext).is_some() {
                            if let Ok(_) = self.import_mod(&path) {
                                imported_count += 1;
                            }
                        }
                    }
                }
            }
        }

        Ok(imported_count)
    }

    pub fn rescan_mods(&mut self) -> Result<usize, String> {
        Self::ensure_directories()?;
        let mut newly_found = 0;

        let scan_folder = |dir: PathBuf, expected_type: ModType, mods: &mut Vec<ModItem>| -> usize {
            let mut count = 0;
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                            if ModType::from_extension(ext) == Some(expected_type) {
                                if !mods.iter().any(|m| m.name.eq_ignore_ascii_case(file_name)) {
                                    mods.push(ModItem {
                                        name: file_name.to_string(),
                                        mod_type: expected_type,
                                        enabled: true,
                                        description: String::new(),
                                    });
                                    count += 1;
                                }
                            }
                        }
                    }
                }
            }
            count
        };

        newly_found += scan_folder(Self::get_css_dir(), ModType::Css, &mut self.mods);
        newly_found += scan_folder(Self::get_js_dir(), ModType::Js, &mut self.mods);

        if newly_found > 0 {
            self.mods.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
            self.save()?;
        }

        Ok(newly_found)
    }
}
