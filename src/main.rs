mod bundler;
mod config;
mod discovery;
mod launcher;
mod os_hook;
mod patcher;

use config::{ModConfig, ModType};
use discovery::VivaldiTarget;
use os_hook::HookStatus;
use std::io::{self, Write};
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
            let portable = args.iter().any(|a| a == "--portable");
            let path_arg = get_flag_value(&args, "--path").or_else(|| get_flag_value(&args, "-p"));
            run_setup(portable, path_arg.as_deref());
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
            let path_arg = get_flag_value(&args, "--path").or_else(|| get_flag_value(&args, "-p"));
            let target_arg = get_flag_value(&args, "--target");
            let non_interactive = args.iter().any(|a| a == "--non-interactive");
            run_install_hook(path_arg.as_deref(), target_arg.as_deref(), non_interactive);
        }
        "uninstall-hook" => {
            let target_arg = get_flag_value(&args, "--target");
            run_uninstall_hook(target_arg.as_deref());
        }
        "help" | "--help" | "-h" => {
            print_help();
        }
        "version" | "--version" | "-v" => {
            println!("Vivaldi JIT Mod Interceptor v0.1.2");
        }
        _ => {
            // Forward any unrecognized first argument directly as browser argument (e.g. URL)
            let forwarded = &args[1..];
            run_jit_launch(None, forwarded);
        }
    }
}

fn get_flag_value(args: &[String], flag: &str) -> Option<String> {
    for (i, arg) in args.iter().enumerate() {
        if arg == flag && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
        if let Some(stripped) = arg.strip_prefix(&format!("{}=", flag)) {
            return Some(stripped.to_string());
        }
    }
    None
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

    // Launch Vivaldi with IFEO hardlink bypass
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

fn select_or_confirm_vivaldi_path(
    explicit_arg: Option<&str>,
    config: &mut ModConfig,
) -> Result<PathBuf, String> {
    // 1. Explicit CLI argument given
    if let Some(arg) = explicit_arg {
        let p = PathBuf::from(arg.trim().trim_matches('"'));
        let target_exe = if p.is_file() {
            p
        } else if p.is_dir() {
            if p.join("vivaldi.exe").is_file() {
                p.join("vivaldi.exe")
            } else if p.join("vivaldi_snapshot.exe").is_file() {
                p.join("vivaldi_snapshot.exe")
            } else {
                return Err(format!("No vivaldi.exe found in directory {}", p.display()));
            }
        } else {
            return Err(format!("Specified path does not exist: {}", p.display()));
        };

        let clean = discovery::strip_unc_prefix(&target_exe.canonicalize().unwrap_or(target_exe));
        config.vivaldi_path = Some(clean.to_string_lossy().to_string());
        if let Some(file_name) = clean.file_name().and_then(|n| n.to_str()) {
            config.exe_name = file_name.to_string();
        }
        let _ = config.save();
        return Ok(clean);
    }

    let discovered = discovery::discover_all_installations();

    // If explicit arg was not given, always prompt interactively
    let current_configured = config.vivaldi_path.as_deref();

    println!("\n=== Vivaldi Installation Selection ===");
    println!("The interceptor needs to know where Vivaldi is installed on this machine.");
    println!("Path guideline: You can specify either the full path to 'vivaldi.exe'");
    println!("(e.g., M:\\Vivaldi\\Application\\vivaldi.exe)");
    println!("or the folder where 'vivaldi.exe' lives (e.g., M:\\Vivaldi\\Application).\n");

    if !discovered.is_empty() {
        println!("Discovered installation(s) on this system:");
        for (i, inst) in discovered.iter().enumerate() {
            let is_cur = current_configured.map(|c| c.eq_ignore_ascii_case(&inst.exe_path.to_string_lossy())).unwrap_or(false);
            let cur_marker = if is_cur { " [Current]" } else { "" };
            let kind = if inst.is_snapshot { " [Snapshot]" } else { "" };
            println!("  [{}] {}{}{} (version {})", i + 1, inst.exe_path.display(), kind, cur_marker, inst.version);
        }
        println!("  [C] Specify custom path manually");
    } else {
        println!("No standard Vivaldi installations were automatically discovered.");
        println!("  [C] Specify custom path manually");
    }

    let default_choice = if let Some(existing) = current_configured {
        println!("\nPress Enter to keep current path: [{}]", existing);
        Some(existing.to_string())
    } else if let Some(first) = discovered.first() {
        println!("\nPress Enter to use default: [{}]", first.exe_path.display());
        Some(first.exe_path.to_string_lossy().to_string())
    } else {
        None
    };

    print!("Selection: ");
    let _ = io::stdout().flush();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() || input.trim().is_empty() {
        if let Some(def) = default_choice {
            let p = PathBuf::from(def);
            config.vivaldi_path = Some(p.to_string_lossy().to_string());
            if let Some(file_name) = p.file_name().and_then(|n| n.to_str()) {
                config.exe_name = file_name.to_string();
            }
            let _ = config.save();
            return Ok(p);
        }
    }

    let trimmed = input.trim().trim_matches('"');

    if let Ok(idx) = trimmed.parse::<usize>() {
        if idx >= 1 && idx <= discovered.len() {
            let chosen = &discovered[idx - 1];
            println!("✓ Selected: {}", chosen.exe_path.display());
            config.vivaldi_path = Some(chosen.exe_path.to_string_lossy().to_string());
            config.exe_name = chosen
                .exe_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("vivaldi.exe")
                .to_string();
            let _ = config.save();
            return Ok(chosen.exe_path.clone());
        }
    }

    let custom_path_str = if trimmed.eq_ignore_ascii_case("c") {
        print!("Enter path to vivaldi.exe (or the folder where it lives): ");
        let _ = io::stdout().flush();
        let mut custom_input = String::new();
        io::stdin().read_line(&mut custom_input).map_err(|e| e.to_string())?;
        custom_input.trim().trim_matches('"').to_string()
    } else {
        trimmed.to_string()
    };

    let p = PathBuf::from(&custom_path_str);
    let target_exe = if p.is_file() {
        p
    } else if p.is_dir() {
        if p.join("vivaldi.exe").is_file() {
            p.join("vivaldi.exe")
        } else if p.join("vivaldi_snapshot.exe").is_file() {
            p.join("vivaldi_snapshot.exe")
        } else {
            return Err(format!("Could not find vivaldi.exe in directory {}", p.display()));
        }
    } else {
        return Err(format!("Path does not exist: {}", p.display()));
    };

    let clean = discovery::strip_unc_prefix(&target_exe.canonicalize().unwrap_or(target_exe));
    println!("✓ Selected custom path: {}", clean.display());
    config.vivaldi_path = Some(clean.to_string_lossy().to_string());
    if let Some(file_name) = clean.file_name().and_then(|n| n.to_str()) {
        config.exe_name = file_name.to_string();
    }
    let _ = config.save();
    Ok(clean)
}

fn run_setup(portable: bool, path_arg: Option<&str>) {
    println!("=== Vivaldi JIT Mod Interceptor: Setup ===");

    if portable {
        match ModConfig::enable_portable_mode() {
            Ok(dir) => println!("✓ Portable mode enabled in: {}", dir.display()),
            Err(e) => eprintln!("Warning enabling portable mode: {}", e),
        }
    }

    if let Err(e) = ModConfig::ensure_directories() {
        eprintln!("Error creating mod directories: {}", e);
        return;
    }

    let mode_desc = if ModConfig::is_portable() {
        "Portable Mode"
    } else {
        "Standard Mode (%APPDATA%)"
    };
    println!("✓ Mod storage directory ready ({}): {}", mode_desc, ModConfig::get_app_dir().display());

    let mut config = ModConfig::load().unwrap_or_default();

    // Select or confirm Vivaldi path interactively
    let target_exe = match select_or_confirm_vivaldi_path(path_arg, &mut config) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Path selection failed: {}", e);
            return;
        }
    };

    match discovery::resolve_vivaldi(Some(&target_exe)) {
        Ok(target) => {
            println!("✓ Discovered Vivaldi executable: {}", target.exe_path.display());
            println!("✓ Active Vivaldi version: {}", target.version);
            println!("✓ Resources path: {}", target.resources_dir.display());

            config.vivaldi_path = Some(target.exe_path.to_string_lossy().to_string());

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

            if let Ok(rescan_count) = config.rescan_mods() {
                imported += rescan_count;
            }

            println!("✓ Auto-imported/registered {} mods into configuration", imported);

            if let Err(e) = config.save() {
                eprintln!("Failed to save config: {}", e);
            } else {
                println!("✓ Saved config to: {}", ModConfig::get_config_path().display());
            }

            match ensure_patched_and_bundled(&mut config, &target) {
                Ok(_) => println!("✓ Initial JIT patch & bundle compiled successfully!"),
                Err(e) => eprintln!("Initial patch warning: {}", e),
            }
        }
        Err(e) => {
            eprintln!("Error resolving target Vivaldi files: {}", e);
        }
    }

    println!("\nNext steps:");
    println!("1. Run 'interceptor status' to verify health.");
    println!("2. Run 'interceptor list-mods' to view all registered mods.");
    println!("3. Run 'interceptor install-hook' to enable automatic launch interception via IFEO.");
}

fn run_status(as_json: bool) {
    let config = ModConfig::load().unwrap_or_default();
    let exe_name = &config.exe_name;
    let hook_status = os_hook::get_hook_status(exe_name);
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
                "exe_name": config.exe_name
            },
            "mods": {
                "total": total_mods,
                "enabled_css": enabled_css,
                "enabled_js": enabled_js
            }
        });
        println!("{}", serde_json::to_string_pretty(&json).unwrap());
    } else {
        println!("=== Vivaldi JIT Mod Interceptor: Status ===");
        let mode_desc = if ModConfig::is_portable() {
            "Portable Mode"
        } else {
            "Standard Mode (%APPDATA%)"
        };
        println!("Storage: [{}] {}", mode_desc, ModConfig::get_app_dir().display());

        match &hook_status {
            HookStatus::Installed { debugger_path, matches_current_exe } => {
                if *matches_current_exe {
                    println!("IFEO Hook ({}): [ACTIVE] Correctly routes through this interceptor.", exe_name);
                } else {
                    println!("IFEO Hook ({}): [WARNING] Installed but points to another debugger: {}", exe_name, debugger_path);
                }
            }
            HookStatus::NotInstalled => {
                println!("IFEO Hook ({}): [INACTIVE] (Run 'interceptor install-hook' to enable)", exe_name);
            }
        }

        if vivaldi_found {
            println!("Vivaldi Executable: {}", vivaldi_exe);
            println!("Active Version: {}", version);
            println!("window.html Status: {}", if patched { "Patched [✓]" } else { "Unpatched [ ]" });
        } else {
            println!("Vivaldi Executable: Not found");
        }

        println!("Config: Selector '{}', Delay {}ms, Detach Timeout {}ms", config.ui_ready_selector, config.init_delay_ms, config.detach_timeout_ms);
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

        println!("{:<4} {:<6} {:<6} {:<40} {}", "No.", "Order", "Type", "Name", "Status");
        println!("{:-<4} {:-<6} {:-<6} {:-<40} {:-<8}", "", "", "", "", "");

        for (idx, mod_item) in config.mods.iter().enumerate() {
            let status = if mod_item.enabled { "Enabled" } else { "Disabled" };
            println!(
                "{:<4} {:<6} {:<6} {:<40} [{}]",
                idx + 1,
                mod_item.order,
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
        Ok(target) => {
            // 1. Remove vivaldi_real.exe if present
            match launcher::remove_real_executable(&target.exe_path) {
                Ok(removed) => {
                    if removed {
                        println!("✓ Removed hardlink launcher: {}", launcher::get_real_executable_path(&target.exe_path).display());
                    }
                }
                Err(e) => eprintln!("Notice: {}", e),
            }

            // 2. Remove compiled bundle files from resources/vivaldi
            let bundle_css = target.resources_dir.join("inject_bundle.css");
            if bundle_css.exists() {
                match std::fs::remove_file(&bundle_css) {
                    Ok(_) => println!("✓ Removed compiled stylesheet: {}", bundle_css.display()),
                    Err(e) => eprintln!("Error removing stylesheet {}: {}", bundle_css.display(), e),
                }
            }
            let bundle_js = target.resources_dir.join("inject_bundle.js");
            if bundle_js.exists() {
                match std::fs::remove_file(&bundle_js) {
                    Ok(_) => println!("✓ Removed compiled script: {}", bundle_js.display()),
                    Err(e) => eprintln!("Error removing script {}: {}", bundle_js.display(), e),
                }
            }

            // 3. Restore pristine window.html
            match patcher::unpatch_window_html(&target.window_html) {
                Ok(restored) => {
                    if restored {
                        println!("✓ Restored original window.html at {}", target.window_html.display());
                    } else {
                        println!("window.html was not patched.");
                    }
                }
                Err(e) => eprintln!("Error restoring window.html: {}", e),
            }
        }
        Err(e) => eprintln!("Error discovering Vivaldi: {}", e),
    }
}

fn run_install_hook(path_arg: Option<&str>, target_arg: Option<&str>, non_interactive: bool) {
    let mut config = ModConfig::load().unwrap_or_default();

    // If not non-interactive, always ask or confirm path with user interactively
    let target_path = if !non_interactive {
        match select_or_confirm_vivaldi_path(path_arg, &mut config) {
            Ok(p) => {
                println!("Target Vivaldi path: {}", p.display());
                Some(p)
            }
            Err(e) => {
                eprintln!("Path selection failed: {}", e);
                return;
            }
        }
    } else if let Some(arg) = path_arg {
        match select_or_confirm_vivaldi_path(Some(arg), &mut config) {
            Ok(p) => Some(p),
            Err(_) => None,
        }
    } else {
        config.vivaldi_path.as_ref().map(PathBuf::from)
    };

    let exe_name = target_arg
        .map(|s| s.to_string())
        .unwrap_or_else(|| config.exe_name.clone());

    let extra_elevation_args = target_path
        .as_ref()
        .map(|p| format!(" --path \"{}\"", p.display()));

    match os_hook::install_hook(&exe_name, extra_elevation_args.as_deref()) {
        Ok(exe) => {
            println!("✓ Successfully installed IFEO launch interceptor hook for '{}' in Windows Registry!", exe_name);
            println!("Debugger path: {}", exe.display());
            println!("Vivaldi will now automatically launch through this interceptor.");
        }
        Err(e) => eprintln!("Failed to install IFEO hook: {}", e),
    }
}

fn run_uninstall_hook(target_arg: Option<&str>) {
    let config = ModConfig::load().unwrap_or_default();
    let exe_name = target_arg
        .map(|s| s.to_string())
        .unwrap_or_else(|| config.exe_name.clone());

    // Clean up vivaldi_real.exe if present
    if let Ok(target) = discovery::resolve_vivaldi(config.vivaldi_path.as_deref().map(Path::new)) {
        match launcher::remove_real_executable(&target.exe_path) {
            Ok(removed) => {
                if removed {
                    println!("✓ Removed hardlink launcher for: {}", target.exe_path.display());
                }
            }
            Err(e) => eprintln!("Notice: {}", e),
        }
    }

    match os_hook::uninstall_hook(&exe_name) {
        Ok(_) => println!("✓ Successfully removed IFEO launch interceptor hook for '{}' from Windows Registry.", exe_name),
        Err(e) => eprintln!("Failed to uninstall IFEO hook: {}", e),
    }
}

fn print_help() {
    println!(
r#"Vivaldi JIT Mod Interceptor - Automatic CSS/JS Mod Loader

USAGE:
    interceptor [SUBCOMMAND] [OPTIONS]
    interceptor [VIVALDI_ARGUMENTS...]

SUBCOMMANDS:
    launch [ARGS...]      JIT-checks Vivaldi, applies mods if needed, and launches
    setup [OPTIONS]       Initial setup: interactive path selection and mod auto-import
                          Options:
                            --portable           Enable portable mode in executable directory
                            --path <PATH>        Specify vivaldi.exe or parent directory directly
                            --select-path        Force interactive selection menu
    status [--json]       Display hook status, storage mode, Vivaldi installation info, and mod stats
    list-mods [--json]    List all registered mods, load order, and their enabled/disabled states
    toggle <MOD_NAME>     Toggle a mod between enabled and disabled
    import <FILE_PATH>    Import a new .css or .js mod file into the manager
    patch                 Manually compile bundles and patch window.html
    unpatch               Restore pristine window.html without mod hooks
    install-hook [OPTS]   Register IFEO debugger hook (elevates via UAC if needed)
                          Options:
                            --path <PATH>        Specify target vivaldi.exe or directory
                            --target <EXE_NAME>  Specify target exe name (default: vivaldi.exe)
    uninstall-hook [OPTS] Remove IFEO debugger hook (elevates via UAC if needed)
                          Options:
                            --target <EXE_NAME>  Specify target exe name (default: vivaldi.exe)
    help                  Print this help message
"#);
}
