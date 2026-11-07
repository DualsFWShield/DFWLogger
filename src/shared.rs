use crate::config::Config;
use crate::model::{Event, EventKind};
use crate::store::Store;
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use chrono::Utc;
#[derive(Default, Clone, Debug)]
pub struct Status {
    pub elevated: bool,
    pub session_ok: bool,
    pub foreground_ok: bool,
    pub recent_ok: bool,
    pub security_ok: bool,
    pub security_note: String,
    pub dashboard_listening: bool,
    pub dashboard_error: Option<String>,
}

/// État partagé entre l'UI, les collecteurs, l'icône tray et le serveur dashboard.
pub struct Shared {
    pub store: Arc<Mutex<Store>>,
    pub config: Mutex<Config>,
    pub events: Mutex<VecDeque<Event>>,
    pub counts: Mutex<BTreeMap<&'static str, u64>>,
    /// Temps cumulé (secondes de premier plan) par exécutable, depuis la session active.
    pub app_times: Mutex<BTreeMap<String, u64>>,
    pub status: Mutex<Status>,
    pub monitoring: AtomicBool,
    pub next_id: AtomicU64,
    pub dashboard_token: Mutex<Option<String>>,
    /// true = il y a de l'activité, l'UI re-dessine immédiatement.
    pub dirty: AtomicBool,
    /// true = l'utilisateur veut faire réapparaître la fenêtre (clic tray/menu).
    pub show_request: AtomicBool,
}

impl Shared {
    pub fn new(store: Arc<Mutex<Store>>, config: Config, elevated: bool) -> Arc<Self> {
        Arc::new(Self {
            store,
            config: Mutex::new(config),
            events: Mutex::new(VecDeque::with_capacity(2048)),
            counts: Mutex::new(BTreeMap::new()),
            app_times: Mutex::new(BTreeMap::new()),
            status: Mutex::new(Status {
                elevated,
                ..Status::default()
            }),
            monitoring: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
            dashboard_token: Mutex::new(None),
            dirty: AtomicBool::new(false),
            show_request: AtomicBool::new(false),
        })
    }

    /// Journalise un événement : chiffrement + mémoire. Ignoré si le journal est verrouillé.
    pub fn push(&self, kind: EventKind) {
        if !self.monitoring.load(Ordering::Relaxed) {
            return;
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let event = Event {
            id,
            ts: Utc::now().to_rfc3339(),
            kind,
        };
        let label = event.kind.label();
        {
            let mut store = self.store.lock().unwrap();
            if let Err(e) = store.append(&event) {
                eprintln!("écriture journal : {e}");
            }
        }
        {
            let mut c = self.counts.lock().unwrap();
            *c.entry(label).or_insert(0) += 1;
        }
        {
            let mut ev = self.events.lock().unwrap();
            ev.push_back(event);
            while ev.len() > 2000 {
                ev.pop_front();
            }
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Charge l'historique déchiffré dans la mémoire vive (fin d'historique = premier affiché).
    pub fn import_from_store(&self, keep: usize) {
        let events = {
            let store = self.store.lock().unwrap();
            store.load_all().unwrap_or_default()
        };
        let next = events.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        self.next_id.store(next, Ordering::Relaxed);
        let mut counts = self.counts.lock().unwrap();
        counts.clear();
        for e in &events {
            *counts.entry(e.kind.label()).or_insert(0) += 1;
        }
        drop(counts);
        let mut ev = self.events.lock().unwrap();
        ev.clear();
        let start = events.len().saturating_sub(keep);
        for e in &events[start..] {
            ev.push_back(e.clone());
        }
    }

    /// Ajoute des secondes de premier plan à un exécutable.
    pub fn record_app_seconds(&self, exe: &str, secs: u64) {
        let mut times = self.app_times.lock().unwrap();
        *times.entry(exe.to_string()).or_insert(0) += secs;
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Top applications par temps cumulé, trié.
    pub fn top_apps(&self, n: usize) -> Vec<(String, u64)> {
        let times = self.app_times.lock().unwrap();
        let mut v: Vec<(String, u64)> = times
            .iter()
            .map(|(k, s)| (k.clone(), *s))
            .collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.truncate(n);
        v
    }

    pub fn clear_memory(&self) {
        self.events.lock().unwrap().clear();
        self.counts.lock().unwrap().clear();
        self.app_times.lock().unwrap().clear();
        self.next_id.store(1, Ordering::Relaxed);
        *self.dashboard_token.lock().unwrap() = None;
        self.monitoring.store(false, Ordering::Relaxed);
    }
}
