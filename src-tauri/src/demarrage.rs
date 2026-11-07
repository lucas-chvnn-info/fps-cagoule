// Programmes lancés avec Windows : liste et activation, avec le même mécanisme que le
// Gestionnaire des tâches (clés StartupApproved). Rien n'est supprimé : tout se réactive.

use serde::Serialize;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, REG_BINARY};
use winreg::{RegKey, RegValue};

const APPROUVE: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

#[derive(Serialize, Clone)]
pub struct Programme {
    /// Identifiant de la source (ruche + emplacement), renvoyé par l'interface pour changer l'état
    pub source: String,
    pub nom: String,
    pub commande: String,
    pub actif: bool,
    /// Pour tous les utilisateurs du PC (sinon : seulement toi)
    pub tous: bool,
}

struct Source {
    id: &'static str,
    machine: bool,
    /// Clé Run, ou None pour un dossier Démarrage
    run: Option<&'static str>,
    approuve: &'static str,
}

const SOURCES: [Source; 5] = [
    Source { id: "hkcu-run", machine: false, run: Some(r"Software\Microsoft\Windows\CurrentVersion\Run"), approuve: "Run" },
    Source { id: "hklm-run", machine: true, run: Some(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run"), approuve: "Run" },
    Source { id: "hklm-run32", machine: true, run: Some(r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run"), approuve: "Run32" },
    Source { id: "dossier-utilisateur", machine: false, run: None, approuve: "StartupFolder" },
    Source { id: "dossier-commun", machine: true, run: None, approuve: "StartupFolder" },
];

fn ruche(machine: bool) -> RegKey {
    RegKey::predef(if machine { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER })
}

fn dossier(source: &Source) -> Option<PathBuf> {
    let base = if source.machine { std::env::var("ProgramData").ok()? } else { std::env::var("APPDATA").ok()? };
    let sous = if source.machine { r"Microsoft\Windows\Start Menu\Programs\StartUp" } else { r"Microsoft\Windows\Start Menu\Programs\Startup" };
    Some(PathBuf::from(base).join(sous))
}

/// Premier octet pair = actif, impair = désactivé (valeur absente = actif).
fn est_actif(source: &Source, nom: &str) -> bool {
    ruche(source.machine)
        .open_subkey_with_flags(format!(r"{APPROUVE}\{}", source.approuve), KEY_READ)
        .and_then(|k| k.get_raw_value(nom))
        .map_or(true, |v| v.bytes.first().map_or(true, |o| o % 2 == 0))
}

pub fn liste() -> Vec<Programme> {
    let mut res = Vec::new();
    for s in &SOURCES {
        let mut ajouter = |nom: String, commande: String| {
            res.push(Programme { source: s.id.into(), actif: est_actif(s, &nom), nom, commande, tous: s.machine });
        };
        match s.run {
            Some(chemin) => {
                let Ok(cle) = ruche(s.machine).open_subkey_with_flags(chemin, KEY_READ) else { continue };
                for (nom, _) in cle.enum_values().flatten() {
                    let commande = cle.get_value::<String, _>(&nom).unwrap_or_default();
                    ajouter(nom, commande);
                }
            }
            None => {
                let Some(d) = dossier(s) else { continue };
                let Ok(entrees) = std::fs::read_dir(&d) else { continue };
                for e in entrees.flatten() {
                    let nom = e.file_name().to_string_lossy().to_string();
                    if !nom.eq_ignore_ascii_case("desktop.ini") {
                        ajouter(nom, e.path().to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    res.sort_by_key(|p| p.nom.to_lowercase());
    res
}

pub fn changer(source: &str, nom: &str, actif: bool) -> Result<(), String> {
    let s = SOURCES.iter().find(|s| s.id == source).ok_or("Source inconnue")?;
    if !liste().iter().any(|p| p.source == source && p.nom == nom) {
        return Err("Programme introuvable".into());
    }
    let (cle, _) = ruche(s.machine).create_subkey(format!(r"{APPROUVE}\{}", s.approuve)).map_err(|e| e.to_string())?;
    // Comme le Gestionnaire des tâches : 02 = actif ; 03 + date de désactivation (FILETIME) = désactivé
    let mut octets = vec![if actif { 2u8 } else { 3u8 }, 0, 0, 0];
    let filetime = if actif {
        0u64
    } else {
        let s = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64 / 100);
        s + 116_444_736_000_000_000
    };
    octets.extend_from_slice(&filetime.to_le_bytes());
    cle.set_raw_value(nom, &RegValue { bytes: octets.into(), vtype: REG_BINARY }).map_err(|e| e.to_string())
}
