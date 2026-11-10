// Carte graphique principale : NVIDIA, AMD ou Intel, lue dans le registre des pilotes d'affichage.
// Sert à n'afficher que les réglages et l'overclocking qui correspondent à la carte du PC.

use serde::Serialize;
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
use winreg::RegKey;

const AFFICHAGE: &str = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";

#[derive(Serialize, Clone)]
pub struct Carte {
    /// nvidia, amd, intel ou autre
    pub fabricant: &'static str,
    pub nom: String,
    /// Carte dédiée (et pas la puce graphique du processeur)
    pub dediee: bool,
}

pub fn cartes() -> Vec<Carte> {
    let Ok(classe) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(AFFICHAGE, KEY_READ) else { return vec![] };
    classe
        .enum_keys()
        .flatten()
        .filter(|k| k.len() == 4 && k.chars().all(|c| c.is_ascii_digit()))
        .filter_map(|k| {
            let c = classe.open_subkey_with_flags(&k, KEY_READ).ok()?;
            let nom: String = c.get_value("DriverDesc").ok()?;
            let id: String = c.get_value("MatchingDeviceId").unwrap_or_default();
            Some(decrire(&nom, &id))
        })
        .filter(|c| c.fabricant != "autre")
        .collect()
}

pub fn decrire(nom: &str, id: &str) -> Carte {
    let id = id.to_lowercase();
    let n = nom.to_lowercase();
    let fabricant = if id.contains("ven_10de") || n.contains("nvidia") {
        "nvidia"
    } else if id.contains("ven_1002") || n.contains("radeon") || n.starts_with("amd") {
        "amd"
    } else if id.contains("ven_8086") || n.contains("intel") {
        "intel"
    } else {
        "autre"
    };
    let dediee = match fabricant {
        "nvidia" => true,
        // « AMD Radeon(TM) Graphics » / « Radeon Vega 8 » = puce du processeur ; RX, Pro, VII = carte dédiée
        "amd" => n.contains(" rx ") || n.contains("rx ") || n.contains("radeon pro") || n.contains("radeon vii"),
        "intel" => n.contains("arc"),
        _ => false,
    };
    Carte { fabricant, nom: nom.trim().to_string(), dediee }
}

/// La carte qui fait tourner les jeux : une carte dédiée d'abord (NVIDIA, puis AMD, puis Intel Arc)
pub fn principale() -> Option<Carte> {
    let mut l = cartes();
    let rang = |c: &Carte| match (c.dediee, c.fabricant) {
        (true, "nvidia") => 0,
        (true, "amd") => 1,
        (true, _) => 2,
        (false, "amd") => 3,
        _ => 4,
    };
    l.sort_by_key(rang);
    l.into_iter().next()
}

/// Logiciel du fabricant (Adrenalin, Intel Graphics Software / Arc Control), s'il est installé
pub fn logiciel(fabricant: &str) -> Option<std::path::PathBuf> {
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    let candidats: &[&str] = match fabricant {
        "amd" => &[r"AMD\CNext\CNext\RadeonSoftware.exe"],
        "intel" => &[r"Intel\Intel Graphics Software\IntelGraphicsSoftware.exe", r"Intel\Intel Arc Control\ArcControl.exe"],
        _ => &[],
    };
    candidats.iter().map(|c| std::path::Path::new(&pf).join(c)).find(|p| p.exists())
}
