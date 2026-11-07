// Outil hors-arbre : génère icon.ico (16/32/48/64 px, BMP 32 bits) sans dépendance.
// Utilisation : rustc tools/makeicon.rs && makeicon.exe icon.ico
use std::process::ExitCode;

/// Signed distance au carré arrondi (demi-côté 0.5, rayon 0.14) — négatif = dedans.
fn rounded_square_sdf(dx: f64, dy: f64, r: f64) -> f64 {
    let qx = dx - (0.5 - r);
    let qy = dy - (0.5 - r);
    if qx > 0.0 && qy > 0.0 {
        (qx * qx + qy * qy).sqrt() - r
    } else {
        dx.max(dy) - 0.5
    }
}

fn px(x: u32, y: u32, size: u32) -> [u8; 4] {
    let s = size as f64;
    let f = (x as f64 + 0.5) / s - 0.5;
    let g = (y as f64 + 0.5) / s - 0.5;

    let edge = rounded_square_sdf(f.abs(), g.abs(), 0.14);
    if edge >= 0.0 {
        return [0, 0, 0, 0]; // coin transparent
    }

    // Œil décentré légèrement vers le haut : cercle sombre + pupille blanche.
    let dist_center = (f * f + (g + 0.06) * (g + 0.06)).sqrt() * s;
    if dist_center <= 0.17 * s {
        [255, 255, 255, 255] // pupille
    } else if dist_center <= 0.38 * s {
        [16, 22, 38, 255] // anneau
    } else {
        [79, 140, 255, 255] // fond bleu
    }
}

fn xor_and_planes(size: u32) -> Vec<u8> {
    // XOR : BGRA, lignes du bas vers le haut (bottom-up ICO/BMP).
    let mut xor = Vec::with_capacity((size * size * 4) as usize);
    for y in (0..size).rev() {
        for x in 0..size {
            let [r, g, b, a] = px(x, y, size);
            xor.extend_from_slice(&[b, g, r, a]);
        }
    }
    // AND : 1bpp, lignes paddées sur 32 bits (1 = transparent).
    let stride = ((size + 31) / 32 * 4) as usize;
    let mut mask = vec![0u8; stride * size as usize];
    for y in 0..size {
        for x in 0..size {
            if px(x, y, size)[3] == 0 {
                mask[y as usize * stride + (x / 8) as usize] |= 1 << (7 - (x % 8) as u8);
            }
        }
    }
    xor.extend_from_slice(&mask);
    xor
}

fn entry(size: u32) -> (u8, u8, Vec<u8>) {
    let png = xor_and_planes(size); // données XOR + AND
    let len = 40 + png.len() as u32;
    let mut data = Vec::with_capacity(len as usize);
    data.extend_from_slice(&40u32.to_le_bytes()); // biSize
    data.extend_from_slice(&(size as i32).to_le_bytes()); // biWidth
    data.extend_from_slice(&((size * 2) as i32).to_le_bytes()); // biHeight (XOR + AND)
    data.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    data.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    data.extend_from_slice(&0u32.to_le_bytes()); // sans compression
    data.extend_from_slice(&len.to_le_bytes());
    data.extend_from_slice(&[0u8; 16]); // fin du header
    data.extend_from_slice(&png);
    let code = if size >= 256 { 0 } else { size as u8 };
    (code, code, data)
}

fn main() -> ExitCode {
    let sizes = [64u32, 48, 32, 16];
    let n = sizes.len() as u16;

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes()); // réservé
    out.extend_from_slice(&1u16.to_le_bytes()); // type icône
    out.extend_from_slice(&n.to_le_bytes());

    let mut entries = Vec::new();
    let mut offset: u32 = 6 + 16 * n as u32;
    for &s in &sizes {
        let (w, h, data) = entry(s);
        out.push(w);
        out.push(h);
        out.push(0); // palette
        out.push(0); // réservé
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
        entries.push(data);
    }
    for e in entries {
        out.extend_from_slice(&e);
    }

    let arg = std::env::args().nth(1).unwrap_or_else(|| "icon.ico".into());
    match std::fs::write(&arg, &out) {
        Ok(()) => {
            println!("{} écrit ({} octets)", arg, out.len());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("écriture de {} impossible : {e}", arg);
            ExitCode::FAILURE
        }
    }
}
