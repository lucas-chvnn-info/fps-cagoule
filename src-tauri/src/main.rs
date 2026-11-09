// FPS Cagoule : analyse, optimisation et boost par jeu pour Windows 11.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod benchmark;
mod config;
mod demarrage;
mod edition;
mod ecrans;
mod fenetres;
mod fps;
mod jeux;
mod jeux_config;
mod nettoyage;
mod nvidia;
mod pilotes;
mod maj;
mod programmes;
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
fn reparer_msconfig() -> Res<Vec<String>> {
    tweaks::reparer_limites_demarrage()
}

#[tauri::command(async)]
fn nettoyage_analyser() -> Vec<nettoyage::Categorie> {
    nettoyage::analyser()
}

#[tauri::command(async)]
fn nettoyage_nettoyer(ids: Vec<String>) -> Vec<nettoyage::Bilan> {
    nettoyage::nettoyer(&ids)
}

#[tauri::command(async)]
fn applis_windows() -> Res<Vec<programmes::AppliWindows>> {
    programmes::applis_windows()
}

#[tauri::command(async)]
fn applis_windows_supprimer(paquets: Vec<String>) -> Res<serde_json::Value> {
    programmes::supprimer_applis(&paquets)
}

#[tauri::command(async)]
fn jeux_configs(app: AppHandle) -> Vec<jeux_config::EtatJeu> {
    jeux_config::liste(&etat(&app).stock)
}

#[tauri::command(async)]
fn jeux_config_appliquer(app: AppHandle, id: String) -> Res<usize> {
    jeux_config::appliquer(&etat(&app).stock, &id)
}

#[tauri::command(async)]
fn jeux_config_restaurer(app: AppHandle, id: String) -> Res<()> {
    jeux_config::restaurer(&etat(&app).stock, &id)
}

#[tauri::command(async)]
fn benchmark_cpu(duree: f64) -> benchmark::ResultatCpu {
    benchmark::cpu(duree.clamp(1.0, 10.0))
}

#[tauri::command(async)]
fn benchmark_ram() -> benchmark::ResultatRam {
    benchmark::ram(3.0)
}

#[tauri::command(async)]
fn benchmark_disque() -> Res<benchmark::ResultatDisque> {
    benchmark::disque(512, 4.0)
}

#[tauri::command(async)]
fn benchmark_enregistrer(app: AppHandle, resultat: benchmark::Resultat) -> Vec<benchmark::Resultat> {
    benchmark::enregistrer(&etat(&app).stock, resultat)
}

#[tauri::command(async)]
fn benchmarks(app: AppHandle) -> Vec<benchmark::Resultat> {
    benchmark::historique(&etat(&app).stock)
}

#[tauri::command(async)]
fn optimiser_disques() -> Res<serde_json::Value> {
    nettoyage::optimiser_disques()
}

#[tauri::command(async)]
fn programmes_liste() -> Vec<programmes::Programme> {
    programmes::liste()
}

#[tauri::command]
fn programme_desinstaller(id: String) -> Res<()> {
    programmes::desinstaller(&id)
}

#[tauri::command]
fn redemarrer_pc(app: AppHandle) -> Res<()> {
    // Le plan d'alimentation d'avant la partie est remis avant l'arrêt
    jeux::terminer(&app, &etat(&app));
    systeme::redemarrer()
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
    jeux::synchroniser_priorites(&e.stock);
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
    jeux::synchroniser_priorites(&e.stock);
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

/// Essaie un mode d'affichage : sans confirmation sous 15 s, l'ancien revient tout seul.
#[tauri::command(async)]
fn ecran_mode_essayer(id: String, largeur: u32, hauteur: u32, hz: Option<u32>, echelle: Option<u32>) -> Res<Option<String>> {
    ecrans::essayer_mode(&id, largeur, hauteur, hz, echelle)
}

#[tauri::command(async)]
fn ecran_mode_confirmer(id: String) -> Res<()> {
    ecrans::confirmer_mode(&id)
}

#[tauri::command(async)]
fn ecran_mode_annuler(id: String) {
    ecrans::annuler_essai(&id)
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
            jeux::restaurer_resolution(&etat);
            jeux::synchroniser_priorites(&etat.stock);
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
            nettoyage_analyser,
            reparer_msconfig,
            nettoyage_nettoyer,
            programmes_liste,
            optimiser_disques,
            benchmark_cpu,
            benchmark_ram,
            benchmark_disque,
            benchmark_enregistrer,
            benchmarks,
            jeux_configs,
            jeux_config_appliquer,
            jeux_config_restaurer,
            applis_windows,
            applis_windows_supprimer,
            programme_desinstaller,
            redemarrer_pc,
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
            ecran_mode_essayer,
            ecran_mode_confirmer,
            ecran_mode_annuler,
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
        // Le PC de GitHub a un écran virtuel : on réessaie son mode actuel, puis on annule
        for e in ecrans::liste() {
            assert!(!e.modes.is_empty(), "aucune résolution pour {}", e.id);
            ecrans::essayer_mode(&e.id, e.largeur, e.hauteur, Some(e.hz), None).expect("essai du mode actuel");
            ecrans::annuler_essai(&e.id);
        }
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
    fn alimentation_et_nettoyage() {
        // Le mode d'alimentation n'existe pas forcément sur Windows Server : on vérifie seulement que rien ne plante
        let _ = (systeme::mode_alimentation(), systeme::veille_moderne());
        assert!(systeme::demarrage_windows() > 1_600_000_000);
        let categories = nettoyage::analyser();
        assert_eq!(categories.len(), 13);
        let bilan = nettoyage::nettoyer(&["temp".into(), "rapports".into()]);
        assert_eq!(bilan.len(), 2);
        assert!(!programmes::liste().is_empty());
        programmes::applis_windows().expect("liste des applis préinstallées");
    }

    #[test]
    fn priorite_au_lancement() {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        let chemin = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\fpscagouletest.exe\PerfOptions";
        let stock = Stockage::ouvrir(std::env::temp_dir().join("fps-cagoule-test-priorite"));
        stock.donnees.lock().unwrap().profils = vec![Profil { nom: "Test".into(), exes: vec!["FPSCagouleTest.exe".into()], ..Default::default() }];
        jeux::synchroniser_priorites(&stock);
        let lire = || winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(chemin).and_then(|k| k.get_value::<u32, _>("CpuPriorityClass")).ok();
        assert_eq!(lire(), Some(3));
        stock.donnees.lock().unwrap().profils.clear();
        jeux::synchroniser_priorites(&stock);
        assert_eq!(lire(), None);
        assert!(stock.donnees.lock().unwrap().priorites.is_empty());
        // Pas de carte NVIDIA sur le PC de GitHub : erreur propre, sans plantage
        assert!(nvidia::lire(0x007B_A09E).is_err());
    }

    #[test]
    fn reglages_dans_les_jeux() {
        use edition::{Changement, Format};
        // Fichier UTF-16 en lecture seule, comme certains joueurs les laissent
        let f = std::env::temp_dir().join("fps-cagoule-test-GameUserSettings.ini");
        if let Ok(m) = std::fs::metadata(&f) {
            let mut p = m.permissions();
            p.set_readonly(false);
            let _ = std::fs::set_permissions(&f, p);
            let _ = std::fs::remove_file(&f);
        }
        std::fs::write(&f, edition::encoder("[ScalabilityGroups]\r\nsg.ShadowQuality=3\r\n", edition::Encodage::Utf16)).unwrap();
        let mut p = std::fs::metadata(&f).unwrap().permissions();
        p.set_readonly(true);
        std::fs::set_permissions(&f, p).unwrap();
        let ch = [Changement { section: Some("ScalabilityGroups"), cle: "sg.ShadowQuality", valeur: "0" }];
        assert_eq!(jeux_config::optimiser_fichier(&f, Format::Ini, &ch).unwrap(), 1);
        let (texte, enc) = edition::decoder(&std::fs::read(&f).unwrap());
        assert_eq!((texte.as_str(), enc), ("[ScalabilityGroups]\r\nsg.ShadowQuality=0\r\n", edition::Encodage::Utf16));
        assert!(std::fs::metadata(&f).unwrap().permissions().readonly());
        // Aucun jeu installé sur le PC de GitHub : la liste se fait sans erreur
        let stock = Stockage::ouvrir(std::env::temp_dir().join("fps-cagoule-test-jeux"));
        assert_eq!(jeux_config::liste(&stock).len(), 8);
    }

    #[test]
    fn benchmark_court() {
        // Valeurs brutes du PC de GitHub (référence des points), relues par la CI
        let c = benchmark::cpu(2.0);
        let r = benchmark::ram(2.0);
        let d = benchmark::disque(256, 2.0).expect("benchmark disque");
        assert!(c.un_coeur > 0.0 && c.tous >= c.un_coeur && r.debit > 0.0 && r.latence > 0.0 && d.lecture > 0.0);
        let brut = serde_json::json!({ "cpu": c, "ram": r, "disque": d });
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/target/benchmark.json"), brut.to_string()).unwrap();
    }

    #[test]
    fn disques() {
        let r = nettoyage::optimiser_disques().expect("optimisation des disques");
        assert!(r.as_array().is_some_and(|l| !l.is_empty()), "{r}");
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
        assert_eq!(liste.len(), 56);
        assert!(liste.iter().any(|t| t.tweak.categorie == "reseau"));
    }
}
