//! DFWLogger — superviseur familial transparent.
//!
//! Principe : l'application est VISIBLE (fenêtre, gestionnaire des tâches).
//! Elle journalise de façon chiffrée (AES-256-GCM) : sessions verrouillées /
//! déverrouillées, applications au premier plan, fichiers ouverts récemment,
//! échecs de connexion Windows (option, session élevée), et expose un
//! dashboard HTTPS sécurisé sur le LAN. Aucun keylogger, aucun mode furtif.

mod collect;
mod config;
mod dashhtml;
mod model;
mod net;
mod shared;
mod store;
mod tray;
mod ui;
mod win;

use shared::Shared;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

fn data_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(base).join("DFWLogger")
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // Sous-commande terminal non-admin : consulter le journal chiffré.
    if let Some(pos) = args.iter().position(|a| a == "tail") {
        let n: usize = args
            .get(pos + 1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);
        if let Err(e) = store::cli_tail(&data_dir(), n) {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if let Some(pos) = args.iter().position(|a| a == "export") {
        let out = args
            .get(pos + 1)
            .filter(|a| !a.starts_with('-'))
            .cloned();
        if let Err(e) = store::cli_export(&data_dir(), out.as_deref()) {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if let Some(_) = args.iter().position(|a| a == "help" || a == "--help") {
        print_help();
        return Ok(());
    }

    let elevated = win::is_elevated();
    let dir = data_dir();
    let mut config = config::Config::load(&dir);
    // Le registre est la source de vérité effectif du démarrage auto.
    if win::autostart_enabled() {
        config.autostart = true;
    }
    if elevated {
        // Session élevée (consentie via UAC) : active la collecte du journal Security.
        config.security_log = true;
        let _ = config.save(&dir);
    }

    let mut store = store::Store::new(dir.clone());
    store.set_policy(config.rotate_bytes, config.keep_files);
    let shared = Shared::new(Arc::new(Mutex::new(store)), config.clone(), elevated);

    // Si le journal est déjà initialisé et qu'il y a un mot de passe mémorisé (DPAPI),
    // l'UI tentera une reprise automatique (voir ui::DfwApp::new).

    collect::spawn_all(shared.clone());
    net::spawn_dashboard(shared.clone());

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("DFWLogger — Superviseur familial")
            .with_inner_size([820.0, 680.0]),
        ..Default::default()
    };
    let _ = eframe::run_native(
        "DFWLogger",
        options,
        Box::new(move |cc| -> Box<dyn eframe::App> {
            Box::new(ui::DfwApp::new(cc, shared))
        }),
    );
    std::process::exit(0)
}

fn print_help() {
    println!(
        "DFWLogger — superviseur familial transparent

Utilisation :
  dfwlogger                 ouvre l'interface graphique (verre de consigne : déverrouillez d'abord)
  dfwlogger tail [N]        affiche les N derniers événements déchiffrés (30 par défaut)
  dfwlogger export [f.csv]  exporte tout l'historique déchiffré en CSV (séparateur « ; »)
  dfwlogger help            cette aide

Notes :
  • La collecte du journal Security (échecs de connexion) nécessite une relance en
    élévation, proposée dans Réglages → bouton \"Relancer avec élévation…\".
  • Données chiffrées dans %LOCALAPPDATA%\\DFWLogger."
    );
}
