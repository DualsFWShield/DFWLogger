use serde::{Deserialize, Serialize};

/// Types d'événements journalisés, avec tag JSON `kind`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    Started,
    SessionUnlock,
    SessionLock { cause: String },
    /// Changement d'application au premier plan. `duration_secs` = temps passé
    /// sur l'application précédente (None au tout premier événement).
    #[serde(rename_all = "snake_case")]
    AppFocus {
        exe: String,
        title: String,
        #[serde(default)]
        duration_secs: Option<u64>,
    },
    RecentFile { link: String, target: String },
    FailedLogon {
        account: String,
        workstation: String,
        ip: String,
        code: String,
    },
}

impl EventKind {
    pub fn label(&self) -> &'static str {
        match self {
            EventKind::Started => "démarrage",
            EventKind::SessionUnlock => "déverrouillage",
            EventKind::SessionLock { .. } => "verrouillage",
            EventKind::AppFocus { .. } => "application",
            EventKind::RecentFile { .. } => "fichier ouvert",
            EventKind::FailedLogon { .. } => "connexion échouée",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            EventKind::Started => "🟢",
            EventKind::SessionUnlock => "🔓",
            EventKind::SessionLock { .. } => "🔒",
            EventKind::AppFocus { .. } => "🪟",
            EventKind::RecentFile { .. } => "📁",
            EventKind::FailedLogon { .. } => "⚠️",
        }
    }

    pub fn summary(&self) -> String {
        match self {
            EventKind::Started => "Surveillance démarrée".to_string(),
            EventKind::SessionUnlock => "Session déverrouillée".to_string(),
            EventKind::SessionLock { cause } => format!("Session verrouillée ({cause})"),
            EventKind::AppFocus { exe, title, duration_secs } => {
                let mut base = if title.trim().is_empty() {
                    exe.clone()
                } else {
                    format!("{exe} — « {} »", title.trim())
                };
                if let Some(s) = duration_secs {
                    if *s >= 2 {
                        base.push_str(&format!(" ({} d'utilisation)", format_hms(*s)));
                    }
                }
                base
            }
            EventKind::RecentFile { link, target } => format!("{target} (via {link})"),
            EventKind::FailedLogon {
                account,
                workstation,
                ip,
                code,
            } => format!("Compte «{account}» — poste {workstation}, IP {ip}, code {code}"),
        }
    }
}

/// Une événement horodaté et identifié de façon monotone.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Event {
    pub id: u64,
    pub ts: String,
    #[serde(flatten)]
    pub kind: EventKind,
}

/// Formate une durée en français : « 1 h 05 », « 4 min 12 s », « 45 s ».
pub fn format_hms(secs: u64) -> String {
    if secs >= 3600 {
        format!("{} h {:02}", secs / 3600, (secs % 3600) / 60)
    } else if secs >= 60 {
        format!("{} min {:02} s", secs / 60, secs % 60)
    } else {
        format!("{secs} s")
    }
}
