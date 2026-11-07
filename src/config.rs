use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    /// Collecter le journal Security (échecs de connexion) — nécessite une session élévation.
    pub security_log: bool,
    pub dashboard_enabled: bool,
    pub dashboard_port: u16,
    pub recent_poll_secs: u64,
    pub security_poll_secs: u64,
    /// Taille au-delà de laquelle le fichier courant est pivoté.
    pub rotate_bytes: u64,
    /// Nombre de fichiers pivotés conservés.
    pub keep_files: u32,
    pub autostart: bool,
    /// Notifications toast Windows à chaque verrouillage de session.
    pub notify_lock: bool,
    /// Notifications toast Windows à chaque échec de connexion (4625).
    pub notify_failed_logon: bool,
    /// Dépôt GitHub `compte/repo` pour la vérification de mise à jour.
    pub github_repo: Option<String>,
    /// Emprunte Argon2 (PHC) du mot de passe « admin » protégeant les réglages.
    pub admin_phc: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            security_log: false,
            dashboard_enabled: true,
            dashboard_port: 4782,
            recent_poll_secs: 30,
            security_poll_secs: 60,
            rotate_bytes: 8 * 1024 * 1024,
            keep_files: 6,
            autostart: false,
            notify_lock: true,
            notify_failed_logon: true,
            github_repo: None,
            admin_phc: None,
        }
    }
}

impl Config {
    pub fn path(dir: &Path) -> std::path::PathBuf {
        dir.join("config.json")
    }

    pub fn load(dir: &Path) -> Config {
        std::fs::read(Config::path(dir))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let path = Config::path(dir);
        let data = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, data).map_err(|e| e.to_string())
    }
}
