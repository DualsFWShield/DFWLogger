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
    BigBrother,
    Acces,
    Aide,
}

pub struct DfwApp {
    pub shared: Arc<Shared>,
    tab: Tab,
    password: String,
    confirm: String,
    show_password: bool,
    remember: bool,
    err: Option<String>,
    info: Option<String>,
    /// Mot de passe « admin » en cours de saisie (déverrouillage des réglages).
    admin_password: String,
    show_admin_password: bool,
    /// Création du mot de passe admin : saisie + confirmation.
    new_admin: String,
    new_admin_confirm: String,
    show_new_admin: bool,
    /// Réglages déverrouillés pour cette session (quand un mot de passe admin est défini).
    admin_unlocked: bool,
    /// Configuration & état de Big Brother (Mode École) et pilote noyau Serbert.
    big_brother_cfg: crate::big_brother::BigBrotherConfig,
    big_brother_state: crate::big_brother::BigBrotherState,
    big_brother_status: Option<String>,
    serbert_status: Option<crate::serbert::SerbertStatusResponse>,
    serbert_driver_running: bool,
    big_brother_new_domain: String,
    big_brother_new_proc: String,
    big_brother_new_deny: String,
    big_brother_custom_dns: String,
    big_brother_quota_mins: u64,
    big_brother_quota_enabled: bool,
    big_brother_time_window_enabled: bool,
    big_brother_window_from: String,
    big_brother_window_to: String,
    big_brother_window_weekdays_only: bool,
    last_status_check: std::time::Instant,
}

impl DfwApp {
    pub fn current_tier(&self) -> crate::big_brother::PrivilegeTier {
        if self.serbert_driver_running {
            crate::big_brother::PrivilegeTier::Kernel
        } else if self.big_brother_status.as_deref() == Some("RUNNING") {
            crate::big_brother::PrivilegeTier::Admin
        } else {
            crate::big_brother::PrivilegeTier::Standard
        }
    }

    pub fn new(cc: &eframe::CreationContext<'_>, shared: Arc<Shared>) -> Self {
        // Chargement des polices natives Windows pour éliminer tout caractère manquant (tofu □)
        let mut fonts = egui::FontDefinitions::default();
        if let Ok(data) = std::fs::read("C:\\Windows\\Fonts\\segoeui.ttf") {
            fonts.font_data.insert("segoe_ui".to_owned(), egui::FontData::from_owned(data));
            if let Some(prop) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                prop.insert(0, "segoe_ui".to_owned());
            }
        }
        if let Ok(data) = std::fs::read("C:\\Windows\\Fonts\\seguiemj.ttf") {
            fonts.font_data.insert("segoe_emoji".to_owned(), egui::FontData::from_owned(data));
            if let Some(prop) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                prop.push("segoe_emoji".to_owned());
            }
        }
        cc.egui_ctx.set_fonts(fonts);

        // Icône tray : le thread vit tout le processus ; il demande les repaints
        // de l'UI via le Context egui.
        crate::tray::spawn_tray(shared.clone(), cc.egui_ctx.clone());
        let big_brother_cfg = crate::big_brother::BigBrotherConfig::load();
        let big_brother_state = crate::big_brother::BigBrotherState::load();
        let big_brother_status = crate::big_brother::service_status();
        let serbert_status = crate::serbert::get_status();
        let serbert_driver_running = crate::serbert::is_driver_running();
        let big_brother_quota_enabled = big_brother_cfg.daily_minutes.is_some();
        let big_brother_quota_mins = big_brother_cfg.daily_minutes.unwrap_or(120);
        let big_brother_custom_dns = big_brother_cfg.dns_filter.clone().unwrap_or_default();
        let big_brother_time_window_enabled = !big_brother_cfg.allowed_windows.is_empty();
        let (big_brother_window_from, big_brother_window_to) = if let Some(w) = big_brother_cfg.allowed_windows.first() {
            (w.from.clone(), w.to.clone())
        } else {
            ("08:00".to_string(), "18:00".to_string())
        };
        let big_brother_window_weekdays_only = big_brother_cfg
            .allowed_windows
            .first()
            .map(|w| w.days == vec![1, 2, 3, 4, 5])
            .unwrap_or(true);

        let mut app = Self {
            shared,
            tab: Tab::Statut,
            password: String::new(),
            confirm: String::new(),
            show_password: false,
            remember: true,
            err: None,
            info: None,
            admin_password: String::new(),
            show_admin_password: false,
            new_admin: String::new(),
            new_admin_confirm: String::new(),
            show_new_admin: false,
            admin_unlocked: false,
            big_brother_cfg,
            big_brother_state,
            big_brother_status,
            serbert_status,
            serbert_driver_running,
            big_brother_new_domain: String::new(),
            big_brother_new_proc: String::new(),
            big_brother_new_deny: String::new(),
            big_brother_custom_dns,
            big_brother_quota_mins,
            big_brother_quota_enabled,
            big_brother_time_window_enabled,
            big_brother_window_from,
            big_brother_window_to,
            big_brother_window_weekdays_only,
            last_status_check: std::time::Instant::now(),
        };
        // Reprise automatique via DPAPI si dispo.
        let mut store_guard = app.shared.store.lock().unwrap();
        let restored = store_guard.try_restore_from_dpapi();
        drop(store_guard);
        if matches!(restored, Ok(true)) {
            app.after_unlock(true);
            app.info = Some("Session restaurée automatiquement.".to_string());
        }
        // Personnalisation du thème sombre : moderne, contrasté et réactif
        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(egui::Color32::from_rgb(0xF1, 0xF5, 0xF9));
        visuals.window_fill = egui::Color32::from_rgb(0x0F, 0x11, 0x17);
        visuals.panel_fill = egui::Color32::from_rgb(0x13, 0x16, 0x20);
        visuals.faint_bg_color = egui::Color32::from_rgb(0x1A, 0x1E, 0x2A);
        visuals.extreme_bg_color = egui::Color32::from_rgb(0x0B, 0x0D, 0x12);

        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(0x16, 0x1A, 0x25);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0x24, 0x2B, 0x3D));
        visuals.widgets.noninteractive.rounding = egui::Rounding::same(8.0);

        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(0x1C, 0x22, 0x32);
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0x2D, 0x37, 0x4E));
        visuals.widgets.inactive.rounding = egui::Rounding::same(6.0);

        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(0x27, 0x33, 0x4D);
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0x3B, 0x82, 0xF6));
        visuals.widgets.hovered.rounding = egui::Rounding::same(6.0);

        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(0x25, 0x63, 0xEB);
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0x60, 0xA5, 0xFA));
        visuals.widgets.active.rounding = egui::Rounding::same(6.0);

        visuals.selection.bg_fill = egui::Color32::from_rgb(0x1D, 0x4E, 0xD8);
        visuals.selection.stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0x93, 0xC5, 0xFD));

        cc.egui_ctx.set_visuals(visuals);

        let mut style = (*cc.egui_ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 7.0);
        style.spacing.window_margin = egui::Margin::same(14.0);
        cc.egui_ctx.set_style(style);

        app
    }

    // ---- flux déverrouillage / verrouillage ----

    fn after_unlock(&mut self, persist_dpapi: bool) {
        self.shared.import_from_store(2000);
        self.shared.monitoring.store(true, Ordering::Relaxed);
        self.shared.push(EventKind::Started);
        self.shared.issue_dashboard_access();
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
        self.show_password = false;
        self.err = None;
    }

    // ---- rendu ----

    fn draw_unlock_screen(&mut self, ui: &mut egui::Ui, fresh: bool) {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.heading("⛨ DFWLogger");
            ui.add_space(4.0);
            if fresh {
                ui.label("Première utilisation : définissez le mot de passe parent.");
            } else {
                ui.label("Entrez le mot de passe parent pour déverrouiller le journal.");
            }
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 340.0).max(0.0) / 2.0);
                let pass_edit = egui::TextEdit::singleline(&mut self.password)
                    .password(!self.show_password)
                    .hint_text("Mot de passe parent")
                    .desired_width(260.0);
                ui.add(pass_edit);
                let eye = if self.show_password { "Masquer" } else { "Afficher" };
                if ui.button(eye).on_hover_text(if self.show_password { "Masquer le mot de passe" } else { "Afficher le mot de passe" }).clicked() {
                    self.show_password = !self.show_password;
                }
            });
            if fresh {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - 340.0).max(0.0) / 2.0);
                    let confirm_edit = egui::TextEdit::singleline(&mut self.confirm)
                        .password(!self.show_password)
                        .hint_text("Confirmation")
                        .desired_width(260.0);
                    ui.add(confirm_edit);
                });
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
        ui.heading("● État");
        ui.add_space(6.0);

        egui::Grid::new("status-grid").striped(true).num_columns(2).show(ui, |ui| {
            ui.label("Journal");
            ui.label(if opened { "● déverrouillé — collecte active" } else { "■ verrouillé" });
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
                format!("✓ actif{note}")
            } else if st.elevated {
                format!("✕ {note}")
            } else {
                format!("i nécessite élévation{note}")
            });
            ui.end_row();

            ui.label("Dashboard réseau");
            ui.label(match (&st.dashboard_error, st.dashboard_listening) {
                (None, true) => format!("✓ en écoute (port {})", self.shared.config.lock().unwrap().dashboard_port),
                (Some(e), _) => format!("✕ {e}"),
                (None, false) => "… inactif".to_string(),
            });
            ui.end_row();
        });

        ui.add_space(10.0);
        ui.heading("≡ Compteurs (depuis le déverrouillage)");
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
        ui.heading("◷ Temps par application (depuis le déverrouillage)");
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

        ui.add_space(14.0);
        ui.heading("▦ Usage par jour et par heure (7 derniers jours)");
        ui.add_space(4.0);
        draw_heatmap(ui, &self.shared);

        ui.add_space(10.0);
        draw_update_section(ui, &self.shared, &mut self.info, &mut self.err);

        ui.add_space(10.0);
        if ui.button("■ Verrouiller le journal maintenant").clicked() {
            self.do_lock();
        }
    }

    fn draw_events(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("≡ Événements (mémoire)");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("▤ Exporter en CSV…").clicked() {
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
            ui.label("■ Déverrouillez le journal pour consulter les événements.");
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
        ui.heading("⚙ Réglages");
        ui.add_space(6.0);
        let mut cfg = self.shared.config.lock().unwrap().clone();
        let dir = store_dir(&self.shared);

        // ---- Verrou « admin » : deuxième mot de passe distinct du mot de passe journal ----
        let admin_required = cfg.admin_phc.is_some();
        if admin_required && !self.admin_unlocked {
            ui.group(|ui| {
                ui.label("■ Réglages protégés par le mot de passe admin");
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.admin_password)
                            .password(!self.show_admin_password)
                            .hint_text("Mot de passe admin")
                            .desired_width(220.0),
                    );
                    let eye = if self.show_admin_password { "Masquer" } else { "Afficher" };
                    if ui.button(eye).on_hover_text(if self.show_admin_password { "Masquer" } else { "Afficher" }).clicked() {
                        self.show_admin_password = !self.show_admin_password;
                    }
                    if ui.button("Déverrouiller les réglages").clicked() {
                        let pass = std::mem::take(&mut self.admin_password);
                        self.show_admin_password = false;
                        if verify_admin_phc(cfg.admin_phc.as_deref().unwrap_or(""), &pass) {
                            self.admin_unlocked = true;
                            self.info = Some("Réglages déverrouillés pour cette session.".into());
                            self.err = None;
                        } else {
                            self.err = Some("Mot de passe admin incorrect.".into());
                            self.info = None;
                        }
                    }
                });
            });
            show_message(self, ui);
            return;
        }

        ui.group(|ui| {
            ui.label(if admin_required {
                "● Protection admin active (déverrouillée pour cette session)"
            } else {
                "○ Aucun mot de passe admin — quiconque ouvre cette fenêtre peut modifier les réglages"
            });
            ui.label(
                egui::RichText::new(
                    "Astuce : définissez un mot de passe admin pour empêcher un enfant de désactiver l'autostart, le dashboard ou la collecte. Il est distinct du mot de passe du journal.",
                ).small(),
            );
        });

        ui.add_space(6.0);
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
            ui.label("Notifications Windows");
            ui.checkbox(&mut cfg.notify_lock, "Prévenir quand la session est verrouillée");
            ui.checkbox(
                &mut cfg.notify_failed_logon,
                "Prévenir à chaque échec de connexion (4625)",
            );
            ui.add_space(4.0);
            if ui.button("Enregistrer").clicked() {
                match cfg.save(&dir) {
                    Ok(()) => {
                        *self.shared.config.lock().unwrap() = cfg.clone();
                        self.info = Some("Notifications enregistrées.".into());
                    }
                    Err(e) => self.err = Some(e),
                }
            }
        });

        ui.add_space(6.0);
        ui.group(|ui| {
            ui.label("↻ Vérification de mise à jour");
            let mut repo = cfg.github_repo.clone().unwrap_or_default();
            ui.add(
                egui::TextEdit::singleline(&mut repo)
                    .hint_text("compte/repo (ex. famille/DFWLogger)")
                    .desired_width(280.0),
            );
            ui.add_space(4.0);
            if ui.button("Enregistrer").clicked() {
                cfg.github_repo = {
                    let r = repo.trim().to_string();
                    (!r.is_empty()).then_some(r)
                };
                match cfg.save(&dir) {
                    Ok(()) => {
                        *self.shared.config.lock().unwrap() = cfg.clone();
                        self.info = Some("Dépôt de mise à jour enregistré.".into());
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
                ui.label("✓ L'application tourne avec élévation.");
            } else {
                ui.label("… La lecture du journal Security (événement 4625) nécessite une session admin.");
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

        ui.add_space(6.0);
        // ---- Administration du mot de passe admin ----
        ui.group(|ui| {
            if admin_required {
                ui.label("⛨ Mot de passe admin");
                if ui.button("Désactiver la protection").clicked() {
                    cfg.admin_phc = None;
                    self.admin_unlocked = true;
                    match cfg.save(&dir) {
                        Ok(()) => {
                            *self.shared.config.lock().unwrap() = cfg.clone();
                            self.info =
                                Some("Protection admin désactivée (Réglages à nouveau libres).".into());
                        }
                        Err(e) => self.err = Some(e),
                    }
                }
                if ui.button("Reverrouiller maintenant").clicked() {
                    self.admin_unlocked = false;
                }
            } else {
                ui.label("⛨ Créer un mot de passe admin (protéger les réglages)");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_admin)
                            .password(!self.show_new_admin)
                            .hint_text("Mot de passe admin")
                            .desired_width(220.0),
                    );
                    let eye = if self.show_new_admin { "Masquer" } else { "Afficher" };
                    if ui.button(eye).on_hover_text(if self.show_new_admin { "Masquer" } else { "Afficher" }).clicked() {
                        self.show_new_admin = !self.show_new_admin;
                    }
                });
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_admin_confirm)
                            .password(!self.show_new_admin)
                            .hint_text("Confirmation")
                            .desired_width(220.0),
                    );
                });
                if ui.button("Activer la protection").clicked() {
                    let pass = std::mem::take(&mut self.new_admin);
                    let confirm = std::mem::take(&mut self.new_admin_confirm);
                    if pass.len() < 8 {
                        self.err = Some("mot de passe admin : 8 caractères minimum".into());
                    } else if pass != confirm {
                        self.err = Some("les deux mots de passe ne correspondent pas".into());
                    } else if let Some(phc) = hash_admin_phc(&pass) {
                        cfg.admin_phc = Some(phc);
                        self.admin_unlocked = true;
                        match cfg.save(&dir) {
                            Ok(()) => {
                                *self.shared.config.lock().unwrap() = cfg.clone();
                                self.info = Some(
                                    "Protection admin activée : les réglages demandent désormais \
le mot de passe admin."
                                        .into(),
                                );
                                self.err = None;
                            }
                            Err(e) => self.err = Some(e),
                        }
                    } else {
                        self.err = Some("dérivation Argon2 impossible".into());
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
        let code = self.shared.dashboard_code.lock().unwrap().clone();
        let port = self.shared.config.lock().unwrap().dashboard_port;
        let ip = crate::win::primary_lan_ip();
        if let Some(tok) = token.as_ref() {
            // ---- QR code (généré en direct depuis la matrice du QR) ----
            ui.add_space(6.0);
            ui.label("QR code à scanner :");
            let qr_size = 160.0;
            draw_qr(ui, &format!("https://{ip}:{port}/?auth={tok}"), qr_size);
            ui.add_space(10.0);

            // ---- Token d'accès (renouvelé à chaque déverrouillage) ----
            ui.label("Token d'accès :");
            ui.monospace(egui::RichText::new(tok).text_style(egui::TextStyle::Monospace));
            if ui.button("Copier le lien complet").clicked() {
                let link = format!("https://{ip}:{port}/?auth={tok}");
                ui.ctx().copy_text(link);
                self.info = Some("Lien copié dans le presse-papiers.".into());
            }

            // ---- Code à 6 caractères maj/min (second moyen d'auth) ----
            if let Some(code_val) = code.as_ref() {
                ui.add_space(6.0);
                ui.label("Code à 6 caractères (majuscules/minuscules) :");
                ui.monospace(egui::RichText::new(code_val).text_style(egui::TextStyle::Monospace));
                if ui.button("Copier le code").clicked() {
                    ui.ctx().copy_text(code_val.clone());
                    self.info = Some("Code copié dans le presse-papiers.".into());
                }
            }
            ui.label(egui::RichText::new(
                "Le certificat est auto-signé : le navigateur affichera un avertissement, cliquez « Avancé » puis « Accepter et continuer ». \
Le port doit être autorisé dans le pare-feu Windows (Windows le demande au premier lancement).",
            ).small());
        } else {
            ui.label("■ Déverrouillez le journal pour générer le token d'accès.");
        }

        // ---- Outil de diagnostic réseau : journal des requêtes DNS (Wireshark simplifié) ----
        ui.add_space(14.0);
        ui.heading("☵ Journal DNS (diagnostic)");
        ui.add_space(2.0);
        let (listening, error) = {
            let st = self.shared.status.lock().unwrap();
            (st.dns_listening, st.dns_error.clone())
        };
        let label: String = if listening {
            "✓ Requêtes DNS capturées en temps réel (écoute sur 127.0.0.1:53).".to_string()
        } else if let Some(e) = error.as_ref() {
            format!("✕ Journal DNS indisponible : {e}")
        } else {
            "… Journal DNS inactif.".to_string()
        };
        ui.label(&label);
        let mut dns_on = self.shared.config.lock().unwrap().dns_log;
        if ui.checkbox(&mut dns_on, "Journaliser les requêtes DNS résolues").changed() {
            crate::net::set_dns_logging(&self.shared, dns_on);
        }
        ui.add_space(4.0);
        let queries: Vec<crate::model::DnsQuery> = self.shared.dns_queries.lock().unwrap().clone();
        if queries.is_empty() {
            ui.label(egui::RichText::new("Aucune requête DNS pour le moment. Activez le journal ci-dessus et configurez le DNS sur 127.0.0.1.").weak());
        } else {
            egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                for q in queries.iter().rev().take(30) {
                    ui.monospace(format!("{:<22} {:<8} {}", q.ts, q.qtype, q.name));
                }
            });
        }
    }

    fn draw_help(&mut self, ui: &mut egui::Ui) {
        ui.heading("? À propos");
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

    fn draw_big_brother(&mut self, ui: &mut egui::Ui) {
        let tier = self.current_tier();

        ui.horizontal(|ui| {
            ui.heading("⛊ Les 3 Modes d'Accès & de Protection");
            ui.add_space(8.0);
            ui.colored_label(
                match tier {
                    crate::big_brother::PrivilegeTier::Standard => egui::Color32::from_rgb(0x6f, 0xd6, 0x9e),
                    crate::big_brother::PrivilegeTier::Admin => egui::Color32::from_rgb(0x88, 0xAA, 0xFF),
                    crate::big_brother::PrivilegeTier::Kernel => egui::Color32::from_rgb(0xFF, 0xCC, 0x00),
                },
                format!("Actif : {}", tier.badge()),
            );
        });
        ui.add_space(4.0);
            ui.label(egui::RichText::new(tier.description()).weak());
            ui.add_space(8.0);

            // Présentation dynamique et aérée des 3 Modes sous forme de cartes modernes
            let avail = ui.available_width();
            let card_w = ((avail - 32.0) / 3.0).max(250.0);

            egui::Grid::new("modes-grid").num_columns(3).spacing([14.0, 10.0]).show(ui, |ui| {
                // CARTE 1 : Mode Standard
                let f1 = egui::Frame::none()
                    .fill(egui::Color32::from_rgb(0x15, 0x1A, 0x24))
                    .stroke(egui::Stroke::new(1.5_f32, if tier == crate::big_brother::PrivilegeTier::Standard { egui::Color32::from_rgb(0x10, 0xB9, 0x81) } else { egui::Color32::from_rgb(0x27, 0x2F, 0x42) }))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::same(12.0));
                f1.show(ui, |ui| {
                    ui.set_width(card_w);
                    ui.strong("○ 1. Sans autorisation");
                    ui.label(egui::RichText::new("(Mode Standard)").small().weak());
                    ui.add_space(4.0);
                    if tier == crate::big_brother::PrivilegeTier::Standard {
                        ui.colored_label(egui::Color32::from_rgb(0x10, 0xB9, 0x81), "● ACTIF PAR DÉFAUT");
                    } else {
                        ui.label(egui::RichText::new("○ Inactif").weak());
                    }
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("• Démarrage sans élévation\n• Surveillance locale transparente\n• Journal chiffré AES-256-GCM\n• Aucune altération système").small());
                    ui.add_space(8.0);
                    if tier == crate::big_brother::PrivilegeTier::Standard {
                        let btn = egui::Button::new("↑ Passer en Accès Admin")
                            .fill(egui::Color32::from_rgb(0x1D, 0x4E, 0xD8))
                            .rounding(egui::Rounding::same(6.0));
                        if ui.add(btn).clicked() {
                            self.promote_to_admin();
                        }
                    } else {
                        if ui.button("↩ Revenir en Standard").clicked() {
                            self.demote_to_standard();
                        }
                    }
                });

                // CARTE 2 : Mode Admin
                let f2 = egui::Frame::none()
                    .fill(egui::Color32::from_rgb(0x15, 0x1A, 0x24))
                    .stroke(egui::Stroke::new(1.5_f32, if tier == crate::big_brother::PrivilegeTier::Admin { egui::Color32::from_rgb(0x3B, 0x82, 0xF6) } else { egui::Color32::from_rgb(0x27, 0x2F, 0x42) }))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::same(12.0));
                f2.show(ui, |ui| {
                    ui.set_width(card_w);
                    ui.strong("● 2. Accès Admin");
                    ui.label(egui::RichText::new("(Big Brother SYSTEM)").small().weak());
                    ui.add_space(4.0);
                    match self.big_brother_status.as_deref() {
                        Some("RUNNING") => {
                            if tier == crate::big_brother::PrivilegeTier::Admin {
                                ui.colored_label(egui::Color32::from_rgb(0x3B, 0x82, 0xF6), "● ACTIF (SYSTEM)");
                            } else {
                                ui.colored_label(egui::Color32::from_rgb(0x10, 0xB9, 0x81), "● SERVICE PRÊT");
                            }
                        }
                        Some("STOPPED") => {
                            ui.colored_label(egui::Color32::from_rgb(0xE5, 0xA0, 0x5C), "○ ARRÊTÉ");
                        }
                        _ => {
                            ui.label(egui::RichText::new("○ Non installé").weak());
                        }
                    }
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("• Service SYSTEM indomptable\n• Filtrage web DNS & hosts\n• Blocage installateurs MSI\n• Blocage dossiers élèves (SRP)\n• Quotas d'écran & watchdog").small());
                    ui.add_space(8.0);
                    match tier {
                        crate::big_brother::PrivilegeTier::Standard => {
                            let btn = egui::Button::new("↑ Activer le Mode Admin")
                                .fill(egui::Color32::from_rgb(0x1D, 0x4E, 0xD8))
                                .rounding(egui::Rounding::same(6.0));
                            if ui.add(btn).clicked() {
                                self.promote_to_admin();
                            }
                        }
                        crate::big_brother::PrivilegeTier::Admin => {
                            ui.horizontal(|ui| {
                                let btn_k = egui::Button::new("✦ Passer en Noyau")
                                    .fill(egui::Color32::from_rgb(0xD9, 0x77, 0x06))
                                    .rounding(egui::Rounding::same(6.0));
                                if ui.add(btn_k).clicked() {
                                    self.promote_to_kernel();
                                }
                                if ui.button("■ Désactiver").clicked() {
                                    self.demote_to_standard();
                                }
                            });
                        }
                        crate::big_brother::PrivilegeTier::Kernel => {
                            if ui.button("↩ Rétrograder en Admin").clicked() {
                                self.demote_to_admin();
                            }
                        }
                    }
                });

                // CARTE 3 : Mode Noyau
                let f3 = egui::Frame::none()
                    .fill(egui::Color32::from_rgb(0x15, 0x1A, 0x24))
                    .stroke(egui::Stroke::new(1.5_f32, if tier == crate::big_brother::PrivilegeTier::Kernel { egui::Color32::from_rgb(0xF5, 0x9E, 0x0B) } else { egui::Color32::from_rgb(0x27, 0x2F, 0x42) }))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::same(12.0));
                f3.show(ui, |ui| {
                    ui.set_width(card_w);
                    ui.strong("✦ 3. Accès Noyau");
                    ui.label(egui::RichText::new("(Serbert Ring 0 Driver)").small().weak());
                    ui.add_space(4.0);
                    if self.serbert_driver_running {
                        ui.colored_label(egui::Color32::from_rgb(0xF5, 0x9E, 0x0B), "● ACTIF (Ring 0)");
                    } else {
                        ui.colored_label(egui::Color32::from_rgb(0xE5, 0xA0, 0x5C), "○ Non chargé");
                    }
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("• Pilote \\Device\\Serbert\n• PsSetCreateProcessNotifyRoutineEx\n• Blocage STATUS_ACCESS_DENIED\n• 0 cycle CPU en userland\n• Protection anti-kill matérielle").small());
                    ui.add_space(8.0);
                    if self.serbert_driver_running {
                        if ui.button("↩ Rétrograder en Admin").clicked() {
                            self.demote_to_admin();
                        }
                    } else {
                        let btn = egui::Button::new("✦ Activer l'Accès Noyau")
                            .fill(egui::Color32::from_rgb(0xD9, 0x77, 0x06))
                            .rounding(egui::Rounding::same(6.0));
                        if ui.add(btn).clicked() {
                            self.promote_to_kernel();
                        }
                    }
                });
                ui.end_row();
            });

            ui.add_space(10.0);

            // Note d'intégrité & modèle de sécurité
            ui.group(|ui| {
                ui.label(egui::RichText::new("Modèle de sécurité & Règles du parc :").strong());
                ui.label(egui::RichText::new(
                    "• Condition d'inviolabilité : Les comptes élèves ne doivent pas être administrateurs locaux.\n\
• Dossier ProgramData\\DFWLogger verrouillé par ACL NTFS (SYSTEM et Administrateurs en contrôle total, élèves en lecture seule).\n\
• Mode Noyau : Requiert testsigning actif (bcdedit /set testsigning on) ou un certificat pour charger serbert.sys sur Windows 64-bit."
                ).small());
            });

            ui.add_space(8.0);

            if let Some(ks) = self.serbert_status {
                ui.group(|ui| {
                    ui.strong("≡ Statistiques en direct du Pilote Noyau Serbert :");
                    ui.label(format!("• Version du pilote : {}", ks.version));
                    ui.label(format!("• Règles d'interdiction actives au Ring 0 : {}", ks.active_rules_count));
                    ui.label(format!("• Exécutions physiquement bloquées au niveau noyau : {}", ks.blocked_executions_count));
                });
                ui.add_space(8.0);
            }

            // Commandes du service et du pilote
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.strong("Actions de contrôle rapide :");
                    if ui.button("↻ Forcer l'application (tick)").clicked() {
                        crate::big_brother::tick();
                        self.info = Some("Politique Big Brother appliquée immédiatement.".into());
                    }
                    if ui.button("≡ Recharger l'état").clicked() {
                        self.refresh_modes();
                    }
                });
            });

            ui.add_space(8.0);

            // 3. Interrupteurs généraux
            ui.group(|ui| {
                ui.checkbox(&mut self.big_brother_cfg.enabled, "✦ Activer les restrictions actives (Big Brother & Serbert)");
                ui.checkbox(&mut self.big_brother_cfg.enable_kernel_driver, "⛨ Coopération active avec le pilote noyau Serbert");
                if !self.big_brother_cfg.enabled {
                    ui.label(egui::RichText::new("! Les protections sont désactivées dans big_brother.json (mode inactif / fail-open).").weak());
                }
            });

            ui.add_space(8.0);

            // 4. Filtrage Web (hosts & DNS)
            ui.collapsing("✦ Filtrage Web (Domaines bloqués & DNS filtré)", |ui| {
                ui.label(egui::RichText::new("Redirection 0.0.0.0 via le fichier hosts Windows et/ou DNS imposé au niveau des interfaces réseau.").small());
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label("Packs rapides :");
                    if ui.button("+ Réseaux & Streaming").clicked() {
                        let pack = [
                            "youtube.com", "www.youtube.com", "youtu.be",
                            "tiktok.com", "www.tiktok.com",
                            "instagram.com", "www.instagram.com",
                            "twitch.tv", "www.twitch.tv",
                            "discord.com", "discord.gg",
                            "netflix.com", "www.netflix.com",
                            "x.com", "twitter.com",
                            "reddit.com", "www.reddit.com",
                        ];
                        for d in pack {
                            if !self.big_brother_cfg.domains.iter().any(|x| x.eq_ignore_ascii_case(d)) {
                                self.big_brother_cfg.domains.push(d.to_string());
                            }
                        }
                    }
                    if ui.button("+ Jeux vidéo").clicked() {
                        let pack = [
                            "roblox.com", "www.roblox.com",
                            "steamcommunity.com", "store.steampowered.com",
                            "epicgames.com", "www.epicgames.com",
                            "poki.com", "crazygames.com",
                            "riotgames.com",
                        ];
                        for d in pack {
                            if !self.big_brother_cfg.domains.iter().any(|x| x.eq_ignore_ascii_case(d)) {
                                self.big_brother_cfg.domains.push(d.to_string());
                            }
                        }
                    }
                    if ui.button("+ IA grand public").clicked() {
                        let pack = ["chatgpt.com", "openai.com", "claude.ai"];
                        for d in pack {
                            if !self.big_brother_cfg.domains.iter().any(|x| x.eq_ignore_ascii_case(d)) {
                                self.big_brother_cfg.domains.push(d.to_string());
                            }
                        }
                    }
                    if ui.button("Vider").clicked() {
                        self.big_brother_cfg.domains.clear();
                    }
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.big_brother_new_domain).hint_text("ex. snapchat.com").desired_width(200.0));
                    if ui.button("Ajouter domaine").clicked() {
                        let d = self.big_brother_new_domain.trim().to_ascii_lowercase();
                        if !d.is_empty() && !self.big_brother_cfg.domains.iter().any(|x| x.eq_ignore_ascii_case(&d)) {
                            self.big_brother_cfg.domains.push(d);
                            self.big_brother_new_domain.clear();
                        }
                    }
                });

                ui.add_space(4.0);
                ui.label(format!("Domaines bloqués actuellement ({}) :", self.big_brother_cfg.domains.len()));
                let mut remove_domain_idx = None;
                egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                    for (i, d) in self.big_brother_cfg.domains.iter().enumerate() {
                        ui.horizontal(|ui| {
                            if ui.small_button("✕").clicked() {
                                remove_domain_idx = Some(i);
                            }
                            ui.label(d);
                        });
                    }
                });
                if let Some(i) = remove_domain_idx {
                    self.big_brother_cfg.domains.remove(i);
                }

                ui.add_space(8.0);
                ui.separator();
                ui.label(egui::RichText::new("DNS filtrant imposé (au niveau interfaces réseau) :").strong());
                ui.horizontal(|ui| {
                    if ui.selectable_label(self.big_brother_cfg.dns_filter.is_none(), "Défaut / DHCP").clicked() {
                        self.big_brother_cfg.dns_filter = None;
                    }
                    if ui.selectable_label(self.big_brother_cfg.dns_filter.as_deref() == Some("127.0.0.1"), "Intercepteur Local (127.0.0.1)").clicked() {
                        self.big_brother_cfg.dns_filter = Some("127.0.0.1".to_string());
                    }
                    if ui.selectable_label(self.big_brother_cfg.dns_filter.as_deref() == Some("1.1.1.3"), "Cloudflare Family (1.1.1.3)").clicked() {
                        self.big_brother_cfg.dns_filter = Some("1.1.1.3".to_string());
                    }
                    if ui.selectable_label(self.big_brother_cfg.dns_filter.as_deref() == Some("185.228.168.10"), "CleanBrowsing (185.228.168.10)").clicked() {
                        self.big_brother_cfg.dns_filter = Some("185.228.168.10".to_string());
                    }
                    if ui.selectable_label(self.big_brother_cfg.dns_filter.as_deref() == Some("94.140.14.15"), "AdGuard Family (94.140.14.15)").clicked() {
                        self.big_brother_cfg.dns_filter = Some("94.140.14.15".to_string());
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("DNS personnalisé (IP) :");
                    ui.add(egui::TextEdit::singleline(&mut self.big_brother_custom_dns).desired_width(140.0));
                    if ui.button("Appliquer IP").clicked() {
                        let ip = self.big_brother_custom_dns.trim().to_string();
                        if !ip.is_empty() {
                            self.big_brother_cfg.dns_filter = Some(ip);
                        }
                    }
                });
            });

            ui.add_space(8.0);

            // 5. Blocage d'Applications (Serbert Ring 0 & Watchdog Ring 3)
            ui.collapsing("⚙ Applications & Processus interdits (Serbert Ring 0 & Watchdog)", |ui| {
                ui.label(egui::RichText::new("Interception immédiate avant exécution (Serbert Ring 0) + surveillance continue (Watchdog SYSTEM).").small());
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Packs rapides :");
                    if ui.button("+ Jeux & Launchers").clicked() {
                        let pack = [
                            "steam.exe", "epicgameslauncher.exe", "robloxplayerbeta.exe",
                            "minecraft.exe", "riotclientservices.exe", "battlenet.exe",
                        ];
                        for p in pack {
                            if !self.big_brother_cfg.blocked_processes.iter().any(|x| x.eq_ignore_ascii_case(p)) {
                                self.big_brother_cfg.blocked_processes.push(p.to_string());
                            }
                        }
                    }
                    if ui.button("+ Messageries").clicked() {
                        let pack = ["discord.exe", "telegram.exe", "whatsapp.exe", "spotify.exe"];
                        for p in pack {
                            if !self.big_brother_cfg.blocked_processes.iter().any(|x| x.eq_ignore_ascii_case(p)) {
                                self.big_brother_cfg.blocked_processes.push(p.to_string());
                            }
                        }
                    }
                    if ui.button("+ Outils P2P / Tor").clicked() {
                        let pack = ["utorrent.exe", "bittorrent.exe", "qbittorrent.exe", "tor.exe"];
                        for p in pack {
                            if !self.big_brother_cfg.blocked_processes.iter().any(|x| x.eq_ignore_ascii_case(p)) {
                                self.big_brother_cfg.blocked_processes.push(p.to_string());
                            }
                        }
                    }
                    if ui.button("Vider").clicked() {
                        self.big_brother_cfg.blocked_processes.clear();
                    }
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.big_brother_new_proc).hint_text("ex. game.exe").desired_width(180.0));
                    if ui.button("Ajouter processus").clicked() {
                        let p = self.big_brother_new_proc.trim().to_ascii_lowercase();
                        if !p.is_empty() && !self.big_brother_cfg.blocked_processes.iter().any(|x| x.eq_ignore_ascii_case(&p)) {
                            self.big_brother_cfg.blocked_processes.push(p);
                            self.big_brother_new_proc.clear();
                        }
                    }
                });

                ui.add_space(4.0);
                ui.label(format!("Processus surveillés ({}) :", self.big_brother_cfg.blocked_processes.len()));
                let mut remove_proc_idx = None;
                egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                    for (i, p) in self.big_brother_cfg.blocked_processes.iter().enumerate() {
                        ui.horizontal(|ui| {
                            if ui.small_button("✕").clicked() {
                                remove_proc_idx = Some(i);
                            }
                            ui.label(p);
                        });
                    }
                });
                if let Some(i) = remove_proc_idx {
                    self.big_brother_cfg.blocked_processes.remove(i);
                }
            });

            ui.add_space(8.0);

            // 6. Blocage des installations et exécutions (MSI & SRP)
            ui.collapsing("■ Restrictions d'installations & SRP (DisableMSI & Chemins)", |ui| {
                let msi_active = crate::big_brother::msi_policy_active();
                ui.checkbox(
                    &mut self.big_brother_cfg.block_msi,
                    format!(
                        "Interdire toute installation Windows Installer (.msi) — DisableMSI=2 (état registre : {})",
                        if msi_active { "actif" } else { "inactif" }
                    ),
                );
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Interdire l'exécution d'exécutables depuis les dossiers inscriptibles par l'élève (Software Restriction Policies) :").strong());
                ui.label(egui::RichText::new("Empêche le lancement d'exécutables téléchargés sans droits admin (ex. installateurs .exe, jeux portables).").small());

                let mut has_downloads = self.big_brother_cfg.deny_paths.iter().any(|p| p.eq_ignore_ascii_case("Downloads"));
                let mut has_desktop = self.big_brother_cfg.deny_paths.iter().any(|p| p.eq_ignore_ascii_case("Desktop"));
                let mut has_temp = self.big_brother_cfg.deny_paths.iter().any(|p| p.eq_ignore_ascii_case("AppData\\Local\\Temp"));

                if ui.checkbox(&mut has_downloads, "Bloquer l'exécution dans Téléchargements (Downloads)").changed() {
                    if has_downloads {
                        self.big_brother_cfg.deny_paths.push("Downloads".to_string());
                    } else {
                        self.big_brother_cfg.deny_paths.retain(|p| !p.eq_ignore_ascii_case("Downloads"));
                    }
                }
                if ui.checkbox(&mut has_desktop, "Bloquer l'exécution sur le Bureau (Desktop)").changed() {
                    if has_desktop {
                        self.big_brother_cfg.deny_paths.push("Desktop".to_string());
                    } else {
                        self.big_brother_cfg.deny_paths.retain(|p| !p.eq_ignore_ascii_case("Desktop"));
                    }
                }
                if ui.checkbox(&mut has_temp, "Bloquer l'exécution dans AppData\\Local\\Temp").changed() {
                    if has_temp {
                        self.big_brother_cfg.deny_paths.push("AppData\\Local\\Temp".to_string());
                    } else {
                        self.big_brother_cfg.deny_paths.retain(|p| !p.eq_ignore_ascii_case("AppData\\Local\\Temp"));
                    }
                }

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.big_brother_new_deny).hint_text("Dossier personnalisé (ex. Documents)").desired_width(200.0));
                    if ui.button("Ajouter dossier").clicked() {
                        let path = self.big_brother_new_deny.trim().to_string();
                        if !path.is_empty() && !self.big_brother_cfg.deny_paths.iter().any(|p| p.eq_ignore_ascii_case(&path)) {
                            self.big_brother_cfg.deny_paths.push(path);
                            self.big_brother_new_deny.clear();
                        }
                    }
                });
            });

            ui.add_space(8.0);

            // 7. Quota Quotidien & Plages Horaires
            ui.collapsing("◷ Temps d'écran & Plages horaires autorisées", |ui| {
                ui.label(egui::RichText::new("Lorsque le quota est atteint ou hors de la plage autorisée, la session console active est automatiquement déconnectée (verrouillée).").small());
                ui.add_space(6.0);

                ui.checkbox(&mut self.big_brother_quota_enabled, "Activer un quota quotidien de session console");
                if self.big_brother_quota_enabled {
                    ui.horizontal(|ui| {
                        ui.label("Limite :");
                        ui.add(egui::DragValue::new(&mut self.big_brother_quota_mins).clamp_range(5..=1440).suffix(" min"));
                    });
                    self.big_brother_cfg.daily_minutes = Some(self.big_brother_quota_mins);
                } else {
                    self.big_brother_cfg.daily_minutes = None;
                }

                let used_mins = self.big_brother_state.active_secs / 60;
                let quota_mins = self.big_brother_quota_mins;
                let pct = if quota_mins > 0 { (used_mins as f32 / quota_mins as f32).clamp(0.0, 1.0) } else { 0.0 };
                ui.horizontal(|ui| {
                    ui.label(format!("Temps actif aujourd'hui : {} min", used_mins));
                    if self.big_brother_quota_enabled {
                        ui.add(egui::ProgressBar::new(pct).text(format!("{used_mins} / {quota_mins} min")).desired_width(160.0));
                    }
                });

                if self.big_brother_state.quota_lock {
                    if let Some(r) = &self.big_brother_state.quota_reason {
                        ui.colored_label(egui::Color32::from_rgb(0xE5, 0x5C, 0x5C), format!("■ Restriction actuellement active : {r}"));
                    }
                }

                if ui.button("↻ Remettre à zéro le compteur d'aujourd'hui").clicked() {
                    self.big_brother_state.reset_today();
                    self.info = Some("Compteur de session active du jour réinitialisé.".into());
                }

                ui.add_space(8.0);
                ui.separator();

                ui.checkbox(&mut self.big_brother_time_window_enabled, "Restreindre l'accès à une plage horaire autorisée");
                if self.big_brother_time_window_enabled {
                    ui.checkbox(&mut self.big_brother_window_weekdays_only, "Du Lundi au Vendredi uniquement (jours d'école)");
                    ui.horizontal(|ui| {
                        ui.label("De :");
                        ui.add(egui::TextEdit::singleline(&mut self.big_brother_window_from).desired_width(60.0));
                        ui.label("à :");
                        ui.add(egui::TextEdit::singleline(&mut self.big_brother_window_to).desired_width(60.0));
                        ui.label("(format HH:MM, ex. 08:00 à 18:00)");
                    });

                    let days = if self.big_brother_window_weekdays_only {
                        vec![1, 2, 3, 4, 5]
                    } else {
                        vec![1, 2, 3, 4, 5, 6, 7]
                    };
                    self.big_brother_cfg.allowed_windows = vec![crate::big_brother::TimeWindow {
                        days,
                        from: self.big_brother_window_from.clone(),
                        to: self.big_brother_window_to.clone(),
                    }];
                } else {
                    self.big_brother_cfg.allowed_windows.clear();
                }
            });

            ui.add_space(16.0);

            // Barre d'action d'enregistrement moderne
            ui.horizontal(|ui| {
                let save_btn = egui::Button::new(
                    egui::RichText::new("💾 Enregistrer et Appliquer la Politique")
                        .color(egui::Color32::WHITE)
                        .strong(),
                )
                .min_size(egui::vec2(280.0, 38.0))
                .fill(egui::Color32::from_rgb(0x10, 0xB9, 0x81))
                .rounding(egui::Rounding::same(8.0));

                if ui.add(save_btn).on_hover_text("Enregistre la configuration. En Mode Standard, demande automatiquement l'élévation UAC sans échec.").clicked() {
                    self.save_big_brother_policy();
                }

                let sync_btn = egui::Button::new(
                    egui::RichText::new("↻ Appliquer immédiatement (tick)")
                        .strong(),
                )
                .min_size(egui::vec2(210.0, 38.0))
                .rounding(egui::Rounding::same(8.0));

                if ui.add(sync_btn).clicked() {
                    crate::big_brother::tick();
                    self.info = Some("Politique Big Brother réappliquée immédiatement.".into());
                    self.err = None;
                }

                let reload_btn = egui::Button::new("≡ Recharger l'état")
                    .min_size(egui::vec2(140.0, 38.0))
                    .rounding(egui::Rounding::same(8.0));

                if ui.add(reload_btn).clicked() {
                    self.refresh_modes();
                    self.info = Some("État rechargé depuis le système.".into());
                }
            });

            ui.add_space(14.0);
    }

    pub fn save_big_brother_policy(&mut self) {
        if self.big_brother_time_window_enabled {
            let days = if self.big_brother_window_weekdays_only {
                vec![1, 2, 3, 4, 5]
            } else {
                vec![0, 1, 2, 3, 4, 5, 6]
            };
            self.big_brother_cfg.allowed_windows = vec![crate::big_brother::TimeWindow {
                days,
                from: self.big_brother_window_from.clone(),
                to: self.big_brother_window_to.clone(),
            }];
        } else {
            self.big_brother_cfg.allowed_windows.clear();
        }

        match self.big_brother_cfg.save() {
            Ok(()) => {
                self.info = Some("Politique système enregistrée et appliquée avec succès.".into());
                self.err = None;
                crate::big_brother::tick();
            }
            Err(e) => {
                // Si l'utilisateur n'a pas les droits en écriture sur ProgramData (os error 5),
                // on écrit le fichier dans LocalAppData et on déclenche l'importation UAC
                let dir = crate::data_dir();
                let _ = std::fs::create_dir_all(&dir);
                let pending_path = dir.join("big_brother_pending.json");
                match serde_json::to_vec_pretty(&self.big_brother_cfg) {
                    Ok(data) => {
                        if let Err(write_err) = std::fs::write(&pending_path, data) {
                            self.err = Some(format!("Échec de préparation du fichier : {write_err}"));
                            self.info = None;
                            return;
                        }
                        let path_str = pending_path.to_string_lossy().to_string();
                        match crate::win::relaunch_elevated_args(&["--import-policy", &path_str]) {
                            Ok(()) => {
                                self.info = Some("Demande d'autorisation UAC envoyée. Cliquez sur 'Oui' pour valider l'enregistrement.".into());
                                self.err = None;
                            }
                            Err(elev_err) => {
                                self.err = Some(format!("Élévation requise pour modifier les règles ({e}). {elev_err}"));
                                self.info = None;
                            }
                        }
                    }
                    Err(ser_err) => {
                        self.err = Some(format!("Erreur de sérialisation : {ser_err}"));
                        self.info = None;
                    }
                }
            }
        }
    }

    pub fn promote_to_admin(&mut self) {
        if crate::win::is_admin() {
            match crate::big_brother::install_service() {
                Ok(()) => {
                    self.info = Some("Mode 2 (Accès Admin - Big Brother) activé avec succès.".into());
                    self.err = None;
                    self.refresh_modes();
                }
                Err(e) => self.err = Some(format!("Erreur activation Admin : {e}")),
            }
        } else if let Err(e) = crate::win::relaunch_elevated_args(&["--mode", "admin"]) {
            self.err = Some(e);
        }
    }

    pub fn promote_to_kernel(&mut self) {
        if crate::win::is_admin() {
            let p = crate::serbert::default_driver_path();
            let _ = crate::serbert::install_driver_service(&p);
            match crate::serbert::start_driver() {
                Ok(_) => {
                    self.info = Some("Mode 3 (Accès Noyau - Serbert Ring 0) activé avec succès.".into());
                    self.err = None;
                    self.refresh_modes();
                }
                Err(e) => {
                    self.err = Some(format!("Erreur chargement pilote noyau Serbert : {e}. (Assurez-vous que testsigning est actif ou que le pilote est signé)"));
                }
            }
        } else if let Err(e) = crate::win::relaunch_elevated_args(&["--mode", "kernel"]) {
            self.err = Some(e);
        }
    }

    pub fn demote_to_admin(&mut self) {
        if crate::win::is_admin() {
            match crate::serbert::uninstall_driver_service() {
                Ok(()) => {
                    self.info = Some("Pilote noyau Serbert déchargé. Retour en Mode 2 : Admin (Big Brother).".into());
                    self.err = None;
                    self.refresh_modes();
                }
                Err(e) => self.err = Some(format!("Erreur déchargement pilote : {e}")),
            }
        } else if let Err(e) = crate::win::relaunch_elevated_args(&["--serbert-uninstall"]) {
            self.err = Some(e);
        }
    }

    pub fn demote_to_standard(&mut self) {
        if crate::win::is_admin() {
            let _ = crate::serbert::uninstall_driver_service();
            match crate::big_brother::uninstall_service() {
                Ok(()) => {
                    self.info = Some("Service Big Brother désinstallé et restrictions retirées. Retour en Mode 1 : Standard (Sans autorisation).".into());
                    self.err = None;
                    self.refresh_modes();
                }
                Err(e) => self.err = Some(format!("Erreur désinstallation service : {e}")),
            }
        } else if let Err(e) = crate::win::relaunch_elevated_args(&["--mode", "standard"]) {
            self.err = Some(e);
        }
    }

    pub fn refresh_modes(&mut self) {
        self.big_brother_status = crate::big_brother::service_status();
        self.big_brother_state = crate::big_brother::BigBrotherState::load();
        self.serbert_status = crate::serbert::get_status();
        self.serbert_driver_running = crate::serbert::is_driver_running();
        self.last_status_check = std::time::Instant::now();
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
    if ok { format!("✓ {op}") } else { format!("… en attente de confirmation") }
}

/// Grille 7 jours × 24 heures d'usage ; cellules colorées selon l'intensité.
fn draw_heatmap(ui: &mut egui::Ui, shared: &Arc<Shared>) {
    let rows = {
        let map = shared.heatmap.lock().unwrap();
        crate::shared::heatmap_rows(&map, 7)
    };
    ui.label(egui::RichText::new("Estimation : applications au premier plan, reconstituée depuis le journal chiffré.").weak());
    ui.add_space(4.0);
    let cell = egui::vec2(20.0, 13.0);
    // Étiquettes d'heures au-dessus.
    egui::Grid::new("heat-hours").num_columns(25).show(ui, |ui| {
        ui.weak("");
        for h in 0..24 {
            ui.label(
                egui::RichText::new(if h % 3 == 0 { format!("{h}") } else { String::new() })
                    .small(),
            );
        }
        ui.end_row();
    });
    egui::Grid::new("heat-grid").num_columns(25).show(ui, |ui| {
        for day_idx in 0..7 {
            // heatmap_rows est du plus récent au plus ancien ; affiche du vieux en haut.
            let base = (6 - day_idx) * 24;
            let label = rows[base].0.clone();
            ui.label(egui::RichText::new(label).small());
            for h in 0..24 {
                let secs = rows[base + h].2;
                let intensity = ((secs as f32) / 1800.0).clamp(0.0, 1.0);
                let c = if secs == 0 {
                    egui::Color32::from_rgb(38, 40, 52)
                } else {
                    let lerp = |a: f32, b: f32| (a + (b - a) * intensity) as u8;
                    egui::Color32::from_rgb(
                        lerp(45.0, 70.0),
                        lerp(55.0, 190.0),
                        lerp(70.0, 120.0),
                    )
                };
                let (rect, resp) = ui.allocate_exact_size(cell, egui::Sense::hover());
                ui.painter().rect_filled(rect, 2.0, c);
                if resp.hovered() && secs > 0 {
                    resp.clone()
                        .on_hover_text(format!(
                            "{} {:02}:00 — {}",
                            rows[base + h].0,
                            h,
                            crate::model::format_hms(secs)
                        ));
                }
            }
            ui.end_row();
        }
    });
    ui.add_space(4.0);
    ui.label(egui::RichText::new("Vert foncé → clair : usage croissant (1800 s = teinte max).").weak());
}

/// Section « Mise à jour » du Statut : note + bouton clic.
fn draw_update_section(
    ui: &mut egui::Ui,
    shared: &Arc<Shared>,
    info: &mut Option<String>,
    err: &mut Option<String>,
) {
    let (note, url) = {
        let st = shared.status.lock().unwrap();
        (st.update_note.clone(), st.update_url.clone())
    };
    ui.heading("↻ Mise à jour");
    ui.add_space(4.0);
    match note {
        Some(n) => {
            ui.label(n);
        }
        None => {
            ui.label(egui::RichText::new("Aucune vérification effectuée.").weak());
        }
    }
    ui.horizontal(|ui| {
        if ui.button("Rechercher une mise à jour").clicked() {
            let sh = shared.clone();
            std::thread::spawn(move || crate::update::check_once(&sh));
        }
        if let Some(u) = url {
            if ui.button("↗ Ouvrir la release…").clicked() {
                open_url(&u);
                *info = Some(format!("Ouverture de {u}"));
                *err = None;
            }
        }
    });
}

/// Dessine un QR code dans l'UI à partir de la matrice interne du code (crate `qrcode`).
fn draw_qr(ui: &mut egui::Ui, text: &str, max_size: f32) {
    let code = match qrcode::QrCode::new(text) {
        Ok(c) => c,
        Err(_) => return,
    };
    let n = code.width() as f32;
    let module = (max_size / (n + 8.0)).floor().max(2.0);
    let quiet = module * 4.0;
    let total = (n + quiet * 2.0) * module;
    let (rect, _resp) = ui.allocate_exact_size(egui::vec2(total, total), egui::Sense::hover());
    let painter = ui.painter();
    let colors = code.to_colors();
    let n_int = code.width() as usize;
    for row in 0..n_int {
        for col in 0..n_int {
            let dark = colors[row * n_int + col] != qrcode::Color::Light;
            if dark {
                let x = rect.min.x + (col as f32 + quiet) * module;
                let y = rect.min.y + (row as f32 + quiet) * module;
                painter.rect_filled(
                    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(module, module)),
                    0.0,
                    egui::Color32::BLACK,
                );
            }
        }
    }
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

/// Vérifie un mot de passe admin contre l'empreinte PHC enregistrée (Argon2id).
pub(crate) fn verify_admin_phc(phc: &str, password: &str) -> bool {
    use argon2::{Argon2, PasswordHash, PasswordVerifier};
    match PasswordHash::new(phc) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// Hache un mot de passe admin en PHC (Argon2id, même puissance que le journal).
fn hash_admin_phc(password: &str) -> Option<String> {
    use argon2::password_hash::rand_core::OsRng;
    use argon2::password_hash::SaltString;
    use argon2::{Algorithm, Argon2, Params, PasswordHasher, Version};
    let salt = SaltString::generate(&mut OsRng);
    let params = Params::new(19456, 2, 1, None).ok()?;
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password(password.as_bytes(), &salt)
        .ok()
        .map(|h| h.to_string())
}

/// Ouvre une URL dans le navigateur par défaut.
fn open_url(url: &str) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("cmd")
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .args(["/C", "start", "", url])
        .spawn();
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

        // Refresh réactif et fluide (snappy) pour les animations et interactions utilisateur
        if self.shared.dirty.swap(false, Ordering::Relaxed) {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(120));
        }

        egui::TopBottomPanel::top("topbar")
            .frame(egui::Frame::none()
                .fill(egui::Color32::from_rgb(0x13, 0x16, 0x22))
                .inner_margin(egui::Margin::symmetric(14.0, 8.0))
                .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0x22, 0x28, 0x38))))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(egui::RichText::new("⛨ DFWLogger").color(egui::Color32::from_rgb(0x5B, 0x9C, 0xFF)).strong());
                    ui.add_space(8.0);
                    let monitoring = self.shared.monitoring.load(Ordering::Relaxed);
                    let (chip_txt, chip_col) = if monitoring {
                        ("● SURVEILLANCE ACTIVE", egui::Color32::from_rgb(0x10, 0xB9, 0x81))
                    } else {
                        ("■ JOURNAL VERROUILLÉ", egui::Color32::from_rgb(0xEF, 0x44, 0x44))
                    };
                    ui.label(egui::RichText::new(chip_txt).color(chip_col).small().strong());

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut nav_item = |ui: &mut egui::Ui, tab: Tab, icon: &str, title: &str| {
                            let active = self.tab == tab;
                            let text = egui::RichText::new(format!("{icon} {title}"))
                                .color(if active { egui::Color32::WHITE } else { egui::Color32::from_rgb(0x94, 0xA3, 0xB8) })
                                .strong();
                            let btn = egui::Button::new(text)
                                .fill(if active { egui::Color32::from_rgb(0x25, 0x63, 0xEB) } else { egui::Color32::from_rgb(0x18, 0x1C, 0x28) })
                                .stroke(egui::Stroke::new(1.0_f32, if active { egui::Color32::from_rgb(0x60, 0xA5, 0xFA) } else { egui::Color32::from_rgb(0x2A, 0x32, 0x46) }))
                                .rounding(egui::Rounding::same(6.0));
                            if ui.add(btn).clicked() {
                                self.tab = tab;
                            }
                        };

                        nav_item(ui, Tab::Aide, "?", "Aide");
                        nav_item(ui, Tab::Acces, "☵", "Accès LAN");
                        nav_item(ui, Tab::Reglages, "⚙", "Réglages");
                        nav_item(ui, Tab::BigBrother, "⛊", "Modes & Règles");
                        nav_item(ui, Tab::Evenements, "≡", "Journal");
                        nav_item(ui, Tab::Statut, "◆", "Aperçu");
                    });
                });
            });

        // Actualisation périodique légère (sans bloquer le thread graphique ni spawner de process par frame)
        if self.last_status_check.elapsed() > Duration::from_secs(4) {
            self.refresh_modes();
        }
        let current_tier = self.current_tier();

        egui::TopBottomPanel::top("mode_bar")
            .frame(egui::Frame::none()
                .fill(egui::Color32::from_rgb(0x17, 0x1B, 0x26))
                .inner_margin(egui::Margin::symmetric(14.0, 7.0))
                .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0x24, 0x2C, 0x3E))))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Niveau d'Accès :").color(egui::Color32::from_rgb(0x94, 0xA3, 0xB8)).strong());
                    ui.add_space(4.0);

                    let (tier_name, tier_col, tier_desc) = match current_tier {
                        crate::big_brother::PrivilegeTier::Standard => ("1. Sans Autorisation (Standard)", egui::Color32::from_rgb(0x10, 0xB9, 0x81), "Non-intrusif — Journal chiffré local"),
                        crate::big_brother::PrivilegeTier::Admin => ("2. Administrateur (Big Brother SYSTEM)", egui::Color32::from_rgb(0x3B, 0x82, 0xF6), "Service Windows — Filtrage DNS, hosts & SRP"),
                        crate::big_brother::PrivilegeTier::Kernel => ("3. Noyau Matériel (Serbert Ring 0)", egui::Color32::from_rgb(0xF5, 0x9E, 0x0B), "Interception PsSetCreateProcess... Ring 0"),
                    };

                    ui.label(egui::RichText::new(tier_name).color(tier_col).strong());
                    ui.label(egui::RichText::new(format!("— {tier_desc}")).weak().small());

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        match current_tier {
                            crate::big_brother::PrivilegeTier::Standard => {
                                let btn = egui::Button::new(egui::RichText::new("↑ Élever en Accès Admin").color(egui::Color32::WHITE).strong())
                                    .fill(egui::Color32::from_rgb(0x1D, 0x4E, 0xD8))
                                    .rounding(egui::Rounding::same(6.0));
                                if ui.add(btn).clicked() {
                                    self.promote_to_admin();
                                }
                            }
                            crate::big_brother::PrivilegeTier::Admin => {
                                let btn_k = egui::Button::new(egui::RichText::new("✦ Activer le Mode Noyau").color(egui::Color32::WHITE).strong())
                                    .fill(egui::Color32::from_rgb(0xD9, 0x77, 0x06))
                                    .rounding(egui::Rounding::same(6.0));
                                if ui.add(btn_k).clicked() {
                                    self.promote_to_kernel();
                                }
                                if ui.button("↩ Revenir sans élévation").clicked() {
                                    self.demote_to_standard();
                                }
                            }
                            crate::big_brother::PrivilegeTier::Kernel => {
                                if ui.button("↩ Rétrograder en Accès Admin").clicked() {
                                    self.demote_to_admin();
                                }
                            }
                        }
                    });
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            let opened = self.shared.store.lock().unwrap().is_open();
            if !opened {
                let fresh = !self.is_initialized_before();
                self.draw_unlock_screen(ui, fresh);
                return;
            }
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    match self.tab {
                        Tab::Statut => self.draw_status(ui),
                        Tab::Evenements => self.draw_events(ui),
                        Tab::Reglages => self.draw_settings(ui),
                        Tab::BigBrother => self.draw_big_brother(ui),
                        Tab::Acces => self.draw_access(ui),
                        Tab::Aide => self.draw_help(ui),
                    }
                    ui.add_space(8.0);
                    if self.info.is_some() || self.err.is_some() {
                        show_message(self, ui);
                    }
                });
        });
    }
}
