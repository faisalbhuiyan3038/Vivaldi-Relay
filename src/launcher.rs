use crate::discovery::{encode_wide, strip_unc_prefix};
use std::fs;
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::{CloseHandle, BOOL, FALSE};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONWARNING, MB_SYSTEMMODAL, MB_YESNO,
};

pub fn launch_vivaldi(exe_path: &Path, args: &[String]) -> Result<(), String> {
    if !exe_path.exists() {
        return Err(format!("Vivaldi executable not found: {}", exe_path.display()));
    }

    let clean_exe_path = strip_unc_prefix(exe_path);

    // Prepare hardlinked executable name to prevent IFEO infinite loop
    let real_exe = ensure_real_executable(&clean_exe_path)?;

    let working_dir = clean_exe_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| Path::new(".").to_path_buf());

    let cmd_line_str = build_command_line(&real_exe, args);
    let mut cmd_line_wide = encode_wide(&cmd_line_str);
    let working_dir_wide = encode_wide(&working_dir.to_string_lossy());

    unsafe {
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;

        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();

        // Standard process creation without debugger flags (bypasses IFEO via hardlink)
        let success: BOOL = CreateProcessW(
            std::ptr::null(),
            cmd_line_wide.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            FALSE,
            0,
            std::ptr::null(),
            working_dir_wide.as_ptr(),
            &si,
            &mut pi,
        );

        if success == 0 {
            let err = std::io::Error::last_os_error();
            return Err(format!("Failed to create Vivaldi process: {}", err));
        }

        // Clean up handles immediately
        CloseHandle(pi.hProcess);
        CloseHandle(pi.hThread);

        Ok(())
    }
}

fn ensure_real_executable(target_exe: &Path) -> Result<PathBuf, String> {
    let file_name = target_exe
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("vivaldi.exe");

    // e.g. vivaldi.exe -> vivaldi_real.exe
    let real_name = if file_name.eq_ignore_ascii_case("vivaldi.exe") {
        "vivaldi_real.exe"
    } else {
        "vivaldi_app_real.exe"
    };

    let real_exe = target_exe.with_file_name(real_name);

    let needs_update = match (target_exe.metadata(), real_exe.metadata()) {
        (Ok(target_meta), Ok(real_meta)) => {
            target_meta.len() != real_meta.len()
                || target_meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                    != real_meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        }
        _ => true,
    };

    if needs_update {
        if real_exe.exists() {
            let _ = fs::remove_file(&real_exe);
        }

        // Try hardlink first (instant, 0 disk space)
        if let Err(_) = fs::hard_link(target_exe, &real_exe) {
            // Fallback to copy if hardlink fails (e.g. non-NTFS drive)
            fs::copy(target_exe, &real_exe)
                .map_err(|e| format!("Failed to create executable link {}: {}", real_exe.display(), e))?;
        }
    }

    Ok(real_exe)
}


pub fn prompt_launch_without_mods(error_reason: &str) -> bool {
    let title = encode_wide("Vivaldi JIT Mod Interceptor - Warning");
    let text = encode_wide(&format!(
        "Mod injection could not be completed:\n{}\n\nWould you like to launch Vivaldi without mods anyway?",
        error_reason
    ));

    unsafe {
        let result = MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_YESNO | MB_ICONWARNING | MB_SYSTEMMODAL,
        );
        result == IDYES
    }
}

fn build_command_line(exe: &Path, args: &[String]) -> String {
    let mut cmd = String::new();

    // Quote the exe path
    cmd.push('"');
    cmd.push_str(&exe.to_string_lossy());
    cmd.push('"');

    for arg in args {
        cmd.push(' ');
        if arg.contains(' ') || arg.contains('\t') || arg.contains('"') || arg.is_empty() {
            cmd.push('"');
            let mut backslashes = 0;
            for c in arg.chars() {
                match c {
                    '\\' => backslashes += 1,
                    '"' => {
                        for _ in 0..backslashes * 2 + 1 {
                            cmd.push('\\');
                        }
                        backslashes = 0;
                        cmd.push('"');
                    }
                    _ => {
                        for _ in 0..backslashes {
                            cmd.push('\\');
                        }
                        backslashes = 0;
                        cmd.push(c);
                    }
                }
            }
            for _ in 0..backslashes * 2 {
                cmd.push('\\');
            }
            cmd.push('"');
        } else {
            cmd.push_str(arg);
        }
    }

    cmd
}
