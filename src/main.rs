mod bundler;
mod config;
mod discovery;
mod launcher;
mod os_hook;
mod patcher;

use config::{ModConfig, ModType};
use discovery::VivaldiTarget;
use os_hook::HookStatus;
use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() <= 1 {
        // No arguments: default to JIT launch
        run_jit_launch(None, &[]);
        return;
    }

    let first_arg = &args[1];

    // Check if invoked via IFEO or with an executable path
    let is_exe = first_arg.to_ascii_lowercase().ends_with(".exe");
    if is_exe {
        let target_exe = PathBuf::from(first_arg);
        let forwarded = &args[2..];
        run_jit_launch(Some(&target_exe), forwarded);
        return;
    }

    match first_arg.as_str() {
        "launch" => {
            let forwarded = &args[2..];
            run_jit_launch(None, forwarded);
        }
        "setup" => {
            run_setup();
        }
        "status" => {
            let as_json = args.iter().any(|a| a == "--json");
            run_status(as_json);
        }
        "list-mods" | "list" => {
            let as_json = args.iter().any(|a| a == "--json");
            run_list_mods(as_json);
        }
        "toggle" => {
            if args.len() < 3 {
                eprintln!("Usage: interceptor toggle <mod_name>");
                std::process::exit(1);
            }
            run_toggle(&args[2]);
        }
        "import" => {
            if args.len() < 3 {
                eprintln!("Usage: interceptor import <file_path>");
                std::process::exit(1);
            }
            run_import(&args[2]);
        }
        "patch" => {
            run_patch_command();
        }
        "unpatch" => {
            run_unpatch_command();
        }
        "install-hook" => {
            run_install_hook();
        }
        "uninstall-hook" => {
            run_uninstall_hook();
        }
        "help" | "--help" | "-h" => {
            print_help();
        }
        "version" | "--version" | "-v" => {
            println!("Vivaldi JIT Mod Interceptor v0.1.0");
        }
        _ => {
            // Forward any unrecognized first argument directly as browser argument (e.g. URL)
            let forwarded = &args[1..];
            run_jit_launch(None, forwarded);
        }
    }
}

fn run_jit_launch(target_hint: Option<&Path>, forwarded_args: &[String]) {
    let mut config = ModConfig::load().unwrap_or_default();

    // Resolve Vivaldi
    let config_path_hint = config.vivaldi_path.as_ref().map(PathBuf::from);
    let hint = target_hint.or(config_path_hint.as_deref());

    let target = match discovery::resolve_vivaldi(hint) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[VivaldiModInterceptor] Error discovering Vivaldi: {}", e);
            if launcher::prompt_launch_without_mods(&e) {
                if let Some(h) = hint {
                    let _ = launcher::launch_vivaldi(h, forwarded_args);
                }
            }
            return;
        }
    };

    // JIT Patch verification
    if config.auto_patch {
        if let Err(patch_err) = ensure_patched_and_bundled(&mut config, &target) {
            eprintln!("[VivaldiModInterceptor] JIT Patch error: {}", patch_err);
            if !launcher::prompt_launch_without_mods(&patch_err) {
                return;
            }
        }
    }

    // Launch Vivaldi with IFEO recursion prevention
    if let Err(launch_err) = launcher::launch_vivaldi(&target.exe_path, forwarded_args) {
        eprintln!("[VivaldiModInterceptor] Launch failed: {}", launch_err);
        launcher::prompt_launch_without_mods(&launch_err);
    }
}

fn ensure_patched_and_bundled(config: &mut ModConfig, target: &VivaldiTarget) -> Result<(), String> {
    let patched = patcher::is_patched(&target.window_html)?;
    let bundle_js = target.resources_dir.join("inject_bundle.js");
    let bundle_css = target.resources_dir.join("inject_bundle.css");

    let needs_bundle = !bundle_js.exists() || !bundle_css.exists();

    if !patched || needs_bundle {
        // Compile bundle first
        let _ = bundler::compile_bundle(config, &target.resources_dir)?;

        // Inject script into window.html
        if !patched {
            patcher::patch_window_html(&target.window_html)?;
        }
    }

    Ok(())
}

fn run_setup() {
    println!("=== Vivaldi JIT Mod Interceptor: Setup ===");

    if let Err(e) = ModConfig::ensure_directories() {
        eprintln!("Error creating mod directories: {}", e);
        return;
    }
    println!("✓ Mod storage directory ready: {}", ModConfig::get_app_dir().display());

    let mut config = ModConfig::load().unwrap_or_default();

    // Discover Vivaldi
    match discovery::resolve_vivaldi(None) {
        Ok(target) => {
            println!("✓ Discovered Vivaldi executable: {}", target.exe_path.display());
            println!("✓ Active Vivaldi version: {}", target.version);
            println!("✓ Resources path: {}", target.resources_dir.display());

            config.vivaldi_path = Some(target.exe_path.to_string_lossy().to_string());

            // Check if existing mods can be imported from target's user_mods or .vivaldimods
            let mut imported = 0;
            let existing_user_mods = target.resources_dir.join("user_mods");
            if existing_user_mods.exists() {
                if let Ok(count) = config.auto_import_from_dir(&existing_user_mods) {
                    imported += count;
                }
            }

            let vivaldi_mods_backup = target.app_dir.join(".vivaldimods").join(&target.version);
            if vivaldi_mods_backup.exists() {
                if let Ok(count) = config.auto_import_from_dir(&vivaldi_mods_backup) {
                    imported += count;
                }
            }

            // Also check any mods already inside %APPDATA%\VivaldiModManager\user_mods
            if let Ok(rescan_count) = config.rescan_mods() {
                imported += rescan_count;
            }

            println!("✓ Auto-imported/registered {} mods into configuration", imported);

            // Save config
            if let Err(e) = config.save() {
                eprintln!("Failed to save config: {}", e);
            } else {
                println!("✓ Saved config to: {}", ModConfig::get_config_path().display());
            }

            // Perform initial patch & bundle
            match ensure_patched_and_bundled(&mut config, &target) {
                Ok(_) => println!("✓ Initial JIT patch & bundle compiled successfully!"),
                Err(e) => eprintln!("Initial patch warning: {}", e),
            }
        }
        Err(e) => {
            eprintln!("Vivaldi not automatically detected: {}", e);
            eprintln!("You can set your Vivaldi path manually in: {}", ModConfig::get_config_path().display());
        }
    }

    println!("\nNext steps:");
    println!("1. Run 'interceptor status' to verify health.");
    println!("2. Run 'interceptor list-mods' to view all registered mods.");
    println!("3. Run 'interceptor install-hook' to enable automatic launch interception via IFEO.");
}

fn run_status(as_json: bool) {
    let config = ModConfig::load().unwrap_or_default();
    let hook_status = os_hook::get_hook_status();
    let target_result = discovery::resolve_vivaldi(config.vivaldi_path.as_deref().map(Path::new));

    let (vivaldi_found, vivaldi_exe, version, patched) = match &target_result {
        Ok(t) => {
            let is_p = patcher::is_patched(&t.window_html).unwrap_or(false);
            (true, t.exe_path.to_string_lossy().to_string(), t.version.clone(), is_p)
        }
        Err(_) => (false, String::new(), String::new(), false),
    };

    let total_mods = config.mods.len();
    let enabled_css = config.mods.iter().filter(|m| m.enabled && m.mod_type == ModType::Css).count();
    let enabled_js = config.mods.iter().filter(|m| m.enabled && m.mod_type == ModType::Js).count();

    if as_json {
        let (hook_active, debugger_path, matches_self) = match &hook_status {
            HookStatus::Installed { debugger_path, matches_current_exe } => (true, debugger_path.clone(), *matches_current_exe),
            HookStatus::NotInstalled => (false, String::new(), false),
        };

        let json = serde_json::json!({
            "hook": {
                "active": hook_active,
                "debugger_path": debugger_path,
                "matches_interceptor": matches_self
            },
            "vivaldi": {
                "found": vivaldi_found,
                "path": vivaldi_exe,
                "version": version,
                "patched": patched
            },
            "mods": {
                "total": total_mods,
                "enabled_css": enabled_css,
                "enabled_js": enabled_js
            },
            "app_dir": ModConfig::get_app_dir().to_string_lossy()
        });
        println!("{}", serde_json::to_string_pretty(&json).unwrap());
    } else {
        println!("=== Vivaldi JIT Mod Interceptor: Status ===");
        println!("Configuration Directory: {}", ModConfig::get_app_dir().display());

        match &hook_status {
            HookStatus::Installed { debugger_path, matches_current_exe } => {
                if *matches_current_exe {
                    println!("IFEO Hook: [ACTIVE] Correctly routes vivaldi.exe through this interceptor.");
                } else {
                    println!("IFEO Hook: [WARNING] Installed but points to another debugger: {}", debugger_path);
                }
            }
            HookStatus::NotInstalled => {
                println!("IFEO Hook: [INACTIVE] (Run 'interceptor install-hook' to enable)");
            }
        }

        if vivaldi_found {
            println!("Vivaldi Executable: {}", vivaldi_exe);
            println!("Active Version: {}", version);
            println!("window.html Status: {}", if patched { "Patched [✓]" } else { "Unpatched [ ]" });
        } else {
            println!("Vivaldi Executable: Not found");
        }

        println!("Mods Summary: {} total ({} CSS active, {} JS active)", total_mods, enabled_css, enabled_js);
    }
}

fn run_list_mods(as_json: bool) {
    let mut config = ModConfig::load().unwrap_or_default();
    let _ = config.rescan_mods();

    if as_json {
        let json = serde_json::to_string_pretty(&config.mods).unwrap_or_default();
        println!("{}", json);
    } else {
        println!("=== Registered Mods ===");
        if config.mods.is_empty() {
            println!("No mods currently registered in {}.", ModConfig::get_mods_dir().display());
            println!("Use 'interceptor import <file>' or run 'interceptor setup' to scan existing mods.");
            return;
        }

        println!("{:<4} {:<6} {:<40} {}", "No.", "Type", "Name", "Status");
        println!("{:-<4} {:-<6} {:-<40} {:-<8}", "", "", "", "");

        for (idx, mod_item) in config.mods.iter().enumerate() {
            let status = if mod_item.enabled { "Enabled" } else { "Disabled" };
            println!(
                "{:<4} {:<6} {:<40} [{}]",
                idx + 1,
                mod_item.mod_type.as_str().to_uppercase(),
                mod_item.name,
                status
            );
        }
    }
}

fn run_toggle(mod_name: &str) {
    let mut config = match ModConfig::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load config: {}", e);
            return;
        }
    };

    match config.toggle_mod(mod_name) {
        Ok(new_state) => {
            let state_str = if new_state { "ENABLED" } else { "DISABLED" };
            println!("✓ Mod '{}' is now {}.", mod_name, state_str);

            // Recompile bundle if Vivaldi can be discovered
            if let Ok(target) = discovery::resolve_vivaldi(config.vivaldi_path.as_deref().map(Path::new)) {
                let _ = bundler::compile_bundle(&config, &target.resources_dir);
                println!("✓ Recompiled bundle for active version {}", target.version);
            }
        }
        Err(e) => eprintln!("Error toggling mod: {}", e),
    }
}

fn run_import(file_path_str: &str) {
    let path = PathBuf::from(file_path_str);
    let mut config = match ModConfig::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load config: {}", e);
            return;
        }
    };

    match config.import_mod(&path) {
        Ok(imported_name) => {
            println!("✓ Successfully imported mod '{}'", imported_name);
            if let Ok(target) = discovery::resolve_vivaldi(config.vivaldi_path.as_deref().map(Path::new)) {
                let _ = bundler::compile_bundle(&config, &target.resources_dir);
                println!("✓ Updated active bundle with imported mod.");
            }
        }
        Err(e) => eprintln!("Import failed: {}", e),
    }
}

fn run_patch_command() {
    let config = ModConfig::load().unwrap_or_default();
    match discovery::resolve_vivaldi(config.vivaldi_path.as_deref().map(Path::new)) {
        Ok(target) => {
            match bundler::compile_bundle(&config, &target.resources_dir) {
                Ok(b) => println!("✓ Compiled bundle ({} CSS, {} JS)", b.css_count, b.js_count),
                Err(e) => eprintln!("Error compiling bundle: {}", e),
            }

            match patcher::patch_window_html(&target.window_html) {
                Ok(newly_patched) => {
                    if newly_patched {
                        println!("✓ Patched window.html at {}", target.window_html.display());
                    } else {
                        println!("✓ window.html is already patched.");
                    }
                }
                Err(e) => eprintln!("Error patching window.html: {}", e),
            }
        }
        Err(e) => eprintln!("Error discovering Vivaldi: {}", e),
    }
}

fn run_unpatch_command() {
    let config = ModConfig::load().unwrap_or_default();
    match discovery::resolve_vivaldi(config.vivaldi_path.as_deref().map(Path::new)) {
        Ok(target) => match patcher::unpatch_window_html(&target.window_html) {
            Ok(restored) => {
                if restored {
                    println!("✓ Restored original window.html at {}", target.window_html.display());
                } else {
                    println!("window.html was not patched.");
                }
            }
            Err(e) => eprintln!("Error restoring window.html: {}", e),
        },
        Err(e) => eprintln!("Error discovering Vivaldi: {}", e),
    }
}

fn run_install_hook() {
    match os_hook::install_hook() {
        Ok(exe) => {
            println!("✓ Successfully installed IFEO launch interceptor hook in Windows Registry!");
            println!("Debugger path: {}", exe.display());
            println!("Vivaldi will now automatically launch through this interceptor.");
        }
        Err(e) => eprintln!("Failed to install IFEO hook: {}", e),
    }
}

fn run_uninstall_hook() {
    match os_hook::uninstall_hook() {
        Ok(_) => println!("✓ Successfully removed IFEO launch interceptor hook from Windows Registry."),
        Err(e) => eprintln!("Failed to uninstall IFEO hook: {}", e),
    }
}

fn print_help() {
    println!(
r#"Vivaldi JIT Mod Interceptor - Automatic CSS/JS Mod Loader

USAGE:
    interceptor [SUBCOMMAND]
    interceptor [VIVALDI_ARGUMENTS...]

SUBCOMMANDS:
    launch [ARGS...]      JIT-checks Vivaldi, applies mods if needed, and launches
    setup                 Initial setup: creates directories and auto-imports existing mods
    status [--json]       Display hook status, Vivaldi installation info, and mod stats
    list-mods [--json]    List all registered mods and their enabled/disabled states
    toggle <MOD_NAME>     Toggle a mod between enabled and disabled
    import <FILE_PATH>    Import a new .css or .js mod file into the manager
    patch                 Manually compile bundles and patch window.html
    unpatch               Restore pristine window.html without mod hooks
    install-hook          Register IFEO debugger hook (elevates via UAC if needed)
    uninstall-hook        Remove IFEO debugger hook (elevates via UAC if needed)
    help                  Print this help message
"#);
}
