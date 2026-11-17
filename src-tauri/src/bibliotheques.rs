// Jeux installés : bibliothèques Steam, Epic, GOG, Ubisoft Connect et jeux Battle.net, EA, Activision, Rockstar
// (désinstalleurs Windows). Lancé à la demande depuis l'interface, jamais en arrière-plan.

use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
use winreg::RegKey;

#[derive(Serialize, Clone)]
pub struct JeuTrouve {
    pub nom: String,
    pub exes: Vec<String>,
    pub source: &'static str,
    pub dossier: String,
    /// Identifiant Steam (jaquette dans le cache de Steam ou sur son CDN)
    #[serde(skip)]
    pub steam: Option<u32>,
    /// Identifiant du catalogue Epic (jaquette dans le cache du launcher)
    #[serde(skip)]
    pub epic: Option<String>,
    /// Icône .ico déclarée par le désinstalleur (jeux Riot…)
    #[serde(skip)]
    pub icone: Option<String>,
    /// Comment lancer le jeu par son launcher (sinon : son exécutable)
    #[serde(skip)]
    pub lancement: Option<Lancement>,
}

/// Lancement d'un jeu : lien du launcher (steam://, com.epicgames.launcher://, uplay://) ou programme avec arguments
#[derive(Clone, Debug, PartialEq)]
pub enum Lancement {
    Lien(String),
    Programme(String, Vec<String>),
}

impl JeuTrouve {
    pub(crate) fn nouveau(nom: String, exes: Vec<String>, source: &'static str, dossier: String) -> Self {
        Self { nom, exes, source, dossier, steam: None, epic: None, icone: None, lancement: None }
    }
}

/// Lien Epic : « namespace:catalogue:appli » encodé, comme le fait le launcher pour ses raccourcis
pub fn lien_epic(espace: &str, catalogue: &str, appli: &str) -> String {
    format!("com.epicgames.launcher://apps/{espace}%3A{catalogue}%3A{appli}?action=launch&silent=true")
}

/// Désinstalleur du Riot Client (« RiotClientServices.exe --uninstall-product=valorant --uninstall-patchline=live »)
/// → le même programme avec --launch-product et --launch-patchline
pub fn lancement_riot(desinstalleur: &str) -> Option<Lancement> {
    let (programme, reste) = desinstalleur.split_once("RiotClientServices.exe")?;
    let programme = format!("{}RiotClientServices.exe", programme.trim_start_matches('"'));
    let valeur = |cle: &str| reste.split_whitespace().find_map(|m| m.trim_matches('"').strip_prefix(cle).map(String::from));
    let produit = valeur("--uninstall-product=")?;
    let canal = valeur("--uninstall-patchline=").unwrap_or_else(|| "live".into());
    Some(Lancement::Programme(programme, vec![format!("--launch-product={produit}"), format!("--launch-patchline={canal}")]))
}

/// Exécutables du jeu, ou rien quand on veut seulement la liste (pas de parcours des dossiers)
fn exes_si(avec: bool, dossier: &Path) -> Vec<String> {
    if avec {
        exes_du_jeu(dossier)
    } else {
        vec![]
    }
}

/// Dossier d'installation de Steam
pub fn steam_racine() -> Option<PathBuf> {
    let cle = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(r"Software\Valve\Steam", KEY_READ).ok()?;
    let racine: String = cle.get_value("SteamPath").ok()?;
    Some(PathBuf::from(racine.replace('/', "\\")))
}

/// Programmes qui ne sont pas le jeu lui-même (installeurs, anti-triches, lanceurs, rapports de plantage…)
fn exe_annexe(nom: &str) -> bool {
    let n = nom.to_lowercase();
    const MOTS: [&str; 34] = [
        "unins", "setup", "install", "redist", "vcredist", "vc_redist", "dxsetup", "directx", "crash", "report", "launcher",
        "updater", "update", "easyanticheat", "eac_", "battleye", "beservice", "_be.exe", "helper", "cef", "prereq", "dotnet",
        "physx", "oalinst", "touchup", "cleanup", "editor", "server", "uploader", "sendlog", "bugsplat", "overlay", "browser",
        "benchmark",
    ];
    MOTS.iter().any(|m| n.contains(m))
}

/// Dossiers où il n'y a jamais l'exe du jeu
fn dossier_annexe(nom: &str) -> bool {
    let n = nom.to_lowercase();
    ["_commonredist", "redist", "__installer", "support", "directx", "dotnet", "vcredist", "easyanticheat", "battleye", "thirdparty", "prereq"]
        .iter()
        .any(|m| n.contains(m))
}

/// Exécutables probables du jeu : les « -Shipping » d'Unreal d'abord, puis les plus gros (3 au maximum).
pub fn exes_du_jeu(dossier: &Path) -> Vec<String> {
    let mut trouves: Vec<(String, u64)> = Vec::new();
    let mut pile = vec![(dossier.to_path_buf(), 0u8)];
    let mut vus = 0usize;
    while let Some((d, profondeur)) = pile.pop() {
        let Ok(entrees) = fs::read_dir(&d) else { continue };
        for e in entrees.flatten() {
            vus += 1;
            // Garde-fou pour les très gros dossiers
            if vus > 20_000 {
                pile.clear();
                break;
            }
            let chemin = e.path();
            let nom = e.file_name().to_string_lossy().to_string();
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                if profondeur < 4 && !dossier_annexe(&nom) {
                    pile.push((chemin, profondeur + 1));
                }
            } else if nom.to_lowercase().ends_with(".exe") && !exe_annexe(&nom) && meta.len() >= 1 << 20 {
                trouves.push((nom, meta.len()));
            }
        }
    }
    trouves.sort_by(|a, b| {
        let sa = a.0.to_lowercase().contains("shipping");
        let sb = b.0.to_lowercase().contains("shipping");
        sb.cmp(&sa).then(b.1.cmp(&a.1))
    });
    let mut exes: Vec<String> = Vec::new();
    for (nom, _) in trouves {
        if !exes.iter().any(|e| e.eq_ignore_ascii_case(&nom)) {
            exes.push(nom);
        }
        if exes.len() == 3 {
            break;
        }
    }
    exes
}

/// Lignes « "clé" "valeur" » d'un fichier VDF de Steam
fn paires_vdf(texte: &str) -> Vec<(String, String)> {
    texte
        .lines()
        .filter_map(|l| {
            let morceaux: Vec<&str> = l.split('"').collect();
            (morceaux.len() >= 5).then(|| (morceaux[1].to_lowercase(), morceaux[3].replace("\\\\", "\\")))
        })
        .collect()
}

fn steam(avec_exes: bool) -> Vec<JeuTrouve> {
    let Some(racine) = steam_racine() else { return vec![] };
    let mut bibliotheques = vec![racine.clone()];
    if let Ok(t) = fs::read_to_string(racine.join(r"steamapps\libraryfolders.vdf")) {
        bibliotheques.extend(paires_vdf(&t).into_iter().filter(|(k, _)| k == "path").map(|(_, v)| PathBuf::from(v)));
    }
    let mut vues = HashSet::new();
    let mut jeux = Vec::new();
    for b in bibliotheques {
        let apps = b.join("steamapps");
        if !vues.insert(apps.to_string_lossy().to_lowercase()) {
            continue;
        }
        let Ok(entrees) = fs::read_dir(&apps) else { continue };
        for e in entrees.flatten() {
            let nom_fichier = e.file_name().to_string_lossy().to_string();
            if !(nom_fichier.starts_with("appmanifest_") && nom_fichier.ends_with(".acf")) {
                continue;
            }
            let Ok(t) = fs::read_to_string(e.path()) else { continue };
            let p = paires_vdf(&t);
            let val = |k: &str| p.iter().find(|(c, _)| c == k).map(|(_, v)| v.clone());
            let (Some(nom), Some(dossier)) = (val("name"), val("installdir")) else { continue };
            // Outils de Steam, pas des jeux
            let n = nom.to_lowercase();
            if ["redistributable", "steamworks", "proton", "steam linux runtime", "steamvr", "wallpaper engine", "soundpad", "dedicated server"]
                .iter()
                .any(|m| n.contains(m))
            {
                continue;
            }
            let chemin = apps.join("common").join(&dossier);
            let mut j = JeuTrouve::nouveau(nom, exes_si(avec_exes, &chemin), "Steam", chemin.to_string_lossy().into());
            j.steam = val("appid").and_then(|a| a.parse().ok());
            j.lancement = j.steam.map(|id| Lancement::Lien(format!("steam://rungameid/{id}")));
            jeux.push(j);
        }
    }
    jeux
}

fn epic(avec_exes: bool) -> Vec<JeuTrouve> {
    let dossier = Path::new(&std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into())).join(r"Epic\EpicGamesLauncher\Data\Manifests");
    let Ok(entrees) = fs::read_dir(dossier) else { return vec![] };
    entrees
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("item")))
        .filter_map(|e| {
            let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(e.path()).ok()?).ok()?;
            if v["AppCategories"].as_array().is_some_and(|c| !c.iter().any(|x| x.as_str() == Some("games"))) {
                return None;
            }
            let nom = v["DisplayName"].as_str()?.to_string();
            let dossier = v["InstallLocation"].as_str()?.to_string();
            let lance = v["LaunchExecutable"].as_str().unwrap_or("");
            let mut exes = exes_si(avec_exes, Path::new(&dossier));
            // L'exe déclaré par Epic passe en premier
            if let Some(n) = Path::new(lance).file_name().map(|n| n.to_string_lossy().to_string()).filter(|n| n.to_lowercase().ends_with(".exe")) {
                exes.retain(|e| !e.eq_ignore_ascii_case(&n));
                exes.insert(0, n);
                exes.truncate(3);
            }
            let mut j = JeuTrouve::nouveau(nom, exes, "Epic Games", dossier);
            j.epic = v["CatalogItemId"].as_str().map(String::from);
            if let (Some(espace), Some(catalogue), Some(appli)) = (v["CatalogNamespace"].as_str(), v["CatalogItemId"].as_str(), v["AppName"].as_str()) {
                j.lancement = Some(Lancement::Lien(lien_epic(espace, catalogue, appli)));
            }
            Some(j)
        })
        .collect()
}

fn sous_cles(ruche: &RegKey, chemin: &str) -> Vec<RegKey> {
    let Ok(cle) = ruche.open_subkey_with_flags(chemin, KEY_READ) else { return vec![] };
    cle.enum_keys().flatten().filter_map(|k| cle.open_subkey_with_flags(k, KEY_READ).ok()).collect()
}

fn gog(avec_exes: bool) -> Vec<JeuTrouve> {
    sous_cles(&RegKey::predef(HKEY_LOCAL_MACHINE), r"SOFTWARE\WOW6432Node\GOG.com\Games")
        .into_iter()
        .filter_map(|c| {
            let nom: String = c.get_value("gameName").ok()?;
            let dossier: String = c.get_value("path").ok()?;
            let exe: String = c.get_value("exe").unwrap_or_default();
            let mut exes = exes_si(avec_exes, Path::new(&dossier));
            if let Some(n) = Path::new(&exe).file_name().map(|n| n.to_string_lossy().to_string()) {
                exes.retain(|e| !e.eq_ignore_ascii_case(&n));
                exes.insert(0, n);
                exes.truncate(3);
            }
            let mut j = JeuTrouve::nouveau(nom, exes, "GOG", dossier);
            j.lancement = Path::new(&exe).is_file().then(|| Lancement::Programme(exe, vec![]));
            Some(j)
        })
        .collect()
}

fn ubisoft(avec_exes: bool) -> Vec<JeuTrouve> {
    let Ok(installs) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(r"SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs", KEY_READ) else { return vec![] };
    installs
        .enum_keys()
        .flatten()
        .filter_map(|id| {
            let c = installs.open_subkey_with_flags(&id, KEY_READ).ok()?;
            let dossier: String = c.get_value("InstallDir").ok()?;
            let chemin = PathBuf::from(dossier.replace('/', "\\"));
            let nom = chemin.file_name()?.to_string_lossy().to_string();
            let mut j = JeuTrouve::nouveau(nom, exes_si(avec_exes, &chemin), "Ubisoft Connect", chemin.to_string_lossy().into());
            // La clé porte l'identifiant du jeu dans Ubisoft Connect
            j.lancement = id.chars().all(|c| c.is_ascii_digit()).then(|| Lancement::Lien(format!("uplay://launch/{id}/0")));
            Some(j)
        })
        .collect()
}

/// Jeux des autres éditeurs, retrouvés par leur désinstalleur Windows
fn desinstalleurs(avec_exes: bool) -> Vec<JeuTrouve> {
    const EDITEURS: [(&str, &str); 5] = [
        ("blizzard entertainment", "Battle.net"),
        ("electronic arts", "EA app"),
        ("activision", "Battle.net"),
        ("rockstar games", "Rockstar"),
        ("riot games", "Riot"),
    ];
    let mut jeux = Vec::new();
    for (ruche, chemin) in [
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
        (HKEY_CURRENT_USER, r"Software\Microsoft\Windows\CurrentVersion\Uninstall"),
    ] {
        for c in sous_cles(&RegKey::predef(ruche), chemin) {
            let editeur: String = c.get_value("Publisher").unwrap_or_default();
            let Some((_, source)) = EDITEURS.iter().find(|(e, _)| editeur.to_lowercase().contains(e)) else { continue };
            let (Ok(nom), Ok(dossier)) = (c.get_value::<String, _>("DisplayName"), c.get_value::<String, _>("InstallLocation")) else { continue };
            let n = nom.to_lowercase();
            // Les lanceurs eux-mêmes
            if ["battle.net", "ea app", "launcher", "riot client", "social club", "origin"].iter().any(|m| n.contains(m)) {
                continue;
            }
            let dossier = dossier.trim_matches('"').trim_end_matches('\\').to_string();
            if dossier.is_empty() {
                continue;
            }
            // « "C:\…\jeu.ico",0 » → chemin de l'icône
            let icone = c
                .get_value::<String, _>("DisplayIcon")
                .ok()
                .map(|i| i.split(',').next().unwrap_or("").trim().trim_matches('"').to_string())
                .filter(|i| i.to_lowercase().ends_with(".ico"));
            let mut j = JeuTrouve::nouveau(nom, exes_si(avec_exes, Path::new(&dossier)), source, dossier);
            j.icone = icone;
            j.lancement = c.get_value::<String, _>("UninstallString").ok().and_then(|u| lancement_riot(&u));
            jeux.push(j);
        }
    }
    jeux
}

/// Jeux de toutes les bibliothèques, sans doublons. Sans `avec_exes`, rien n'est parcouru sur le disque (rapide).
pub fn sources(avec_exes: bool) -> Vec<JeuTrouve> {
    let mut tous = Vec::new();
    for f in [steam, epic, gog, ubisoft, desinstalleurs] {
        tous.extend(f(avec_exes));
    }
    let mut vus = HashSet::new();
    tous.retain(|j| vus.insert(j.dossier.to_lowercase()));
    tous
}

/// Tous les jeux installés avec au moins un exécutable trouvé, sans doublons
pub fn chercher() -> Vec<JeuTrouve> {
    let mut tous = sources(true);
    tous.retain(|j| !j.exes.is_empty());
    tous.sort_by_key(|j| j.nom.to_lowercase());
    tous
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lancement_des_jeux_riot() {
        let d = r#""C:\Riot Games\Riot Client\RiotClientServices.exe" --uninstall-product=valorant --uninstall-patchline=live"#;
        assert_eq!(
            lancement_riot(d),
            Some(Lancement::Programme(
                r"C:\Riot Games\Riot Client\RiotClientServices.exe".into(),
                vec!["--launch-product=valorant".into(), "--launch-patchline=live".into()]
            ))
        );
        assert_eq!(lancement_riot(r"C:\Jeu\unins000.exe"), None);
        assert!(lien_epic("fn", "4fe75b", "Fortnite").starts_with("com.epicgames.launcher://apps/fn%3A4fe75b%3AFortnite?"));
    }
}
