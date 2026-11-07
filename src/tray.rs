//! Icône dans la zone de notification (tray) : menu « Ouvrir » / « Quitter »,
//! clic gauche = réafficher la fenêtre. tray-icon livre les événements sur un
//! canal crossbeam ; ce thread fait aussi tourner une pompe de messages Win32
//! (PeekMessage) pour que l'icône reste réactive en permanence.

use crate::shared::Shared;
use egui::Context;
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// Nouvelle icône DFWLogger : bouclier protecteur avec œil (design SVG),
/// dessinée à la volée via le crate `resvg` (source SVG embarquée).
const ICON_SVG: &str = r##"
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="512" height="512">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#5B9CFF"/>
      <stop offset="1" stop-color="#1D4ED8"/>
    </linearGradient>
  </defs>
  <rect x="16" y="16" width="480" height="480" rx="112" fill="url(#bg)"/>
  <path d="M256 88 L56 160 V256 C56 372 170 444 256 488 C342 444 456 372 456 256 V160 Z" fill="#FFFFFF"/>
  <ellipse cx="256" cy="256" rx="88" ry="64" fill="#1D4ED8"/>
  <circle cx="256" cy="256" r="28" fill="#FFFFFF"/>
</svg>
"##;

/// Dessine l'icône SVG (RGBA, taille carrée) et retourne les octets bruts
/// (32 bits par pixel : R, G, B, A — rangées de haut en bas).
fn render_svg_rgba(svg: &str, size: u32) -> Option<Vec<u8>> {
    use resvg::tiny_skia;
    use resvg::usvg;
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_data(svg.as_bytes(), &opt).ok()?;
    let scale = size as f32 / tree.size().width();
    let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let data = pixmap.data();
    let mut rgba = Vec::with_capacity(data.len());
    for chunk in data.chunks_exact(4) {
        let a = chunk[3] as u32;
        if a == 0 {
            rgba.extend_from_slice(&[0, 0, 0, 0]);
        } else if a == 255 {
            rgba.extend_from_slice(chunk);
        } else {
            let r = ((chunk[0] as u32 * 255 + a / 2) / a).min(255) as u8;
            let g = ((chunk[1] as u32 * 255 + a / 2) / a).min(255) as u8;
            let b = ((chunk[2] as u32 * 255 + a / 2) / a).min(255) as u8;
            rgba.extend_from_slice(&[r, g, b, a as u8]);
        }
    }
    Some(rgba)
}

/// Icône DFWLogger actuelle (SVG « bouclier + œil »), rendue en RGBA brut 64×64.
pub fn ico_rgba() -> (Vec<u8>, u32, u32) {
    const SIZE: u32 = 64;
    match render_svg_rgba(ICON_SVG, SIZE) {
        Some(rgba) if rgba.len() == (SIZE * SIZE * 4) as usize => (rgba, SIZE, SIZE),
        _ => (vec![79, 140, 255, 255].repeat((SIZE * SIZE) as usize), SIZE, SIZE),
    }
}

/// Génère un fichier .ico complet (multi-résolution 256, 128, 64, 48, 32, 16 px, PNG embarqué)
/// directement à partir de la source vectorielle SVG.
#[allow(dead_code)]
pub fn generate_ico_bytes() -> Option<Vec<u8>> {
    use resvg::tiny_skia;
    use resvg::usvg;
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_data(ICON_SVG.as_bytes(), &opt).ok()?;

    let sizes: &[u32] = &[256, 128, 64, 48, 32, 16];
    let mut png_images: Vec<(u32, Vec<u8>)> = Vec::new();

    for &size in sizes {
        let scale = size as f32 / tree.size().width();
        let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
        resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
        let png = pixmap.encode_png().ok()?;
        png_images.push((size, png));
    }

    let n = png_images.len() as u16;
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes()); // réservé
    out.extend_from_slice(&1u16.to_le_bytes()); // type icône (1)
    out.extend_from_slice(&n.to_le_bytes());

    let mut offset: u32 = 6 + 16 * n as u32;
    for (size, png) in &png_images {
        let b_w = if *size >= 256 { 0u8 } else { *size as u8 };
        let b_h = if *size >= 256 { 0u8 } else { *size as u8 };
        out.push(b_w);
        out.push(b_h);
        out.push(0); // palette
        out.push(0); // réservé
        out.extend_from_slice(&1u16.to_le_bytes()); // plans
        out.extend_from_slice(&32u16.to_le_bytes()); // bpp
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }

    for (_, png) in png_images {
        out.extend_from_slice(&png);
    }

    Some(out)
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
                    crate::win::show_main_window();
                    dirty = true;
                }
                _ => {}
            }
        }
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if ev.id == m_show.id() {
                shared.show_request.store(true, Ordering::Relaxed);
                crate::win::show_main_window();
                dirty = true;
            } else if ev.id == m_quit.id() {
                std::process::exit(0);
            }
        }
        if dirty {
            ctx.request_repaint();
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_svg_rendering_output() {
        let (rgba, w, h) = ico_rgba();
        assert_eq!(w, 64);
        assert_eq!(h, 64);
        assert_eq!(rgba.len(), (64 * 64 * 4) as usize);
        let has_visible_pixels = rgba.chunks_exact(4).any(|p| p[3] > 0);
        assert!(has_visible_pixels, "L'icône SVG rendue doit contenir des pixels visibles");
    }

    #[test]
    fn test_generate_ico_file() {
        let ico_bytes = generate_ico_bytes().expect("Génération du fichier ICO depuis le SVG");
        assert!(ico_bytes.len() > 1000);
        // Sauvegarde de l'icône dans la racine pour l'embarquement PE (build.rs)
        let _ = std::fs::write("icon.ico", &ico_bytes);
    }
}
