//! DFWLogger — superviseur familial transparent.
//!
//! Principe : l'application est VISIBLE (fenêtre, gestionnaire des tâches).
//! Elle journalise de façon chiffrée (AES-256-GCM) : sessions verrouillées /
//! déverrouillées, applications au premier plan, fichiers ouverts récemment,
//! échecs de connexion Windows (option, session élevée), et expose un
//! dashboard HTTPS sécurisé sur le LAN. Aucun keylogger, aucun mode furtif.

mod big_brother;
mod collect;
mod config;
mod dashhtml;
mod model;
mod net;
mod notify;
mod serbert;
mod shared;
mod store;
mod svc;
mod tray;
mod ui;
mod update;
mod win;

use shared::Shared;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn data_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
    let dir = PathBuf::from(base).join("DFWLogger");
    dir
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // Mode service Windows : boucle Big Brother (SYSTEM). Pas de GUI ici.
    if args.iter().any(|a| a == "--service") {
        if let Err(e) = svc::run_dispatcher() {
            big_brother::log(&format!("dispatcher : {e}"));
        }
        return Ok(());
    }

    let quiet = args.iter().any(|a| a == "--quiet" || a == "-q" || a == "--silent");

    // Commandes des 3 Modes : Standard, Admin, Kernel
    if let Some(pos) = args.iter().position(|a| a == "--mode") {
        let requested = args.get(pos + 1).map(|s| s.to_ascii_lowercase());
        match requested.as_deref() {
            Some("standard") => {
                if !win::is_admin() {
                    let _ = win::relaunch_elevated_args(&["--mode", "standard"]);
                    return Ok(());
                }
                let _ = serbert::uninstall_driver_service();
                let _ = big_brother::uninstall_service();
                println!("Passage en Mode 1 : Standard (Sans autorisation) effectue.");
                return Ok(());
            }
            Some("admin") => {
                if !win::is_admin() {
                    let _ = win::relaunch_elevated_args(&["--mode", "admin"]);
                    return Ok(());
                }
                let _ = serbert::uninstall_driver_service();
                if let Err(e) = big_brother::install_service() {
                    eprintln!("Erreur activation Mode Admin : {e}");
                    std::process::exit(1);
                }
                println!("Passage en Mode 2 : Admin (Big Brother SYSTEM) effectue.");
                return Ok(());
            }
            Some("kernel") | Some("noyau") => {
                if !win::is_admin() {
                    let _ = win::relaunch_elevated_args(&["--mode", "kernel"]);
                    return Ok(());
                }
                let _ = big_brother::install_service();
                let p = serbert::default_driver_path();
                let _ = serbert::install_driver_service(&p);
                if let Err(e) = serbert::start_driver() {
                    eprintln!("Avertissement demarrage pilote noyau : {e}");
                }
                println!("Passage en Mode 3 : Noyau (Serbert Ring 0) effectue.");
                return Ok(());
            }
            Some("status") | None => {
                let current_tier = big_brother::PrivilegeTier::current();
                println!("Mode actuel : {}", current_tier.badge());
                println!("Description : {}", current_tier.description());
                return Ok(());
            }
            Some(other) => {
                eprintln!("Mode inconnu : '{other}'. Modes valides : standard, admin, kernel, status.");
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|a| a == "--status" || a == "--bigbrother-status") {
        let current_tier = big_brother::PrivilegeTier::current();
        println!("=== Statut des 3 Modes DFWLogger ===");
        println!("Mode actuel : {}", current_tier.badge());
        println!("Description : {}", current_tier.description());
        println!();
        let status = big_brother::service_status().unwrap_or_else(|| "NON_INSTALLE".to_string());
        let cfg = big_brother::BigBrotherConfig::load();
        let state = big_brother::BigBrotherState::load();
        let k_status = serbert::get_status();
        let k_running = serbert::is_driver_running();
        println!("Service Big Brother (Admin) : {status}");
        println!("Protection active activee   : {}", cfg.enabled);
        println!("Pilote Serbert (Noyau)      : {}", if k_running { "ACTIF (Ring 0)" } else { "NON_CHARGE" });
        if let Some(ks) = k_status {
            println!("  Statistiques noyau        : v{}, {} regles, {} executions bloquees",
                ks.version, ks.active_rules_count, ks.blocked_executions_count);
        }
        println!("Domaines bloques (hosts)    : {}", cfg.domains.len());
        println!("Processus bloques           : {}", cfg.blocked_processes.len());
        println!("Blocage installateurs MSI   : {}", cfg.block_msi);
        println!("Dossiers interdits (SRP)    : {:?}", cfg.deny_paths);
        println!("Temps session aujourd'hui   : {} min", state.active_secs / 60);
        return Ok(());
    }

    if args.iter().any(|a| a == "--serbert-status") {
        let running = serbert::is_driver_running();
        let svc = serbert::driver_service_status().unwrap_or_else(|| "NON_INSTALLE".to_string());
        println!("Pilote noyau Serbert service: {svc}");
        println!("Pilote noyau Serbert ioctl: {}", if running { "CONNECTE (Ring 0)" } else { "DECONNECTE" });
        if let Some(ks) = serbert::get_status() {
            println!("  Statut noyau : {} regles chargees, {} processus bloques",
                ks.active_rules_count, ks.blocked_executions_count);
        }
        return Ok(());
    }

    if args.iter().any(|a| a == "--serbert-install") {
        if !win::is_admin() {
            eprintln!("Droits administrateur requis pour installer le pilote Serbert.");
            std::process::exit(1);
        }
        let p = serbert::default_driver_path();
        match serbert::install_driver_service(&p) {
            Ok(()) => println!("Pilote noyau Serbert installe ({})", p.display()),
            Err(e) => eprintln!("Erreur installation Serbert : {e}"),
        }
        let _ = serbert::start_driver();
        return Ok(());
    }

    if args.iter().any(|a| a == "--serbert-uninstall") {
        if !win::is_admin() {
            eprintln!("Droits administrateur requis pour desinstaller le pilote Serbert.");
            std::process::exit(1);
        }
        match serbert::uninstall_driver_service() {
            Ok(()) => println!("Pilote noyau Serbert desinstalle."),
            Err(e) => eprintln!("Erreur desinstallation Serbert : {e}"),
        }
        return Ok(());
    }

    if args.iter().any(|a| a == "--bigbrother-apply") {
        if !win::is_admin() {
            eprintln!("DFWLogger Big Brother : privilèges administrateur requis pour appliquer la politique.");
            std::process::exit(1);
        }
        big_brother::tick();
        println!("Politique Big Brother appliquée avec succès.");
        return Ok(());
    }

    // Installation / désinstallation de Big Brother (session admin requise).
    if args.iter().any(|a| a == "--install-bigbrother") {
        if !win::is_admin() {
            if let Err(e) = win::relaunch_elevated_args(&["--install-bigbrother"]) {
                big_brother::log(&e);
                eprintln!("Erreur élévation : {e}");
            }
            return Ok(());
        }
        match big_brother::install_service() {
            Ok(()) => {
                if quiet {
                    println!("DFWLoggerBigBrother installé et démarré.");
                } else {
                    win::message_box(
                        "DFWLogger Big Brother",
                        "Le service DFWLoggerBigBrother est installé et démarré.\n\
La politique dans C:\\ProgramData\\DFWLogger\\big_brother.json\n\
est appliquée automatiquement toutes les 15 secondes.",
                    );
                }
            }
            Err(e) => {
                if quiet {
                    eprintln!("Installation impossible : {e}");
                    std::process::exit(1);
                } else {
                    win::message_box(
                        "DFWLogger Big Brother — erreur",
                        &format!("Installation impossible : {e}"),
                    );
                }
            }
        }
        return Ok(());
    }
    if args.iter().any(|a| a == "--uninstall-bigbrother") {
        if !win::is_admin() {
            if let Err(e) = win::relaunch_elevated_args(&["--uninstall-bigbrother"]) {
                big_brother::log(&e);
                eprintln!("Erreur élévation : {e}");
            }
            return Ok(());
        }
        match big_brother::uninstall_service() {
            Ok(()) => {
                if quiet {
                    println!("DFWLoggerBigBrother désinstallé et protections retirées.");
                } else {
                    win::message_box(
                        "DFWLogger Big Brother",
                        "Le service est désinstallé :\n- filtrage web / DNS retirés,\n- pilote noyau Serbert déchargé,\n- politiques MSI/SRP retirées,\n- section hosts retirée.",
                    );
                }
            }
            Err(e) => {
                if quiet {
                    eprintln!("Désinstallation incomplète : {e}");
                    std::process::exit(1);
                } else {
                    win::message_box(
                        "DFWLogger Big Brother — erreur",
                        &format!("Désinstallation incomplète : {e}"),
                    );
                }
            }
        }
        return Ok(());
    }

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
    net::spawn_dns_logger(shared.clone());
    // Toasts : enregistrement du nom d'app (HKCU) + vérification de mise à jour.
    notify::ensure_aumid();
    update::start_checker(shared.clone());

    let (rgba, icon_w, icon_h) = crate::tray::ico_rgba();
    let icon = std::sync::Arc::new(eframe::egui::IconData {
        rgba,
        width: icon_w,
        height: icon_h,
    });
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("DFWLogger — Superviseur familial")
            .with_inner_size([820.0, 680.0])
            .with_icon(icon),
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
        "DFWLogger — Superviseur familial & Protection active (3 Modes de Privilège)

Utilisation générale :
  dfwlogger                         ouvre l'interface graphique (démarre sans autorisation)
  dfwlogger tail [N]                affiche les N derniers événements déchiffrés (30 par défaut)
  dfwlogger export [f.csv]          exporte tout l'historique déchiffré en CSV (séparateur « ; »)
  dfwlogger --status                affiche le mode actif actuel et l'état des composants

Les 3 Modes de Privilège :
  dfwlogger --mode standard         rétrograde en Mode 1 (Sans autorisation, désinstalle Admin & Noyau)
  dfwlogger --mode admin            active le Mode 2 : Admin (installe et démarre Big Brother SYSTEM)
  dfwlogger --mode kernel           active le Mode 3 : Noyau (installe et démarre le pilote Ring 0 Serbert)

Commandes d'administration avancées :
  dfwlogger --install-bigbrother    installe et démarre le service Big Brother (SYSTEM)
  dfwlogger --uninstall-bigbrother  désinstalle le service Big Brother et retire toutes les restrictions
  dfwlogger --bigbrother-apply      applique immédiatement la politique big_brother.json (admin requis)
  dfwlogger --serbert-status        vérifie la communication IOCTL avec le pilote noyau Serbert
  dfwlogger --serbert-install       enregistre le pilote noyau Serbert (type= kernel)
  dfwlogger --serbert-uninstall     arrête et désenregistre le pilote noyau Serbert
  dfwlogger help                    cette aide

Options :
  --quiet, -q                       mode silencieux (pas de boîte de dialogue Win32, idéal scripts)

Architecture des 3 Modes :
  • Mode 1 (Standard) : aucun droit requis, non-intrusif, données chiffrées dans %LOCALAPPDATA%\\DFWLogger.
  • Mode 2 (Admin)    : service Big Brother SYSTEM (filtrage hosts/DNS, SRP, DisableMSI, quotas).
  • Mode 3 (Noyau)    : pilote Serbert Ring 0 (PsSetCreateProcessNotifyRoutineEx, blocage matériel sans thread)."
    );
}
