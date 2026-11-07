//! Collecteurs : session (verrouillage/déverrouillage), application au premier plan,
//! fichiers récemment ouverts, échecs de connexion (journal Security, requiere élévation).

use crate::model::EventKind;
use crate::shared::Shared;
use std::collections::HashSet;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const POLL_SESSION_MS: u64 = 1500;
const POLL_FOCUS_MS: u64 = 2000;

pub fn spawn_all(shared: Arc<Shared>) {
    let s2 = shared.clone();
    let s3 = shared.clone();
    let s4 = shared.clone();
    std::thread::spawn(move || session_monitor(shared));
    std::thread::spawn(move || foreground_monitor(s2));
    std::thread::spawn(move || recent_files_monitor(s3));
    std::thread::spawn(move || security_log_monitor(s4));
}

// ---- Session : verrouillage / déverrouillage ----

fn session_monitor(shared: Arc<Shared>) {
    let mut last_open: Option<bool> = None;
    loop {
        std::thread::sleep(Duration::from_millis(POLL_SESSION_MS));
        if !shared.monitoring.load(Ordering::Relaxed) {
            last_open = None;
            continue;
        }
        let open = crate::win::is_unlocked();
        match last_open {
            None => {
                if open {
                    shared.push(EventKind::SessionUnlock);
                } else {
                    shared.push(EventKind::SessionLock {
                        cause: "session déjà verrouillée".into(),
                    });
                }
            }
            Some(prev) if prev != open => {
                if open {
                    shared.push(EventKind::SessionUnlock);
                } else {
                    shared.push(EventKind::SessionLock {
                        cause: "verrouillage / UAC / écran de veille".into(),
                    });
                }
            }
            _ => {}
        }
        last_open = Some(open);
        shared.status.lock().unwrap().session_ok = true;
    }
}

// ---- Application au premier plan + temps passé par application ----

fn foreground_monitor(shared: Arc<Shared>) {
    use std::time::Instant;
    let mut current: Option<(String, String)> = None; // (exe, titre courant)
    let mut started: Option<Instant> = None;
    loop {
        std::thread::sleep(Duration::from_millis(POLL_FOCUS_MS));
        let monitoring = shared.monitoring.load(Ordering::Relaxed);
        let open = monitoring && crate::win::is_unlocked();

        // Fin de session d'application : verrouillage (ou collecte coupée).
        if !open {
            if let (Some((exe, _)), Some(t0)) = (current.as_mut(), started) {
                let elapsed = t0.elapsed().as_secs();
                if elapsed > 0 {
                    shared.record_app_time(exe, elapsed);
                    shared.push(EventKind::AppFocus {
                        exe: exe.clone(),
                        title: String::new(),
                        duration_secs: Some(elapsed),
                    });
                }
            }
            current = None;
            started = None;
            if !monitoring {
                continue;
            }
            shared.status.lock().unwrap().foreground_ok = true;
            continue;
        }

        if let Some((exe, title)) = crate::win::foreground_app() {
            let exe = if exe.is_empty() {
                "(processus protégé)".to_string()
            } else {
                exe
            };
            let switched = current.as_ref().map(|(c, _)| c.as_str()) != Some(exe.as_str());
            if switched {
                let prev_elapsed = started
                    .map(|t0| t0.elapsed().as_secs())
                    .unwrap_or(0);
                if let Some((prev_exe, _)) = &current {
                    if prev_elapsed > 0 {
                        shared.record_app_time(prev_exe, prev_elapsed);
                    }
                    let prev_exe = prev_exe.clone();
                    shared.push(EventKind::AppFocus {
                        exe: prev_exe,
                        title: String::new(),
                        duration_secs: Some(prev_elapsed),
                    });
                }
                shared.push(EventKind::AppFocus {
                    exe: exe.clone(),
                    title: title.clone(),
                    duration_secs: None,
                });
                current = Some((exe, title));
                started = Some(Instant::now());
            }
        }
        shared.status.lock().unwrap().foreground_ok = true;
    }
}

// ---- Fichiers récemment ouverts (dossier Shell Recent) ----

fn recent_dir() -> Option<PathBuf> {
    let appdata = std::env::var("APPDATA").ok()?;
    let d = PathBuf::from(appdata).join("Microsoft").join("Windows").join("Recent");
    d.exists().then_some(d)
}

fn recent_files_monitor(shared: Arc<Shared>) {
    let mut known: std::collections::HashMap<PathBuf, (u64, u32)> = std::collections::HashMap::new();
    loop {
        let poll = {
            let cfg = shared.config.lock().unwrap();
            cfg.recent_poll_secs.max(5)
        };
        std::thread::sleep(Duration::from_secs(poll));
        if !shared.monitoring.load(Ordering::Relaxed) {
            continue;
        }
        let Some(dir) = recent_dir() else { continue };
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let mut latest: std::collections::HashMap<PathBuf, (u64, u32)> = std::collections::HashMap::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("lnk")) != Some(true)
            {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    let secs = modified
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let nanos = modified
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.subsec_nanos())
                        .unwrap_or(0);
                    latest.insert(path, (secs, nanos));
                }
            }
        }
        for (path, stamp) in &latest {
            let fresh = match known.get(path) {
                None => true,
                Some(old) => *old != *stamp,
            };
            if !fresh {
                continue;
            }
            if let Some(target) = resolve_lnk(path) {
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                shared.push(EventKind::RecentFile {
                    link: name,
                    target,
                });
            }
        }
        known = latest;
        shared.status.lock().unwrap().recent_ok = true;
    }
}

fn resolve_lnk(path: &std::path::Path) -> Option<String> {
    let lnk = parselnk::Lnk::try_from(path).ok()?;
    // Cible : LocalBasePath(Unicode) + suffixe, sinon chemin relatif.
    let info = &lnk.link_info;
    let base = info
        .local_base_path_unicode
        .clone()
        .or_else(|| info.local_base_path.clone());
    let suffix = info
        .common_path_suffix_unicode
        .clone()
        .or_else(|| info.common_path_suffix.clone());
    let mut target = base.unwrap_or_default();
    if let Some(s) = suffix {
        if !s.is_empty() {
            if !target.ends_with('\\') {
                target.push('\\');
            }
            target.push_str(&s);
        }
    }
    if target.is_empty() {
        target = lnk.relative_path()?.to_string_lossy().to_string();
        if let Some(wd) = lnk.working_dir() {
            if !wd.to_string_lossy().is_empty() && !target.contains(':') && !target.starts_with('\\')
            {
                let mut full = wd.to_string_lossy().to_string();
                if !full.ends_with('\\') {
                    full.push('\\');
                }
                full.push_str(&target);
                target = full;
            }
        }
    }
    if target.is_empty() {
        None
    } else {
        Some(target)
    }
}

// ---- Échecs de connexion (journal Security, event 4625) ----

#[derive(Debug)]
struct FailedLogonEvent {
    time: String,
    account: String,
    workstation: String,
    ip: String,
    code: String,
}

fn security_log_monitor(shared: Arc<Shared>) {
    let mut seen: HashSet<String> = HashSet::new();
    loop {
        let (enabled, poll) = {
            let cfg = shared.config.lock().unwrap();
            (cfg.security_log, cfg.security_poll_secs.max(30))
        };
        std::thread::sleep(Duration::from_secs(poll));
        if !enabled {
            set_note(&shared, "désactivé".into());
            continue;
        }
        if !shared.status.lock().unwrap().elevated {
            set_note(&shared, "nécessite une relance en élévation".into());
            continue;
        }
        match query_failed_logons() {
            Ok(events) => {
                for ev in &events {
                    let key = format!("{}|{}|{}|{}", ev.time, ev.account, ev.ip, ev.code);
                    if seen.insert(key) && shared.monitoring.load(Ordering::Relaxed) {
                        shared.push(EventKind::FailedLogon {
                            account: ev.account.clone(),
                            workstation: ev.workstation.clone(),
                            ip: ev.ip.clone(),
                            code: ev.code.clone(),
                        });
                    }
                    if seen.len() > 10_000 {
                        seen = seen.iter().skip(5_000).cloned().collect();
                    }
                }
                set_note(&shared, format!("actif — {} événement(s) connus", seen.len()));
                shared.status.lock().unwrap().security_ok = true;
            }
            Err(e) => {
                set_note(&shared, format!("erreur : {e}"));
                shared.status.lock().unwrap().security_ok = false;
            }
        }
    }
}

fn set_note(shared: &Arc<Shared>, note: String) {
    let mut st = shared.status.lock().unwrap();
    if st.security_note != note {
        st.security_note = note;
    }
}

fn query_failed_logons() -> Result<Vec<FailedLogonEvent>, String> {
    let out = std::process::Command::new("wevtutil")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "qe",
            "Security",
            "/q:*[System[(EventID=4625)]]",
            "/c:100",
            "/rd:true",
            "/f:xml",
        ])
        .output()
        .map_err(|e| format!("lancement wevtutil : {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("").to_string();
        return Err(if err.is_empty() {
            format!("wevtutil a échoué (code {:?})", out.status.code())
        } else {
            err
        });
    }
    parse_security_xml(&String::from_utf8_lossy(&out.stdout))
}

fn parse_security_xml(xml: &str) -> Result<Vec<FailedLogonEvent>, String> {
    use quick_xml::events::Event as Xe;
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.trim_text(true);

    let mut result = Vec::new();
    let mut in_event_data = false;
    let mut current_field: Option<String> = None;
    let mut current_value = String::new();
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut time: Option<String> = None;
    let mut scratch = Vec::new();

    loop {
        scratch.clear();
        match reader.read_event_into(&mut scratch) {
            Ok(Xe::Start(e)) => match e.name().as_ref() {
                b"TimeCreated" => {
                    for a in e.attributes().flatten() {
                        if a.key.as_ref() == b"SystemTime" {
                            let raw: &[u8] = &a.value;
                            time = Some(String::from_utf8_lossy(raw).to_string());
                        }
                    }
                }
                b"EventData" => {
                    in_event_data = true;
                }
                b"Data" if in_event_data => {
                    current_field = e
                        .try_get_attribute("Name")
                        .ok()
                        .flatten()
                        .and_then(|a| {
                            let raw: &[u8] = &a.value;
                            Some(String::from_utf8_lossy(raw).to_string())
                        });
                    current_value.clear();
                }
                _ => {}
            },
            Ok(Xe::Text(t)) => {
                if in_event_data && current_field.is_some() {
                    if let Ok(txt) = t.unescape() {
                        current_value.push_str(&txt);
                    }
                }
            }
            Ok(Xe::End(e)) => match e.name().as_ref() {
                b"Data" => {
                    if let Some(name) = current_field.take() {
                        fields.push((name, std::mem::take(&mut current_value)));
                    }
                }
                b"EventData" => {
                    let get = |n: &str| {
                        fields
                            .iter()
                            .find(|(k, _)| k == n)
                            .map(|(_, v)| v.clone())
                            .unwrap_or_default()
                    };
                    result.push(FailedLogonEvent {
                        time: time.clone().unwrap_or_default(),
                        account: get("TargetUserName"),
                        workstation: get("WorkstationName"),
                        ip: get("IpAddress"),
                        code: {
                            let sub = get("SubStatus");
                            if sub.is_empty() {
                                get("Status")
                            } else {
                                sub
                            }
                        },
                    });
                    fields.clear();
                    in_event_data = false;
                }
                _ => {}
            },
            Ok(Xe::Empty(e)) => {
                // Balise auto-fermée : utile uniquement pour <TimeCreated SystemTime=.../>
                if e.name().as_ref() == b"TimeCreated" {
                    for a in e.attributes().flatten() {
                        if a.key.as_ref() == b"SystemTime" {
                            let raw: &[u8] = &a.value;
                            time = Some(String::from_utf8_lossy(raw).to_string());
                        }
                    }
                }
            }
            Ok(Xe::Eof) => break,
            Ok(_) => {}
            Err(_) => break,
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_failed_logon_xml() {
        let xml = r#"<Event xmlns="http://schemas.microsoft.com/win/2004/08/events/event">
<System><TimeCreated SystemTime="2026-02-03T10:11:12.000000000Z"/></System>
<EventData>
<Data Name="TargetUserName">jean</Data>
<Data Name="WorkstationName">DESKTOP-ABC</Data>
<Data Name="IpAddress">192.168.1.20</Data>
<Data Name="SubStatus">0xC0000064</Data>
</EventData>
</Event>"#;
        let evs = parse_security_xml(xml).unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].account, "jean");
        assert_eq!(evs[0].workstation, "DESKTOP-ABC");
        assert_eq!(evs[0].ip, "192.168.1.20");
        assert_eq!(evs[0].code, "0xC0000064");
        assert_eq!(evs[0].time, "2026-02-03T10:11:12.000000000Z");
    }
}
