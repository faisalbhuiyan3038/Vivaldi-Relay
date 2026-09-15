use crate::discovery::encode_wide;
use std::path::Path;
use windows_sys::Win32::Foundation::{CloseHandle, BOOL, FALSE};
use windows_sys::Win32::System::Diagnostics::Debug::{
    ContinueDebugEvent, DebugActiveProcessStop, DebugSetProcessKillOnExit, WaitForDebugEvent,
    DEBUG_EVENT,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DEBUG_ONLY_THIS_PROCESS, PROCESS_INFORMATION, STARTUPINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONWARNING, MB_SYSTEMMODAL, MB_YESNO,
};

const DBG_CONTINUE: i32 = 0x00010002;

pub fn launch_vivaldi(exe_path: &Path, args: &[String]) -> Result<(), String> {
    if !exe_path.exists() {
        return Err(format!("Vivaldi executable not found: {}", exe_path.display()));
    }

    let working_dir = exe_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| Path::new(".").to_path_buf());

    let cmd_line_str = build_command_line(exe_path, args);
    let mut cmd_line_wide = encode_wide(&cmd_line_str);
    let working_dir_wide = encode_wide(&working_dir.to_string_lossy());

    unsafe {
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;

        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();

        // Use DEBUG_ONLY_THIS_PROCESS to bypass Windows IFEO loop
        let flags = DEBUG_ONLY_THIS_PROCESS;

        let success: BOOL = CreateProcessW(
            std::ptr::null(),
            cmd_line_wide.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            FALSE,
            flags,
            std::ptr::null(),
            working_dir_wide.as_ptr(),
            &si,
            &mut pi,
        );

        if success == 0 {
            let err = std::io::Error::last_os_error();
            return Err(format!("Failed to create Vivaldi process: {}", err));
        }

        // Configure debugger to NOT kill child on exit
        DebugSetProcessKillOnExit(FALSE);

        // Process the initial debug event (CREATE_PROCESS_DEBUG_EVENT)
        let mut debug_event: DEBUG_EVENT = std::mem::zeroed();
        if WaitForDebugEvent(&mut debug_event, 3000) != 0 {
            ContinueDebugEvent(debug_event.dwProcessId, debug_event.dwThreadId, DBG_CONTINUE);
        }

        // Immediately detach so Vivaldi runs freely as a standalone app
        DebugActiveProcessStop(pi.dwProcessId);

        // Clean up handles
        CloseHandle(pi.hProcess);
        CloseHandle(pi.hThread);

        Ok(())
    }
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
