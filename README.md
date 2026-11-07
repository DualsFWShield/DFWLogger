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

## Tray, fenêtre et terminal

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
   `https://<IP>:4782` et un **token** (renouvelé à chaque déverrouillage).
2. Depuis un téléphone/navigation sur le même réseau : ouvrez l'URL (le certificat local
   est auto-signé : « Avancé → Accepter et continuer »), le lien pré-rempli `?auth=<token>`
   peut être copié en un clic.
3. Le pré-écrivain HTTPS écoute sur un port visible documenté (défaut 4782), exige le token
   (comparaison à temps constant) et sert uniquement des APIs JSON en **lecture seule** :
   événements récents, compteurs et **top 10 du temps passé par application**.
   Le journal verrouillé répond « locked » : aucune donnée ne quitte l'app à froid.
4. Le pare-feu Windows demande d'autoriser le port au premier lancement.

## Terminal non-admin sur le PC surveillé

```bat
dfwlogger.exe tail 30
dfwlogger.exe export historique.csv
```
demande le mot de passe parent et affiche / exporte l'historique déchiffré.

## Compilation

```bat
winget install --id Rustlang.Rustup -e
cargo build --release
rem → target\release\dfwlogger.exe
cargo test
```

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
