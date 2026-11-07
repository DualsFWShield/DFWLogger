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
        // Authentification multi-méthodes : token actuel | code 6 maj/min | mot de passe admin.
        let ok = token.as_deref().map(|t| shared.verify_dashboard_credential(t)).unwrap_or(false);
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
    if path == "/api/status" {
        let body = api_status(&shared).to_string();
        return respond(&mut stream, 200, "application/json", &body).await;
    }
    if path == "/api/config" {
        let body = api_config(&shared).to_string();
        return respond(&mut stream, 200, "application/json", &body).await;
    }
    if path == "/api/bigbrother" {
        let body = api_bigbrother(&shared).to_string();
        return respond(&mut stream, 200, "application/json", &body).await;
    }
    if path == "/api/dns" {
        let body = api_dns(&shared).to_string();
        return respond(&mut stream, 200, "application/json", &body).await;
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
    let heatmap: Vec<serde_json::Value> = {
        let map = shared.heatmap.lock().unwrap();
        crate::shared::heatmap_rows(&map, 7)
    }
    .into_iter()
    .map(|(day, hour, secs)| json!({ "day": day, "hour": hour, "secs": secs }))
    .collect();
    let body = json!({
        "locked": false,
        "events": events,
        "next_after": next_after,
        "counts": counts,
        "times": times,
        "heatmap": heatmap,
        "computer": crate::win::computer_name(),
    });
    (true, body.to_string())
}

/// Snapshot d'état complet du superviseur, servi depuis le serveur local (LAN).
fn api_status(shared: &Arc<Shared>) -> serde_json::Value {
    let st = shared.status.lock().unwrap();
    let cfg = shared.config.lock().unwrap();
    let big_brother_cfg = crate::big_brother::BigBrotherConfig::load();
    let big_brother_state = crate::big_brother::BigBrotherState::load();
    json!({
        "locked": !shared.monitoring.load(Ordering::Relaxed),
        "monitoring": shared.monitoring.load(Ordering::Relaxed),
        "elevated": st.elevated,
        "session_ok": st.session_ok,
        "foreground_ok": st.foreground_ok,
        "recent_ok": st.recent_ok,
        "security_ok": st.security_ok,
        "security_note": st.security_note,
        "computer": crate::win::computer_name(),
        "version": env!("CARGO_PKG_VERSION"),
        "dashboard": {
            "enabled": cfg.dashboard_enabled,
            "port": cfg.dashboard_port,
            "listening": st.dashboard_listening,
            "error": st.dashboard_error,
        },
        "dns": {
            "listening": st.dns_listening,
            "error": st.dns_error,
        },
        "counts": shared.counts.lock().unwrap().clone(),
        "big_brother": {
            "enabled": big_brother_cfg.enabled,
            "domains_count": big_brother_cfg.domains.len(),
            "blocked_processes_count": big_brother_cfg.blocked_processes.len(),
            "block_msi": big_brother_cfg.block_msi,
            "deny_paths_count": big_brother_cfg.deny_paths.len(),
            "daily_minutes": big_brother_cfg.daily_minutes,
            "allowed_windows": big_brother_cfg.allowed_windows.len(),
            "dns_filter": big_brother_cfg.dns_filter,
            "kernel_driver_active": big_brother_state.kernel_driver_active,
        },
    })
}

/// Snapshot des réglages (sans secrets : le hash PHC admin n'est jamais renvoyé).
fn api_config(shared: &Arc<Shared>) -> serde_json::Value {
    let cfg = shared.config.lock().unwrap();
    json!({
        "security_log": cfg.security_log,
        "dashboard_enabled": cfg.dashboard_enabled,
        "dashboard_port": cfg.dashboard_port,
        "recent_poll_secs": cfg.recent_poll_secs,
        "security_poll_secs": cfg.security_poll_secs,
        "rotate_bytes": cfg.rotate_bytes,
        "keep_files": cfg.keep_files,
        "autostart": cfg.autostart,
        "notify_lock": cfg.notify_lock,
        "notify_failed_logon": cfg.notify_failed_logon,
        "github_repo": cfg.github_repo,
        "admin_protected": cfg.admin_phc.is_some(),
        "dns_log": cfg.dns_log,
    })
}

/// Snapshot de la politique Big Brother (Mode École) pour le dashboard.
fn api_bigbrother(_shared: &Arc<Shared>) -> serde_json::Value {
    let cfg = crate::big_brother::BigBrotherConfig::load();
    let state = crate::big_brother::BigBrotherState::load();
    json!({
        "enabled": cfg.enabled,
        "domains": cfg.domains,
        "blocked_processes": cfg.blocked_processes,
        "block_msi": cfg.block_msi,
        "deny_paths": cfg.deny_paths,
        "daily_minutes": cfg.daily_minutes,
        "allowed_windows": cfg.allowed_windows,
        "dns_filter": cfg.dns_filter,
        "kernel_driver_active": state.kernel_driver_active,
        "kernel_blocks_count": state.kernel_blocks_count,
        "active_secs": state.active_secs,
        "quota_lock": state.quota_lock,
    })
}

/// Journal des requêtes DNS résolues (dernier état, pour le dashboard).
fn api_dns(shared: &Arc<Shared>) -> serde_json::Value {
    let queries = shared.dns_queries.lock().unwrap().clone();
    json!({
        "count": queries.len(),
        "queries": queries.iter().map(|q| json!({
            "id": q.id, "ts": q.ts, "name": q.name, "qtype": q.qtype,
        })).collect::<Vec<_>>(),
    })
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

// ------------------------------------------------------------------ journal DNS

/// Démarre le proxy DNS local : écoute sur `127.0.0.1:53`, redirige les requêtes
/// vers le résolveur (upstream Big Brother sauvegardé ou 8.8.8.8) et journalise
/// les requêtes interceptées. Outil de diagnostic réseau — protocole DNS uniquement.
pub fn spawn_dns_logger(shared: Arc<Shared>) {
    std::thread::Builder::new()
        .name("dfwlogger-dns".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let mut st = shared.status.lock().unwrap();
                    st.dns_error = Some(format!("runtime : {e}"));
                    return;
                }
            };
            rt.block_on(run_dns_loop(shared));
        })
        .ok();
}

/// Active / désactive le journal DNS (cocher de l'onglet Accès réseau).
pub fn set_dns_logging(shared: &Arc<Shared>, on: bool) {
    {
        let mut cfg = shared.config.lock().unwrap();
        cfg.dns_log = on;
    }
    if on {
        shared.dns_stop.store(false, Ordering::Relaxed);
        spawn_dns_logger(shared.clone());
    } else {
        shared.dns_stop.store(true, Ordering::Relaxed);
    }
}

async fn run_dns_loop(shared: Arc<Shared>) {
    if shared.dns_stop.load(Ordering::Relaxed) {
        return;
    }
    {
        let cfg = shared.config.lock().unwrap();
        if !cfg.dns_log {
            return;
        }
    }
    let listener = match tokio::net::UdpSocket::bind(("127.0.0.1", 53)).await {
        Ok(s) => s,
        Err(e) => {
            let mut st = shared.status.lock().unwrap();
            st.dns_listening = false;
            st.dns_error = Some(format!("port 53 : {e} (droits admin requis ?)"));
            return;
        }
    };
    {
        let mut st = shared.status.lock().unwrap();
        st.dns_listening = true;
        st.dns_error = None;
    }
    let mut buf = vec![0u8; 1500];
    while !shared.dns_stop.load(Ordering::Relaxed) {
        {
            let cfg = shared.config.lock().unwrap();
            if !cfg.dns_log {
                break;
            }
        }
        let (n, peer) = match tokio::time::timeout(
            std::time::Duration::from_millis(1000),
            listener.recv_from(&mut buf),
        )
        .await
        {
            Ok(Ok(x)) => x,
            _ => continue,
        };
        handle_dns(&shared, &listener, &buf[..n], peer).await;
    }
    let mut st = shared.status.lock().unwrap();
    st.dns_listening = false;
}

/// Intercepte une requête DNS, la journalise immédiatement, la transmet au résolveur amont,
/// puis renvoie la réponse exacte au client.
async fn handle_dns(
    shared: &Arc<Shared>,
    listener: &tokio::net::UdpSocket,
    msg: &[u8],
    peer: std::net::SocketAddr,
) {
    let Some((id, name, qtype)) = parse_dns_query(msg) else {
        return;
    };

    // Journalisation immédiate dès la réception de la requête
    shared.push_dns_query(crate::model::DnsQuery {
        id,
        ts: chrono::Utc::now().to_rfc3339(),
        name,
        qtype,
    });

    let state = crate::big_brother::BigBrotherState::load();
    let (host, port) = upstream_addr(&state);
    let Ok(up) = tokio::net::UdpSocket::bind(("0.0.0.0", 0)).await else {
        return;
    };
    if up.connect((host.as_str(), port)).await.is_err() {
        return;
    }
    if tokio::time::timeout(std::time::Duration::from_secs(3), up.send(msg)).await.is_err() {
        return;
    }
    let mut out = vec![0u8; 1500];
    if let Ok(Ok(recv_len)) = tokio::time::timeout(std::time::Duration::from_secs(3), up.recv(&mut out)).await {
        if recv_len > 0 {
            let _ = listener.send_to(&out[..recv_len], peer).await;
        }
    }
}

/// Décode une requête DNS : (id, nom interrogé, type) — labels avec pointeur compression supportés.
fn parse_dns_query(msg: &[u8]) -> Option<(u64, String, String)> {
    if msg.len() < 12 {
        return None;
    }
    let id = u16::from_be_bytes([msg[0], msg[1]]) as u64;
    let qdcount = u16::from_be_bytes([msg[4], msg[5]]) as usize;
    if qdcount == 0 {
        return None;
    }
    let mut labels: Vec<String> = Vec::new();
    let mut pos = 12usize;
    let mut jumped = false;
    let mut end_offset: Option<usize> = None;
    let mut jumps = 0usize;
    loop {
        if pos >= msg.len() {
            return None;
        }
        let len = msg[pos] as usize;
        if len == 0 {
            pos += 1;
            break;
        }
        if len & 0xC0 == 0xC0 {
            if jumps > 0 {
                return None;
            }
            jumps += 1;
            let ptr = (((len & 0x3F) as usize) << 8) | msg[pos + 1] as usize;
            if !jumped {
                end_offset = Some(pos + 2);
                jumped = true;
            }
            pos = ptr;
            continue;
        }
        if len & 0xC0 != 0 {
            return None;
        }
        let start = pos + 1;
        if start + len > msg.len() {
            return None;
        }
        labels.push(String::from_utf8_lossy(&msg[start..start + len]).to_string());
        pos = start + len + 1;
    }
    let after = end_offset.unwrap_or(pos);
    if after + 4 > msg.len() {
        return None;
    }
    let qtype = u16::from_be_bytes([msg[after], msg[after + 1]]);
    let mut name = labels.join(".");
    while name.ends_with('.') {
        name.pop();
    }
    let qtype_str: String = match qtype {
        1 => "A".to_string(),
        5 => "CNAME".to_string(),
        6 => "SOA".to_string(),
        15 => "MX".to_string(),
        16 => "TXT".to_string(),
        28 => "AAAA".to_string(),
        33 => "PTR".to_string(),
        47 => "NS".to_string(),
        _ => format!("type{qtype}"),
    };
    Some((id, name, qtype_str))
}

/// Résolveur upstream pour le proxy DNS : sauvegarde Big Brother (interfaces),
/// sinon le DNS public par défaut.
fn upstream_addr(state: &crate::big_brother::BigBrotherState) -> (String, u16) {
    let mut server = String::from("8.8.8.8");
    for v in state.dns_backup.values() {
        let t = v.trim();
        if !t.is_empty() && t != "__DHCP__" {
            server = t.to_string();
            break;
        }
    }
    let server = server.trim();
    if let Some((host, port)) = server.rsplit_once(':') {
        let port = port.parse::<u16>().unwrap_or(53);
        (host.to_string(), port)
    } else {
        (server.to_string(), 53)
    }
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
