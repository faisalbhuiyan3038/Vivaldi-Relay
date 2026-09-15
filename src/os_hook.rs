use crate::discovery::encode_wide;
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::Security::{
    AllocateAndInitializeSid, CheckTokenMembership, FreeSid, SECURITY_NT_AUTHORITY,
};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows_sys::Win32::System::Threading::{WaitForSingleObject, INFINITE};
use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const SECURITY_BUILTIN_DOMAIN_RID: u32 = 0x00000020;
const DOMAIN_ALIAS_RID_ADMINS: u32 = 0x00000220;

#[derive(Debug, Clone)]
pub enum HookStatus {
    NotInstalled,
    Installed {
        debugger_path: String,
        matches_current_exe: bool,
    },
}

pub fn is_elevated() -> bool {
    unsafe {
        let mut nt_authority = SECURITY_NT_AUTHORITY;
        let mut admin_sid = std::ptr::null_mut();

        let success = AllocateAndInitializeSid(
            &mut nt_authority,
            2,
            SECURITY_BUILTIN_DOMAIN_RID,
            DOMAIN_ALIAS_RID_ADMINS,
            0,
            0,
            0,
            0,
            0,
            0,
            &mut admin_sid,
        );

        if success == 0 {
            return false;
        }

        let mut is_admin: i32 = 0;
        let check_res = CheckTokenMembership(std::ptr::null_mut(), admin_sid, &mut is_admin);
        FreeSid(admin_sid);

        check_res != 0 && is_admin != 0
    }
}

pub fn relaunch_as_admin(command_args: &str) -> Result<(), String> {
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("Failed to get current executable path: {}", e))?;

    let exe_wide = encode_wide(&current_exe.to_string_lossy());
    let verb_wide = encode_wide("runas");
    let args_wide = encode_wide(command_args);

    unsafe {
        let mut sei: SHELLEXECUTEINFOW = std::mem::zeroed();
        sei.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        sei.fMask = SEE_MASK_NOCLOSEPROCESS;
        sei.lpVerb = verb_wide.as_ptr();
        sei.lpFile = exe_wide.as_ptr();
        sei.lpParameters = args_wide.as_ptr();
        sei.nShow = SW_SHOWNORMAL;

        let success = ShellExecuteExW(&mut sei);
        if success == 0 {
            let err = std::io::Error::last_os_error();
            return Err(format!("UAC Elevation request was cancelled or failed: {}", err));
        }

        if !sei.hProcess.is_null() {
            WaitForSingleObject(sei.hProcess, INFINITE);
            CloseHandle(sei.hProcess);
        }

        Ok(())
    }
}

pub fn get_ifeo_key_path(exe_name: &str) -> String {
    format!(
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\{}",
        exe_name
    )
}

pub fn get_hook_status(exe_name: &str) -> HookStatus {
    unsafe {
        let key_str = get_ifeo_key_path(exe_name);
        let key_wide = encode_wide(&key_str);
        let mut hkey: HKEY = std::mem::zeroed();

        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, key_wide.as_ptr(), 0, KEY_READ, &mut hkey)
            != ERROR_SUCCESS
        {
            return HookStatus::NotInstalled;
        }

        let val_name_wide = encode_wide("Debugger");
        let mut buf = [0u16; 1024];
        let mut buf_len: u32 = (buf.len() * std::mem::size_of::<u16>()) as u32;
        let mut val_type: u32 = 0;

        let status = RegQueryValueExW(
            hkey,
            val_name_wide.as_ptr(),
            std::ptr::null_mut(),
            &mut val_type,
            buf.as_mut_ptr() as *mut u8,
            &mut buf_len,
        );

        RegCloseKey(hkey);

        if status == ERROR_SUCCESS && val_type == REG_SZ {
            let u16_len = (buf_len as usize) / std::mem::size_of::<u16>();
            let s = String::from_utf16_lossy(&buf[..u16_len.saturating_sub(1)]);
            let trimmed = s.trim_matches('\0').trim_matches('"').to_string();

            let current_exe = std::env::current_exe()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();

            let matches = Path::new(&trimmed) == Path::new(&current_exe);

            HookStatus::Installed {
                debugger_path: trimmed,
                matches_current_exe: matches,
            }
        } else {
            HookStatus::NotInstalled
        }
    }
}

pub fn install_hook(exe_name: &str) -> Result<PathBuf, String> {
    if !is_elevated() {
        println!("Requesting Administrator elevation for IFEO registry setup...");
        relaunch_as_admin(&format!("install-hook --target \"{}\"", exe_name))?;
        return Ok(std::env::current_exe().unwrap_or_default());
    }

    let current_exe = std::env::current_exe()
        .map_err(|e| format!("Failed to get current executable path: {}", e))?;

    let debugger_cmd = format!("\"{}\"", current_exe.to_string_lossy());
    let debugger_wide = encode_wide(&debugger_cmd);

    unsafe {
        let key_str = get_ifeo_key_path(exe_name);
        let key_wide = encode_wide(&key_str);
        let mut hkey: HKEY = std::mem::zeroed();
        let mut disposition: u32 = 0;

        let res = RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            key_wide.as_ptr(),
            0,
            std::ptr::null_mut(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            std::ptr::null_mut(),
            &mut hkey,
            &mut disposition,
        );

        if res != ERROR_SUCCESS {
            return Err(format!("Failed to open or create registry key. Error code: {}", res));
        }

        let val_name_wide = encode_wide("Debugger");
        let byte_len = (debugger_wide.len() * std::mem::size_of::<u16>()) as u32;

        let set_res = RegSetValueExW(
            hkey,
            val_name_wide.as_ptr(),
            0,
            REG_SZ,
            debugger_wide.as_ptr() as *const u8,
            byte_len,
        );

        RegCloseKey(hkey);

        if set_res != ERROR_SUCCESS {
            return Err(format!("Failed to set Debugger registry value. Error code: {}", set_res));
        }

        Ok(current_exe)
    }
}

pub fn uninstall_hook(exe_name: &str) -> Result<(), String> {
    if !is_elevated() {
        println!("Requesting Administrator elevation to remove IFEO registry hook...");
        relaunch_as_admin(&format!("uninstall-hook --target \"{}\"", exe_name))?;
        return Ok(());
    }

    unsafe {
        let key_str = get_ifeo_key_path(exe_name);
        let key_wide = encode_wide(&key_str);
        let mut hkey: HKEY = std::mem::zeroed();

        let open_res = RegOpenKeyExW(HKEY_LOCAL_MACHINE, key_wide.as_ptr(), 0, KEY_WRITE, &mut hkey);
        if open_res == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        if open_res != ERROR_SUCCESS {
            return Err(format!("Failed to open registry key. Error code: {}", open_res));
        }

        let val_name_wide = encode_wide("Debugger");
        let del_res = RegDeleteValueW(hkey, val_name_wide.as_ptr());
        RegCloseKey(hkey);

        if del_res != ERROR_SUCCESS && del_res != ERROR_FILE_NOT_FOUND {
            return Err(format!("Failed to delete Debugger registry value. Error code: {}", del_res));
        }

        Ok(())
    }
}
