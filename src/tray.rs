//! Icône dans la zone de notification (tray) : menu « Ouvrir » / « Quitter »,
//! clic gauche = réafficher la fenêtre. tray-icon livre les événements sur un
//! canal crossbeam ; ce thread fait aussi tourner une pompe de messages Win32
//! (PeekMessage) pour que l'icône reste réactive en permanence.

use crate::shared::Shared;
use egui::Context;
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// Dessine l'icône en RGBA brut (carré arrondi bleu, œil stylisé) — pas de dépendance image.
pub fn ico_rgba() -> (Vec<u8>, u32, u32) {
    const S: u32 = 32;
    const CORNERS: [(u32, u32); 20] = [
        (2, 2), (3, 1), (4, 1), (1, 3), (1, 4),
        (29, 2), (28, 1), (27, 1), (30, 3), (30, 4),
        (2, 29), (3, 30), (4, 30), (1, 28), (1, 27),
        (29, 29), (28, 30), (27, 30), (30, 28), (30, 27),
    ];
    let mut rgba = Vec::with_capacity((S * S * 4) as usize);
    for y in 0..S {
        for x in 0..S {
            let transparent = CORNERS.iter().any(|&(cx, cy)| x == cx && y == cy);
            let dx = i64::from(x) - 16;
            let dy = i64::from(y) - 16;
            let dist = dx * dx + dy * dy;
            let color: (u8, u8, u8, u8) = if transparent {
                (0, 0, 0, 0)
            } else if dist < 144 {
                (255, 255, 255, 255) // pupille blanche
            } else if dist < 900 {
                (16, 22, 38, 255) // anneau sombre
            } else {
                (79, 140, 255, 255) // carré bleu
            };
            let (r, g, b, a) = color;
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }
    (rgba, S, S)
}

pub fn spawn_tray(shared: Arc<Shared>, ctx: Context) {
    std::thread::Builder::new()
        .name("dfwlogger-tray".into())
        .spawn(move || tray_loop(shared, ctx))
        .ok();
}

fn tray_loop(shared: Arc<Shared>, ctx: Context) {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent};

    let set_error = |shared: &Arc<Shared>, msg: String| {
        shared.status.lock().unwrap().dashboard_error = Some(msg);
    };

    let (rgba, w, h) = ico_rgba();
    let icon = match tray_icon::Icon::from_rgba(rgba, w, h) {
        Ok(i) => i,
        Err(e) => {
            set_error(&shared, format!("icône tray : {e}"));
            return;
        }
    };

    let menu = Menu::new();
    let m_show = MenuItem::new("Ouvrir DFWLogger", true, None);
    let m_quit = MenuItem::new("Quitter", true, None);
    if menu.append(&m_show).is_err() || menu.append(&m_quit).is_err() {
        set_error(&shared, "tray : menu indisponible".into());
        return;
    }

    let tray: TrayIcon = match TrayIconBuilder::new()
        .with_tooltip("DFWLogger — superviseur familial")
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_icon(icon)
        .build()
    {
        Ok(t) => t,
        Err(e) => {
            set_error(&shared, format!("tray non disponible : {e}"));
            return;
        }
    };
    // L'icône reste en vie tant que ce thread tourne (processus entier).
    let _keep_alive: &TrayIcon = &tray;

    use windows::Win32::UI::WindowsAndMessaging::MSG;
    use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, PM_REMOVE};
    loop {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = windows::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
                let _ = DispatchMessageW(&msg);
            }
        }

        let mut dirty = false;
        while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
            match ev {
                TrayIconEvent::Click { .. } | TrayIconEvent::DoubleClick { .. } => {
                    shared.show_request.store(true, Ordering::Relaxed);
                    dirty = true;
                }
                _ => {}
            }
        }
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if ev.id == m_show.id() {
                shared.show_request.store(true, Ordering::Relaxed);
                dirty = true;
            } else if ev.id == m_quit.id() {
                std::process::exit(0);
            }
        }
        if dirty {
            ctx.request_repaint();
        }
        std::thread::sleep(std::time::Duration::from_millis(120));
    }
}
