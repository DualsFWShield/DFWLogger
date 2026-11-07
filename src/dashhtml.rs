//! Page du dashboard LAN, embarquée dans l'exécutable.

pub const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="fr">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>DFWLogger — Activité</title>
<style>
  :root {
    --bg: #0f1420; --panel: #171e30; --panel2: #1d2740; --text: #e6ecf5;
    --muted: #8fa0bd; --accent: #4f8cff; --ok: #34b37a; --warn: #e2a03f; --bad: #e05c5c;
  }
  * { box-sizing: border-box; margin: 0; padding: 0; }
  body { background: var(--bg); color: var(--text); font-family: "Segoe UI", system-ui, sans-serif; padding: 18px; }
  h1 { font-size: 19px; display: flex; align-items: center; gap: 10px; margin-bottom: 4px; }
  h1 span.dot { width: 10px; height: 10px; border-radius: 50%; background: var(--bad); display: inline-block; }
  h1 span.dot.live { background: var(--ok); box-shadow: 0 0 8px var(--ok); }
  .sub { color: var(--muted); font-size: 13px; margin-bottom: 14px; }
  .counts { display: flex; gap: 8px; flex-wrap: wrap; margin-bottom: 14px; }
  .chip { background: var(--panel2); border-radius: 10px; padding: 8px 14px; font-size: 13px; color: var(--muted); }
  .chip b { color: var(--text); font-size: 15px; margin-left: 6px; }
  .card { background: var(--panel); border-radius: 12px; padding: 14px; max-width: 900px; }
  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  th { text-align: left; color: var(--muted); font-weight: 500; padding: 6px 8px; border-bottom: 1px solid #2a3557; }
  td { padding: 7px 8px; border-bottom: 1px solid #22304f; vertical-align: top; }
  td.time { color: var(--muted); white-space: nowrap; }
  td.k { white-space: nowrap; font-weight: 600; }
  .badge { padding: 2px 8px; border-radius: 8px; font-size: 11px; }
  .b-app { background: #24406e; color: #9db8ff; }
  .b-unlock { background: #1f4a38; color: #7fe0b3; }
  .b-lock { background: #4a3721; color: #f0c585; }
  .b-file { background: #3d2f52; color: #c9a6ff; }
  .b-failed { background: #52262a; color: #ffa8a8; }
  .b-started { background: #2a3a4a; color: #9fc6dd; }
  #gate { max-width: 460px; margin: 60px auto; }
  #gate input[type=password] { width: 100%; padding: 12px; margin: 10px 0; border-radius: 10px; border: 1px solid #2a3557; background: var(--panel2); color: var(--text); font-size: 15px; }
  #gate button { width: 100%; padding: 12px; border: 0; border-radius: 10px; background: var(--accent); color: white; font-size: 15px; cursor: pointer; }
  .error { color: var(--bad); font-size: 13px; min-height: 18px; }
  .paused { opacity: 0.6; }
</style>
</head>
<body>
<div id="gate" class="card">
  <h1>🔐 DFWLogger</h1>
  <p class="sub">Entrez le token d'accès affiché dans l'application sur l'ordinateur surveillé.</p>
  <input id="tok" type="password" placeholder="Token" autocomplete="off">
  <button onclick="saveTok()">Continuer</button>
  <p class="error" id="gerr"></p>
</div>
<div id="app" style="display:none">
  <h1><span class="dot" id="dot"></span> <span id="title">DFWLogger</span></h1>
  <p class="sub" id="subtitle">Chargement…</p>
  <div class="counts" id="counts"></div>
  <div class="card" style="margin-bottom:14px">
    <table>
      <thead><tr><th style="width:150px">Date</th><th style="width:150px">Type</th><th>Événement</th></tr></thead>
      <tbody id="rows"></tbody>
    </table>
  </div>
  <div class="card">
    <h3 style="font-size:15px;margin-bottom:10px">⏱ Temps par application (cumulés depuis le déverrouillage)</h3>
    <table>
      <tbody id="times"></tbody>
    </table>
  </div>
</div>
<script>
let token = null, after = 0, paused = false, timer = null;

const urlTok = new URLSearchParams(location.search).get("auth");
if (urlTok) { localStorage.setItem("dfw_token", urlTok);
  history.replaceState(null, "", location.pathname); }

function saveTok() { token = document.getElementById("tok").value.trim();
  localStorage.setItem("dfw_token", token); boot(); }

async function boot() {
  token = localStorage.getItem("dfw_token");
  if (!token) return; // reste sur la porte
  const ok = await refresh();
  if (ok) { document.getElementById("gate").style.display = "none";
            document.getElementById("app").style.display = "";
            if (!timer) timer = setInterval(() => { if (!paused) refresh(); }, 5000); }
}

async function refresh() {
  try {
    const r = await fetch(`/api/events?after=${after}&limit=300`, { headers: { "Authorization": "Bearer " + token } });
    if (r.status === 401) { localStorage.removeItem("dfw_token"); return false; }
    const data = await r.json();
    const dot = document.getElementById("dot");
    if (data.locked) {
      dot.classList.remove("live");
      document.getElementById("subtitle").textContent = "🔒 " + (data.message || "Journal verrouillé.");
      return true;
    }
    dot.classList.add("live");
    document.getElementById("title").textContent = "DFWLogger — " + (data.computer || "");
    document.getElementById("subtitle").textContent = "Synchronisé — actualisation toutes les 5 s";
    const countsEl = document.getElementById("counts");
    countsEl.innerHTML = Object.entries(data.counts || {}).map(([k, v]) =>
      `<div class="chip">${k}<b>${v}</b></div>`).join("");
    const timesEl = document.getElementById("times");
    timesEl.innerHTML = (data.times || []).map(t =>
      `<tr><td style="width:60%"><b>${esc(t.app)}</b></td><td class="time">${fmtDur(t.secs)}</td></tr>`
    ).join("") || `<tr><td class="time">Aucun temps mesuré pour l'instant.</td></tr>`;
    const rows = document.getElementById("rows");
    for (const e of data.events.slice().reverse()) {
      const tr = document.createElement("tr");
      const klass = { "application": "b-app", "déverrouillage": "b-unlock", "verrouillage": "b-lock",
                      "fichier ouvert": "b-file", "connexion échouée": "b-failed", "démarrage": "b-started" }[e.kind] || "b-started";
      tr.innerHTML = `<td class="time">${fmt(e.ts)}</td>
                      <td class="k"><span class="badge ${klass}">${e.icon} ${e.kind}</span></td>
                      <td>${esc(e.summary)}</td>`;
      rows.appendChild(tr);
    }
    if (data.next_after > after) after = data.next_after;
    while (rows.children.length > 500) rows.deleteRow(-1);
    return true;
  } catch (err) {
    document.getElementById("subtitle").textContent = "Erreur réseau : " + err;
    return true;
  }
}

function fmt(ts) { const d = new Date(ts); return d.toLocaleString("fr-FR"); }
function fmtDur(s) {
  if (s >= 3600) return Math.floor(s / 3600) + " h " + String(Math.floor((s % 3600) / 60)).padStart(2, "0");
  if (s >= 60) return Math.floor(s / 60) + " min " + String(s % 60).padStart(2, "0") + " s";
  return s + " s";
}
function esc(s) { const d = document.createElement("div"); d.textContent = s; return d.innerHTML; }
boot();
</script>
</body>
</html>"#;
