//! Interface de configuration et de consultation (egui/eframe).

use crate::model::EventKind;
use crate::shared::Shared;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Statut,
    Evenements,
    Reglages,
    Acces,
    Aide,
}

pub struct DfwApp {
    pub shared: Arc<Shared>,
    tab: Tab,
    password: String,
    confirm: String,
    remember: bool,
    err: Option<String>,
    info: Option<String>,
}

impl DfwApp {
    pub fn new(cc: &eframe::CreationContext<'_>, shared: Arc<Shared>) -> Self {
        // Icône tray : le thread vit tout le processus ; il demande les repaints
        // de l'UI via le Context egui.
        crate::tray::spawn_tray(shared.clone(), cc.egui_ctx.clone());
        let mut app = Self {
            shared,
            tab: Tab::Statut,
            password: String::new(),
            confirm: String::new(),
            remember: true,
            err: None,
            info: None,
        };
        // Reprise automatique via DPAPI si dispo.
        let mut store_guard = app.shared.store.lock().unwrap();
        let restored = store_guard.try_restore_from_dpapi();
        drop(store_guard);
        if matches!(restored, Ok(true)) {
            app.after_unlock(true);
            app.info = Some("Session restaurée automatiquement.".to_string());
        }
        app
    }

    // ---- flux déverrouillage / verrouillage ----

    fn after_unlock(&mut self, persist_dpapi: bool) {
        self.shared.import_from_store(2000);
        self.shared.monitoring.store(true, Ordering::Relaxed);
        self.shared.push(EventKind::Started);
        let token = new_token();
        *self.shared.dashboard_token.lock().unwrap() = Some(token);
        if !persist_dpapi {
            self.shared.store.lock().unwrap().forget_dpapi();
        }
        self.wipe_fields();
    }

    fn do_unlock(&mut self) {
        self.err = None;
        let password = self.password.clone();
        let confirm = self.confirm.clone();
        self.password = String::new();
        self.confirm = String::new();
        let mut store = self.shared.store.lock().unwrap();
        let res = if store.is_initialized() {
            store.unlock(&password)
        } else if password != confirm {
            Err("les deux mots de passe ne correspondent pas".to_string())
        } else {
            store.init(&password)
        };
        match res {
            Err(e) => self.err = Some(e),
            Ok(()) => {
                let dir = store.dir.clone();
                drop(store);
                let _ = self.shared.config.lock().unwrap().save(&dir);
                self.after_unlock(self.remember);
            }
        }
    }

    fn is_initialized_before(&self) -> bool {
        self.shared.store.lock().unwrap().is_initialized()
    }

    fn do_lock(&mut self) {
        self.shared.store.lock().unwrap().lock();
        self.shared.clear_memory();
        self.wipe_fields();
    }

    fn wipe_fields(&mut self) {
        self.password = String::new();
        self.confirm = String::new();
        self.err = None;
    }

    // ---- rendu ----

    fn draw_unlock_screen(&mut self, ui: &mut egui::Ui, fresh: bool) {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.heading("🔐 DFWLogger");
            ui.add_space(4.0);
            if fresh {
                ui.label("Première utilisation : définissez le mot de passe parent.");
            } else {
                ui.label("Entrez le mot de passe parent pour déverrouiller le journal.");
            }
            ui.add_space(18.0);
            let pass_edit = egui::TextEdit::singleline(&mut self.password)
                .password(true)
                .hint_text("Mot de passe parent")
                .desired_width(320.0);
            ui.add(pass_edit);
            if fresh {
                let confirm_edit = egui::TextEdit::singleline(&mut self.confirm)
                    .password(true)
                    .hint_text("Confirmation")
                    .desired_width(320.0);
                ui.add(confirm_edit);
            }
            ui.add_space(6.0);
            ui.checkbox(&mut self.remember, "Mémoriser (reprise auto après redémarrage, protégée par DPAPI)");
            ui.add_space(10.0);
            if ui.add(egui::Button::new("  Déverrouiller  ").min_size(egui::vec2(160.0, 32.0))).clicked() {
                self.do_unlock();
            }
            if let Some(err) = &self.err {
                ui.add_space(8.0);
                ui.colored_label(egui::Color32::from_rgb(0xE5, 0x5C, 0x5C), err);
            }
        });
    }

    fn draw_status(&mut self, ui: &mut egui::Ui) {
        let opened = self.shared.store.lock().unwrap().is_open();
        let st = self.shared.status.lock().unwrap().clone();
        ui.heading("🩺 État");
        ui.add_space(6.0);

        egui::Grid::new("status-grid").striped(true).num_columns(2).show(ui, |ui| {
            ui.label("Journal");
            ui.label(if opened { "🔓 déverrouillé — collecte active" } else { "🔒 verrouillé" });
            ui.end_row();

            ui.label("Surveillance session");
            ui.label(indic(st.session_ok, "active"));
            ui.end_row();

            ui.label("Surveillance applications");
            ui.label(indic(st.foreground_ok, "active"));
            ui.end_row();

            ui.label("Fichiers récents");
            ui.label(indic(st.recent_ok, "active"));
            ui.end_row();

            ui.label("Journal Security (échecs de connexion)");
            let note = if st.security_note.is_empty() {
                String::new()
            } else {
                format!(" — {}", st.security_note)
            };
            ui.label(if st.elevated && st.security_ok {
                format!("✅ actif{note}")
            } else if st.elevated {
                format!("⛔ {note}")
            } else {
                format!("ⓘ nécessite élévation{note}")
            });
            ui.end_row();

            ui.label("Dashboard réseau");
            ui.label(match (&st.dashboard_error, st.dashboard_listening) {
                (None, true) => format!("✅ en écoute (port {})", self.shared.config.lock().unwrap().dashboard_port),
                (Some(e), _) => format!("⛔ {e}"),
                (None, false) => "ⓘ éteint".to_string(),
            });
            ui.end_row();
        });

        ui.add_space(10.0);
        ui.heading("📊 Compteurs (depuis le déverrouillage)");
        ui.add_space(4.0);
        let counts = self.shared.counts.lock().unwrap();
        egui::Grid::new("counts-grid").striped(true).num_columns(2).show(ui, |ui| {
            for (k, v) in counts.iter() {
                ui.label(k.to_string());
                ui.label(v.to_string());
                ui.end_row();
            }
        });
        drop(counts);
        ui.add_space(14.0);
        ui.heading("⏱ Temps par application (depuis le déverrouillage)");
        ui.add_space(4.0);
        let tops = self.shared.top_apps(12);
        if tops.is_empty() {
            ui.label(egui::RichText::new("Aucun temps mesuré pour l'instant.").weak());
        } else {
            egui::Grid::new("times-grid")
                .striped(true)
                .num_columns(3)
                .show(ui, |ui| {
                    ui.weak("Application");
                    ui.weak("Temps");
                    ui.weak("Part");
                    ui.end_row();
                    let total: u64 = tops.iter().map(|(_, s)| *s).sum();
                    for (exe, secs) in &tops {
                        ui.label(exe);
                        ui.label(crate::model::format_hms(*secs));
                        let pct = if total > 0 { *secs as f32 / total as f32 * 100.0 } else { 0.0 };
                        ui.add(
                            egui::ProgressBar::new(pct / 100.0)
                                .text(format!("{pct:.0} %"))
                                .desired_width(120.0),
                        );
                        ui.end_row();
                    }
                });
            ui.add_space(4.0);
            if ui.button("Réinitialiser les compteurs de temps (mémoire seule)").clicked() {
                self.shared.app_times.lock().unwrap().clear();
                self.info = Some("Compteurs de temps remis à zéro (l'historique journalisé est conservé).".into());
            }
        }

        ui.add_space(10.0);
        if ui.button("🔒 Verrouiller le journal maintenant").clicked() {
            self.do_lock();
        }
    }

    fn draw_events(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("📜 Événements (mémoire)");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("📄 Exporter en CSV…").clicked() {
                    match export_csv(&self.shared) {
                        Ok(path) => {
                            self.info = Some(format!("CSV exporté : {}", path.display()));
                            self.err = None;
                        }
                        Err(e) => {
                            self.err = Some(e);
                            self.info = None;
                        }
                    }
                }
            });
        });
        ui.add_space(4.0);
        let monitoring = self.shared.monitoring.load(Ordering::Relaxed);
        if !monitoring {
            ui.label("🔒 Déverrouillez le journal pour consulter les événements.");
            return;
        }
        let events: Vec<crate::model::Event> = self
            .shared
            .events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .cloned()
            .collect();
        ui.label(format!("{} événement(s) affiché(s) — l'historique complet est chiffré sur disque.", events.len()));
        ui.add_space(6.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for e in events.iter() {
                let dt = chrono::DateTime::parse_from_rfc3339(&e.ts)
                    .map(|d| d.with_timezone(&chrono::Local))
                    .unwrap_or_else(|_| chrono::Local::now().into());
                egui::Grid::new(format!("ev{}", e.id))
                    .num_columns(3)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.monospace(dt.format("%Y-%m-%d %H:%M:%S").to_string());
                        ui.label(format!("{} {}", e.kind.icon(), e.kind.label()));
                        ui.label(e.kind.summary());
                        ui.end_row();
                    });
            }
        });
    }

    fn draw_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("⚙️ Réglages");
        ui.add_space(6.0);
        let mut cfg = self.shared.config.lock().unwrap().clone();
        let dir = store_dir(&self.shared);

        ui.group(|ui| {
            ui.label("Démarrage");
            ui.checkbox(&mut cfg.autostart, "Démarrer automatiquement avec la session (registre utilisateur)");
            if ui.button("Appliquer").clicked() {
                match crate::win::set_autostart(cfg.autostart) {
                    Ok(()) => {
                        if let Ok(()) = cfg.save(&dir) {
                            *self.shared.config.lock().unwrap() = cfg.clone();
                        }
                        self.info = Some("Réglages démarrage appliqués.".into());
                    }
                    Err(e) => self.err = Some(e),
                }
            }
        });

        ui.add_space(6.0);
        ui.group(|ui| {
            ui.label("Dashboard réseau (LAN)");
            ui.checkbox(&mut cfg.dashboard_enabled, "Activer le dashboard");
            ui.add(
                egui::DragValue::new(&mut cfg.dashboard_port)
                    .clamp_range(1024..=65535)
                    .prefix("Port "),
            );
            ui.add_space(4.0);
            if ui.button("Enregistrer").clicked() {
                match cfg.save(&dir) {
                    Ok(()) => {
                        *self.shared.config.lock().unwrap() = cfg.clone();
                        self.info = Some(
                            "Réglages enregistrés. Redémarrez l'application pour changer le port ou l'état du dashboard.".into(),
                        );
                    }
                    Err(e) => self.err = Some(e),
                }
            }
        });

        ui.add_space(6.0);
        ui.group(|ui| {
            ui.label("Collecte");
            ui.add(
                egui::DragValue::new(&mut cfg.recent_poll_secs)
                    .clamp_range(5..=3600)
                    .prefix("Fichiers récents : toutes les ")
                    .suffix(" s"),
            );
            ui.add(
                egui::DragValue::new(&mut cfg.security_poll_secs)
                    .clamp_range(30..=3600)
                    .prefix("Échecs de connexion : toutes les ")
                    .suffix(" s"),
            );
        });

        ui.add_space(6.0);
        ui.group(|ui| {
            ui.label("Stockage chiffré");
            ui.add(
                egui::DragValue::new(&mut cfg.rotate_bytes)
                    .clamp_range(65536..=256 * 1024 * 1024)
                    .prefix("Pivot à ")
                    .suffix(" octets"),
            );
            ui.add(
                egui::DragValue::new(&mut cfg.keep_files)
                    .clamp_range(1..=32)
                    .prefix("Historique conservé : ")
                    .suffix(" fichiers"),
            );
            ui.label(format!("Dossier : {}", dir.display()));
            ui.add_space(4.0);
            if ui.button("Enregistrer la politique").clicked() {
                match cfg.save(&dir) {
                    Ok(()) => {
                        *self.shared.config.lock().unwrap() = cfg.clone();
                        self.shared.store.lock().unwrap().set_policy(cfg.rotate_bytes, cfg.keep_files);
                        self.info = Some("Politique de stockage enregistrée.".into());
                    }
                    Err(e) => self.err = Some(e),
                }
            }
        });

        ui.add_space(6.0);
        ui.group(|ui| {
            ui.label("Journal Security — échecs de connexion (élévation optionnelle)");
            let st = self.shared.status.lock().unwrap();
            if st.elevated {
                ui.label("✅ L'application tourne avec élévation.");
            } else {
                ui.label("⌛ La lecture du journal Security (événement 4625) nécessite une session admin.");
                if ui.button("Relancer avec élévation…").clicked() {
                    drop(st);
                    let mut cfg2 = self.shared.config.lock().unwrap().clone();
                    cfg2.security_log = true;
                    let _ = cfg2.save(&dir);
                    if let Err(e) = crate::win::relaunch_elevated() {
                        self.err = Some(e);
                    } else {
                        std::process::exit(0);
                    }
                }
            }
        });

        show_message(self, ui);
    }

    fn draw_access(&mut self, ui: &mut eframe::egui::Ui) {
        ui.heading("📡 Accès réseau");
        ui.add_space(4.0);
        let token = self.shared.dashboard_token.lock().unwrap().clone();
        let port = self.shared.config.lock().unwrap().dashboard_port;
        let ip = crate::win::primary_lan_ip();
        if let Some(tok) = token {
            ui.label("Ouvrez depuis un autre appareil du même réseau :");
            ui.add_space(4.0);
            ui.monospace(egui::RichText::new(format!("https://{ip}:{port}")).text_style(egui::TextStyle::Heading));
            ui.add_space(8.0);
            ui.label("Token d'accès (renouvelé à chaque déverrouillage) :");
            ui.monospace(egui::RichText::new(&tok).text_style(egui::TextStyle::Monospace));
            if ui.button("Copier le lien complet").clicked() {
                let link = format!("https://{ip}:{port}/?auth={tok}");
                ui.ctx().copy_text(link);
                self.info = Some("Lien copié dans le presse-papiers.".into());
            }
            ui.add_space(10.0);
            ui.label(egui::RichText::new(
                "ℹ️ Le certificat est auto-signé : le navigateur affichera un avertissement, cliquez « Avancé » puis « Accepter et continuer ». \
Le port doit être autorisé dans le pare-feu Windows (Windows le demande au premier lancement).",
            ).small());
        } else {
            ui.label("🔒 Déverrouillez le journal pour générer le token d'accès.");
        }
    }

    fn draw_help(&mut self, ui: &mut egui::Ui) {
        ui.heading("❓ À propos");
        ui.add_space(4.0);
        ui.label(egui::RichText::new(
            "DFWLogger est un superviseur familial transparent. Il est visible dans la barre des tâches, \
dans le Gestionnaire des tâches et dans la liste des applications : c'est voulu et assumé. \
Il permet aux parents de suivre l'usage d'un ordinateur familial (temps de session, applications \
utilisées, temps passé par application, fichiers ouverts, tentatives de connexion échouées) avec \
le consentement des enfants."
        ).weak());
        ui.add_space(10.0);
        ui.label("Fenêtre et tray :");
        ui.label("• La croix ✕ masque la fenêtre dans la zone de notification (l'icône reste visible).");
        ui.label("• Clic sur l'icône tray : rouvrir la fenêtre. Menu tray : Ouvrir ou Quitter.");
        ui.add_space(6.0);
        ui.label("Consultation en terminal sur ce PC :");
        ui.monospace(egui::RichText::new("  dfwlogger tail 30   — affiche les 30 derniers événements").text_style(egui::TextStyle::Monospace));
        ui.label("dans un terminal : `cd` vers le dossier de l'exécutable puis `dfwlogger.exe tail 30`");
        ui.add_space(10.0);
        ui.label("Sécurité :");
        ui.label("• Journal chiffré AES-256-GCM, clé dérivée du mot de passe parent (Argon2id 19 456 KiB).");
        ui.label("• Dossier dans %LOCALAPPDATA%\\DFWLogger — visible, mais illisible sans le mot de passe.");
        ui.label("• Dashboard HTTPS avec certificat local + token à usage unique par déverrouillage.");
        ui.label("• Aucun enregistrement de frappes clavier, aucun mode furtif, aucune télémétrie externe.");
    }
}

fn show_message(app: &mut DfwApp, ui: &mut egui::Ui) {
    if let Some(info) = app.info.clone() {
        ui.colored_label(egui::Color32::from_rgb(0x6f, 0xd6, 0x9e), info);
    }
    if let Some(err) = app.err.clone() {
        ui.colored_label(egui::Color32::from_rgb(0xE5, 0x5C, 0x5C), err);
    }
}

fn indic(ok: bool, op: &str) -> String {
    if ok { format!("✅ {op}") } else { format!("⏳ en attente de confirmation") }
}

fn store_dir(shared: &Arc<Shared>) -> std::path::PathBuf {
    shared.store.lock().unwrap().dir.clone()
}

/// Échappe un champ CSV (séparateur `;`, guillemets doublés, retours à la ligne).
fn export_csv(shared: &Arc<Shared>) -> Result<PathBuf, String> {
    let events = {
        let store = shared.store.lock().unwrap();
        store.load_all()?
    };
    if events.is_empty() {
        return Err("aucun événement à exporter (journal vide)".into());
    }

    let default_name = format!(
        "dfwlogger-export-{}.csv",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let Some(path) = rfd::FileDialog::new()
        .set_title("Exporter l'historique déchiffré en CSV")
        .add_filter("CSV (séparateur point-virgule)", &["csv"])
        .set_file_name(&default_name)
        .save_file()
    else {
        return Err("export annulé".into());
    };

    let csv = crate::store::csv_from_events(&events);
    std::fs::write(&path, csv).map_err(|e| format!("écriture CSV : {e}"))?;
    Ok(path)
}
fn new_token() -> String {
    use rand::RngCore;
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

impl eframe::App for DfwApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Le clic sur ✕ masque la fenêtre dans le tray (le vrai Quitter est dans le menu tray).
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        // Demande de réaffichage (clic sur l'icône tray / menu Ouvrir).
        if self.shared.show_request.swap(false, Ordering::Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        // Refresh immédiat quand les collecteurs publient, sinon battement lent.
        if self.shared.dirty.swap(false, Ordering::Relaxed) {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(900));
        }

        egui::TopBottomPanel::top("topbar").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.strong("DFWLogger");
                ui.add_space(4.0);
                let monitoring = self.shared.monitoring.load(Ordering::Relaxed);
                let chip = if monitoring { "🟢 active" } else { "🔒 verrouillé" };
                ui.label(chip);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.selectable_value(&mut self.tab, Tab::Statut, "Statut");
                    ui.selectable_value(&mut self.tab, Tab::Evenements, "Événements");
                    ui.selectable_value(&mut self.tab, Tab::Reglages, "Réglages");
                    ui.selectable_value(&mut self.tab, Tab::Acces, "Accès réseau");
                    ui.selectable_value(&mut self.tab, Tab::Aide, "Aide");
                });
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let opened = self.shared.store.lock().unwrap().is_open();
            if !opened {
                let fresh = !self.is_initialized_before();
                self.draw_unlock_screen(ui, fresh);
                return;
            }
            match self.tab {
                Tab::Statut => self.draw_status(ui),
                Tab::Evenements => self.draw_events(ui),
                Tab::Reglages => self.draw_settings(ui),
                Tab::Acces => self.draw_access(ui),
                Tab::Aide => self.draw_help(ui),
            }
            ui.add_space(8.0);
            if self.info.is_some() || self.err.is_some() {
                show_message(self, ui);
            }
        });
    }
}
