// FPS Cagoule : analyse, optimisation et boost par jeu pour Windows 11.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod amd;
mod benchmark;
mod bibliotheques;
mod bios;
mod config;
mod demarrage;
mod dns;
mod edition;
mod ecrans;
mod fenetres;
mod fps;
mod gpu;
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
    alimentation: systeme::Alimentation,
    /// Message de baisse automatique de l'OC après un plantage (montré une fois)
    oc_baisse_auto: Option<String>,
}

#[tauri::command]
fn etat_app(app: AppHandle) -> EtatApp {
    let e = etat(&app);
    let (plan_perso, plan_max, oc_baisse_auto) = {
        let mut d = e.stock.donnees.lock().unwrap();
        (d.plan_perso.clone(), d.plan_max.clone(), d.oc_baisse_auto.take())
    };
    if oc_baisse_auto.is_some() {
        e.stock.sauver();
    }
    EtatApp {
        version: app.package_info().version.to_string(),
        jeu: e.info_jeu(),
        plan_perso,
        plan_max,
        presentmon: e.presentmon.exists(),
        alimentation: systeme::alimentation(),
        oc_baisse_auto,
    }
}

#[tauri::command(async)]
fn bios_infos() -> Res<serde_json::Value> {
    bios::infos()
}

#[tauri::command]
fn redemarrer_bios(app: AppHandle) -> Res<()> {
    jeux::terminer(&app, &etat(&app));
    bios::redemarrer_dans_le_bios()
}

#[tauri::command(async)]
fn temperature_gpu() -> Option<i32> {
    nvidia::temperature().or_else(|| amd::avec(|s| Ok(s.temperature())).ok().flatten())
}

#[tauri::command(async)]
fn jeux_installes() -> Vec<bibliotheques::JeuTrouve> {
    bibliotheques::chercher()
}

#[tauri::command(async)]
fn carte_graphique() -> Option<gpu::Carte> {
    gpu::principale()
}

#[derive(Serialize)]
struct EtatAmd {
    nom: String,
    /// Indices des préréglages proposés (0 Économie, 1 Silencieux, 2 Équilibré, 3 Turbo, 4 Rage)
    prereglages: Vec<usize>,
    actuel: Option<usize>,
    temperature: Option<i32>,
    logiciel: bool,
}

#[tauri::command(async)]
fn amd_etat() -> Res<EtatAmd> {
    amd::avec(|s| {
        let (prereglages, actuel) = s.prereglages().unwrap_or_default();
        Ok(EtatAmd { nom: s.nom(), prereglages, actuel, temperature: s.temperature(), logiciel: gpu::logiciel("amd").is_some() })
    })
}

#[tauri::command(async)]
fn amd_prereglage(index: usize) -> Res<()> {
    amd::avec(|s| s.choisir_prereglage(index))
}

#[tauri::command(async)]
fn amd_usine() -> Res<()> {
    amd::avec(|s| s.reglages_usine())
}

/// Ouvre Adrenalin ou le logiciel Intel, s'il est installé
#[tauri::command]
fn ouvrir_logiciel_gpu(fabricant: String) -> Res<()> {
    let chemin = gpu::logiciel(&fabricant).ok_or("Logiciel de la carte graphique introuvable")?;
    std::process::Command::new(chemin).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn dns_etat(app: AppHandle) -> Res<dns::EtatDns> {
    dns::etat(&etat(&app).stock)
}

#[tauri::command(async)]
fn dns_appliquer(app: AppHandle, id: String) -> Res<()> {
    dns::appliquer(&etat(&app).stock, &id)
}

#[tauri::command(async)]
fn dns_restaurer(app: AppHandle) -> Res<()> {
    dns::restaurer(&etat(&app).stock)
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

#[derive(Serialize)]
struct EtatOcApp {
    #[serde(flatten)]
    carte: nvidia::EtatOc,
    enregistre: Option<config::ReglageOc>,
    coupe_apres_plantage: bool,
    bornes: [i32; 4],
}

#[tauri::command(async)]
fn oc_etat(app: AppHandle) -> Res<EtatOcApp> {
    let e = etat(&app);
    let carte = nvidia::lire_oc()?;
    let mut d = e.stock.donnees.lock().unwrap();
    // Le message d'arrêt brutal n'est montré qu'une fois
    let coupe = std::mem::take(&mut d.oc_coupe_apres_plantage);
    let enregistre = d.oc.clone();
    drop(d);
    if coupe {
        e.stock.sauver();
    }
    Ok(EtatOcApp { carte, enregistre, coupe_apres_plantage: coupe, bornes: [nvidia::COEUR_MIN, nvidia::COEUR_MAX, nvidia::MEMOIRE_MIN, nvidia::MEMOIRE_MAX] })
}

/// Applique sans enregistrer (essais et mode automatique).
#[tauri::command(async)]
fn oc_essayer(coeur: i32, memoire: i32, puissance: Option<f64>) -> Res<()> {
    nvidia::appliquer_oc(coeur, memoire)?;
    if let Some(p) = puissance {
        nvidia::appliquer_puissance(p)?;
    }
    Ok(())
}

#[tauri::command(async)]
fn oc_enregistrer(app: AppHandle, reglage: config::ReglageOc) -> Res<()> {
    nvidia::appliquer_oc(reglage.coeur_mhz, reglage.memoire_mhz)?;
    if let Some(p) = reglage.puissance_pct {
        nvidia::appliquer_puissance(p)?;
    }
    let e = etat(&app);
    e.stock.donnees.lock().unwrap().oc = Some(reglage);
    e.stock.sauver();
    Ok(())
}

#[tauri::command(async)]
fn oc_defaut(app: AppHandle) -> Res<()> {
    nvidia::remettre_par_defaut()?;
    let e = etat(&app);
    e.stock.donnees.lock().unwrap().oc = None;
    e.stock.sauver();
    Ok(())
}

#[tauri::command(async)]
fn memoire_programmes() -> systeme::EtatMemoire {
    systeme::memoire_par_programme()
}

#[tauri::command(async)]
fn fermer_programme(nom: String) -> Res<u32> {
    systeme::fermer_programme(&nom)
}

#[tauri::command(async)]
fn processeur() -> systeme::InfoProcesseur {
    systeme::info_processeur()
}

#[tauri::command]
fn ouvrir_ryzen_master() -> Res<()> {
    let chemin = systeme::info_processeur().ryzen_master.ok_or("AMD Ryzen Master n'est pas installé")?;
    std::process::Command::new(chemin).spawn().map(|_| ()).map_err(|e| e.to_string())
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
    const AUTORISES: [&str; 5] = [
        "https://www.amd.com/fr/products/software/ryzen-master.html",
        "https://www.intel.com/content/www/us/en/download/17881/intel-extreme-tuning-utility-intel-xtu.html",
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
            jeux::oc_au_demarrage(&etat);
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
            memoire_programmes,
            fermer_programme,
            processeur,
            ouvrir_ryzen_master,
            oc_etat,
            oc_essayer,
            oc_enregistrer,
            oc_defaut,
            temperature_gpu,
            carte_graphique,
            jeux_installes,
            amd_etat,
            amd_prereglage,
            amd_usine,
            ouvrir_logiciel_gpu,
            bios_infos,
            redemarrer_bios,
            dns_etat,
            dns_appliquer,
            dns_restaurer,
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
        let m = systeme::memoire_par_programme();
        assert!(!m.programmes.is_empty() && m.totale_mo > m.libre_mo);
        assert!(systeme::fermer_programme("svchost.exe").is_err());
        assert!(systeme::fermer_programme("Memory Compression").is_err());
        assert!(!systeme::info_processeur().nom.is_empty());
        // Priorité basse d'un programme de fond, puis remise comme avant
        // Lancé en priorité normale explicitement : le test lui-même peut tourner en priorité basse sur la CI
        use std::os::windows::process::CommandExt;
        let mut enfant = std::process::Command::new("ping")
            .args(["-n", "6", "127.0.0.1"])
            .creation_flags(0x0000_0020)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let avant = systeme::priorite_basse(enfant.id()).expect("priorité basse");
        assert!(systeme::priorite_basse(enfant.id()).is_none(), "déjà en priorité basse : pas touché deux fois");
        assert!(systeme::remettre_priorite(enfant.id(), avant));
        let _ = enfant.kill();
        let a = systeme::alimentation();
        assert!(!a.sur_batterie || a.portable);
        // Requêtes du journal d'événements valides (une requête mal formée renverrait faux aussi : on vérifie
        // donc qu'une requête large trouve bien quelque chose)
        let _ = systeme::plantage_depuis(3600, "RainbowSix.exe");
        assert!(systeme::plantage_depuis_test());
        assert_eq!(jeux::baisser(100, 25), 75);
        assert_eq!(jeux::baisser(10, 25), 0);
        assert_eq!(jeux::baisser(-50, 25), -50);
    }

    #[test]
    fn dns() {
        let ip: std::net::Ipv4Addr = "1.1.1.1".parse().unwrap();
        assert!(dns::mesurer(ip).is_some_and(|ms| ms > 0.0 && ms < 1500.0));
        let cartes = dns::cartes().expect("cartes réseau");
        assert!(!cartes.is_empty());
        let stock = Stockage::ouvrir(std::env::temp_dir().join("fps-cagoule-test-dns"));
        let e = dns::etat(&stock).expect("mesure des DNS");
        assert!(e.mesures.iter().any(|m| m.ms.is_some()));
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
        assert!(nvidia::lire_oc().is_err());
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
    fn jeux_installes() {
        // Faux jeu : l'exe « Shipping » passe en premier, puis le plus gros ; installeurs et anti-triche écartés
        let d = std::env::temp_dir().join("fps-cagoule-test-jeu");
        let _ = std::fs::remove_dir_all(&d);
        let gros = vec![0u8; 3 << 20];
        let moyen = vec![0u8; 2 << 20];
        for (f, contenu) in [
            ("MonJeu.exe", &moyen),
            ("unins000.exe", &gros),
            (r"_CommonRedist\vc_redist.x64.exe", &gros),
            (r"EasyAntiCheat\EasyAntiCheat_EOS_Setup.exe", &gros),
            (r"MonJeu\Binaries\Win64\MonJeu-Win64-Shipping.exe", &moyen),
            ("CrashReporter.exe", &gros),
            ("petit.exe", &vec![0u8; 1000]),
        ] {
            let p = d.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, contenu).unwrap();
        }
        assert_eq!(bibliotheques::exes_du_jeu(&d), vec!["MonJeu-Win64-Shipping.exe", "MonJeu.exe"]);
        let _ = std::fs::remove_dir_all(&d);
        // Aucun jeu sur le PC de GitHub : la recherche se fait sans erreur
        let _ = bibliotheques::chercher();
    }

    #[test]
    fn bios() {
        let b = bios::infos().expect("infos du BIOS");
        assert!(b["biosVersion"].is_string() && b["ram"].is_array(), "{b}");
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/target/bios.json"), b.to_string()).unwrap();
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
        assert_eq!(liste.len(), 72);
        // Pas de carte AMD sur le PC de GitHub : réglages AMD indisponibles, sans erreur
        assert!(liste.iter().filter(|t| t.tweak.special.as_deref() == Some("amd")).all(|t| !t.disponible));
        assert!(amd::avec(|s| Ok(s.nom())).is_err());
        let _ = gpu::principale();
        assert_eq!(gpu::decrire("NVIDIA GeForce RTX 3060", r"pci\ven_10de&dev_2503").fabricant, "nvidia");
        let rx = gpu::decrire("AMD Radeon RX 7800 XT", r"pci\ven_1002&dev_747e");
        assert!(rx.fabricant == "amd" && rx.dediee);
        assert!(!gpu::decrire("AMD Radeon(TM) Graphics", r"pci\ven_1002&dev_1638").dediee);
        assert!(gpu::decrire("Intel(R) Arc(TM) A770 Graphics", r"pci\ven_8086&dev_56a0").dediee);
        // Services : présents sur le PC de GitHub ou proprement indisponibles
        let services: Vec<_> = liste.iter().filter(|t| t.tweak.special.as_deref() == Some("service")).collect();
        assert_eq!(services.len(), 11);
        assert!(services.iter().any(|t| t.disponible), "aucun service trouvé");
        assert!(liste.iter().any(|t| t.tweak.categorie == "reseau"));
    }
}
