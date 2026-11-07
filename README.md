# DFWLogger — Superviseur familial & Protection active (3 Modes de Privilège)

Application Windows en Rust combinant :
1. **Mode 1 : Sans autorisation (Standard)** : Démarrage par défaut de l'application sans aucune élévation UAC ni modification du système. Supervision familiale consentie et transparente (temps d'écran, applications, fichiers ouverts, sessions, journal chiffré AES-256-GCM, dashboard LAN HTTPS). L'app est **volontairement visible** (barre des tâches, Gestionnaire des tâches, liste des applications). Aucun mode furtif, aucun keylogger, aucune télémétrie externe.
2. **Mode 2 : Accès Admin (Big Brother SYSTEM)** : Élévation à la demande vers un agent en service Windows (`NT AUTHORITY\SYSTEM`), indomptable par un compte élève non-administrateur, capable de bloquer des sites (hosts/DNS), des applications (watchdog), des installations de logiciels (`DisableMSI=2`), d'interdire l'exécution dans les dossiers élèves (SRP) et d'imposer des quotas/plages horaires WTS.
3. **Mode 3 : Accès Noyau (Serbert Ring 0)** : Élévation optionnelle de haut niveau vers un pilote noyau Windows (`serbert.sys`) agissant directement dans le Ring 0 via `PsSetCreateProcessNotifyRoutineEx` pour interdire physiquement l'exécution des binaires non autorisés (`STATUS_ACCESS_DENIED`) avant même la création du premier thread en espace utilisateur.

---

## 🧭 Le Système des 3 Modes de Privilège

L'application est conçue pour respecter strictement le principe du moindre privilège :
- **L'application démarre TOUJOURS sans autorisation (Mode 1)** : utilisable immédiatement par n'importe quel compte utilisateur standard.
- **Passage en Accès Admin (Mode 2)** : d'un simple clic ou en ligne de commande (`--mode admin`), l'administrateur active le service Big Brother (SYSTEM).
- **Passage en Accès Noyau (Mode 3)** : depuis le mode Admin, l'administrateur peut charger le pilote noyau Serbert (`--mode kernel`) pour un verrouillage matériel au Ring 0.
- **Rétrogradation immédiate** : retour au mode Admin ou Standard en un clic, restaurant instantanément les paramètres réseau et système.

```
┌─────────────────────────┐       Élévation UAC        ┌─────────────────────────┐       Chargement Driver     ┌─────────────────────────┐
│  Mode 1 : Standard      │ ─────────────────────────▶ │  Mode 2 : Admin         │ ─────────────────────────▶ │  Mode 3 : Noyau         │
│  (Sans autorisation)    │                            │  (Big Brother SYSTEM)   │                            │  (Serbert Ring 0)       │
│                         │ ◀───────────────────────── │                         │ ◀───────────────────────── │                         │
│  • Journal AES-256-GCM  │      Rétrogradation        │  • Service SYSTEM       │      Rétrogradation        │  • PsSetCreateProcess...│
│  • Fenêtres & Sessions  │      Désinstallation       │  • Fichier hosts & DNS  │      Déchargement Ring 0   │  • STATUS_ACCESS_DENIED │
│  • Fichiers récents     │                            │  • DisableMSI & SRP     │                            │  • Zéro thread userland │
│  • Dashboard LAN HTTPS  │                            │  • Quotas & Watchdog    │                            │  • Anti-kill matériel   │
└─────────────────────────┘                            └─────────────────────────┘                            └─────────────────────────┘
```

Big Brother et Serbert sont conçus pour les administrateurs de parc informatique en milieu scolaire (écoles, collèges, lycées, centres de formation, médiathèques) qui souhaitent protéger activement les machines sans installer de solution propriétaire lourde ou opaque.

### 🏛️ Architecture de sécurité double niveau (Ring 3 + Ring 0)

DFWLogger propose une architecture de défense en profondeur à deux niveaux :

```
┌────────────────────────────────────────────────────────────────────────┐
│                        Espace Utilisateur (Ring 3)                     │
│                                                                        │
│   [ Compte Élève ] (Standard non-admin)                                │
│          │                                                             │
│          ├── Tente de lancer Discord / Launcher / Malware portable     │
│          ▼                                                             │
│   [ Service Big Brother ] (NT AUTHORITY\SYSTEM)                        │
│          ├── Synchronise les règles toutes les 15 s                     │
│          ├── Watchdog ToolHelp32 (TerminateProcess de secours)         │
│          ├── Fichier hosts (0.0.0.0 domaines bloqués)                  │
│          ├── Registre TCP/IP (DNS filtré forcé)                        │
│          ├── Registre DisableMSI=2 (Blocage installateurs .msi)        │
│          ├── Politiques Safer/SRP (Interdiction exécution AppData/Temp)│
│          └── Quotas WTS (Déconnexion de session hors créneaux)         │
│                                                                        │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │ Communication IOCTL (\\.\Serbert)
┌───────────────────────────────────▼────────────────────────────────────┐
│                        Espace Noyau (Ring 0)                           │
│                                                                        │
│   [ Pilote Serbert (serbert.sys) ]                                     │
│          ├── PsSetCreateProcessNotifyRoutineEx                         │
│          │     └── Intercepte CreateProcess au niveau noyau            │
│          │     └── Retourne STATUS_ACCESS_DENIED immédiatement         │
│          │     └── 0 cycle CPU gaspillé en Ring 3, pas de thread créé  │
│          └── Protection anti-terminaison du PID Big Brother            │
└────────────────────────────────────────────────────────────────────────┘
```

### ⚠️ Note d'intégrité & Modèle de sécurité (À lire impérativement)

> **La condition réelle pour que la protection soit impossible à contourner :**
> **Les comptes élèves / utilisateurs ne doivent pas être membres du groupe Administrateurs locaux.**
>
> C'est la norme fondamentale de sécurité dans tout parc informatique scolaire bien administré.
> 
> Grâce à l'architecture native Windows retenue :
> - **Service `NT AUTHORITY\SYSTEM`** : un utilisateur standard ne dispose pas des privilèges Windows (`SeDebugPrivilege` / jeton élevé) requis pour arrêter ou tuer un service s'exécutant sous le compte `SYSTEM`.
> - **ACL NTFS verrouillée** : le dossier de politique `%ProgramData%\DFWLogger` est configuré avec des listes de contrôle d'accès strictes (`icacls`) : *Contrôle total* pour les Administrateurs et `SYSTEM`, *Lecture seule* (`RX`) pour les Utilisateurs standard. Un élève ne peut ni altérer `big_brother.json`, ni corrompre l'état.
> - **Auto-réparation périodique** : le service vérifie et ré-applique la politique complète toutes les 15 secondes.
> - **Fonctionnement Fail-Safe sans le driver** : Serbert (Ring 0) est un module optionnel de renfort. Si le driver n'est pas déployé ou si Windows bloque les drivers non signés WHQL, le service Big Brother (Ring 3) assure seul 100 % des protections via SRP, hosts, DNS et son watchdog sans aucune interruption de service.

### 🛡️ Détail du pilote noyau Serbert (`driver/serbert/`)

Le sous-dossier `driver/serbert/` contient le code source C du pilote noyau Windows (`serbert.c`, `serbert.h`, `serbert.inf`).

- **Mécanisme Ring 0** : utilise le callback officiel `PsSetCreateProcessNotifyRoutineEx`. À chaque tentative de création de processus dans l'OS, Windows interroge Serbert. Si le chemin du binaire correspond à un exécutable interdit (insensible à la casse), le driver assigne `CreateInfo->CreationStatus = STATUS_ACCESS_DENIED`. L'exécutable ne démarre jamais (aucune allocation de mémoire de processus, aucun thread créé).
- **Protection du contrôleur** : Big Brother enregistre son PID auprès de Serbert via `IOCTL_SERBERT_REGISTER_CONTROLLER`.
- **Signature Windows (DSE)** : Windows x64 impose la signature de pilotes (Driver Signature Enforcement). Pour tester ou déployer un pilote maison en laboratoire ou environnement contrôlé :
  1. Activer le mode test : `bcdedit /set testsigning on` (puis redémarrer).
  2. Compiler le pilote avec Visual Studio + Windows Driver Kit (WDK).
  3. Signer avec un certificat de test : `SignTool sign /v /s PrivateCertStore /n "SerbertCert" serbert.sys`.
  4. Déployer et charger le service pilote :
     ```bat
     dfwlogger.exe --serbert-install --driver-path C:\Chemin\serbert.sys
     ```

### Capacités de protection active

| Protection | Niveau | Mécanisme | Action |
|---|---|---|---|
| 🛑 **Interception Processus Noyau** | **Ring 0 (Serbert)** | `PsSetCreateProcessNotifyRoutineEx` | Refuse le démarrage du processus avec `STATUS_ACCESS_DENIED` au niveau noyau avant tout thread Ring 3. |
| 🌐 **Filtrage de sites web** | Ring 3 (Big Brother) | Fichier `hosts` Windows (`System32\drivers\etc\hosts`) | Redirection immédiate des domaines bloqués vers `0.0.0.0` (section `# >>> DFWLOGGER-BIGBROTHER-BLOCK >>>` auto-réparée, vidage du cache DNS). |
| 🛡️ **DNS filtré sécurisé** | Ring 3 (Big Brother) | Paramètres TCP/IP registre `HKLM\...\Interfaces` | Force un résolveur DNS filtrant (CleanBrowsing, Cloudflare Family 1.1.1.3, AdGuard Family ou personnalisé). Valeurs d'origine sauvegardées pour restauration. |
| 🛑 **Watchdog d'applications** | Ring 3 (Big Brother) | Instantané de processus ToolHelp32 | Énumération continue et arrêt immédiat (`TerminateProcess`) des `.exe` interdits en cas d'absence du pilote Ring 0. |
| 🚫 **Blocage d'installations MSI** | Ring 3 (Big Brother) | Politique machine `DisableMSI=2` | Désactive totalement l'exécutable Windows Installer pour empêcher l'installation de programmes `.msi`. |
| 📁 **Interdiction d'exécuter depuis les dossiers élèves** | Ring 3 (Big Brother) | Règles Safer / Software Restriction Policies (SRP) | Interdit le lancement de tout binaire `.exe` depuis les répertoires inscriptibles sans droits admin (`Downloads`, `Desktop`, `AppData\Local\Temp`). Un élève ne peut pas lancer un binaire téléchargé depuis son navigateur. |
| ⏱️ **Quotas & Plages horaires** | Ring 3 (Big Brother) | Énumération de session WTS (`WTSEnumerateSessionsW`) | Déconnexion automatique de la session console active (`WTSDisconnectSession`) en cas de dépassement du quota journalier ou hors des créneaux scolaires autorisés (ex. Lun-Ven 08:00 - 18:00). |
| 🪂 **Politique Fail-Open** | Ring 3 & 0 | Absence de `big_brother.json` = aucune restriction | Si la politique est supprimée ou désactivée, le système ne bloque rien, évitant tout risque de verrouillage accidentel. |

---

## 🧰 Déploiement et Désinstallation propre

Le Mode École peut être installé, piloté et désinstallé de manière totalement propre soit par interface graphique, soit par scripts batch automatisables (GPO, Microsoft Intune, WAPT, clé USB).

### 1. Via l'interface graphique (onglet « 🛡️ 3 Modes & Protection »)
- **Barre supérieure de progression** : visualise en temps réel `[⚪ 1. Sans autorisation] ➔ [🛡️ 2. Accès Admin] ➔ [⚡ 3. Accès Noyau]`.
- **Passage en Mode Admin (Big Brother)** : bouton « 🛡️ Passer en Accès Admin » (déclenche l'élévation UAC, installe et démarre le service SYSTEM).
- **Passage en Mode Noyau (Serbert)** : bouton « ⚡ Passer en Accès Noyau » (enregistre et charge le pilote Ring 0).
- **Rétrogradation propre** : boutons « ↩️ Rétrograder » pour revenir au mode inférieur en un clic.

### 2. Via les scripts fournis
- **Installation / Déploiement Big Brother** :
  ```bat
  install_big_brother.cmd
  ```
  *(Déploie l'exécutable dans `%ProgramData%\DFWLogger`, enregistre le service SYSTEM `DFWLoggerBigBrother` et applique les permissions NTFS strictes).*
- **Désinstallation propre complète** :
  ```bat
  uninstall_big_brother.cmd
  ```
  *(Arrête et supprime le service Windows `DFWLoggerBigBrother`, décharge le pilote `Serbert` s'il est présent, retire la section du fichier `hosts`, restaure le DNS d'origine, supprime `DisableMSI` et les règles Safer/SRP, vide le cache DNS et propose la purge de `%ProgramData%\DFWLogger`).*

### 3. En ligne de commande (pour scripts d'automatisation)
```bat
rem Basculement direct entre les 3 Modes :
dfwlogger.exe --mode standard                 rem Rétrograde en Mode 1 (Sans autorisation, nettoie Admin et Noyau)
dfwlogger.exe --mode admin                    rem Élève et active le Mode 2 (Service SYSTEM Big Brother)
dfwlogger.exe --mode kernel                   rem Élève et active le Mode 3 (Service SYSTEM + Pilote Ring 0 Serbert)
dfwlogger.exe --status                        rem Affiche le mode actif et les détails du système

rem Commandes de gestion avancée :
dfwlogger.exe --install-bigbrother --quiet    rem Installe et démarre le service SYSTEM
dfwlogger.exe --uninstall-bigbrother --quiet  rem Désinstalle et retire toutes les restrictions
dfwlogger.exe --bigbrother-apply              rem Force l'application immédiate de big_brother.json
dfwlogger.exe --serbert-status                rem Interroge l'état du driver noyau \\.\Serbert
dfwlogger.exe --serbert-install
dfwlogger.exe --serbert-uninstall
```

---

## Ce qui est surveillé (Mode Familial)

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
- **📱 Dashboard LAN v2** : QR code (généré en UI via le crate `qrcode`), authentification
  multi-méthodes (token + code 6 maj/min valable 10 min + mot de passe admin Argon2id), APIs
  `/api/status`, `/api/config`, `/api/bigbrother`, `/api/dns`.
- **🔍 Journal DNS** : outil de diagnostic réseau « type Wireshark » — proxy DNS local
  (`127.0.0.1:53`), journalisation des requêtes DNS uniquement (aucun autre protocole capturé).
- **🪟 Icône de fenêtre** : `eframe::NativeOptions` fixe désormais l'icône dérivée de l'icône
  tray au lancement de la fenêtre (corrige l'affichage par défaut « egui »).

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
   `https://<IP>:4782` et un **token** (renouvelé à chaque déverrouillage), avec **QR code**
   à scanner, copiable en un clic sous forme de lien `?auth=<token>`.
2. Depuis un téléphone (ou tout autre appareil) sur le même réseau : ouvrez l'URL. Le certificat
   local est auto-signé : « Avancé → Accepter et continuer ».
3. Le serveur HTTPS écoute sur un port visible et documenté (défaut 4782), lancé en thread
   background (aucun terminal, aucun processus visible dédié) et sert uniquement des APIs JSON
   en **lecture seule**.
4. **Authentification multi-méthodes** (les trois valables, selon le cas) :
   - **Token** : secret aléatoire (base64, 32 octets) régénéré à chaque déverrouillage — via QR
     code ou lien `?auth=<token>`.
   - **Code à 6 caractères** (`majuscules/minuscules/chiffres`, sans ambiguïté, ≥ 1 maj + 1 min) —
     valable **10 minutes** (`DASHBOARD_CODE_TTL_SECS = 600 s`) ; généré à chaque déverrouillage.
   - **Mot de passe admin** : le second mot de passe « admin » (Argon2id PHC) — qui, lui, verrouille
     les paramètres du logiciel (Réglages → « Protection admin »).
   Toutes les méthodes passent par une **comparaison à temps constant** (`const_time_eq`).
5. Le serveur HTTPS exige une auth (401 sinon) et sert uniquement des APIs JSON en lecture seule :
   événements récents, compteurs, **top 10 du temps passé par application**, heatmap et
   **état complet Big Brother**. Le journal verrouillé répond « locked » : aucune donnée ne quitte
   l'app à froid.
6. Le pare-feu Windows demande d'autoriser le port au premier lancement.

### APIs du serveur local

| Endpoint | Contenu |
|---|---|
| `GET /api/status` | État complet : journal (locked/monitoring), dashboard, DNS, compteurs, politique Big Brother. |
| `GET /api/config` | Paramètres (sans secrets : hash PHC admin exclu). |
| `GET /api/bigbrother` | Politique Big Brother : domaines, processus bloqués, quotas, DNS filter, état noyau. |
| `GET /api/dns` | Journal des requêtes DNS interceptées. |
| `GET /api/events?after=<id>` | Événements récents (JSON) — « locked » si journal verrouillé. |

## Outil de diagnostic réseau : journal DNS (type Wireshark — protocole DNS uniquement)

L'onglet **Accès réseau** intègre un outil de diagnostic réseau « type Wireshark », mais **DNS
uniquement** : aucun autre protocole n'est capturé.

- Fonctionnement : proxy **UDP local sur `127.0.0.1:53`** (forwarding uniquement, aucune donnée
  modifiée). Les requêtes DNS sont redirigées vers le résolveur configuré (backup Big Brother ou
  DNS public par défaut), la réponse est relayée au client, puis la requête est journalisée
  (id, horodatage, nom interrogé, type).
- Activation : case à cocher **« Journaliser les requêtes DNS résolues »** dans l'onglet
  **Accès réseau** (persisted dans les Réglages).
- Consultation : liste temps réel des dernières requêtes (`/api/dns`, 30 dernières, mémoire
  plafonnée à 500 entrées).
- Remarque : l'écoute sur `127.0.0.1:53` peut nécessiter les droits administrateur — sans journal
  DNS activé, aucune redirection n'est effectuée.

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
   Le zip contient `dfwlogger.exe`, `README.md`, `installer.cmd`, `install_big_brother.cmd` et `uninstall_big_brother.cmd`.

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
