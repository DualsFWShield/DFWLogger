//! Vérification de mise à jour : consulte l'API GitHub Releases du dépôt
//! configuré (`compte/repo`), compare la version installée au dernier tag
//! publié et affiche le résultat dans l'onglet Statut. Aucune donnée
//! personnelle n'est envoyée : une simple requête GET publique.

use crate::shared::Shared;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

/// Extrait `major.minor.patch` d'un tag type `v1.2.3` ou `v1.2.3-b42`.
fn tag_version(tag: &str) -> Option<(u64, u64, u64)> {
    let core = tag.trim().trim_start_matches('v');
    let core = core.split(['-', '+']).next()?; // ignore suffixe de build
    let mut it = core.split('.');
    let maj = it.next()?.parse().ok()?;
    let min = it.next()?.parse().ok()?;
    let pat = it.next()?.parse().ok()?;
    Some((maj, min, pat))
}

fn current_version() -> (u64, u64, u64) {
    tag_version(env!("CARGO_PKG_VERSION")).unwrap_or((0, 0, 0))
}

/// Requête publique : dernier tag + lien de la release.
fn fetch_latest(repo: &str) -> Result<(String, String), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build();
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let resp = agent
        .get(&url)
        .set("User-Agent", "DFWLogger")
        .call()
        .map_err(|e| format!("requête : {e}"))?;
    let json: serde_json::Value = resp.into_json().map_err(|e| format!("réponse : {e}"))?;
    let tag = json["tag_name"].as_str().unwrap_or("").to_string();
    let html = json["html_url"].as_str().unwrap_or("").to_string();
    if tag.is_empty() {
        return Err("aucune release publiée dans ce dépôt".into());
    }
    Ok((tag, html))
}

/// Une vérification complète (bloquante, réseau). À appeler dans un thread.
pub fn check_once(shared: &Arc<Shared>) {
    let repo = match shared.config.lock().unwrap().github_repo.clone() {
        Some(r) if !r.trim().is_empty() => r.trim().to_string(),
        _ => {
            let mut st = shared.status.lock().unwrap();
            st.update_note =
                Some("ⓘ dépôt GitHub non renseigné (Réglages → « Vérification de mise à jour »)".into());
            st.update_url = None;
            return;
        }
    };
    shared.status.lock().unwrap().update_note = Some("⌛ vérification en cours…".into());
    shared.dirty.store(true, Ordering::Relaxed);

    match fetch_latest(&repo) {
        Ok((tag, html)) => {
            let cur = current_version();
            let mut st = shared.status.lock().unwrap();
            if tag_version(&tag).map(|v| v > cur).unwrap_or(false) {
                st.update_note = Some(format!(
                    "🔄 mise à jour disponible : {tag} (installé : {})",
                    env!("CARGO_PKG_VERSION")
                ));
                st.update_url = Some(html);
            } else {
                st.update_note = Some(format!(
                    "✅ à jour (installé : {}, dernier tag : {tag})",
                    env!("CARGO_PKG_VERSION")
                ));
                st.update_url = None;
            }
        }
        Err(e) => {
            let mut st = shared.status.lock().unwrap();
            st.update_note = Some(format!("⛔ vérification impossible : {e}"));
            st.update_url = None;
        }
    }
    shared.dirty.store(true, Ordering::Relaxed);
}

/// Thread d'arrière-plan : vérifie au démarrage puis toutes les 24 h.
pub fn start_checker(shared: Arc<Shared>) {
    std::thread::Builder::new()
        .name("dfwlogger-update".into())
        .spawn(move || loop {
            check_once(&shared);
            std::thread::sleep(Duration::from_secs(24 * 3600));
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::tag_version;

    #[test]
    fn parse_tags() {
        assert_eq!(tag_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(tag_version("v0.3.0-b42"), Some((0, 3, 0)));
        assert_eq!(tag_version("nonsense"), None);
    }
}
