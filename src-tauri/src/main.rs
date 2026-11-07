// FPS Cagoule : analyse, optimisation et boost par jeu pour Windows 11.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod ecrans;
mod fenetres;
mod fps;
mod jeux;
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
    presentmon: bool,
}

#[tauri::command]
fn etat_app(app: AppHandle) -> EtatApp {
    let e = etat(&app);
    let plan_perso = e.stock.donnees.lock().unwrap().plan_perso.clone();
    EtatApp {
        version: app.package_info().version.to_string(),
        jeu: e.info_jeu(),
        plan_perso,
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
fn ouvrir_demarrage() -> Res<()> {
    systeme::executer("explorer", &["ms-settings:startupapps"]).map(|_| ()).map_err(|e| e.to_string())
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
            ouvrir_demarrage,
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
    fn lecture_des_optimisations() {
        let dossier = std::env::temp_dir().join("fps-cagoule-test");
        let stock = Stockage::ouvrir(dossier);
        let liste = tweaks::liste(&stock);
        assert_eq!(liste.len(), 16);
        assert!(liste.iter().any(|t| t.tweak.categorie == "reseau"));
    }
}
