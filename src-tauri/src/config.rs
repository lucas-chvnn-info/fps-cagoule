// Réglages, profils de jeux et historique, stockés en JSON dans %APPDATA%\FPS Cagoule.

use crate::ecrans::Couleurs;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Reglages {
    pub boost_auto: bool,
    /// Pendant les parties : plan Performances optimales, minuteur 0,5 ms, mémoire en veille vidée
    pub boost_max: bool,
    pub mesure_fps: bool,
    pub overlay: bool,
    pub maj_auto: bool,
    /// Appliquer les couleurs choisies dans « Écrans » pendant les parties
    pub couleurs_en_jeu: bool,
    /// Programmes fermés au lancement d'un jeu (noms d'exe)
    pub fermer_applis: Vec<String>,
}

impl Default for Reglages {
    fn default() -> Self {
        Self { boost_auto: true, boost_max: false, mesure_fps: true, overlay: false, maj_auto: true, couleurs_en_jeu: false, fermer_applis: vec![] }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Profil {
    pub nom: String,
    pub exes: Vec<String>,
    /// Détection activée
    pub actif: bool,
    pub boost: bool,
    pub priorite_haute: bool,
    pub gpu_perf: bool,
}

impl Default for Profil {
    fn default() -> Self {
        Self { nom: String::new(), exes: vec![], actif: true, boost: true, priorite_haute: true, gpu_perf: true }
    }
}

fn profil(nom: &str, exes: &[&str]) -> Profil {
    Profil { nom: nom.into(), exes: exes.iter().map(|e| e.to_string()).collect(), ..Default::default() }
}

pub fn profils_par_defaut() -> Vec<Profil> {
    vec![
        profil("Rainbow Six Siege", &["RainbowSix.exe", "RainbowSix_Vulkan.exe", "RainbowSix_DX12.exe"]),
        profil("Fortnite", &["FortniteClient-Win64-Shipping.exe"]),
        profil("Valorant", &["VALORANT-Win64-Shipping.exe"]),
        profil("Counter-Strike 2", &["cs2.exe"]),
        profil("Apex Legends", &["r5apex.exe", "r5apex_dx12.exe"]),
        profil("Call of Duty", &["cod.exe"]),
        profil("League of Legends", &["League of Legends.exe"]),
        profil("Rocket League", &["RocketLeague.exe"]),
        profil("Overwatch 2", &["Overwatch.exe"]),
        profil("Marvel Rivals", &["Marvel-Win64-Shipping.exe"]),
        profil("The Finals", &["Discovery.exe"]),
        profil("GTA V", &["GTA5.exe", "GTA5_Enhanced.exe"]),
        profil("Minecraft", &["Minecraft.Windows.exe"]),
    ]
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Donnees {
    pub reglages: Reglages,
    pub profils: Vec<Profil>,
    /// Valeurs d'origine des optimisations appliquées, pour pouvoir les annuler
    pub sauvegardes: HashMap<String, Value>,
    /// Plan Performances élevées créé par l'app quand Windows ne l'avait pas
    pub plan_perso: Option<String>,
    /// Copie de Performances optimales créée par l'app (boost max)
    pub plan_max: Option<String>,
    /// Plan actif avant le dernier boost : restauré au prochain lancement si l'app a été coupée en pleine partie
    pub plan_avant_jeu: Option<String>,
    /// Couleurs en jeu, par écran (\\.\DISPLAY1…)
    pub couleurs: HashMap<String, Couleurs>,
    /// Vibrance NVIDIA d'avant la partie, remise au prochain lancement si l'app a été coupée en pleine partie
    pub vibrance_avant: HashMap<String, i32>,
}

impl Default for Donnees {
    fn default() -> Self {
        Self {
            reglages: Reglages::default(),
            profils: profils_par_defaut(),
            sauvegardes: HashMap::new(),
            plan_perso: None,
            plan_max: None,
            plan_avant_jeu: None,
            couleurs: HashMap::new(),
            vibrance_avant: HashMap::new(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub jeu: String,
    /// Horodatage Unix (secondes)
    pub debut: u64,
    pub duree_s: u64,
    pub boost: bool,
    pub images: u64,
    pub moyenne: f64,
    pub low1: f64,
    pub low01: f64,
}

const MAX_SESSIONS: usize = 500;

pub struct Stockage {
    dossier: PathBuf,
    pub donnees: Mutex<Donnees>,
}

fn lire_json<T: for<'de> Deserialize<'de> + Default>(chemin: &Path) -> T {
    fs::read_to_string(chemin).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn ecrire_json<T: Serialize>(chemin: &Path, valeur: &T) {
    let tmp = chemin.with_extension("tmp");
    if let Ok(texte) = serde_json::to_string_pretty(valeur) {
        if fs::write(&tmp, texte).is_ok() {
            let _ = fs::rename(&tmp, chemin);
        }
    }
}

impl Stockage {
    pub fn ouvrir(dossier: PathBuf) -> Self {
        let _ = fs::create_dir_all(&dossier);
        let donnees = lire_json(&dossier.join("config.json"));
        Self { dossier, donnees: Mutex::new(donnees) }
    }

    pub fn sauver(&self) {
        let d = self.donnees.lock().unwrap();
        ecrire_json(&self.dossier.join("config.json"), &*d);
    }

    pub fn sessions(&self) -> Vec<Session> {
        lire_json(&self.dossier.join("sessions.json"))
    }

    pub fn ajouter_session(&self, s: Session) {
        let mut liste = self.sessions();
        liste.push(s);
        if liste.len() > MAX_SESSIONS {
            liste.drain(..liste.len() - MAX_SESSIONS);
        }
        ecrire_json(&self.dossier.join("sessions.json"), &liste);
    }
}
