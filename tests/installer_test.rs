//! Tests automatisés pour installer.cmd et la gestion des raccourcis.

use std::path::Path;
use std::process::Command;

#[test]
fn test_installer_syntax_and_test_flag() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let installer_path = root.join("installer.cmd");
    assert!(installer_path.exists(), "installer.cmd doit exister à la racine");

    let status = Command::new(&installer_path)
        .current_dir(root)
        .arg("--test")
        .status()
        .expect("Échec d'exécution de installer.cmd");

    assert!(status.success(), "installer.cmd --test doit s'exécuter avec succès (code 0)");
}

#[test]
fn test_installer_script_parentheses_safety() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let content = std::fs::read_to_string(root.join("installer.cmd")).expect("Lecture installer.cmd");

    let mut in_parentheses_block = false;
    for (line_num, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line.ends_with('(') {
            in_parentheses_block = true;
        } else if line == ")" {
            in_parentheses_block = false;
        } else if in_parentheses_block && line.to_ascii_lowercase().starts_with("echo ") {
            // Dans cmd.exe, une parenthèse fermante non échappée ')' dans un bloc if (...)
            // ferme prématurément le bloc et provoque l'erreur classique '. était inattendu.'
            let rest = &line[5..];
            if rest.contains(')') && !rest.contains("^)") {
                panic!(
                    "Ligne {} contient une parenthèse non échappée dans un bloc batch : {}",
                    line_num + 1,
                    line
                );
            }
        }
    }
}

#[test]
fn test_installer_quiet_execution() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let installer_path = root.join("installer.cmd");

    let temp_dir = std::env::temp_dir().join("dfwlogger_installer_test");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).expect("Création dossier temporaire de test");

    let status = Command::new(&installer_path)
        .current_dir(root)
        .env("LOCALAPPDATA", &temp_dir)
        .arg("--quiet")
        .status()
        .expect("Échec d'exécution de installer.cmd --quiet");

    assert!(status.success(), "installer.cmd --quiet doit réussir en mode non-interactif");

    let installed_exe = temp_dir
        .join("Programs")
        .join("DFWLogger")
        .join("dfwlogger.exe");

    assert!(
        installed_exe.exists(),
        "L'exécutable doit être copié dans {:?}",
        installed_exe
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_uninstaller_syntax_and_test_flag() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let uninstaller_path = root.join("uninstaller.cmd");
    assert!(uninstaller_path.exists(), "uninstaller.cmd doit exister à la racine");

    let status = Command::new(&uninstaller_path)
        .current_dir(root)
        .arg("--test")
        .status()
        .expect("Échec d'exécution de uninstaller.cmd");

    assert!(status.success(), "uninstaller.cmd --test doit s'exécuter avec succès (code 0)");
}

#[test]
fn test_uninstaller_script_parentheses_safety() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let content = std::fs::read_to_string(root.join("uninstaller.cmd")).expect("Lecture uninstaller.cmd");

    let mut in_parentheses_block = false;
    for (line_num, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line.ends_with('(') {
            in_parentheses_block = true;
        } else if line == ")" {
            in_parentheses_block = false;
        } else if in_parentheses_block && line.to_ascii_lowercase().starts_with("echo ") {
            let rest = &line[5..];
            if rest.contains(')') && !rest.contains("^)") {
                panic!(
                    "uninstaller.cmd Ligne {} contient une parenthèse non échappée dans un bloc batch : {}",
                    line_num + 1,
                    line
                );
            }
        }
    }
}
