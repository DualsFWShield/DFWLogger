//! Serbert — Contrôleur et pont utilisateur vers le pilote noyau Windows (Kernel-Level Driver).
//!
//! Gère la communication IOCTL avec `\\.\Serbert`, l'enregistrement du service pilote (type kernel),
//! la synchronisation des règles d'interdiction de processus au niveau Ring 0, et la protection
//! anti-terminaison du processus Big Brother.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;

pub const DRIVER_SERVICE_NAME: &str = "Serbert";
pub const DRIVER_DISPLAY_NAME: &str = "Serbert Kernel Driver (DFWLogger Big Brother)";
pub const DRIVER_DEVICE_PATH: PCWSTR = w!("\\\\.\\Serbert");

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const FILE_DEVICE_SERBERT: u32 = 0x00008271;
const METHOD_BUFFERED: u32 = 0;
const FILE_READ_DATA: u32 = 1;
const FILE_WRITE_DATA: u32 = 2;

const fn ctl_code(device_type: u32, function: u32, method: u32, access: u32) -> u32 {
    (device_type << 16) | (access << 14) | (function << 2) | method
}

pub const IOCTL_SERBERT_REGISTER_CONTROLLER: u32 =
    ctl_code(FILE_DEVICE_SERBERT, 0x800, METHOD_BUFFERED, FILE_READ_DATA | FILE_WRITE_DATA);

pub const IOCTL_SERBERT_ADD_RULE: u32 =
    ctl_code(FILE_DEVICE_SERBERT, 0x801, METHOD_BUFFERED, FILE_READ_DATA | FILE_WRITE_DATA);

pub const IOCTL_SERBERT_CLEAR_RULES: u32 =
    ctl_code(FILE_DEVICE_SERBERT, 0x802, METHOD_BUFFERED, FILE_READ_DATA | FILE_WRITE_DATA);

pub const IOCTL_SERBERT_GET_STATUS: u32 =
    ctl_code(FILE_DEVICE_SERBERT, 0x803, METHOD_BUFFERED, FILE_READ_DATA);

#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug)]
pub struct SerbertRegisterRequest {
    pub controller_pid: u32,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct SerbertRuleRequest {
    pub image_name: [u16; 64],
}

impl Default for SerbertRuleRequest {
    fn default() -> Self {
        Self { image_name: [0u16; 64] }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct SerbertStatusResponse {
    pub version: u32,
    pub active_rules_count: u32,
    pub blocked_executions_count: u32,
    pub controller_pid: u32,
    pub process_notify_active: u8,
    pub _reserved: [u8; 3],
}

/// Tente d'ouvrir un descripteur vers le périphérique noyau `\\.\Serbert`.
pub fn open_driver() -> Option<HANDLE> {
    unsafe {
        let handle = CreateFileW(
            DRIVER_DEVICE_PATH,
            FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
            HANDLE(0),
        );
        if handle.is_ok() && handle.as_ref().unwrap() != &INVALID_HANDLE_VALUE {
            Some(handle.unwrap())
        } else {
            None
        }
    }
}

/// Vérifie si le pilote noyau Serbert est actuellement chargé et répond aux IOCTLs.
pub fn is_driver_running() -> bool {
    if let Some(h) = open_driver() {
        unsafe { let _ = CloseHandle(h); }
        true
    } else {
        false
    }
}

/// Interroge le gestionnaire de services Windows pour l'état du service pilote `Serbert`.
pub fn driver_service_status() -> Option<String> {
    let out = std::process::Command::new("cmd")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["/C", "sc", "query", DRIVER_SERVICE_NAME])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("STATE") {
            if t.contains("RUNNING") {
                return Some("RUNNING".into());
            }
            if t.contains("STOPPED") {
                return Some("STOPPED".into());
            }
        }
    }
    if out.status.success() {
        return Some("OTHER".into());
    }
    None
}

/// Installe le service de pilote noyau Serbert (type= kernel).
pub fn install_driver_service(sys_path: &Path) -> Result<(), String> {
    let path_str = sys_path.to_string_lossy();
    let bin_arg = format!("binPath=\"{}\"", path_str);
    let out = std::process::Command::new("cmd")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "/C",
            "sc",
            "create",
            DRIVER_SERVICE_NAME,
            "type=",
            "kernel",
            "start=",
            "demand",
            &bin_arg,
            "displayName=",
            DRIVER_DISPLAY_NAME,
        ])
        .output()
        .map_err(|e| format!("erreur sc create : {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    if text.contains("FAILED") && !text.contains("1073") {
        return Err(text.trim().to_string());
    }
    Ok(())
}

/// Démarre le pilote noyau Serbert via le SCM.
pub fn start_driver() -> Result<String, String> {
    let out = std::process::Command::new("cmd")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["/C", "sc", "start", DRIVER_SERVICE_NAME])
        .output()
        .map_err(|e| format!("erreur sc start : {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    if out.status.success() || text.contains("1056") {
        Ok(text.trim().to_string())
    } else {
        Err(text.trim().to_string())
    }
}

/// Arrête le pilote noyau Serbert.
pub fn stop_driver() -> Result<String, String> {
    let out = std::process::Command::new("cmd")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["/C", "sc", "stop", DRIVER_SERVICE_NAME])
        .output()
        .map_err(|e| format!("erreur sc stop : {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Désinstalle le service du pilote noyau Serbert.
pub fn uninstall_driver_service() -> Result<(), String> {
    let _ = stop_driver();
    let out = std::process::Command::new("cmd")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["/C", "sc", "delete", DRIVER_SERVICE_NAME])
        .output()
        .map_err(|e| format!("erreur sc delete : {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    if out.status.success() || text.contains("1060") {
        Ok(())
    } else {
        Err(text.trim().to_string())
    }
}

/// Récupère l'état et les compteurs depuis le noyau via IOCTL.
pub fn get_status() -> Option<SerbertStatusResponse> {
    let handle = open_driver()?;
    unsafe {
        let mut resp = SerbertStatusResponse::default();
        let mut bytes_returned = 0u32;
        let ok = DeviceIoControl(
            handle,
            IOCTL_SERBERT_GET_STATUS,
            None,
            0,
            Some(&mut resp as *mut _ as *mut _),
            std::mem::size_of::<SerbertStatusResponse>() as u32,
            Some(&mut bytes_returned),
            None,
        );
        let _ = CloseHandle(handle);
        if ok.is_ok() && bytes_returned as usize >= std::mem::size_of::<SerbertStatusResponse>() {
            Some(resp)
        } else {
            None
        }
    }
}

/// Synchronise la liste des processus bloqués et le PID contrôleur dans le pilote noyau.
pub fn sync_rules(blocked_processes: &[String], controller_pid: u32) -> Result<SerbertStatusResponse, String> {
    let handle = open_driver().ok_or_else(|| "pilote noyau Serbert indisponible".to_string())?;
    unsafe {
        let mut bytes_returned = 0u32;

        // 1. Enregistrement du contrôleur
        let reg = SerbertRegisterRequest { controller_pid };
        let _ = DeviceIoControl(
            handle,
            IOCTL_SERBERT_REGISTER_CONTROLLER,
            Some(&reg as *const _ as *const _),
            std::mem::size_of::<SerbertRegisterRequest>() as u32,
            None,
            0,
            Some(&mut bytes_returned),
            None,
        );

        // 2. Réinitialisation des règles
        let _ = DeviceIoControl(
            handle,
            IOCTL_SERBERT_CLEAR_RULES,
            None,
            0,
            None,
            0,
            Some(&mut bytes_returned),
            None,
        );

        // 3. Ajout de chaque processus bloqué
        for name in blocked_processes {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                continue;
            }
            let mut req = SerbertRuleRequest::default();
            let utf16: Vec<u16> = trimmed.encode_utf16().take(63).collect();
            for (idx, &c) in utf16.iter().enumerate() {
                req.image_name[idx] = c;
            }
            let _ = DeviceIoControl(
                handle,
                IOCTL_SERBERT_ADD_RULE,
                Some(&req as *const _ as *const _),
                std::mem::size_of::<SerbertRuleRequest>() as u32,
                None,
                0,
                Some(&mut bytes_returned),
                None,
            );
        }

        // 4. Lecture du statut mis à jour
        let mut resp = SerbertStatusResponse::default();
        let ok = DeviceIoControl(
            handle,
            IOCTL_SERBERT_GET_STATUS,
            None,
            0,
            Some(&mut resp as *mut _ as *mut _),
            std::mem::size_of::<SerbertStatusResponse>() as u32,
            Some(&mut bytes_returned),
            None,
        );
        let _ = CloseHandle(handle);

        if ok.is_ok() {
            Ok(resp)
        } else {
            Err("échec de lecture du statut noyau".to_string())
        }
    }
}

/// Chemin par défaut du binaire du pilote serbert.sys
pub fn default_driver_path() -> PathBuf {
    let base = std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".into());
    PathBuf::from(base).join("DFWLogger").join("serbert.sys")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ctl_code_calculation() {
        assert_eq!(IOCTL_SERBERT_REGISTER_CONTROLLER, 0x8271E000);
        assert_eq!(IOCTL_SERBERT_GET_STATUS & 0x03, METHOD_BUFFERED);
    }

    #[test]
    fn test_rule_request_packing() {
        let mut req = SerbertRuleRequest::default();
        let name = "discord.exe";
        for (i, c) in name.encode_utf16().enumerate() {
            req.image_name[i] = c;
        }
        assert_eq!(std::mem::size_of::<SerbertRuleRequest>(), 128);
    }
}
