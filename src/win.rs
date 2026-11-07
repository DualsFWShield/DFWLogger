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
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
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
pub fn relaunch_elevated_args(extra: &[&str]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut arglist = vec!["'--elevated'".to_string()];
    for a in extra {
        arglist.push(format!("'{a}'"));
    }
    let script = format!(
        "Start-Process -FilePath '{}' -ArgumentList {} -Verb RunAs",
        exe.display(),
        arglist.join(","),
    );
    std::process::Command::new("powershell")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-NoProfile", "-Command", &script])
        .spawn()
        .map_err(|e| format!("lancement élévation : {e}"))?;
    Ok(())
}

/// Relance l'app avec élévation, sans argument supplémentaire.
pub fn relaunch_elevated() -> Result<(), String> {
    relaunch_elevated_args(&[])
}

/// L'app tourne-t-elle avec élévation ?
pub fn is_elevated() -> bool {
    std::env::args().any(|a| a == "--elevated") || is_admin()
}

/// Vérifie si le processus possède de réels privilèges administrateur (écriture HKLM).
pub fn is_admin() -> bool {
    unsafe {
        let mut hk = HKEY::default();
        let code = RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            w!("System\\CurrentControlSet"),
            0,
            KEY_SET_VALUE,
            &mut hk,
        );
        if code == WIN32_ERROR(0) {
            let _ = RegCloseKey(hk);
            true
        } else {
            false
        }
    }
}

/// Boîte de dialogue Win32 simple (rapport d'opération en mode console-less).
pub fn message_box(title: &str, text: &str) {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};
    unsafe {
        let _ = MessageBoxW(
            None,
            &HSTRING::from(text),
            &HSTRING::from(title),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

/// Rendre visible et amener au premier plan la fenêtre principale de DFWLogger.
/// Fonctionne même si la fenêtre a été masquée dans le tray (Visible(false) ou SW_HIDE)
/// ou minimisée.
pub fn show_main_window() {
    unsafe {
        use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
        use windows::Win32::System::Threading::GetCurrentProcessId;
        use windows::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetWindow, GetWindowThreadProcessId, SetForegroundWindow, ShowWindow,
            GW_OWNER, SW_RESTORE, SW_SHOW,
        };

        let my_pid = GetCurrentProcessId();

        unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let target_pid = lparam.0 as u32;
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == target_pid && GetWindow(hwnd, GW_OWNER).0 == 0 {
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
                return BOOL(0);
            }
            BOOL(1)
        }

        let _ = EnumWindows(Some(enum_proc), LPARAM(my_pid as isize));
    }
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

/// Exécute un script PowerShell de manière sécurisée et non-interactive via -EncodedCommand.
fn run_powershell_encoded(script: &str) -> Result<(), String> {
    use base64::Engine;
    let utf16: Vec<u8> = script
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&utf16);

    let status = std::process::Command::new("powershell")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &b64])
        .status()
        .map_err(|e| format!("lancement powershell : {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("powershell a retourné le code : {:?}", status.code()))
    }
}

/// Crée les raccourcis sur le Bureau et dans le Menu Démarrer.
pub fn create_shortcuts() -> Result<(), String> {
    create_shortcuts_in(None, None)
}

/// Version paramétrable pour tests ou environnements spécifiques.
pub fn create_shortcuts_in(custom_desktop: Option<&str>, custom_start_menu: Option<&str>) -> Result<(), String> {
    let desktop_setup = match custom_desktop {
        Some(d) => format!("$desktop = '{}'", d.replace('\'', "''")),
        None => "$desktop = [Environment]::GetFolderPath('Desktop'); if (-not $desktop) { $desktop = Join-Path $env:USERPROFILE 'Desktop' }".to_string(),
    };
    let start_menu_setup = match custom_start_menu {
        Some(s) => format!("$startMenu = '{}'", s.replace('\'', "''")),
        None => "$startMenu = Join-Path ([Environment]::GetFolderPath('StartMenu')) 'Programs\\DFWLogger'".to_string(),
    };

    let script = format!(r#"
$ws = New-Object -ComObject WScript.Shell
{desktop_setup}
{start_menu_setup}
$target = Join-Path $env:LOCALAPPDATA 'Programs\DFWLogger\dfwlogger.exe'

if (-not (Test-Path $desktop)) {{
    New-Item -ItemType Directory -Force -Path $desktop | Out-Null
}}
$s1 = $ws.CreateShortcut((Join-Path $desktop 'DFWLogger.lnk'))
$s1.TargetPath = $target
$s1.IconLocation = "$target,0"
$s1.Description = 'Superviseur familial transparent'
$s1.Save()

if (-not (Test-Path $startMenu)) {{
    New-Item -ItemType Directory -Force -Path $startMenu | Out-Null
}}
$s2 = $ws.CreateShortcut((Join-Path $startMenu 'DFWLogger.lnk'))
$s2.TargetPath = $target
$s2.IconLocation = "$target,0"
$s2.Description = 'Superviseur familial transparent'
$s2.Save()
"#);
    run_powershell_encoded(&script)
}

/// Supprime les raccourcis du Bureau et du Menu Démarrer.
pub fn remove_shortcuts() -> Result<(), String> {
    remove_shortcuts_in(None, None)
}

/// Version paramétrable pour tests ou environnements spécifiques.
pub fn remove_shortcuts_in(custom_desktop: Option<&str>, custom_start_menu: Option<&str>) -> Result<(), String> {
    let desktop_setup = match custom_desktop {
        Some(d) => format!("$desktop = '{}'", d.replace('\'', "''")),
        None => "$desktop = [Environment]::GetFolderPath('Desktop'); if (-not $desktop) { $desktop = Join-Path $env:USERPROFILE 'Desktop' }".to_string(),
    };
    let start_menu_setup = match custom_start_menu {
        Some(s) => format!("$startMenu = '{}'", s.replace('\'', "''")),
        None => "$startMenu = Join-Path ([Environment]::GetFolderPath('StartMenu')) 'Programs\\DFWLogger'".to_string(),
    };

    let script = format!(r#"
{desktop_setup}
{start_menu_setup}
$l1 = Join-Path $desktop 'DFWLogger.lnk'
if (Test-Path $l1) {{ Remove-Item -Force $l1 }}

if (Test-Path $startMenu) {{ Remove-Item -Recurse -Force $startMenu }}
"#);
    run_powershell_encoded(&script)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shortcuts_cycle() {
        let temp = std::env::temp_dir().join("dfwlogger_shortcuts_test");
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();
        let desktop = temp.join("Desktop");
        let start_menu = temp.join("StartMenu");
        std::fs::create_dir_all(&desktop).unwrap();
        std::fs::create_dir_all(&start_menu).unwrap();

        let d_str = desktop.to_str().unwrap();
        let sm_str = start_menu.to_str().unwrap();

        assert!(create_shortcuts_in(Some(d_str), Some(sm_str)).is_ok(), "create_shortcuts doit réussir");
        assert!(desktop.join("DFWLogger.lnk").exists(), "DFWLogger.lnk doit exister sur le Desktop");
        assert!(start_menu.join("DFWLogger.lnk").exists(), "DFWLogger.lnk doit exister dans le StartMenu");

        assert!(remove_shortcuts_in(Some(d_str), Some(sm_str)).is_ok(), "remove_shortcuts doit réussir");
        assert!(!desktop.join("DFWLogger.lnk").exists(), "DFWLogger.lnk doit être supprimé du Desktop");
        let _ = std::fs::remove_dir_all(&temp);
    }
}
