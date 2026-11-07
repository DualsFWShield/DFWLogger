use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::io::Write;
use zeroize::Zeroizing;

use crate::model::{format_hms, Event, EventKind};
use crate::win;
use std::fs;
use std::path::{Path, PathBuf};

const MAGIC: &str = "DFWLOG-1";
const AAD: &[u8] = b"DFWLOG-ADR-1";
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const DEFAULT_ROTATE: u64 = 8 * 1024 * 1024;
const DEFAULT_KEEP: u32 = 6;

/// En-tête du journal, stocké en clair : params KDF + sel. Jamais de secret.
#[derive(Serialize, Deserialize, Clone)]
pub struct Header {
    pub magic: String,
    pub salt: String,
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
    pub created: String,
}

pub struct Store {
    pub dir: PathBuf,
    header: Option<Header>,
    key: Option<Zeroizing<[u8; 32]>>,
    rotate_limit: u64,
    keep_files: u32,
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&dir);
        Store {
            dir,
            header: None,
            key: None,
            rotate_limit: DEFAULT_ROTATE,
            keep_files: DEFAULT_KEEP,
        }
    }

    pub fn set_policy(&mut self, rotate_bytes: u64, keep_files: u32) {
        if rotate_bytes > 0 {
            self.rotate_limit = rotate_bytes;
        }
        if (1..=64).contains(&keep_files) {
            self.keep_files = keep_files;
        }
    }

    fn current_path(&self) -> PathBuf {
        self.dir.join("journal.enc")
    }

    fn rotated_path(&self, i: u32) -> PathBuf {
        self.dir.join(format!("journal.{i}.enc"))
    }

    pub fn exists(&self) -> bool {
        self.current_path().exists()
    }

    pub fn is_open(&self) -> bool {
        self.key.is_some()
    }

    pub fn is_initialized(&self) -> bool {
        self.header.is_some() || self.exists()
    }

    fn load_header(&mut self) -> Result<&Header, String> {
        if self.header.is_none() {
            let bytes = fs::read(self.current_path())
                .map_err(|e| format!("journal illisible : {e}"))?;
            let end = bytes
                .iter()
                .position(|&b| b == b'\n')
                .ok_or("en-tête du journal introuvable")?;
            let h: Header =
                serde_json::from_slice(&bytes[..end]).map_err(|e| format!("en-tête invalide : {e}"))?;
            if h.magic != MAGIC {
                return Err("format de journal inconnu".into());
            }
            self.header = Some(h);
        }
        Ok(self.header.as_ref().unwrap())
    }

    fn derive_key(header: &Header, password: &str) -> Result<[u8; 32], String> {
        let salt = B64
            .decode(&header.salt)
            .map_err(|e| format!("sel invalide : {e}"))?;
        let params = Params::new(header.m_kib, header.t, header.p, Some(32))
            .map_err(|e| format!("params KDF invalides : {e}"))?;
        let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let mut key = [0u8; 32];
        argon
            .hash_password_into(password.as_bytes(), &salt, &mut key)
            .map_err(|e| e.to_string())?;
        Ok(key)
    }

    /// Création initiale du journal avec un nouveau mot de passe parent.
    pub fn init(&mut self, password: &str) -> Result<(), String> {
        if password.len() < 8 {
            return Err("mot de passe : 8 caractères minimum".into());
        }
        let mut salt = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut salt);
        let header = Header {
            magic: MAGIC.to_string(),
            salt: B64.encode(salt),
            m_kib: 19456,
            t: 2,
            p: 1,
            created: chrono::Utc::now().to_rfc3339(),
        };
        let key = Self::derive_key(&header, password)?;
        self.header = Some(header);
        self.key = Some(Zeroizing::new(key));
        self.write_current_header()?;
        self.forget_dpapi();
        self.save_key_dpapi()?;
        Ok(())
    }

    fn write_current_header(&self) -> Result<(), String> {
        let h = self.header.as_ref().ok_or("pas d'en-tête")?;
        let mut data = serde_json::to_vec(h).map_err(|e| e.to_string())?;
        data.push(b'\n');
        let mut f = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(self.current_path())
            .map_err(|e| format!("écriture en-tête : {e}"))?;
        f.write_all(&data).map_err(|e| e.to_string())
    }    /// Déverrouillage par mot de passe parent (avec vérification par déchiffrement).
    pub fn unlock(&mut self, password: &str) -> Result<(), String> {
        let header = self.load_header()?.clone();
        let key = Self::derive_key(&header, password)?;
        // Vérifie le mot de passe en tentant de décrypter le premier enregistrement troué.
        if self.record_count() > 0 {
            let events = self.read_file(&self.current_path(), &key)?;
            if events.is_empty() && self.rotated_exists_any() {
                // fichier courant vide mais rotations existantes : vérifier sur la plus récente
                let p = self.rotated_path(1);
                let evs = self.read_file(&p, &key).map_err(|e| {
                    format!("mot de passe incorrect ou journal corrompu ({e})")
                })?;
                if evs.is_empty() {
                    return Err("mot de passe incorrect ou journal corrompu".into());
                }
            }
        }
        self.key = Some(Zeroizing::new(key));
        self.save_key_dpapi()?;
        Ok(())
    }

    fn rotated_exists_any(&self) -> bool {
        (1..=self.keep_files).any(|i| self.rotated_path(i).exists())
    }

    fn record_count(&self) -> usize {
        match fs::read(self.current_path()) {
            Ok(b) => {
                let mut n = 0usize;
                let mut off = header_len(&b);
                while off + 4 <= b.len() {
                    let len = u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]]) as usize;
                    if len < NONCE_LEN + TAG_LEN || off + 4 + len > b.len() {
                        break;
                    }
                    n += 1;
                    off += 4 + len;
                }
                n
            }
            Err(_) => 0,
        }
    }

    pub fn lock(&mut self) {
        self.key = None;
        self.forget_dpapi();
    }

    // ---- DPAPI : reprise automatique après redémarrage ----

    fn dpapi_path(&self) -> PathBuf {
        self.dir.join("dpapi.key")
    }

    fn save_key_dpapi(&self) -> Result<(), String> {
        let key = self.key.as_ref().ok_or("clé absente")?;
        let protected = win::dpapi_protect(key.as_slice())?;
        fs::write(self.dpapi_path(), B64.encode(&protected)).map_err(|e| e.to_string())
    }

    /// Tente de rouvrir le journal via la clé protégée par DPAPI (profil utilisateur).
    pub fn try_restore_from_dpapi(&mut self) -> Result<bool, String> {
        let raw = match fs::read_to_string(self.dpapi_path()) {
            Ok(s) => s,
            Err(_) => return Ok(false),
        };
        let blob = B64
            .decode(raw.trim())
            .map_err(|e| format!("dpapi.key corrompu : {e}"))?;
        let key_bytes = win::dpapi_unprotect(&blob)?;
        let mut key = [0u8; 32];
        if key_bytes.len() != 32 {
            return Err("clé DPAPI invalide".into());
        }
        key.copy_from_slice(&key_bytes);
        self.load_header()?;
        self.key = Some(Zeroizing::new(key));
        Ok(true)
    }

    pub fn forget_dpapi(&self) {
        let _ = fs::remove_file(self.dpapi_path());
    }

    // ---- Ecriture / lecture chiffrée ----

pub fn append(&mut self, event: &Event) -> Result<(), String> {
        let encrypt = {
            let key = self.key.as_ref().ok_or("journal verrouillé")?.as_ref();
            let plain = serde_json::to_vec(event).map_err(|e| e.to_string())?;
            let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
            let mut nonce_bytes = [0u8; NONCE_LEN];
            rand::thread_rng().fill_bytes(&mut nonce_bytes);
            let ct = cipher
                .encrypt(Nonce::from_slice(&nonce_bytes), Payload { msg: &plain, aad: AAD })
                .map_err(|e| e.to_string())?;
            let mut out = nonce_bytes.to_vec();
            out.extend_from_slice(&ct);
            out
        };
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.current_path())
            .map_err(|e| format!("ouverture journal : {e}"))?;
        use std::io::Write;
        f.write_all(&(encrypt.len() as u32).to_le_bytes())
            .and_then(|_| f.write_all(&encrypt))
            .map_err(|e| format!("écriture journal : {e}"))?;
        drop(f);

        if fs::metadata(self.current_path()).map(|m| m.len()).unwrap_or(0) > self.rotate_limit {
            self.rotate()?;
        }
        Ok(())
    }

    fn rotate(&mut self) -> Result<(), String> {
        for i in (1..self.keep_files).rev() {
            let from = self.rotated_path(i);
            if from.exists() {
                fs::rename(&from, self.rotated_path(i + 1))
                    .map_err(|e| format!("rotation : {e}"))?;
            }
        }
        fs::rename(self.current_path(), self.rotated_path(1))
            .map_err(|e| format!("rotation : {e}"))?;
        // supprime les plus vieux au-delà de la limite
        let old = self.rotated_path(self.keep_files + 1);
        let _ = fs::remove_file(old);
        self.write_current_header()
    }

    fn read_file(&self, path: &Path, key: &[u8; 32]) -> Result<Vec<Event>, String> {
        let bytes = fs::read(path).map_err(|e| format!("lecture {path:?} : {e}"))?;
        let mut out = Vec::new();
        let mut off = header_len(&bytes);
        while off + 4 <= bytes.len() {
            let len = u32::from_le_bytes([
                bytes[off],
                bytes[off + 1],
                bytes[off + 2],
                bytes[off + 3],
            ]) as usize;
            if len < NONCE_LEN + TAG_LEN || off + 4 + len > bytes.len() {
                break;
            }
            let rec = &bytes[off + 4..off + 4 + len];
            let plain = decrypt_record(key, rec).map_err(|e| e.to_string())?;
            if let Ok(ev) = serde_json::from_slice::<Event>(&plain) {
                out.push(ev);
            }
            off += 4 + len;
        }
        Ok(out)
    }

    /// Charge tout l'historique (fichiers pivotés du plus ancien au plus récent).
    pub fn load_all(&self) -> Result<Vec<Event>, String> {
        let key = self.key.as_ref().ok_or("journal verrouillé")?;
        let mut events = Vec::new();
        for i in (1..=self.keep_files).rev() {
            let p = self.rotated_path(i);
            if p.exists() {
                events.extend(self.read_file(&p, &key)?);
            }
        }
        if self.exists() {
            events.extend(self.read_file(&self.current_path(), &key)?);
        }
        Ok(events)
    }
}

fn header_len(bytes: &[u8]) -> usize {
    match bytes.iter().position(|&b| b == b'\n') {
        Some(i) => i + 1,
        None => 0,
    }
}

fn decrypt_record(key: &[u8; 32], rec: &[u8]) -> Result<Vec<u8>, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| e.to_string())?;
    let nonce = Nonce::from_slice(&rec[..NONCE_LEN]);
    cipher
        .decrypt(nonce, Payload { msg: &rec[NONCE_LEN..], aad: AAD })
        .map_err(|_| "déchiffrement impossible (mot de passe incorrect ?)".to_string())
}

/// Génère le contenu CSV complet (séparateur `;`) d'une liste d'événements déchiffrés.
/// Partagé par l'export UI (sélecteur de fichier) et l'export terminal non-admin.
pub fn csv_from_events(events: &[Event]) -> String {
    fn field(s: &str) -> String {
        if s.contains(';') || s.contains('"') || s.contains('\n') || s.contains(',') {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    }
    let mut out = String::from("id;date;type;details;duree\n");
    for ev in events {
        let dt = chrono::DateTime::parse_from_rfc3339(&ev.ts)
            .map(|d| {
                d.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string()
            })
            .unwrap_or_else(|_| ev.ts.clone());
        let duration = match &ev.kind {
            EventKind::AppFocus { duration_secs: Some(s), .. } => format_hms(*s),
            _ => String::new(),
        };
        out.push_str(&format!(
            "{};{};{};{};{}\n",
            ev.id,
            field(&dt),
            field(ev.kind.label()),
            field(&ev.kind.summary()),
            field(&duration)
        ));
    }
    out
}

/// Sous-commande terminal non-admin : `dfwlogger export [fichier.csv]`
pub fn cli_export(dir: &Path, out: Option<&str>) -> Result<(), String> {
    print!("Mot de passe parent : ");
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    let password = rpassword::read_password().map_err(|e| e.to_string())?;
    let mut store = Store::new(dir.to_path_buf());
    store.unlock(&password)?;
    let mut events = store.load_all()?;
    events.sort_by_key(|e| e.id);
    if events.is_empty() {
        return Err("aucun événement dans le journal (journal vide)".into());
    }
    let csv = csv_from_events(&events);
    let path = out
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dir.join(format!(
                "dfwlogger-export-{}.csv",
                chrono::Local::now().format("%Y%m%d-%H%M%S")
            ))
        });
    std::fs::write(&path, csv).map_err(|e| format!("écriture : {e}"))?;
    println!("CSV écrit : {}", path.display());
    Ok(())
}

/// Sous-commande terminal : `dfwlogger tail [N]`
pub fn cli_tail(dir: &Path, n: usize) -> Result<(), String> {
    print!("Mot de passe parent : ");
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let password = rpassword::read_password().map_err(|e| e.to_string())?;
    let mut store = Store::new(dir.to_path_buf());
    store.unlock(&password)?;
    let mut events = store.load_all()?;
    events.sort_by_key(|e| e.id);
    let start = events.len().saturating_sub(n);
    println!("{:<21}  {:<14}  événement", "date", "type");
    println!("{}", "-".repeat(90));
    for ev in &events[start..] {
        let dt = chrono::DateTime::parse_from_rfc3339(&ev.ts)
            .map(|d| d.with_timezone(&chrono::Local))
            .unwrap_or_else(|_| chrono::Local::now().into());
        println!(
            "{:<21}  {} {:<13}  {}",
            dt.format("%Y-%m-%d %H:%M:%S"),
            ev.kind.icon(),
            ev.kind.label(),
            ev.kind.summary()
        );
    }
    Ok(())
}

/// Retourne les N derniers événements et le nombre total, pour le CLI/UI après déverrouillage.
#[allow(dead_code)]
pub fn tail_events(store: &Store, n: usize) -> Result<(Vec<Event>, usize), String> {
    let mut events = store.load_all()?;
    events.sort_by_key(|e| e.id);
    let total = events.len();
    let start = total.saturating_sub(n);
    Ok((events[start..].to_vec(), total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EventKind;
    use std::env;

    fn temp_dir(name: &str) -> PathBuf {
        let mut d = env::temp_dir();
        d.push(format!("dfwlogger-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn event(id: u64, exe: &str) -> Event {
        Event {
            id,
            ts: "2026-01-01T12:00:00Z".into(),
            kind: EventKind::AppFocus {
                exe: exe.into(),
                title: "Fenêtre".into(),
                duration_secs: None,
            },
        }
    }

    #[test]
    fn init_append_unlock_read() {
        let dir = temp_dir("roundtrip");
        let mut store = Store::new(dir.clone());
        store.set_policy(4096, 3);
        store.init("mot-de-passe-parent").unwrap();
        assert!(store.is_open());
        for i in 1..=12 {
            store.append(&event(i, &format!("app{i}.exe"))).unwrap();
        }
        // Force rotation manuelle pour tester le multi-fichiers
        store.rotate().unwrap();
        for i in 13..=16 {
            store.append(&event(i, &format!("app{i}.exe"))).unwrap();
        }
        store.lock();
        assert!(!store.is_open());

        let mut reopened = Store::new(dir.clone());
        reopened.set_policy(4096, 3);
        reopened.unlock("mot-de-passe-parent").unwrap();
        let events = reopened.load_all().unwrap();
        assert_eq!(events.len(), 16, "16 événements attendus");
        assert_eq!(events[0].id, 1);
        if let EventKind::AppFocus { exe, .. } = &events[15].kind {
            assert_eq!(exe, "app16.exe");
            assert_eq!(events[15].id, 16);
        } else {
            panic!("dernier event attendu AppFocus");
        }

        // mauvais mot de passe
        let mut wrong = Store::new(dir.clone());
        assert!(wrong.unlock("mot-de-passe-incorrect").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotation_respects_limit() {
        let dir = temp_dir("rotate");
        let mut store = Store::new(dir.clone());
        store.set_policy(1024, 3);
        store.init("mot-de-passe-parent").unwrap();
        for i in 1..=60 {
            store.append(&event(i, &format!("application-{i}.exe"))).unwrap();
        }
        // la rotation automatique a dû créer des fichiers pivotés
        let rotated = (1..=3).filter(|i| dir.join(format!("journal.{i}.enc")).exists()).count();
        assert!(rotated >= 1, "au moins un fichier pivoté attendu (trouvé {rotated})");
        let events = store.load_all().unwrap();
        // Avec une rotation à ~1 Ko (≈6 événements/fichier) et 3 fichiers pivotés,
        // une partie de l'historique est remplacée : les plus récents sont toujours là.
        assert!(!events.is_empty());
        assert_eq!(events.last().unwrap().id, 60, "le dernier événement doit être conservé");
        // pas plus de keep_files+1 fichiers
        let files = fs::read_dir(&dir).unwrap().filter(|e| e.as_ref().map(|e| e.path().extension().map(|x| x == "enc").unwrap_or(false)).unwrap_or(false)).count();
        assert!(files <= 4, "trop de fichiers : {files}");
        let _ = fs::remove_dir_all(&dir);
    }
}
