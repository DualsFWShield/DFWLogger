//! Notifications toast Windows — transparentes, sans élévation :
//! 1. Un identifiant d'application (AUMID) est enregistré une fois dans HKCU
//!    (`Software\Classes\AppUserModelId\DFWLogger`) pour que l'apparition des
//!    toasts soit autorisée sans exécutable signé.
//! 2. Le toast est envoyé via PowerShell (WinRT `ToastNotificationManager`),
//!    script passé en UTF-16LE base64 (`-EncodedCommand`) pour éviter tout
//!    problème d'échappement, et fenêtre console masquée.

use crate::model::EventKind;
use crate::shared::Shared;
use std::os::windows::process::CommandExt;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::WIN32_ERROR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
    REG_OPTION_NON_VOLATILE, REG_SZ,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const AUMID_PATH: PCWSTR = w!("Software\\Classes\\AppUserModelId\\DFWLogger");

/// Enregistre l'AUMID (HKCU, aucune élévation requise). Échecs silencieux :
/// les toasts ne seront simplement pas affichés.
pub fn ensure_aumid() {
    unsafe {
        let mut hk = HKEY::default();
        let open = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            AUMID_PATH,
            0,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut hk,
            None,
        );
        if open != WIN32_ERROR(0) {
            return;
        }
        let set = |value: PCWSTR, text: &str| {
            let mut bytes: Vec<u16> = text.encode_utf16().collect();
            bytes.push(0);
            let raw =
                std::slice::from_raw_parts(bytes.as_ptr() as *const u8, bytes.len() * 2);
            RegSetValueExW(hk, value, 0, REG_SZ, Some(raw));
        };
        set(w!("DisplayName"), "DFWLogger — superviseur familial");
        let _ = RegCloseKey(hk);
    }
}

/// Envoie un toast (cette fonction est appelée dans un thread d'arrière-plan).
pub fn send(title: &str, body: &str) -> Result<(), String> {
    let esc = |s: &str| s.replace('\'', "''");
    let script = format!(
        "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, \
ContentType = WindowsRuntime] | Out-Null; \
$t=[Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent(\
[Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
$tx=$t.GetElementsByTagName('text'); \
[void]$tx.Item(0).AppendChild($t.CreateTextNode('{t}')); \
[void]$tx.Item(1).AppendChild($t.CreateTextNode('{b}')); \
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{a}')\
.Show([Windows.UI.Notifications.ToastNotification]::new($t))",
        t = esc(title),
        b = esc(body),
        a = "DFWLogger"
    );
    // UTF-16LE en base64 : -EncodedCommand évite les guillemets/accent peu importe l'entrée.
    use base64::Engine;
    let bytes: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let enc = base64::engine::general_purpose::STANDARD.encode(bytes);
    let out = std::process::Command::new("powershell")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &enc])
        .output()
        .map_err(|e| format!("lancement PowerShell : {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "toast refusé (code {:?})",
            out.status.code()
        ));
    }
    Ok(())
}

/// Déclenche les toasts selon la configuration, à la publication d'un événement.
/// Ne bloque jamais l'appelant : le toast part dans un thread détaché.
pub fn maybe_toast(shared: &Shared, kind: &EventKind) {
    let (enabled, title, body) = match kind {
        EventKind::SessionLock { cause } => {
            let en = shared.config.lock().unwrap().notify_lock;
            let comp = crate::win::computer_name();
            (
                en,
                "🔒 Session verrouillée".to_string(),
                format!("{comp} — {cause}"),
            )
        }
        EventKind::FailedLogon {
            account,
            workstation,
            ip,
            code,
        } => {
            let en = shared.config.lock().unwrap().notify_failed_logon;
            let ip = if ip.is_empty() { "réseau local" } else { ip.as_str() };
            (
                en,
                "⚠️ Tentative de connexion échouée".to_string(),
                format!("compte « {account} » depuis {workstation} ({ip}) [{code}]"),
            )
        }
        _ => return,
    };
    if !enabled {
        return;
    }
    let title = title.to_string();
    std::thread::Builder::new()
        .name("dfwlogger-toast".into())
        .spawn(move || {
            let _ = send(&title, &body);
        })
        .ok();
}
