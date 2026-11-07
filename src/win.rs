//! Helpers Win32 : bureau verrouillé/déverrouillé, application au premier plan,
//! DPAPI, registre (démarrage auto), élévation, réseau local.

use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, WIN32_ERROR};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};
use windows::Win32::System::StationsAndDesktops::{CloseDesktop, OpenInputDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_SWITCHDESKTOP};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
};

use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// true si le bureau interactif est accessible (session déverrouillée).
/// false quand Windows est verrouillé, sur l'écran de veille ou une invite de sécurisation (UAC, Ctrl-Alt-Suppr).
pub fn is_unlocked() -> bool {
    unsafe {
        match OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_SWITCHDESKTOP) {
            Ok(h) => {
                let _ = CloseDesktop(h);
                true
            }
            Err(_) => false,
        }
    }
}

/// (chemin exe, titre de fenêtre) de l'application au premier plan.
pub fn foreground_app() -> Option<(String, String)> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0 == 0 {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let exe = if pid != 0 { process_exe(pid) } else { None };

        let len = GetWindowTextLengthW(hwnd).max(0) as usize;
        let mut buf = vec![0u16; len + 1];
        let written = GetWindowTextW(hwnd, &mut buf).max(0) as usize;
        let title = String::from_utf16_lossy(&buf[..written.min(len)]);
        Some((exe.unwrap_or_default(), title))
    }
}

fn process_exe(pid: u32) -> Option<String> {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return None;
        };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let res = QueryFullProcessImageNameW(
            h,
            PROCESS_NAME_WIN32,
            PWSTR::from_raw(buf.as_mut_ptr()),
            &mut len,
        )
        .ok()
        .map(|_| String::from_utf16_lossy(&buf[..len as usize]));
        let _ = CloseHandle(h);
        res
    }
}

// ---- DPAPI (protège la clé en mémoire système, propre au profil utilisateur) ----
// NOTE : les blobs retournés par CryptProtectData sont minuscules et appelés
// occasionnellement ; on n'exécute pas LocalFree (API non exposée dans windows-rs 0.54).

pub fn dpapi_protect(data: &[u8]) -> Result<Vec<u8>, String> {
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        CryptProtectData(&input, w!("DFWLogger"), None, None, None, 0, &mut output)
            .map_err(|e| format!("DPAPI : {e}"))?;
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        Ok(bytes)
    }
}

pub fn dpapi_unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        CryptUnprotectData(&input, None, None, None, None, 0, &mut output)
            .map_err(|e| format!("DPAPI : {e}"))?;
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        Ok(bytes)
    }
}

// ---- Démarrage automatique (HKCU\...\Run — sans élévation) ----

const RUN_PATH: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: PCWSTR = w!("DFWLogger");

fn ok(e: WIN32_ERROR) -> Result<(), String> {
    if e == WIN32_ERROR(0) {
        Ok(())
    } else {
        Err(format!("erreur registre ({e:?})"))
    }
}

pub fn set_autostart(on: bool) -> Result<(), String> {
    unsafe {
        let mut hk = HKEY::default();
        ok(RegOpenKeyExW(
            HKEY_CURRENT_USER,
            RUN_PATH,
            0,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            &mut hk,
        ))?;
        let res = if on {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let value = format!("\"{}\" --autostart", exe.display());
            let mut bytes: Vec<u16> = value.encode_utf16().collect();
            bytes.push(0);
            let raw = std::slice::from_raw_parts(bytes.as_ptr() as *const u8, bytes.len() * 2);
            ok(RegSetValueExW(hk, VALUE_NAME, 0, REG_SZ, Some(raw)))
        } else {
            let code = RegDeleteValueW(hk, VALUE_NAME);
            if code == WIN32_ERROR(0) || code == ERROR_FILE_NOT_FOUND {
                Ok(())
            } else {
                Err(format!("erreur registre ({code:?})"))
            }
        };
        let _ = RegCloseKey(hk);
        res
    }
}

pub fn autostart_enabled() -> bool {
    unsafe {
        let mut hk = HKEY::default();
        let open = RegOpenKeyExW(HKEY_CURRENT_USER, RUN_PATH, 0, KEY_QUERY_VALUE, &mut hk);
        if open != WIN32_ERROR(0) {
            return false;
        }
        let exists = RegQueryValueExW(hk, VALUE_NAME, None, None, None, None) == WIN32_ERROR(0);
        let _ = RegCloseKey(hk);
        exists
    }
}

// ---- Réseau / identité ----

/// IP LAN principale (astuce UDP : connect() ne provoque aucun paquet).
pub fn primary_lan_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .ok()
        .and_then(|s| {
            s.connect("8.8.8.8:53").ok()?;
            s.local_addr().ok().map(|a| a.ip().to_string())
        })
        .unwrap_or_else(|| "127.0.0.1".into())
}

pub fn computer_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into())
}

// ---- Élévation (optionnelle, consentie par l'utilisateur via UAC) ----

/// Relance l'app avec élévation via PowerShell (boîte UAC de l'utilisateur).
pub fn relaunch_elevated() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let script = format!(
        "Start-Process -FilePath '{}' -ArgumentList '--elevated' -Verb RunAs",
        exe.display()
    );
    std::process::Command::new("powershell")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-NoProfile", "-Command", &script])
        .spawn()
        .map_err(|e| format!("lancement élévation : {e}"))?;
    Ok(())
}

/// L'app tourne-t-elle avec élévation ?
pub fn is_elevated() -> bool {
    std::env::args().any(|a| a == "--elevated")
}

/// Vérifie (une fois) si le journal Security est lisible — nécessite une session élevée.
#[allow(dead_code)]
pub fn can_read_security_log() -> bool {
    std::process::Command::new("wevtutil")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["qe", "Security", "/c:1", "/rd:true", "/f:xml"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
