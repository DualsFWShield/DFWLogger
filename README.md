# DFWLogger — Superviseur familial transparent

Application Windows en Rust pour les parents qui veulent suivre l'usage d'un ordinateur
familial avec le **consentement des enfants** : L'application est **volontairement visible**
(barre des tâches, Gestionnaire des tâches, liste des applications). C'est une règle assumée
du projet : il n'y a **aucun mode furtif, aucun keylogger, aucune télémétrie externe**.

## Ce qui est surveillé

| Événement | Méthode (sans droits admin) |
|---|---|
| 🔓 / 🔒 Déverrouillage / verrouillage de session | Bureau interactif (`OpenInputDesktop`) — couvre aussi veille/UAC |
| 🪟 Application au premier plan | Fenêtre au premier plan + chemin de l'exécutable (2 s) |
| 📁 Fichiers ouverts | Dossier Recent (raccourcis `.lnk` résolus, 30 s) |
| ⚠️ Échecs de connexion Windows (event 4625) | Journal Security via `wevtutil` — **nécessite une relance en élévation (option)** |
Tout est écrit, **chiffré**, dans `%LOCALAPPDATA%\DFWLogger\` :

- `journal.enc` (+ `journal.N.enc` pivotés) — AES-256-GCM, un enregistrement = un nonce.
- Un en-tête en clair contient uniquement les paramètres KDF (sel Argon2id 19 456 KiB / t=2 / p=1).
- `dpapi.key` — la clé protégée par DPAPI (profil utilisateur) pour la reprise auto après reboot.
- Ce dossier est **visible** avec l'Explorateur, mais **illisible sans le mot de passe parent**.

## Interface

- Fenêtre egui : Statut (avec **temps par application**) · Événements (avec **export CSV**) ·
  Réglages (autostart registre HKCU, dashboard, rétention) · Accès réseau · Aide.
- Déverrouillage par mot de passe parent (première ouverture : création du mot de passe).
- Bouton « Verrouiller le journal maintenant » : purge la clé mémoire + le blob DPAPI.

## Nouveautés 0.3.0

- **🔔 Notifications toast Windows** : « Session verrouillée » et « Tentative de connexion
  échouée » (désactivables dans Réglages). L'identifiant d'app est enregistré en HKCU,
  envoi via WinRT/PowerShell — aucune élévation.
- **🗓 Heatmap 24 h × 7 j** (UI + dashboard) : usage par créneau horaire, reconstitué
  depuis le journal chiffré.
- **🔄 Vérification de mise à jour** : consulte l'API GitHub Releases de votre dépôt
  (`compte/repo` à renseigner dans Réglages), affiche le résultat dans Statut avec lien
  cliquable. Aucune donnée personnelle envoyée.
- **🔐 Réglages protégés** : second mot de passe « admin » (Argon2id, PHC dans la config)
  pour empêcher la désactivation de l'autostart / dashboard / collecte. Distinct du mot
  de passe du journal ; activable dans Réglages.
- **🧰 Installeur utilisateur** : `installer.cmd` copie l'exe dans
  `%LOCALAPPDATA%\Programs\DFWLogger` + raccourcis Bureau/menu Démarrer (sans élévation).

## Tray, fenêtre et terminal (sans admin)

- La croix ✕ de la fenêtre **masque dans la zone de notification** (icône bleu/œil) ; la
  surveillance continue. Clic sur l'icône : rouvrir. Menu tray : « Ouvrir » / « Quitter ».
- Export CSV déchiffré : bouton dans l'onglet Événements (sélecteur de fichier), ou depuis
  n'importe quel terminal **non-admin** :
  ```bat
  dfwlogger.exe export historique.csv
  dfwlogger.exe tail 50
  ```

## Dashboard LAN (autre appareil)

1. Déverrouillez le journal dans l'application ; l'onglet **Accès réseau** affiche
   `https://<IP>:4782` et un **token** (renouvelé à chaque déverrouillage), copiable en
   un clic sous forme de lien `?auth=<token>`.
2. Depuis un téléphone (ou tout appareil) sur le même réseau : ouvrez l'URL. Le certificat
   local est auto-signé : « Avancé → Accepter et continuer ».
3. Le serveur HTTPS écoute sur un port visible et documenté (défaut 4782), exige le token
   (comparaison à temps constant) et sert uniquement des APIs JSON en **lecture seule** :
   événements récents, compteurs et **top 10 du temps passé par application**. Le journal
   verrouillé répond « locked » : aucune donnée ne quitte l'app à froid.
4. Le pare-feu Windows demande d'autoriser le port au premier lancement.

## Compilation

```bat
winget install --id Rustlang.Rustup -e
cargo build --release
rem → target\release\dfwlogger.exe
cargo test
```

## GitHub : releases automatiques

Le repo embarque une GitHub Action (`.github/workflows/build.yml`) :

1. À chaque push sur `main` (et chaque push de tag `v*`) : `cargo test`, build release,
   empaquetage `dfwlogger-<tag>-win64.zip` (exe + README).
2. Publication automatique d'une **release versionnée** : tag `v<cargo-version>-b<build#>`
   (ex. `v0.2.0-b12`) calculé depuis Cargo.toml + le numéro de run — un push = une nouvelle
   release marquée latest. Les tags manuels `v*` publient sous leur propre nom.
3. Audit des dépendances à chaque build : `cargo deny` (licences MIT/Apache/BSD/ISC +
   vulnérabilités RustSec — `deny.toml`).
4. Le zip est aussi disponible comme artifact de run pour les pull requests.
   Le zip contient `dfwlogger.exe`, `README.md` et `installer.cmd`.

Pour publier la première version :

```bat
git remote add origin https://github.com/<vous>/DFWLogger.git
git push -u origin main
```
et la release est publiée dans la foulée (onglet Releases). Bump de version : éditez
`version = "x.y.z"` dans Cargo.toml puis poussez.

## Élévation optionnelle

Les échecs de connexion Windows sont dans le journal Security, lisible uniquement par une
session admin. Réglages → « Relancer avec élévation… » propose la boîte UAC ; l'instance
relancée enregistre l'option et relance la collecte à chaque démarrage tant que l'option
d'élevation reste active (celle-ci est réaffichée à chaque relance standard non-élévée).

## Limites connues

- Les fichiers détectés passent par le dossier Recent : un fichier ouvert directement
  via une application qui ne crée pas de raccourci n'apparaît pas.
- L'application au premier plan est échantillonnée (2 s) : un très bref alt-tab ne sera
  pas forcément journalisé.
- Le verrouillage détecte aussi UAC/écran de veille (cause affichée générique).
- Changer le mot de passe parent revient à créer un nouveau journal (l'ancien reste
  chiffré avec l'ancien sel).
