// Programmes installés et désinstallation (comme « Applications installées » de Windows).

use serde::Serialize;
use std::os::windows::process::CommandExt;
use std::process::Command;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
use winreg::RegKey;

const SOURCES: [(bool, &str); 3] = [
    (true, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
    (true, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
    (false, r"Software\Microsoft\Windows\CurrentVersion\Uninstall"),
];

#[derive(Serialize)]
pub struct Programme {
    /// Identifiant interne (source + clé) renvoyé par l'interface pour désinstaller
    pub id: String,
    pub nom: String,
    pub editeur: String,
    pub version: String,
    pub taille_ko: u64,
    /// AAAAMMJJ
    pub date: String,
    #[serde(skip)]
    commande: String,
}

pub fn liste() -> Vec<Programme> {
    let mut res: Vec<Programme> = Vec::new();
    for (i, (machine, chemin)) in SOURCES.iter().enumerate() {
        let ruche = RegKey::predef(if *machine { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER });
        let Ok(base) = ruche.open_subkey_with_flags(chemin, KEY_READ) else { continue };
        for cle in base.enum_keys().flatten() {
            let Ok(k) = base.open_subkey_with_flags(&cle, KEY_READ) else { continue };
            let texte = |n: &str| k.get_value::<String, _>(n).unwrap_or_default();
            let nom = texte("DisplayName");
            let commande = texte("UninstallString");
            // Composants système et mises à jour : Windows les cache aussi
            let cache = k.get_value::<u32, _>("SystemComponent").unwrap_or(0) == 1 || !texte("ParentKeyName").is_empty();
            if nom.is_empty() || commande.is_empty() || cache || res.iter().any(|p| p.nom == nom) {
                continue;
            }
            res.push(Programme {
                id: format!("{i}|{cle}"),
                nom,
                editeur: texte("Publisher"),
                version: texte("DisplayVersion"),
                taille_ko: k.get_value::<u32, _>("EstimatedSize").unwrap_or(0) as u64,
                date: texte("InstallDate"),
                commande,
            });
        }
    }
    res.sort_by_key(|p| p.nom.to_lowercase());
    res
}

/// Sépare « "C:\…\uninst.exe" /S » ou « MsiExec.exe /X{…} » en programme et arguments.
fn decouper(commande: &str) -> (String, String) {
    let c = commande.trim();
    if let Some(reste) = c.strip_prefix('"') {
        if let Some(fin) = reste.find('"') {
            return (reste[..fin].to_string(), reste[fin + 1..].trim().to_string());
        }
    }
    match c.to_lowercase().find(".exe") {
        Some(i) => (c[..i + 4].to_string(), c[i + 4..].trim().to_string()),
        None => (c.to_string(), String::new()),
    }
}

/// Lance le désinstalleur du programme (sa propre fenêtre s'ouvre).
pub fn desinstaller(id: &str) -> Result<(), String> {
    let p = liste().into_iter().find(|p| p.id == id).ok_or("Programme introuvable : il est peut-être déjà désinstallé")?;
    let (programme, arguments) = decouper(&p.commande);
    Command::new(&programme)
        .raw_arg(&arguments)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Impossible de lancer le désinstalleur de {} : {e}", p.nom))
}

#[cfg(test)]
mod tests {
    use super::decouper;

    #[test]
    fn commandes_de_desinstallation() {
        assert_eq!(decouper(r#""C:\Program Files\Jeu\unins000.exe" /SILENT"#), (r"C:\Program Files\Jeu\unins000.exe".into(), "/SILENT".into()));
        assert_eq!(decouper("MsiExec.exe /X{1234}"), ("MsiExec.exe".into(), "/X{1234}".into()));
        assert_eq!(decouper(r"C:\Program Files\Outil\uninstall.exe"), (r"C:\Program Files\Outil\uninstall.exe".into(), String::new()));
    }
}
