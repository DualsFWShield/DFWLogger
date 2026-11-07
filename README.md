# DFWLogger — Superviseur Familial & Gardien de Parc Windows

<div align="center">

![GitHub Actions Workflow Status](https://img.shields.io/badge/CI-passing-brightgreen?style=flat-square&logo=githubactions)
![Version](https://img.shields.io/badge/version-0.3.0-blue?style=flat-square)
![Rust Version](https://img.shields.io/badge/rust-1.85%2B-orange?style=flat-square&logo=rust)
![Platform](https://img.shields.io/badge/platform-Windows%2010%20%7C%2011%20x64-0078D6?style=flat-square&logo=windows)
![Security Audit](https://img.shields.io/badge/cargo--deny-verified-success?style=flat-square)
![License](https://img.shields.io/badge/license-MIT-green?style=flat-square)

**Supervision familiale transparente et protection active de parc informatique (écoles, médiathèques, parcs publics).**  
Développé exclusivement en **Rust natif** pour Windows x64. Zéro télémétrie externe, chiffrement militaire local, binaire autonome haute performance.

[Fonctionnalités](#-fonctionnalités-principales) •
[Les 3 Niveaux de Privilège](#-les-3-niveaux-de-privilège) •
[Architecture Ring 3 & Ring 0](#-architecture-technique--défense-en-profondeur) •
[Dashboard LAN](#-dashboard-lan-https--qr-code) •
[Déploiement](#-installation--déploiement) •
[Compilation](#-compilation--développement)

</div>

---

## 🌟 Philosophie & Engagements

Contrairement aux solutions de surveillance opaques ou aux logiciels espions (stalkerware / keyloggers) :
- **100 % Transparent & Éthique** : L'application est **volontairement visible** (icône SVG moderne dans la barre des tâches et la zone de notification, entrée dans le Gestionnaire des tâches, liste des programmes installés).
- **Zéro fuite dans le Cloud** : Aucune donnée, aucun identifiant, aucune statistique n'est envoyée vers un serveur tiers. L'intégralité du traitement et du stockage reste sur la machine locale et le réseau local (LAN).
- **Chiffrement de bout en bout** : Le journal d'activité est scellé par chiffrement authentifié **AES-256-GCM** avec dérivation de clé **Argon2id** (19 MiB RAM, 2 itérations, 1 thread). Seul le mot de passe maître permet le déchiffrement.
- **Principe du Moindre Privilège** : L'application démarre **toujours sans aucune élévation de privilèges** (compte utilisateur standard). L'élévation en mode Administrateur (SYSTEM) ou Noyau (Ring 0) est strictement optionnelle et réversible en un clic.

---

## 🧭 Les 3 Niveaux de Privilège

DFWLogger intègre un modèle modulaire unique à 3 paliers, configurable via l'interface graphique ou par ligne de commande :

```
┌────────────────────────────────┐         Élévation UAC         ┌────────────────────────────────┐       Chargement Pilote       ┌────────────────────────────────┐
│   Palier 1 : Standard          │  ──────────────────────────▶  │   Palier 2 : Administrateur    │  ──────────────────────────▶  │   Palier 3 : Noyau             │
│   (Sans autorisation)          │                               │   (Big Brother SYSTEM)         │                               │   (Serbert Ring 0)             │
│                                │  ◀──────────────────────────  │                                │  ◀──────────────────────────  │                                │
│   • Démarrage immédiat sans UAC│       Rétrogradation          │   • Service Windows SYSTEM     │       Rétrogradation          │   • Driver serbert.sys (WDM)   │
│   • Journal chiffré AES-GCM    │       Désinstallation         │   • Fichier hosts & DNS forcé  │       Déchargement Ring 0     │   • PsSetCreateProcess...      │
│   • Fenêtres & Sessions        │                               │   • DisableMSI & Règles SRP    │                               │   • STATUS_ACCESS_DENIED       │
│   • Fichiers récents (.lnk)    │                               │   • Quotas & Watchdog Ring 3   │                               │   • Zéro thread userland       │
│   • Dashboard LAN HTTPS + QR   │                               │   • Inattaquable par non-admin │                               │   • Protection anti-kill       │
└────────────────────────────────┘                               └────────────────────────────────┘                               └────────────────────────────────┘
```

### 1. Palier 1 — Mode Standard (Sans autorisation)
- **Cible** : Utilisation familiale, contrôle parental consenti, productivité personnelle.
- **Fonctionnement** : Démarre sans aucune boîte de dialogue UAC. Écoute les événements du bureau interactif (`OpenInputDesktop`), les applications au premier plan et les fichiers récents.
- **Sécurité** : Données stockées dans `%LOCALAPPDATA%\DFWLogger`, chiffrées avec mot de passe maître + clé de secours DPAPI locale.

### 2. Palier 2 — Mode Administrateur (Big Brother SYSTEM)
- **Cible** : Salles de classe, collèges, lycées, centres de formation, médiathèques.
- **Fonctionnement** : S'exécute sous le compte `NT AUTHORITY\SYSTEM` en service Windows natif (`DFWLoggerBigBrother`). Inarrêtable par un compte élève standard (dépourvu de `SeDebugPrivilege`).
- **Capacités** :
  - **Filtrage Web DNS & Hosts** : Redirection des domaines interdits vers `0.0.0.0` et forçage de résolveurs DNS sécurisés (CleanBrowsing, Cloudflare Family, AdGuard).
  - **Anti-Installations & SRP** : Verrouillage `DisableMSI=2` et Software Restriction Policies interdisant le lancement d'exécutables depuis `Downloads`, `AppData` et `Temp`.
  - **Quotas & Horaires WTS** : Déconnexion automatique de la session (`WTSDisconnectSession`) hors des créneaux autorisés.

### 3. Palier 3 — Mode Noyau (Serbert Ring 0)
- **Cible** : Protection matérielle absolue contre les contournements avancés.
- **Fonctionnement** : Pilote noyau Windows 64 bits (`serbert.sys`) s'enregistrant via le callback officiel `PsSetCreateProcessNotifyRoutineEx`.
- **Efficacité** : Lorsqu'un binaire interdit est invoqué, le noyau assigne immédiatement `CreateInfo->CreationStatus = STATUS_ACCESS_DENIED`. Aucun thread en espace utilisateur n'est créé, 0 cycle CPU n'est gaspillé, et aucun hook Ring 3 n'est nécessaire.

---

## 🏛️ Architecture Technique & Défense en Profondeur

```
┌────────────────────────────────────────────────────────────────────────┐
│                      Espace Utilisateur (Ring 3)                       │
│                                                                        │
│   [ Compte Utilisateur / Élève ] (Standard non-admin)                  │
│          │                                                             │
│          ├── Tente de lancer un exécutable portable / jeu / VPN        │
│          ▼                                                             │
│   [ Service Big Brother ] (NT AUTHORITY\SYSTEM)                        │
│          ├── Surveillance continue & auto-réparation (15 s)            │
│          ├── Watchdog ToolHelp32 (Arrêt d'urgence Ring 3)              │
│          ├── Fichier hosts (%SystemRoot%\System32\drivers\etc\hosts)   │
│          ├── Registre HKLM (DNS sécurisé & DisableMSI)                 │
│          ├── Politiques Safer/SRP (Interdiction répertoires élèves)    │
│          └── Déconnexion WTS des sessions hors quotas                  │
│                                                                        │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │ Communication IOCTL (\\.\Serbert)
┌───────────────────────────────────▼────────────────────────────────────┐
│                         Espace Noyau (Ring 0)                          │
│                                                                        │
│   [ Pilote Serbert (serbert.sys) ]                                     │
│          ├── PsSetCreateProcessNotifyRoutineEx                         │
│          │     ├── Intercepte toute création de processus OS           │
│          │     ├── Bloque net avec STATUS_ACCESS_DENIED                │
│          │     └── Empêche physiquement le chargement du binaire       │
│          └── Protection anti-terminaison du PID Big Brother            │
└────────────────────────────────────────────────────────────────────────┘
```

> [!IMPORTANT]
> **Condition essentielle de sécurité :**  
> Les comptes supervisés (enfants ou élèves) doivent être configurés comme **comptes standards (non-administrateurs)**.  
> Sous cette condition standard de sécurité, l'architecture `SYSTEM` + NTFS ACLs (`icacls`) rend la protection rigoureusement inattaquable. En l'absence du pilote Ring 0, le watchdog Ring 3 assure 100 % des restrictions en mode fail-safe.

---

## 📊 Tableau Comparatif des Capacités

| Fonctionnalité | Palier 1 : Standard | Palier 2 : Admin (Big Brother) | Palier 3 : Noyau (Serbert) |
|---|:---:|:---:|:---:|
| **Élévation UAC requise** | ❌ Non | ✔️ Oui (à l'installation) | ✔️ Oui (chargement driver) |
| **Journal d'activité chiffré AES-256** | ✔️ Oui | ✔️ Oui | ✔️ Oui |
| **Événements fenêtres & fichiers ouverts** | ✔️ Oui | ✔️ Oui | ✔️ Oui |
| **Notifications toast Windows (WinRT)** | ✔️ Oui | ✔️ Oui | ✔️ Oui |
| **Dashboard LAN HTTPS + QR code** | ✔️ Oui | ✔️ Oui | ✔️ Oui |
| **Diagnostic réseau DNS (type Wireshark)** | ✔️ Oui | ✔️ Oui | ✔️ Oui |
| **Filtrage Web & Redirection DNS / hosts** | ❌ Non | ✔️ Oui | ✔️ Oui |
| **Blocage installations MSI & SRP** | ❌ Non | ✔️ Oui | ✔️ Oui |
| **Quotas horaires & Déconnexion WTS** | ❌ Non | ✔️ Oui | ✔️ Oui |
| **Watchdog processus Ring 3** | ❌ Non | ✔️ Oui | ✔️ Oui |
| **Interception matérielle Kernel Ring 0** | ❌ Non | ❌ Non | ✔️ **`PsSetCreateProcess...`** |
| **Protection anti-kill du service** | ❌ Non | ✔️ Par jeton SYSTEM | ✔️ **Matérielle au Ring 0** |

---

## 📱 Dashboard LAN HTTPS & QR Code

DFWLogger intègre un serveur web HTTPS autonome (propulsé par `tokio` et `rustls`) permettant aux parents ou aux administrateurs de consulter l'activité depuis un smartphone ou un autre poste du réseau local :

1. **Déverrouillage sécurisé** : L'onglet **Accès réseau** génère à la demande une URL locale `https://<IP-LOCALE>:4782` accompagnée d'un **QR Code vectoriel**.
2. **Certificat TLS éphémère** : Certificat auto-signé généré en mémoire via `rcgen` au lancement de l'application (aucune clé privée stockée sur disque).
3. **Triple méthode d'authentification** :
   - **Token cryptographique** : Clé aléatoire de 256 bits (32 octets base64) renouvelée à chaque déverrouillage.
   - **Code PIN temporaire** : Code à 6 caractères alphanumériques sans ambiguïté, valable **10 minutes** (`DASHBOARD_CODE_TTL_SECS = 600`).
   - **Mot de passe administrateur** : Validation par hash PHC Argon2id à comparaison en temps constant (`const_time_eq`).
4. **Endpoints REST (lecture seule)** :
   - `GET /api/status` : État général (chiffrement, compteurs, mode de privilège actif).
   - `GET /api/events?after=<id>` : Flux d'activité déchiffré en temps réel (répond `locked` si verrouillé).
   - `GET /api/bigbrother` : État complet des règles actives (domaines bloqués, quotas, statut Ring 0).
   - `GET /api/dns` : Historique des requêtes DNS résolues.

---

## 🧰 Installation & Déploiement

### Déploiement interactif ou familial
1. Téléchargez la dernière archive depuis l'onglet **Releases** (`dfwlogger-vX.Y.Z-win64.zip`).
2. Décompressez l'archive et lancez :
   ```bat
   installer.cmd
   ```
   *Installe le binaire dans `%LOCALAPPDATA%\Programs\DFWLogger`, crée les raccourcis Bureau et menu Démarrer, et propose l'activation optionnelle du Mode École (Big Brother SYSTEM + pilote Serbert Ring 0) avec auto-élévation UAC.*

### Déploiement de parc scolaire (GPO / Intune / WAPT)
Pour déployer silencieusement sur l'ensemble d'un parc :
```bat
dfwlogger.exe --install-bigbrother --quiet
```

### Désinstallation complète et propre (Zéro trace)
Pour restaurer la configuration d'origine de la machine (DNS, hosts, registre, suppression des services et purge des données) :
```bat
uninstaller.cmd
```
*Le script s'auto-élève en administrateur (UAC), retire toutes les protections, arrête et supprime les services Windows (`DFWLoggerBigBrother`, `Serbert`), restaure le système, puis propose la suppression des dossiers applicatifs et des données utilisateur.*

---

## 💻 Pilotage en Ligne de Commande (CLI)

DFWLogger peut être entièrement piloté sans interface graphique :

```bat
rem --- Basculement direct entre les 3 Niveaux de Privilège ---
dfwlogger.exe --mode standard                 rem Rétrograde en Mode 1 (Standard sans élévation)
dfwlogger.exe --mode admin                    rem Active le Mode 2 (Service SYSTEM Big Brother)
dfwlogger.exe --mode kernel                   rem Active le Mode 3 (Service SYSTEM + Pilote Ring 0 Serbert)
dfwlogger.exe --status                        rem Affiche le statut complet du système et du pilote

rem --- Administration & Restrictions Big Brother ---
dfwlogger.exe --install-bigbrother --quiet    rem Déploie le service SYSTEM
dfwlogger.exe --uninstall-bigbrother --quiet  rem Désinstalle Big Brother et restaure le système
dfwlogger.exe --bigbrother-apply              rem Force l'application immédiate de la politique JSON

rem --- Pilote Noyau Serbert (Ring 0) ---
dfwlogger.exe --serbert-status                rem Interroge l'état du device \\.\Serbert
dfwlogger.exe --serbert-install               rem Enregistre et démarre le pilote noyau
dfwlogger.exe --serbert-uninstall             rem Décharge et supprime le pilote noyau

rem --- Consultation locale sans droits administrateur ---
dfwlogger.exe tail 50                         rem Affiche les 50 derniers événements déchiffrés
dfwlogger.exe export historique.csv           rem Exporte l'intégralité du journal en CSV
```

---

## 🛡️ Compilation du Pilote Noyau Serbert (`driver/serbert/`)

Pour compiler le pilote Ring 0 en environnement de test ou de laboratoire :
1. Installez **Visual Studio 2022** (avec la charge de travail *Développement C++*) et le **Windows Driver Kit (WDK)**.
2. Activez le mode signature de test sur la machine cible :
   ```bat
   bcdedit /set testsigning on
   ```
   *(Redémarrage obligatoire)*
3. Lancez le script de compilation automatique :
   ```bat
   cd driver\serbert
   build_driver.cmd
   ```
   *Produit le binaire `driver\serbert\x64\Release\serbert.sys`.*
4. Signez le pilote avec un certificat de test :
   ```bat
   SignTool sign /v /s PrivateCertStore /n "SerbertCert" serbert.sys
   ```

---

## ⚙️ Compilation & Développement

### Prérequis
- Windows 10 ou Windows 11 (64 bits).
- Rust 1.85 ou version ultérieure (outil officiel rustup) :
  ```bat
  winget install --id Rustlang.Rustup -e
  ```

### Build & Tests unitaires
```bat
# Compilation en mode debug rapide
cargo check

# Exécution de la suite de tests (chiffrement, SVG, formats, Big Brother, Serbert)
cargo test

# Compilation du binaire de production optimisé
cargo build --release
# Le binaire autonome est généré dans target\release\dfwlogger.exe
```

### Audit des dépendances & Licences
Le projet respecte une politique stricte d'audit de sécurité (`deny.toml`) intégrée au pipeline CI GitHub :
```bat
cargo deny check licenses advisories sources
```

---

## 🚀 Intégration Continue (GitHub Actions)

À chaque modification poussée sur la branche `main` ou lors de la publication d'un tag `v*` :
1. Compilation et exécution de l'intégralité des tests unitaires (`cargo test`).
2. Audit automatisé des vulnérabilités de dépendances et des licences via `cargo-deny`.
3. Génération du binaire release optimisé (`lto = "thin"`, `strip = true`).
4. Empaquetage automatique de l'archive `dfwlogger-win64.zip` contenant le binaire, les scripts d'installation/désinstallation et la documentation.
5. Publication automatique d'une release GitHub horodatée et versionnée.

---

## 📄 Licence

Ce projet est distribué sous licence **MIT**. Vous êtes libre de l'utiliser, l'adapter et le déployer en environnement éducatif, associatif, professionnel ou familial. Voir le fichier [LICENSE](file:///LICENSE) pour les termes complets.
