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
    /// Note de vérification de mise à jour (résultat ou état du processus).
    pub update_note: Option<String>,
    /// URL de la release plus récente, cliquable depuis l'UI.
    pub update_url: Option<String>,
}

/// État partagé entre l'UI, les collecteurs, l'icône tray et le serveur dashboard.
pub struct Shared {
    pub store: Arc<Mutex<Store>>,
    pub config: Mutex<Config>,
    pub events: Mutex<VecDeque<Event>>,
    pub counts: Mutex<BTreeMap<&'static str, u64>>,
    /// Temps cumulé (secondes de premier plan) par exécutable, depuis la session active.
    pub app_times: Mutex<BTreeMap<String, u64>>,
    /// Heatmap d'usage : (début de créneau horaire UTC en s d'époque) → secondes d'app au premier plan.
    pub heatmap: Mutex<BTreeMap<u64, u64>>,
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
            heatmap: Mutex::new(BTreeMap::new()),
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
            ev.push_back(event.clone());
            while ev.len() > 2000 {
                ev.pop_front();
            }
        }
        self.dirty.store(true, Ordering::Relaxed);
        crate::notify::maybe_toast(self, &event.kind);
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

        // Heatmap : reconstitution des créneaux horaires d'usage à partir des
        // sessions d'application journalisées (un événement avec durée == une
        // session d'usage terminée — pas de double comptage).
        {
            let mut heat = self.heatmap.lock().unwrap();
            heat.clear();
            for e in &events {
                if let EventKind::AppFocus {
                    duration_secs: Some(s),
                    ..
                } = &e.kind
                {
                    if *s > 0 {
                        if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&e.ts) {
                            add_heat_range(
                                &mut heat,
                                ts.timestamp() - *s as i64,
                                *s,
                            );
                        }
                    }
                }
            }
        }

        let mut ev = self.events.lock().unwrap();
        ev.clear();
        let start = events.len().saturating_sub(keep);
        for e in &events[start..] {
            ev.push_back(e.clone());
        }
    }

    /// Ajoute des secondes de premier plan à un exécutable, et met à jour la
    /// heatmap (créneaux horaires reconstitués depuis l'instant de début).
    pub fn record_app_time(&self, exe: &str, secs: u64) {
        if secs == 0 {
            return;
        }
        {
            let mut times = self.app_times.lock().unwrap();
            *times.entry(exe.to_string()).or_insert(0) += secs;
        }
        {
            let now = Utc::now().timestamp();
            let mut heat = self.heatmap.lock().unwrap();
            add_heat_range(&mut heat, now - secs as i64, secs);
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    /// Ne pas utiliser : la heatmap est refaite depuis le journal au déverrouillage.
    pub fn clear_heatmap(&self) {
        self.heatmap.lock().unwrap().clear();
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
        self.heatmap.lock().unwrap().clear();
        self.next_id.store(1, Ordering::Relaxed);
        *self.dashboard_token.lock().unwrap() = None;
        self.monitoring.store(false, Ordering::Relaxed);
    }
}

/// Répartit `secs` secondes d'usage sur les créneaux horaires UTC couverts
/// (slot = début de l'heure en s d'époque, arrondi au dessous).
fn add_heat_range(map: &mut BTreeMap<u64, u64>, start: i64, secs: u64) {
    if secs == 0 {
        return;
    }
    let mut cur = start;
    let mut left = secs as i64;
    while left > 0 {
        let slot = cur.div_euclid(3600) * 3600;
        let slot_end = slot + 3600;
        let take = (slot_end - cur).max(0).min(left);
        if take == 0 {
            break;
        }
        *map.entry(slot as u64).or_insert(0) += take as u64;
        left -= take;
        cur = slot_end;
    }
}

/// Données de heatmap des `days` derniers jours, pour l'UI et le dashboard :
/// (libellé du jour local, heure locale 0–23, secondes d'usage).
pub fn heatmap_rows(map: &BTreeMap<u64, u64>, days: u32) -> Vec<(String, u32, u64)> {
    use chrono::{Datelike, TimeZone};
    let today = chrono::Local::now().date_naive();
    let mut out = Vec::new();
    for d in 0..days {
        let date = today - chrono::Duration::days(d as i64);
        for h in 0..24 {
            let (y, m, day) = (date.year(), date.month(), date.day());
            let secs = match chrono::Local.with_ymd_and_hms(y, m, day, h, 0, 0) {
                chrono::LocalResult::Single(ts) => {
                    let slot = ts.timestamp().div_euclid(3600) * 3600;
                    map.get(&(slot as u64)).copied().unwrap_or(0)
                }
                _ => 0,
            };
            out.push((date.to_string(), h, secs));
        }
    }
    out
}
