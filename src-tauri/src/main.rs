// FPS Cagoule : analyse, optimisation et boost par jeu pour Windows 11.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod demarrage;
mod ecrans;
mod fenetres;
mod fps;
mod jeux;
mod pilotes;
mod maj;
mod ps;
mod rampe;
mod reseau;
mod stats;
mod systeme;
mod tweaks;

use config::{Profil, Reglages, Session, Stockage};
use jeux::{Etat, InfoJeu};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, RunEvent};

type Res<T> = Result<T, String>;

fn etat(app: &AppHandle) -> Arc<Etat> {
    app.state::<Arc<Etat>>().inner().clone()
}

#[tauri::command(async)]
fn analyser() -> Res<serde_json::Value> {
    ps::json(include_str!("analyse.ps1"))
}

#[derive(Serialize)]
struct EtatApp {
    version: String,
    jeu: Option<InfoJeu>,
    plan_perso: Option<String>,
    plan_max: Option<String>,
    presentmon: bool,
}

#[tauri::command]
fn etat_app(app: AppHandle) -> EtatApp {
    let e = etat(&app);
    let (plan_perso, plan_max) = {
        let d = e.stock.donnees.lock().unwrap();
        (d.plan_perso.clone(), d.plan_max.clone())
    };
    EtatApp {
        version: app.package_info().version.to_string(),
        jeu: e.info_jeu(),
        plan_perso,
        plan_max,
        presentmon: e.presentmon.exists(),
    }
}

#[tauri::command(async)]
fn tweaks_liste(app: AppHandle) -> Vec<tweaks::TweakEtat> {
    tweaks::liste(&etat(&app).stock)
}

#[tauri::command(async)]
fn tweaks_appliquer(app: AppHandle, ids: Vec<String>, restauration: bool) -> (Option<String>, Vec<tweaks::Resultat>) {
    let point = restauration.then(|| match tweaks::point_restauration() {
        Ok(r) if r == "recent" => "Point de restauration déjà créé il y a moins de 24 h : il reste valable".to_string(),
        Ok(_) => "Point de restauration Windows créé".to_string(),
        Err(e) => e,
    });
    (point, tweaks::appliquer(&etat(&app).stock, &ids))
}

#[tauri::command(async)]
fn tweaks_annuler(app: AppHandle, ids: Vec<String>) -> Vec<tweaks::Resultat> {
    tweaks::annuler(&etat(&app).stock, &ids)
}

#[tauri::command(async)]
fn nettoyer() -> tweaks::Nettoyage {
    tweaks::nettoyer()
}

#[tauri::command]
fn demarrage_liste() -> Vec<demarrage::Programme> {
    demarrage::liste()
}

#[tauri::command]
fn demarrage_changer(source: String, nom: String, actif: bool) -> Res<()> {
    demarrage::changer(&source, &nom, actif)
}

#[tauri::command]
fn profils_get(app: AppHandle) -> Vec<Profil> {
    etat(&app).stock.donnees.lock().unwrap().profils.clone()
}

#[tauri::command]
fn profils_set(app: AppHandle, profils: Vec<Profil>) {
    let e = etat(&app);
    e.stock.donnees.lock().unwrap().profils = profils;
    e.stock.sauver();
}

#[tauri::command]
fn profils_defaut() -> Vec<Profil> {
    config::profils_par_defaut()
}

#[tauri::command]
fn reglages_get(app: AppHandle) -> Reglages {
    etat(&app).stock.donnees.lock().unwrap().reglages.clone()
}

#[tauri::command]
fn reglages_set(app: AppHandle, reglages: Reglages) {
    let e = etat(&app);
    e.stock.donnees.lock().unwrap().reglages = reglages;
    e.stock.sauver();
}

#[tauri::command(async)]
fn sessions(app: AppHandle) -> Vec<Session> {
    etat(&app).stock.sessions()
}

#[derive(Serialize)]
struct Conso {
    app_mo: f64,
    interface_mo: f64,
    presentmon_mo: f64,
    cpu: f64,
}

/// Consommation de FPS Cagoule elle-même, pour que l'utilisateur puisse la vérifier.
#[tauri::command]
fn conso() -> Conso {
    let moi = std::process::id();
    let procs = systeme::processus();
    // Descendants : processus WebView2 de l'interface et PresentMon
    let mut famille = vec![moi];
    let mut i = 0;
    while i < famille.len() {
        let parent = famille[i];
        famille.extend(procs.iter().filter(|p| p.parent == parent && p.pid != parent).map(|p| p.pid));
        i += 1;
    }
    let mut c = Conso { app_mo: 0.0, interface_mo: 0.0, presentmon_mo: 0.0, cpu: systeme::cpu_app() };
    for pid in famille {
        let mo = systeme::memoire(pid) as f64 / 1048576.0;
        let nom = procs.iter().find(|p| p.pid == pid).map(|p| p.nom.to_lowercase()).unwrap_or_default();
        if pid == moi {
            c.app_mo += mo;
        } else if nom.starts_with("presentmon") {
            c.presentmon_mo += mo;
        } else {
            c.interface_mo += mo;
        }
    }
    c
}

#[tauri::command]
fn processus_liste() -> Vec<String> {
    let mut noms: Vec<String> = systeme::processus().into_iter().map(|p| p.nom).filter(|n| n.to_lowercase().ends_with(".exe")).collect();
    noms.sort_by_key(|n| n.to_lowercase());
    noms.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    noms
}

#[tauri::command(async)]
fn demarrage_auto_get() -> bool {
    tweaks::demarrage_auto_actif()
}

#[tauri::command(async)]
fn demarrage_auto_set(actif: bool) -> Res<()> {
    tweaks::demarrage_auto(actif)
}

#[tauri::command]
async fn maj_verifier(app: AppHandle) -> Res<maj::InfoMaj> {
    maj::verifier(&app).await
}

#[tauri::command]
async fn maj_installer(app: AppHandle) -> Res<bool> {
    maj::installer(&app).await
}

#[tauri::command(async)]
fn ecrans_liste() -> Vec<ecrans::Ecran> {
    ecrans::liste()
}

#[tauri::command(async)]
fn ecran_frequence(id: String, hz: u32) -> Res<()> {
    ecrans::changer_frequence(&id, hz)
}

#[tauri::command]
fn couleurs_get(app: AppHandle) -> HashMap<String, ecrans::Couleurs> {
    etat(&app).stock.donnees.lock().unwrap().couleurs.clone()
}

#[tauri::command]
fn couleurs_set(app: AppHandle, couleurs: HashMap<String, ecrans::Couleurs>) {
    let e = etat(&app);
    e.stock.donnees.lock().unwrap().couleurs = couleurs;
    e.stock.sauver();
}

/// Aperçu immédiat des couleurs (remises comme avant avec couleurs_retablir).
#[tauri::command(async)]
fn couleurs_essayer(app: AppHandle, couleurs: HashMap<String, ecrans::Couleurs>) -> Vec<String> {
    let erreurs = ecrans::appliquer(&couleurs);
    let e = etat(&app);
    e.stock.donnees.lock().unwrap().vibrance_avant = ecrans::vibrances_modifiees();
    e.stock.sauver();
    erreurs
}

#[tauri::command(async)]
fn couleurs_retablir(app: AppHandle) {
    let e = etat(&app);
    // Pendant une partie, les couleurs de jeu restent en place
    if e.session.lock().unwrap().is_none() {
        jeux::restaurer_couleurs(&e);
    }
}

#[tauri::command(async)]
fn reseau_ping() -> Vec<reseau::ResultatPing> {
    reseau::tester()
}

#[tauri::command(async)]
fn reseau_cartes() -> Res<serde_json::Value> {
    reseau::cartes()
}

#[tauri::command(async)]
fn peripheriques() -> Res<serde_json::Value> {
    pilotes::peripheriques()
}

#[tauri::command]
fn pilote_chercher_web(nom: String, materiel: String) -> Res<()> {
    pilotes::chercher_sur_le_web(&nom, &materiel)
}

#[tauri::command(async)]
fn pilote_nvidia() -> Res<serde_json::Value> {
    pilotes::nvidia()
}

#[tauri::command(async)]
fn pilotes_wu_rechercher() -> Res<serde_json::Value> {
    pilotes::wu_rechercher()
}

#[tauri::command(async)]
fn pilotes_wu_installer(ids: Vec<String>) -> Res<serde_json::Value> {
    pilotes::wu_installer(&ids)
}

#[tauri::command]
fn pilote_nvidia_telecharger(url: String, version: String) -> Res<()> {
    pilotes::telecharger_nvidia(&url, &version)
}

#[tauri::command]
fn pilote_nvidia_progression() -> Option<pilotes::Telechargement> {
    pilotes::progression()
}

#[tauri::command]
fn pilote_nvidia_installer() -> Res<()> {
    pilotes::installer_nvidia()
}

/// Pages officielles des fabricants (liste fermée : l'interface ne peut pas ouvrir n'importe quoi).
#[tauri::command]
fn ouvrir_lien(lien: String) -> Res<()> {
    const AUTORISES: [&str; 3] = [
        "https://www.amd.com/fr/support/download/drivers.html",
        "https://www.intel.com/content/www/us/en/download-center/home.html",
        "https://www.nvidia.com/fr-fr/drivers/",
    ];
    if !AUTORISES.contains(&lien.as_str()) {
        return Err("Lien non autorisé".into());
    }
    systeme::executer("explorer", &[&lien]).map(|_| ()).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _dossier| fenetres::ouvrir_principale(app)))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let dossier = app.path().app_data_dir()?;
            let presentmon = app.path().resource_dir()?.join("PresentMon.exe");
            let etat = Arc::new(Etat {
                stock: Stockage::ouvrir(dossier),
                session: Mutex::new(None),
                fps_live: Arc::new(Mutex::new(None)),
                presentmon,
            });
            // L'app a été coupée en pleine partie : on remet le plan d'alimentation d'avant
            jeux::restaurer_plan(&etat);
            jeux::restaurer_couleurs(&etat);
            app.manage(etat);

            fenetres::creer_icone(app.handle())?;
            if !std::env::args().any(|a| a == "--cache") {
                fenetres::ouvrir_principale(app.handle());
            }
            jeux::lancer(app.handle().clone());
            maj::lancer(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            analyser,
            etat_app,
            tweaks_liste,
            tweaks_appliquer,
            tweaks_annuler,
            nettoyer,
            demarrage_liste,
            demarrage_changer,
            profils_get,
            profils_set,
            profils_defaut,
            reglages_get,
            reglages_set,
            sessions,
            conso,
            processus_liste,
            demarrage_auto_get,
            demarrage_auto_set,
            maj_verifier,
            maj_installer,
            ecrans_liste,
            ecran_frequence,
            couleurs_get,
            couleurs_set,
            couleurs_essayer,
            couleurs_retablir,
            reseau_ping,
            reseau_cartes,
            peripheriques,
            pilote_chercher_web,
            pilote_nvidia,
            pilotes_wu_rechercher,
            pilotes_wu_installer,
            pilote_nvidia_telecharger,
            pilote_nvidia_progression,
            pilote_nvidia_installer,
            ouvrir_lien,
        ])
        .build(tauri::generate_context!())
        .expect("démarrage de FPS Cagoule")
        .run(|app, ev| match ev {
            // Fenêtre fermée : l'app reste dans la zone de notification, sans interface en mémoire
            RunEvent::ExitRequested { api, code: None, .. } => api.prevent_exit(),
            RunEvent::Exit => {
                if let Some(e) = app.try_state::<Arc<Etat>>() {
                    jeux::terminer(app, &e);
                }
            }
            _ => {}
        });
}

// Tests sur un vrai Windows (lancés par la CI GitHub sur windows-latest)
#[cfg(test)]
mod tests_windows {
    use super::*;

    #[test]
    fn analyse_reelle() {
        let info = ps::json(include_str!("analyse.ps1")).expect("analyse PowerShell");
        for cle in ["os", "cpu", "gpus", "ram", "volumes", "plan", "reglages"] {
            assert!(info.get(cle).is_some(), "clé {cle} absente");
        }
        assert!(info["gpus"].is_array() && info["ram"].is_array());
        // Relu par scripts/verifier-analyse.mjs avec l'évaluation de l'interface
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/target/analyse.json"), info.to_string()).unwrap();
    }

    #[test]
    fn processus_et_plan() {
        let procs = systeme::processus();
        assert!(procs.iter().any(|p| p.pid == std::process::id()));
        assert!(systeme::plan_actif().is_some());
        assert!(systeme::memoire(std::process::id()) > 0);
    }

    #[test]
    fn ecrans_et_reseau() {
        // Le PC de GitHub a un écran virtuel : la liste ne doit pas planter, même vide
        let _ = ecrans::liste();
        let ping = reseau::tester();
        assert!(!ping.is_empty());
        assert!(reseau::cartes().is_ok());
    }

    #[test]
    fn programmes_au_demarrage() {
        // Aller-retour sur une entrée de test, sans toucher aux vrais programmes
        let (run, _) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run")
            .unwrap();
        run.set_value("FPSCagouleTest", &"notepad.exe").unwrap();
        let trouve = |l: &[demarrage::Programme]| l.iter().find(|p| p.nom == "FPSCagouleTest").map(|p| p.actif);
        assert_eq!(trouve(&demarrage::liste()), Some(true));
        demarrage::changer("hkcu-run", "FPSCagouleTest", false).unwrap();
        assert_eq!(trouve(&demarrage::liste()), Some(false));
        demarrage::changer("hkcu-run", "FPSCagouleTest", true).unwrap();
        assert_eq!(trouve(&demarrage::liste()), Some(true));
        run.delete_value("FPSCagouleTest").unwrap();
    }

    #[test]
    fn boost_max() {
        assert!(systeme::minuteur_precis(true), "minuteur 0,5 ms");
        assert!(systeme::minuteur_precis(false));
        // Le PC de GitHub tourne en administrateur, comme l'app
        systeme::vider_memoire_veille().expect("mémoire en veille");
        assert!(systeme::memoire_libre() > 0);
    }

    #[test]
    fn pilotes() {
        let liste = pilotes::peripheriques().expect("liste des périphériques");
        assert!(liste.as_array().is_some_and(|l| l.len() > 10));
        // Pas de carte NVIDIA sur le PC de GitHub : la réponse doit être null, sans erreur
        assert!(pilotes::nvidia().expect("recherche NVIDIA").is_null());
        assert!(pilotes::wu_rechercher().expect("recherche Windows Update").is_array());
    }

    #[test]
    fn lecture_des_optimisations() {
        let dossier = std::env::temp_dir().join("fps-cagoule-test");
        let stock = Stockage::ouvrir(dossier);
        let liste = tweaks::liste(&stock);
        assert_eq!(liste.len(), 36);
        assert!(liste.iter().any(|t| t.tweak.categorie == "reseau"));
    }
}
