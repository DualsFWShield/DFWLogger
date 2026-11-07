//! Dashboard LAN : HTTPS (certificat auto-signé local), authentification par token,
//! API JSON en lecture seule. Le token change à chaque déverrouillage du journal.

use crate::shared::Shared;
use base64::Engine;
use serde_json::json;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub fn spawn_dashboard(shared: Arc<Shared>) {
    std::thread::spawn(move || {
        let (enabled, port, dir) = {
            let cfg = shared.config.lock().unwrap();
            let dir = shared.store.lock().unwrap().dir.clone();
            (cfg.dashboard_enabled, cfg.dashboard_port, dir)
        };
        if !enabled {
            let mut st = shared.status.lock().unwrap();
            st.dashboard_listening = false;
            st.dashboard_error = Some("désactivé".to_string());
            return;
        }
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                let mut st = shared.status.lock().unwrap();
                st.dashboard_error = Some(format!("runtime : {e}"));
                return;
            }
        };
        rt.block_on(run(shared, port, dir));
    });
}

async fn run(shared: Arc<Shared>, port: u16, dir: std::path::PathBuf) {
    let tls_config = match load_or_create_tls(&dir) {
        Ok(c) => c,
        Err(e) => {
            let mut st = shared.status.lock().unwrap();
            st.dashboard_listening = false;
            st.dashboard_error = Some(format!("TLS : {e}"));
            return;
        }
    };
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls_config));
    let listener = match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
        Ok(l) => l,
        Err(e) => {
            let mut st = shared.status.lock().unwrap();
            st.dashboard_listening = false;
            st.dashboard_error = Some(format!("port {port} : {e} (déjà utilisé ?)"));
            return;
        }
    };
    {
        let mut st = shared.status.lock().unwrap();
        st.dashboard_listening = true;
        st.dashboard_error = None;
    }
    eprintln!("dashboard en écoute : https://{}:{port}", crate::win::primary_lan_ip());

    loop {
        let (stream, _peer) = match listener.accept().await {
            Ok(x) => x,
            Err(_) => continue,
        };
        let acceptor = acceptor.clone();
        let shared = shared.clone();
        tokio::spawn(async move {
            if let Ok(tls_stream) = acceptor.accept(stream).await {
                let _ = handle_conn(shared, tls_stream).await;
            }
        });
    }
}

async fn handle_conn(
    shared: Arc<Shared>,
    mut stream: tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
) -> std::io::Result<()> {
    use tokio::io::AsyncReadExt;

    // Lit l'en-tête HTTP (GET seulement).
    let mut request = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        if request.len() >= 4 && request[request.len() - 4..] == *b"\r\n\r\n" {
            break;
        }
        if request.len() > 8192 {
            break;
        }
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..n]);
    }
    let text = String::from_utf8_lossy(&request);
    let mut lines = text.lines();
    let mut first_line = lines.next().unwrap_or("").trim().to_string();
    if first_line.is_empty() {
        first_line = "<empty>".into();
    }
    // Extraction basics.
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    if method != "GET" {
        return respond(&mut stream, 405, "text/plain", "method not allowed").await;
    }
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p, q),
        None => (target, ""),
    };

    let is_api = path.starts_with("/api/");
    let token = query_param(query, "auth").or_else(|| bearer_token(&text));

    if is_api {
        let expected = shared.dashboard_token.lock().unwrap().clone();
        let ok = match (&token, &expected) {
            (Some(given), Some(exp)) if const_time_eq(given, exp) => true,
            _ => false,
        };
        if !ok {
            return respond(&mut stream, 401, "text/plain", "token absent ou invalide").await;
        }
    }

    if path == "/" || path == "/index.html" {
        return respond(&mut stream, 200, "text/html; charset=utf-8", crate::dashhtml::INDEX_HTML)
            .await;
    }
    if path == "/api/events" {
        let after: u64 = query_param(query, "after").and_then(|v| v.parse().ok()).unwrap_or(0);
        let (monitoring, body) = api_events(&shared, after);
        let status = if monitoring { 200 } else { 200 };
        return respond(&mut stream, status, "application/json", &body).await;
    }
    respond(&mut stream, 404, "text/plain", "not found").await
}

fn api_events(shared: &Arc<Shared>, after: u64) -> (bool, String) {
    let monitoring = shared.monitoring.load(Ordering::Relaxed);
    if !monitoring {
        return (
            false,
            json!({ "locked": true, "message": "Journal verrouillé ou monté en attente." })
                .to_string(),
        );
    }
    let (events, counts) = {
        let ev = shared.events.lock().unwrap();
        let filtered: Vec<&crate::model::Event> = ev.iter().filter(|e| e.id > after).collect();
        let counts = shared.counts.lock().unwrap().clone();
        let ser: Vec<serde_json::Value> = filtered
            .iter()
            .map(|e| {
                json!({
                    "id": e.id, "ts": e.ts, "kind": e.kind.label(),
                    "icon": e.kind.icon(), "summary": e.kind.summary()
                })
            })
            .collect();
        (ser, counts)
    };
    let next_after = events.last().and_then(|e| e.get("id")).and_then(|v| v.as_u64()).unwrap_or(after);
    let times: Vec<serde_json::Value> = shared
        .top_apps(10)
        .into_iter()
        .map(|(app, secs)| json!({ "app": app, "secs": secs }))
        .collect();
    let body = json!({
        "locked": false,
        "events": events,
        "next_after": next_after,
        "counts": counts,
        "times": times,
        "computer": crate::win::computer_name(),
    });
    (true, body.to_string())
}

async fn respond<W>(stream: &mut W, code: u16, ctype: &str, body: &str) -> std::io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;
    let reason = match code {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let resp = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{}",
        code,
        reason,
        ctype,
        body.len(),
        body
    );
    let _ = stream.write_all(resp.as_bytes()).await;
    let _ = stream.flush().await;
    let _ = stream.shutdown().await;
    Ok(())
}

fn query_param<'a>(query: &'a str, key: &str) -> Option<String> {
    let mut found = None;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == key {
            found = Some(v.to_string());
        }
    }
    found
}

fn bearer_token(headers_text: &str) -> Option<String> {
    for line in headers_text.lines() {
        if let Some(rest) = line.strip_prefix("Authorization: Bearer ") {
            return Some(rest.trim().to_string());
        }
        // headers HTTPS en minuscule selon client
        if let Some(rest) = line.strip_prefix("authorization: Bearer ") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// Comparaison à temps constant (moins de fuite par temporisation).
fn const_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn load_or_create_tls(dir: &Path) -> Result<rustls::ServerConfig, String> {
    let meta_path = dir.join("dashboard-tls.json");
    let (cert_der, key_der): (Vec<u8>, Vec<u8>) = if let Ok(text) = std::fs::read_to_string(&meta_path) {
        let parsed: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("tls.json : {e}"))?;
        let cert = parsed
            .get("cert")
            .and_then(|v| v.as_str())
            .ok_or("tls.json : cert manquant")?;
        let key = parsed
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or("tls.json : key manquante")?;
        (
            base64::engine::general_purpose::STANDARD
                .decode(cert)
                .map_err(|e| e.to_string())?,
            base64::engine::general_purpose::STANDARD
                .decode(key)
                .map_err(|e| e.to_string())?,
        )
    } else {
        let cert = rcgen::generate_simple_self_signed(vec![
            "localhost".to_string(),
            crate::win::computer_name().to_lowercase(),
        ])
        .map_err(|e| format!("génération certificat : {e}"))?;
        let cert_der = cert.serialize_der().map_err(|e| format!("sérialisation certificat : {e}"))?;
        let key_der = cert.serialize_private_key_der();
        #[derive(serde::Serialize)]
        struct TlsMeta<'a> {
            cert: &'a str,
            key: &'a str,
        }
        let enc = &base64::engine::general_purpose::STANDARD;
        let meta = TlsMeta {
            cert: &enc.encode(&cert_der),
            key: &enc.encode(&key_der),
        };
        std::fs::write(&meta_path, serde_json::to_string(&meta).unwrap())
            .map_err(|e| e.to_string())?;
        (cert_der, key_der)
    };

    let certs = vec![rustls::pki_types::CertificateDer::from(cert_der)];
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(
        key_der,
    ));
    rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| format!("config TLS : {e}"))
}
