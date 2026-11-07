//! Big Brother — protection active & supervision (Mode École). Tourne dans le service
//! Windows `DFWLoggerBigBrother` (SYSTEM) et coopère avec le pilote noyau `Serbert` (Ring 0)
//! pour interdire les processus, filtrer le web, bloquer les installations et gérer les quotas.
//!
//! Mécanismes :
//! - Noyau (Ring 0) : coopération avec le pilote `Serbert` (`PsSetCreateProcessNotifyRoutineEx`).
//! - Filtrage web par fichier hosts (section balisée auto-réparée).
//! - DNS filtré imposé sur les interfaces TCP/IP (sauvegardé/restaurable).
//! - Watchdog de processus Ring 3 (défense en profondeur).
//! - Politiques : `DisableMSI` + règles Safer/SRP « Disallowed » (dossiers non-admin).
//! - Quota quotidien + plages horaires comptés sur la session console active (déconnexion WTS).
//!
//! Politique « fail-open » : sans `big_brother.json`, Big Brother n'impose rien.

use chrono::{Datelike, Timelike};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, ERROR_NO_MORE_ITEMS, WIN32_ERROR};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegDeleteValueW, RegEnumKeyExW, RegOpenKeyExW,
    RegQueryValueExW, RegSetValueExW, HKEY, HKEY_LOCAL_MACHINE, REG_DWORD, REG_EXPAND_SZ,
    REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE, KEY_ENUMERATE_SUB_KEYS, KEY_QUERY_VALUE, KEY_READ,
    KEY_SET_VALUE, REG_OPTION_NON_VOLATILE,
};
use windows::Win32::System::RemoteDesktop::{
    WTSEnumerateSessionsW, WTSDisconnectSession, WTSActive, WTSFreeMemory, WTS_SESSION_INFOW,
    WTS_CURRENT_SERVER_HANDLE,
};
use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

pub const SERVICE_NAME: &str = "DFWLoggerBigBrother";
pub const SERVICE_DISPLAY: &str = "DFWLogger Big Brother (protection active)";
pub const SERVICE_DESC: &str =
    "Agent de supervision et protection active DFWLogger Big Brother : filtrage web, blocage d'applications Ring 0 / Ring 3, quotas.";

pub const HOSTS_BEGIN: &str = "# >>> DFWLOGGER-BIGBROTHER-BLOCK >>>";
pub const HOSTS_END: &str = "# <<< DFWLOGGER-BIGBROTHER-BLOCK <<<";

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DNS_IFACE_KEY: &str = "SYSTEM\\CurrentControlSet\\Services\\Tcpip\\Parameters\\Interfaces";
const SAFER_KEY: &str = "Software\\Policies\\Microsoft\\Windows\\Safer\\CodeIdentifiers";
const SRP_PATHS: &str = "Software\\Policies\\Microsoft\\Windows\\Safer\\CodeIdentifiers\\0\\Paths";
const MSI_POLICY: &str = "Software\\Policies\\Microsoft\\Windows\\Installer";
const SRP_OWN_TAG: &str = "DFWLogger BigBrother";

// ------------------------------------------------------------------ config

/// Politique du poste — `C:\ProgramData\DFWLogger\big_brother.json`.
#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct BigBrotherConfig {
    pub enabled: bool,
    /// Domaines bloqués via hosts (ex. ["youtube.com", "discord.gg"]).
    pub domains: Vec<String>,
    /// Resolver DNS filtrant imposé (ex. "185.228.168.10"). None = ne rien toucher.
    pub dns_filter: Option<String>,
    /// Exécutables interdits (nom, insensible à la casse ; ex. "setup.exe", "steam.exe").
    pub blocked_processes: Vec<String>,
    /// Interdire toute installation MSI (politique machine).
    pub block_msi: bool,
    /// Dossiers (par nom dans le profil utilisateur) où l'exécution est interdite : "Downloads"…
    pub deny_paths: Vec<String>,
    /// Quota quotidien en minutes de session console active. None = illimité.
    pub daily_minutes: Option<u64>,
    /// Plages horaires autorisées (vide = tout le temps).
    pub allowed_windows: Vec<TimeWindow>,
    /// Pilote noyau Serbert actif si disponible.
    pub enable_kernel_driver: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TimeWindow {
    /// Numéro du jour dans la semaine ISO : 1 = lundi … 7 = dimanche.
    pub days: Vec<u32>,
    pub from: String,
    pub to: String,
}

impl Default for BigBrotherConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            domains: vec![],
            dns_filter: None,
            blocked_processes: vec![],
            block_msi: false,
            deny_paths: vec![],
            daily_minutes: None,
            allowed_windows: vec![],
            enable_kernel_driver: true,
        }
    }
}

impl BigBrotherConfig {
    pub fn path() -> PathBuf {
        big_brother_program_dir().join("big_brother.json")
    }
    pub fn load() -> Self {
        if let Ok(b) = std::fs::read(Self::path()) {
            if let Ok(cfg) = serde_json::from_slice(&b) {
                return cfg;
            }
        }
        Self::default()
    }
    pub fn save(&self) -> Result<(), String> {
        let dir = big_brother_program_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let data = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(Self::path(), data).map_err(|e| e.to_string())
    }
}

/// État persistant (usage du jour, sauvegarde DNS, verrou de quota).
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct BigBrotherState {
    pub day: String,
    /// Secondes de session console active aujourd'hui.
    pub active_secs: u64,
    /// Interface (GUID) → valeur NameServer d'origine, pour restauration.
    pub dns_backup: std::collections::BTreeMap<String, String>,
    pub quota_lock: bool,
    pub quota_reason: Option<String>,
    pub kernel_driver_active: bool,
    pub kernel_blocks_count: u32,
}

impl BigBrotherState {
    pub fn path() -> PathBuf {
        big_brother_program_dir().join("big_brother.state.json")
    }
    pub fn load() -> Self {
        std::fs::read(Self::path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) {
        let _ = std::fs::create_dir_all(big_brother_program_dir());
        if let Ok(data) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(Self::path(), data);
        }
    }
    pub fn reset_today(&mut self) {
        self.active_secs = 0;
        self.quota_lock = false;
        self.quota_reason = None;
        self.save();
    }
}

pub fn big_brother_program_dir() -> PathBuf {
    let base = std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".into());
    PathBuf::from(base).join("DFWLogger")
}

pub fn log(msg: &str) {
    let dir = big_brother_program_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("big_brother.log");
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > 256 * 1024 {
            let _ = std::fs::write(&path, "");
        }
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(
            f,
            "{} {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            msg
        );
    }
}

// ------------------------------------------------------------------- 3 Modes de Privilège

/// Les 3 modes d'accès et d'exécution de DFWLogger :
/// 1. Standard (Sans autorisation) : collecte locale non-intrusive (fenêtres, fichiers récents, sessions, journal chiffré).
/// 2. Admin (Big Brother SYSTEM) : service Windows SYSTEM, filtrage DNS/hosts, blocage installateurs MSI, SRP, quotas WTS.
/// 3. Noyau (Serbert Ring 0) : pilote Ring 0 (serbert.sys), interception matérielle PsSetCreateProcessNotifyRoutineEx, STATUS_ACCESS_DENIED.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivilegeTier {
    /// Mode 1 : Démarrage standard sans autorisation
    Standard,
    /// Mode 2 : Accès administrateur (Service Big Brother SYSTEM)
    Admin,
    /// Mode 3 : Accès noyau (Pilote Serbert Ring 0)
    Kernel,
}

impl PrivilegeTier {
    pub fn current() -> Self {
        if crate::serbert::is_driver_running() {
            PrivilegeTier::Kernel
        } else if is_service_running() {
            PrivilegeTier::Admin
        } else {
            PrivilegeTier::Standard
        }
    }

    #[allow(dead_code)]
    pub fn title(&self) -> &'static str {
        match self {
            PrivilegeTier::Standard => "Mode 1 : Sans autorisation (Standard)",
            PrivilegeTier::Admin => "Mode 2 : Accès Admin (Big Brother)",
            PrivilegeTier::Kernel => "Mode 3 : Accès Noyau (Serbert Ring 0)",
        }
    }

    pub fn badge(&self) -> &'static str {
        match self {
            PrivilegeTier::Standard => "⚪ Standard",
            PrivilegeTier::Admin => "🛡️ Admin",
            PrivilegeTier::Kernel => "⚡ Noyau",
        }
    }

    #[allow(dead_code)]
    pub fn short_name(&self) -> &'static str {
        match self {
            PrivilegeTier::Standard => "standard",
            PrivilegeTier::Admin => "admin",
            PrivilegeTier::Kernel => "kernel",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            PrivilegeTier::Standard => "Mode par défaut non-intrusif : aucune modification système, suivi familial consenti, journal chiffré local.",
            PrivilegeTier::Admin => "Service Windows SYSTEM (DFWLoggerBigBrother) actif : filtrage DNS & hosts, interdiction d'exécution dans Downloads/Temp (SRP), blocage MSI, quotas WTS.",
            PrivilegeTier::Kernel => "Pilote noyau Windows (\\Device\\Serbert) actif au Ring 0 : interception PsSetCreateProcessNotifyRoutineEx, rejet physique immédiat sans création de thread.",
        }
    }
}

// ------------------------------------------------------------------- SCM service

pub fn is_service_running() -> bool {
    matches!(service_status().as_deref(), Some("RUNNING"))
}

/// État du service : "RUNNING", "STOPPED", ou None si non installé.
pub fn service_status() -> Option<String> {
    if let Ok(out) = std::process::Command::new("cmd")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["/C", "sc", "query", SERVICE_NAME])
        .output()
    {
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
    }
    None
}

/// Installe et démarre le service Big Brother (nécessite une session admin).
pub fn install_service() -> Result<(), String> {
    std::fs::create_dir_all(big_brother_program_dir()).map_err(|e| e.to_string())?;

    // ACL NTFS : les Utilisateurs ont uniquement le droit de lecture (RX).
    let dir = big_brother_program_dir();
    let _ = run_tool(
        "icacls",
        &[
            &dir.to_string_lossy(),
            "/inheritance:d",
            "/grant:r",
            "*S-1-5-32-544:(OI)(CI)F",
            "*S-1-5-18:(OI)(CI)F",
            "*S-1-5-32-545:(OI)(CI)RX",
        ],
    );

    if !BigBrotherConfig::path().exists() {
        let _ = BigBrotherConfig::default().save();
    }

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let binpath = format!("\"{}\" --service", exe.display());
    let output = run_tool(
        "sc",
        &[
            "create",
            SERVICE_NAME,
            "binPath=",
            &binpath,
            "start=",
            "auto",
            "obj=",
            "LocalSystem",
            "displayName=",
            SERVICE_DISPLAY,
        ],
    )?;
    if !output.contains("FAILED") && !output.contains("1063") {
        // Enregistré avec succès
    }
    let _ = run_tool("sc", &["description", SERVICE_NAME, SERVICE_DESC]);
    let _ = run_tool("sc", &["config", SERVICE_NAME, "start=", "auto"]);
    let _ = run_tool("sc", &["start", SERVICE_NAME]);

    // Tentative de démarrage du pilote noyau Serbert si présent
    let driver_path = crate::serbert::default_driver_path();
    if driver_path.exists() {
        let _ = crate::serbert::install_driver_service(&driver_path);
        let _ = crate::serbert::start_driver();
    }

    log("service Big Brother installé et démarré");
    Ok(())
}

/// Désinstalle proprement : service arrêté/supprimé + toutes les protections retirées.
pub fn uninstall_service() -> Result<(), String> {
    let _ = run_tool("sc", &["stop", SERVICE_NAME]);
    let _ = run_tool("sc", &["delete", SERVICE_NAME]);

    // Déchargement du pilote noyau Serbert
    let _ = crate::serbert::uninstall_driver_service();

    let _ = remove_all_protections();
    log("service Big Brother désinstallé, protections retirées");
    Ok(())
}

#[allow(dead_code)]
pub fn start_service() -> Result<String, String> {
    run_tool("sc", &["start", SERVICE_NAME])
}

#[allow(dead_code)]
pub fn stop_service() -> Result<String, String> {
    run_tool("sc", &["stop", SERVICE_NAME])
}

/// Retire : politiques MSI/SRP, pilote Serbert, DNS restauré, section hosts, verrou de quota.
pub fn remove_all_protections() -> Result<(), String> {
    let mut errs = Vec::new();
    if let Err(e) = remove_srp_ours() {
        errs.push(format!("SRP : {e}"));
    }
    if let Err(e) = apply_msi_policy(false) {
        errs.push(format!("MSI : {e}"));
    }
    let state = BigBrotherState::load();
    if let Err(e) = remove_dns(&state) {
        errs.push(format!("DNS : {e}"));
    }
    match remove_hosts_section() {
        Ok(true) => flush_dns_cache(),
        Ok(false) => {}
        Err(e) => errs.push(format!("hosts : {e}")),
    }

    let mut state = BigBrotherState::load();
    state.quota_lock = false;
    state.quota_reason = None;
    state.save();

    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs.join(" | "))
    }
}

fn run_tool(tool: &str, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new(tool)
        .creation_flags(CREATE_NO_WINDOW)
        .args(args)
        .output()
        .map_err(|e| format!("lancement {tool} : {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

// -------------------------------------------------------------------- hosts

pub fn hosts_path() -> PathBuf {
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    Path::new(&windir)
        .join("System32")
        .join("drivers")
        .join("etc")
        .join("hosts")
}

pub fn apply_hosts(domains: &[String]) -> Result<bool, String> {
    let path = hosts_path();
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    let section = build_hosts_section(domains);
    let new_content = replace_section(&current, &section);
    if new_content == current {
        return Ok(false);
    }
    std::fs::write(&path, &new_content).map_err(|e| format!("écriture hosts : {e}"))?;
    Ok(true)
}

pub fn remove_hosts_section() -> Result<bool, String> {
    let path = hosts_path();
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    let new_content = replace_section(&current, "");
    if new_content == current {
        return Ok(false);
    }
    std::fs::write(&path, &new_content).map_err(|e| format!("hosts : {e}"))?;
    Ok(true)
}

fn build_hosts_section(domains: &[String]) -> String {
    let valid: Vec<String> = domains
        .iter()
        .map(|d| d.trim().trim_start_matches('.').to_string())
        .filter(|d| !d.is_empty() && !d.contains(' '))
        .collect();
    if valid.is_empty() {
        return String::new();
    }
    let mut s = String::from(format!("{HOSTS_BEGIN}\n"));
    for d in valid {
        s.push_str(&format!("0.0.0.0 {d}\n"));
    }
    s.push_str(&format!("{HOSTS_END}\n"));
    s
}

fn replace_section(content: &str, new_section: &str) -> String {
    if let Some((before, mid)) = content.split_once(HOSTS_BEGIN) {
        let after = match mid.split_once(HOSTS_END) {
            Some((_, tail)) => tail,
            None => "",
        };
        let mut out = String::from(before.trim_end_matches('\n'));
        if !out.is_empty() {
            out.push('\n');
        }
        if !new_section.is_empty() {
            out.push_str(new_section);
        }
        out.push_str(after.trim_start_matches('\n'));
        return out;
    }
    if new_section.is_empty() {
        return content.to_string();
    }
    let mut out = content.to_owned();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(new_section);
    out
}



pub fn flush_dns_cache() {
    let _ = std::process::Command::new("cmd")
        .args(["/C", "ipconfig", "/flushdns"])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

// -------------------------------------------------------------------- DNS

pub fn apply_dns(dns: &str, state: &mut BigBrotherState) -> Result<usize, String> {
    unsafe {
        let mut changed = 0;
        let root = open_key(DNS_IFACE_KEY, KEY_ENUMERATE_SUB_KEYS)?;
        let names = enum_subkeys(&root);
        let _ = RegCloseKey(root);
        for iface in names {
            let path = format!("{DNS_IFACE_KEY}\\{iface}");
            let ifh = open_key(&path, KEY_SET_VALUE | KEY_QUERY_VALUE)?;
            if !state.dns_backup.contains_key(&iface) {
                let saved = match query_string_h(&ifh, "NameServer") {
                    Some(s) => s,
                    None => "__DHCP__".to_string(),
                };
                state.dns_backup.insert(iface.clone(), saved);
            }
            match query_string_h(&ifh, "NameServer") {
                Some(cur) if cur == dns => {}
                _ => {
                    set_string(&ifh, "NameServer", dns)
                        .map_err(|_| "échec configuration NameServer".to_string())?;
                    changed += 1;
                }
            }
            let _ = RegCloseKey(ifh);
        }
        if changed > 0 {
            flush_dns_cache();
            state.save();
        }
        Ok(changed)
    }
}

pub fn remove_dns(state: &BigBrotherState) -> Result<usize, String> {
    unsafe {
        let mut changed = 0;
        for (iface, saved) in &state.dns_backup {
            let path = format!("{DNS_IFACE_KEY}\\{iface}");
            let Ok(ifh) = open_key(&path, KEY_SET_VALUE) else {
                continue;
            };
            match saved.as_str() {
                "__DHCP__" => {
                    let _ = RegDeleteValueW(ifh, w!("NameServer"));
                }
                v => {
                    let _ = set_string(&ifh, "NameServer", v);
                }
            }
            let _ = RegCloseKey(ifh);
            changed += 1;
        }
        if changed > 0 {
            flush_dns_cache();
        }
        Ok(changed)
    }
}

// --------------------------------------------------------------- watchdog

pub fn kill_blocked_processes(blocked: &[String]) -> u32 {
    let lowered: Vec<String> = blocked
        .iter()
        .map(|b| b.trim().to_ascii_lowercase())
        .filter(|b| !b.is_empty())
        .collect();
    if lowered.is_empty() {
        return 0;
    }
    let mut killed = 0;
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return 0;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut entry).is_err() {
            let _ = CloseHandle(snap);
            return 0;
        }
        loop {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
            let lname = name.to_ascii_lowercase();
            if lowered.iter().any(|b| lname == *b) {
                if close_process(entry.th32ProcessID).is_ok() {
                    log(&format!("watchdog: processus interdit terminé : {name}"));
                    killed += 1;
                }
            }
            if Process32NextW(snap, &mut entry).is_err() {
                break;
            }
        }
        let _ = CloseHandle(snap);
    }
    killed
}

fn close_process(pid: u32) -> Result<(), ()> {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) else {
            return Err(());
        };
        let _ = TerminateProcess(h, 1);
        let _ = CloseHandle(h);
        Ok(())
    }
}

// -------------------------------------------------------- politiques MSI/SRP

pub fn apply_msi_policy(block: bool) -> Result<(), String> {
    unsafe {
        if block {
            let hk = create_key(MSI_POLICY)?;
            let v = 2u32.to_ne_bytes();
            let _ = RegSetValueExW(hk, w!("DisableMSI"), 0, REG_DWORD, Some(&v));
            let _ = RegCloseKey(hk);
        } else if let Ok(hk) = open_key(MSI_POLICY, KEY_SET_VALUE) {
            let _ = RegDeleteValueW(hk, w!("DisableMSI"));
            let _ = RegCloseKey(hk);
        }
        Ok(())
    }
}

pub fn msi_policy_active() -> bool {
    unsafe {
        let Ok(hk) = open_key(MSI_POLICY, KEY_QUERY_VALUE) else {
            return false;
        };
        let mut v = 0u32;
        let mut cb = 4u32;
        let ok = RegQueryValueExW(
            hk,
            w!("DisableMSI"),
            None,
            None,
            Some(&mut v as *mut u32 as *mut u8),
            Some(&mut cb),
        ) == WIN32_ERROR(0)
            && v == 2;
        let _ = RegCloseKey(hk);
        ok
    }
}

pub fn apply_srp_deny_paths(folder_names: &[String]) -> Result<(), String> {
    unsafe {
        let cid = create_key(SAFER_KEY)?;
        let lvl = 393216u32.to_ne_bytes(); // Unrestricted
        let _ = RegSetValueExW(cid, w!("DefaultLevel"), 0, REG_DWORD, Some(&lvl));
        let trans = 2u32.to_ne_bytes();
        let _ = RegSetValueExW(cid, w!("TransparentEnabled"), 0, REG_DWORD, Some(&trans));
        let _ = RegCloseKey(cid);

        let mut targets: Vec<String> = Vec::new();
        let users_root = PathBuf::from("C:\\Users");
        if let Ok(rd) = std::fs::read_dir(&users_root) {
            for entry in rd.flatten() {
                for folder in folder_names {
                    let folder = folder.trim();
                    if folder.is_empty() {
                        continue;
                    }
                    let p = entry.path().join(folder);
                    if p.is_dir() {
                        targets.push(format!("{}\\*", p.display()));
                    }
                }
            }
        }
        targets.dedup();

        let existing = srp_rules();
        for (subkey, item, desc) in existing {
            if desc == SRP_OWN_TAG && !targets.contains(&item) {
                let paths_key = open_key(SRP_PATHS, KEY_SET_VALUE)?;
                let wide = utf16_vec(&subkey);
                let _ = RegDeleteKeyW(paths_key, PCWSTR(wide.as_ptr()));
                let _ = RegCloseKey(paths_key);
            }
        }
        for rule in &targets {
            let guid = deterministic_guid(rule);
            let rule_path = format!("{SRP_PATHS}\\{{{guid}}}");
            let hk = create_key(&rule_path)?;
            set_expand(&hk, "ItemData", rule);
            set_dword(&hk, "SaferFlags", 0);
            set_dword(&hk, "ValueType", 1);
            set_expand(&hk, "Description", SRP_OWN_TAG);
            let _ = RegCloseKey(hk);
        }
        Ok(())
    }
}

pub fn remove_srp_ours() -> Result<(), String> {
    unsafe {
        let rules = srp_rules();
        let paths_key = match open_key(SRP_PATHS, KEY_SET_VALUE) {
            Ok(k) => k,
            Err(_) => return Ok(()),
        };
        for (subkey, _item, desc) in rules {
            if desc == SRP_OWN_TAG || desc == "DFWLogger" {
                let wide = utf16_vec(&subkey);
                let _ = RegDeleteKeyW(paths_key, PCWSTR(wide.as_ptr()));
            }
        }
        let _ = RegCloseKey(paths_key);
        Ok(())
    }
}

unsafe fn srp_rules() -> Vec<(String, String, String)> {
    match open_key(SRP_PATHS, KEY_ENUMERATE_SUB_KEYS) {
        Ok(hk) => {
            let mut out = Vec::new();
            for subkey in enum_subkeys(&hk) {
                let path = format!("{SRP_PATHS}\\{subkey}");
                if let Ok(rk) = open_key(&path, KEY_QUERY_VALUE) {
                    let item = query_string_h(&rk, "ItemData").unwrap_or_default();
                    let desc = query_string_h(&rk, "Description").unwrap_or_default();
                    let _ = RegCloseKey(rk);
                    out.push((subkey, item, desc));
                }
            }
            let _ = RegCloseKey(hk);
            out
        }
        Err(_) => Vec::new(),
    }
}

fn deterministic_guid(seed: &str) -> String {
    let mut h1: u64 = 0xcbf2_9ce4_8422_2325;
    for b in seed.as_bytes() {
        h1 ^= u64::from(*b);
        h1 = h1.wrapping_mul(0x0100_0000_01b3);
    }
    let h2 = h1.wrapping_mul(0x9c7f_e3ad_bf7a_11d9);
    let a = (h1 & 0xFFFF_FFFF) as u32;
    let b = ((h1 >> 32) & 0xFFFF) as u16;
    let c = (((h2 >> 16) & 0x0FFF) | 0x4000) as u16;
    let d = (((h2 >> 48) & 0x3FFF) | 0x8000) as u16;
    let e = h2 & 0xFFFF_FFFF_FFFF;
    format!("{a:08x}-{b:04x}-{c:04x}-{d:04x}-{e:012x}")
}

// ------------------------------------------------------- sessions & compteurs

pub fn active_sessions() -> Vec<u32> {
    unsafe {
        let mut pp: *mut WTS_SESSION_INFOW = std::ptr::null_mut();
        let mut count: u32 = 0;
        if WTSEnumerateSessionsW(WTS_CURRENT_SERVER_HANDLE, 0, 1, &mut pp, &mut count).is_err()
            || pp.is_null()
        {
            return Vec::new();
        }
        let slice = std::slice::from_raw_parts(pp, count as usize);
        let active: Vec<u32> = slice
            .iter()
            .filter(|s| s.State == WTSActive)
            .map(|s| s.SessionId)
            .collect();
        WTSFreeMemory(pp as *mut _);
        active
    }
}

pub fn disconnect_active_sessions() {
    unsafe {
        for sid in active_sessions() {
            if WTSDisconnectSession(WTS_CURRENT_SERVER_HANDLE, sid, true).is_ok() {
                log(&format!("session {sid} déconnectée (restriction Big Brother)"));
            }
        }
    }
}

fn parse_hhmm(s: &str) -> Option<u32> {
    let mut it = s.trim().split(':');
    let h: u32 = it.next()?.trim().parse().ok()?;
    let m: u32 = it.next().unwrap_or("0").trim().parse().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    Some(h * 60 + m)
}

pub fn in_allowed_window(windows: &[TimeWindow]) -> bool {
    if windows.is_empty() {
        return true;
    }
    let now = chrono::Local::now();
    let weekday = now.weekday().number_from_monday();
    let cur = now.hour() as u32 * 60 + now.minute() as u32;
    for w in windows {
        if !w.days.contains(&weekday) {
            continue;
        }
        let (Some(from), Some(to)) = (parse_hhmm(&w.from), parse_hhmm(&w.to)) else {
            continue;
        };
        if from <= to {
            if cur >= from && cur < to {
                return true;
            }
        } else if cur >= from || cur < to {
            return true;
        }
    }
    false
}

// ------------------------------------------------------------------ registre

fn utf16_vec(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v
}

unsafe fn open_key(path: &str, access: REG_SAM_FLAGS) -> Result<HKEY, String> {
    let mut hk = HKEY::default();
    let wide = utf16_vec(path);
    let open = RegOpenKeyExW(
        HKEY_LOCAL_MACHINE,
        PCWSTR(wide.as_ptr()),
        0,
        access,
        &mut hk,
    );
    if open != WIN32_ERROR(0) {
        return Err(format!("ouverture registre : {path}"));
    }
    Ok(hk)
}

unsafe fn create_key(path: &str) -> Result<HKEY, String> {
    let mut hk = HKEY::default();
    let wide = utf16_vec(path);
    let res = RegCreateKeyExW(
        HKEY_LOCAL_MACHINE,
        PCWSTR(wide.as_ptr()),
        0,
        None,
        REG_OPTION_NON_VOLATILE,
        KEY_SET_VALUE | KEY_READ,
        None,
        &mut hk,
        None,
    );
    if res != WIN32_ERROR(0) {
        return Err(format!("création clé : {path}"));
    }
    Ok(hk)
}

unsafe fn enum_subkeys(root: &HKEY) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0u32;
    loop {
        let mut name = vec![0u16; 256];
        let mut len = name.len() as u32;
        let res = RegEnumKeyExW(
            *root,
            i,
            PWSTR(name.as_mut_ptr()),
            &mut len,
            None,
            PWSTR::null(),
            None,
            None,
        );
        if res == ERROR_NO_MORE_ITEMS {
            break;
        }
        if res == WIN32_ERROR(0) {
            out.push(String::from_utf16_lossy(&name[..len as usize]));
            i += 1;
        } else {
            break;
        }
    }
    out
}

unsafe fn query_string_h(hk: &HKEY, name: &str) -> Option<String> {
    let wide = utf16_vec(name);
    let mut len: u32 = 0;
    let mut ty = REG_VALUE_TYPE::default();
    if RegQueryValueExW(
        *hk,
        PCWSTR(wide.as_ptr()),
        None,
        Some(&mut ty),
        None,
        Some(&mut len),
    ) != WIN32_ERROR(0)
    {
        return None;
    }
    if ty != REG_SZ {
        return None;
    }
    let mut buf = vec![0u8; len as usize];
    if RegQueryValueExW(
        *hk,
        PCWSTR(wide.as_ptr()),
        None,
        Some(&mut ty),
        Some(buf.as_mut_ptr()),
        Some(&mut len),
    ) != WIN32_ERROR(0)
    {
        return None;
    }
    let u16s: Vec<u16> = buf[..len as usize]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    Some(String::from_utf16_lossy(&u16s))
}

unsafe fn set_string(hk: &HKEY, name: &str, data: &str) -> Result<(), ()> {
    let wide = utf16_vec(name);
    let value = utf16_bytes(data);
    let res = RegSetValueExW(*hk, PCWSTR(wide.as_ptr()), 0, REG_SZ, Some(&value));
    if res == WIN32_ERROR(0) {
        Ok(())
    } else {
        Err(())
    }
}

unsafe fn set_expand(hk: &HKEY, name: &str, data: &str) {
    let wide = utf16_vec(name);
    let value = utf16_bytes(data);
    let _ = RegSetValueExW(*hk, PCWSTR(wide.as_ptr()), 0, REG_EXPAND_SZ, Some(&value));
}

fn utf16_bytes(s: &str) -> Vec<u8> {
    utf16_vec(s).iter().flat_map(|u| u.to_le_bytes()).collect()
}

unsafe fn set_dword(hk: &HKEY, name: &str, value: u32) {
    let wide = utf16_vec(name);
    let v = value.to_ne_bytes();
    let _ = RegSetValueExW(
        *hk,
        PCWSTR(wide.as_ptr()),
        0,
        REG_DWORD,
        Some(&v),
    );
}

// ------------------------------------------------------------------ tick

/// Un pas de la boucle de Big Brother (service SYSTEM).
pub fn tick() {
    let cfg = BigBrotherConfig::load();
    if !cfg.enabled {
        return;
    }
    let mut state = BigBrotherState::load();

    // 1) Filtrage web par hosts.
    match apply_hosts(&cfg.domains) {
        Ok(true) => {
            flush_dns_cache();
            log("hosts mis à jour (Big Brother)");
        }
        Ok(false) => {}
        Err(e) => log(&format!("hosts : {e}")),
    }

    // 2) DNS filtré.
    if let Some(dns) = cfg.dns_filter.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if let Err(e) = apply_dns(dns, &mut state) {
            log(&format!("DNS : {e}"));
        }
    }

    // 3) Synchronisation noyau Ring 0 avec le pilote Serbert
    if cfg.enable_kernel_driver && crate::serbert::is_driver_running() {
        match crate::serbert::sync_rules(&cfg.blocked_processes, std::process::id()) {
            Ok(kstats) => {
                state.kernel_driver_active = true;
                state.kernel_blocks_count = kstats.blocked_executions_count;
            }
            Err(e) => {
                log(&format!("serbert kernel sync : {e}"));
                state.kernel_driver_active = false;
            }
        }
    } else {
        state.kernel_driver_active = false;
    }

    // 4) Watchdog Ring 3 (défense en profondeur si Serbert n'est pas chargé)
    let killed = kill_blocked_processes(&cfg.blocked_processes);
    if killed > 0 {
        log(&format!("watchdog Ring 3 : {killed} processus interdit(s) terminés"));
    }

    // 5) Politiques d'installation (MSI + SRP)
    if let Err(e) = apply_msi_policy(cfg.block_msi) {
        log(&format!("MSI : {e}"));
    }
    if let Err(e) = apply_srp_deny_paths(&cfg.deny_paths) {
        log(&format!("SRP : {e}"));
    }

    // 6) Compteur du jour (session console active)
    let active = !active_sessions().is_empty();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    if state.day != today {
        state.day = today;
        state.active_secs = 0;
    }
    if active {
        state.active_secs += 15;
    }

    // 7) Quotas et plages horaires -> déconnexion de session console
    let mut reason: Option<String> = None;
    if let Some(mins) = cfg.daily_minutes {
        let used = state.active_secs / 60;
        if used >= mins {
            reason = Some(format!("quota quotidien atteint ({used}/{mins} min)"));
        }
    }
    if reason.is_none() && !in_allowed_window(&cfg.allowed_windows) {
        reason = Some("hors plage horaire autorisée".to_string());
    }
    match reason {
        Some(r) => {
            if state.quota_reason.as_deref() != Some(r.as_str()) {
                log(&format!("restriction Big Brother active : {r}"));
            }
            state.quota_lock = true;
            state.quota_reason = Some(r.clone());
            disconnect_active_sessions();
        }
        None => {
            if state.quota_lock {
                log("restriction Big Brother levée");
            }
            state.quota_lock = false;
            state.quota_reason = None;
        }
    }
    state.save();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hosts_section_build_and_replace() {
        let domains = vec!["youtube.com".into(), "discord.gg".into()];
        let section = build_hosts_section(&domains);
        assert!(section.contains("0.0.0.0 youtube.com"));
        assert!(section.contains("0.0.0.0 discord.gg"));
        assert!(section.contains(HOSTS_BEGIN));
        assert!(section.contains(HOSTS_END));

        let initial = "127.0.0.1 localhost\n";
        let replaced = replace_section(initial, &section);
        assert!(replaced.contains("127.0.0.1 localhost"));
        assert!(replaced.contains("0.0.0.0 youtube.com"));

        let removed = replace_section(&replaced, "");
        assert_eq!(removed.trim(), "127.0.0.1 localhost");
    }

    #[test]
    fn test_parse_hhmm() {
        assert_eq!(parse_hhmm("08:00"), Some(480));
        assert_eq!(parse_hhmm("18:30"), Some(1110));
        assert_eq!(parse_hhmm("23:59"), Some(1439));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("invalid"), None);
    }

    #[test]
    fn test_in_allowed_window() {
        assert!(in_allowed_window(&[]));

        let w = TimeWindow {
            days: vec![1, 2, 3, 4, 5, 6, 7],
            from: "00:00".into(),
            to: "23:59".into(),
        };
        assert!(in_allowed_window(&[w]));
    }

    #[test]
    fn test_deterministic_guid() {
        let g1 = deterministic_guid("Downloads\\*");
        let g2 = deterministic_guid("Downloads\\*");
        let g3 = deterministic_guid("Desktop\\*");
        assert_eq!(g1, g2);
        assert_ne!(g1, g3);
        assert_eq!(g1.len(), 36);
        assert_eq!(&g1[14..15], "4");
    }

    #[test]
    fn test_big_brother_config_serde() {
        let mut cfg = BigBrotherConfig::default();
        cfg.enabled = true;
        cfg.domains.push("test.local".into());
        cfg.blocked_processes.push("game.exe".into());
        cfg.block_msi = true;
        cfg.daily_minutes = Some(90);
        cfg.enable_kernel_driver = true;

        let json = serde_json::to_string(&cfg).unwrap();
        let loaded: BigBrotherConfig = serde_json::from_str(&json).unwrap();
        assert!(loaded.enabled);
        assert_eq!(loaded.domains, vec!["test.local"]);
        assert_eq!(loaded.blocked_processes, vec!["game.exe"]);
        assert!(loaded.block_msi);
        assert_eq!(loaded.daily_minutes, Some(90));
        assert!(loaded.enable_kernel_driver);
    }

    #[test]
    fn test_privilege_tier_labels() {
        assert_eq!(PrivilegeTier::Standard.short_name(), "standard");
        assert_eq!(PrivilegeTier::Admin.short_name(), "admin");
        assert_eq!(PrivilegeTier::Kernel.short_name(), "kernel");
    }
}
