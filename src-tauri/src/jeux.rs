// Surveillance des jeux en arrière-plan et boost automatique.
// Un seul fil qui dort la plupart du temps : un instantané des processus toutes les 4 s.

use crate::config::{Session, Stockage};
use crate::{fps, stats, systeme, tweaks};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

pub struct EnCours {
    pub profil: String,
    pub exe: String,
    pub pid: u32,
    debut: Instant,
    debut_unix: u64,
    boost: bool,
    pub actions: Vec<String>,
    capture: Option<fps::Capture>,
    /// Vidages de la mémoire en veille pendant la partie
    derniere_purge: Option<Instant>,
    purges: u32,
    /// Température maximale de la carte graphique (°C) et dernière lecture
    temp_max: Option<i32>,
    temp: Option<i32>,
    /// Overclocking mis de côté pour ce jeu (décalages à remettre à la fin)
    oc_suspendu: Option<(i32, i32)>,
    /// Partie jouée avec un overclocking actif
    oc_actif: bool,
    /// Programmes de fond passés en priorité basse : (pid, priorité d'avant)
    abaisses: Vec<(u32, u32)>,
    /// Pids déjà vus (abaissés ou laissés tels quels), pour ne pas les retraiter toutes les 3 s
    vus: Vec<u32>,
    /// Statut Discord de la partie (effacé quand il est lâché)
    presence: Option<crate::discord::Presence>,
    /// Enregistrement pour les clips (arrêté et tampon effacé quand il est lâché)
    pub enregistreur: Option<crate::clips::Enregistreur>,
    /// Replays de manches de R6 déjà traités pour les clips automatiques
    replays_vus: Vec<PathBuf>,
    debut_systeme: SystemTime,
    /// Compte Ubisoft qui joue (lu dans le premier replay de la partie), pour nommer les clips de la touche
    pseudo_r6: Option<String>,
    /// Clips faits à la touche pendant la partie : (fichier, fin, durée en s), pour y ajouter les repères des kills
    clips_manuels: Vec<(PathBuf, SystemTime, f64)>,
    /// Dossiers « MatchReplay » de R6 trouvés pour cette partie
    dossiers_replays: Vec<PathBuf>,
    /// Démarrage de l'enregistrement déjà tenté pour cette partie
    clips_lances: bool,
    /// Recherche des dossiers de replays déjà notée dans le journal
    replays_cherches: bool,
    /// Anciennes parties des dossiers de replays déjà relues pour l'historique des rencontres
    rencontres_relues: bool,
    /// Pourquoi l'enregistrement n'a pas pu démarrer
    pub clips_erreur: Option<String>,
}

pub struct Etat {
    pub stock: Stockage,
    pub session: Mutex<Option<EnCours>>,
    pub fps_live: Arc<Mutex<Option<f64>>>,
    pub presentmon: PathBuf,
    /// Touche de sauvegarde des clips (raccourci global)
    pub raccourci: Mutex<Option<crate::raccourci::Raccourci>>,
    /// Ctrl+X : tableau de la partie de R6 (enregistrée seulement pendant le jeu, Ctrl+X reste « couper » ailleurs)
    pub touche_tableau: Mutex<Option<crate::raccourci::Raccourci>>,
    /// ↑ ↓ Entrée pour naviguer dans le tableau : (touche, virtual-key), prises seulement tant qu'il est ouvert
    pub touches_navigation: Mutex<Vec<(crate::raccourci::Raccourci, u32)>>,
    /// Dégâts de la dernière manche de R6 (fenêtre de fin de manche)
    /// Dégâts des manches de R6 lues depuis le lancement (fenêtre de fin de manche : la dernière et celles d'avant)
    pub degats: Mutex<Vec<crate::degats_r6::Bilan>>,
    /// Coups de chaque tireur (« pseudo|opérateur ») dans les manches déjà lues : dégât de base pour les zones touchées
    pub degats_historique: Mutex<std::collections::HashMap<String, Vec<u32>>>,
    /// Joueurs croisés dans R6 (rencontres.json)
    pub rencontres: Mutex<crate::rencontres::Rencontres>,
    /// Stats Ubisoft des joueurs croisés : profil → (demandées à, stats ; None = rien reçu, nom caché…)
    pub ubisoft: Mutex<std::collections::HashMap<String, (Instant, Option<crate::ubisoft::StatsUbi>)>>,
}

/// Stats Ubisoft de profils, depuis le cache (20 min) ou la source : le compte Ubisoft perso s'il y en a un,
/// sinon le serveur FPS Cagoule. Chaque saison reçue est aussi gardée sur le PC. None = rien reçu (nom caché…).
pub fn stats_ubisoft(etat: &Etat, profils: &[String]) -> Result<std::collections::HashMap<String, Option<crate::ubisoft::StatsUbi>>, String> {
    let (actif, perso, serveur) = {
        let r = &etat.stock.donnees.lock().unwrap().reglages;
        (r.tracker_r6, r.ubisoft_pseudo.is_some(), r.tracker_serveur.clone())
    };
    if !actif {
        return Err("Le tracker est désactivé.".into());
    }
    let profils: Vec<String> = profils.iter().filter(|p| !p.is_empty() && !p.starts_with("pseudo:")).map(|p| p.to_lowercase()).collect();
    let manquants: Vec<String> = {
        let cache = etat.ubisoft.lock().unwrap();
        profils.iter().filter(|p| cache.get(*p).is_none_or(|(t, _)| t.elapsed() > Duration::from_secs(20 * 60))).cloned().collect()
    };
    if !manquants.is_empty() {
        let mut recues: std::collections::HashMap<String, crate::ubisoft::StatsUbi> = if perso {
            crate::ubisoft::stats(&manquants)?
        } else {
            crate::ubisoft::stats_serveur(&serveur, &manquants)?.into_iter().filter_map(|(p, s)| s.map(|s| (p, s))).collect()
        };
        crate::ubisoft::pics_generaux(etat.stock.dossier(), &mut recues);
        let mut cache = etat.ubisoft.lock().unwrap();
        for p in &manquants {
            cache.insert(p.clone(), (Instant::now(), recues.remove(p)));
        }
    }
    let cache = etat.ubisoft.lock().unwrap();
    Ok(profils.iter().map(|p| (p.clone(), cache.get(p).and_then(|(_, s)| s.clone()))).collect())
}

/// Stats Ubisoft des profils pas encore en cache, en arrière-plan ; « degats » prévient les fenêtres à l'arrivée
pub fn rafraichir_ubisoft(app: &AppHandle, profils: Vec<String>) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static EN_COURS: AtomicBool = AtomicBool::new(false);
    let etat = app.state::<Arc<Etat>>();
    if !etat.stock.donnees.lock().unwrap().reglages.tracker_r6 {
        return;
    }
    let manquants = {
        let cache = etat.ubisoft.lock().unwrap();
        profils.iter().any(|p| !p.is_empty() && !p.starts_with("pseudo:") && cache.get(&p.to_lowercase()).is_none_or(|(t, _)| t.elapsed() > Duration::from_secs(20 * 60)))
    };
    if !manquants || EN_COURS.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    let _ = thread::Builder::new().name("ubisoft".into()).spawn(move || {
        let etat = app.state::<Arc<Etat>>();
        match stats_ubisoft(&etat, &profils) {
            Ok(_) => {
                let _ = app.emit("degats", ());
            }
            Err(e) => crate::clips::noter(&etat.stock, &format!("Stats Ubisoft : {e}")),
        }
        EN_COURS.store(false, Ordering::SeqCst);
    });
}

#[derive(Serialize, Clone)]
pub struct InfoJeu {
    pub nom: String,
    pub exe: String,
    pub depuis_s: u64,
    pub boost: bool,
    pub actions: Vec<String>,
    pub fps: Option<f64>,
    pub temp_gpu: Option<i32>,
}

impl Etat {
    pub fn info_jeu(&self) -> Option<InfoJeu> {
        let s = self.session.lock().unwrap();
        s.as_ref().map(|s| InfoJeu {
            nom: s.profil.clone(),
            exe: s.exe.clone(),
            depuis_s: s.debut.elapsed().as_secs(),
            boost: s.boost,
            actions: s.actions.clone(),
            fps: *self.fps_live.lock().unwrap(),
            temp_gpu: s.temp,
        })
    }
}

pub fn lancer(app: AppHandle) {
    thread::Builder::new()
        .name("surveillance-jeux".into())
        .spawn(move || loop {
            let en_jeu = verifier(&app);
            thread::sleep(Duration::from_secs(if en_jeu { 3 } else { 4 }));
        })
        .expect("fil de surveillance");
}

/// Renvoie vrai si un jeu est en cours.
fn verifier(app: &AppHandle) -> bool {
    let etat = app.state::<Arc<Etat>>();
    let procs = systeme::processus();
    let pid_en_cours = etat.session.lock().unwrap().as_ref().map(|s| s.pid);
    match pid_en_cours {
        Some(pid) => {
            if !procs.iter().any(|p| p.pid == pid) {
                terminer(app, &etat);
                return false;
            }
            entretenir_memoire(&etat);
            suivre_temperature(app, &etat);
            maj_discord(&etat);
            lancer_clips_si_besoin(app, &etat, pid);
            surveiller_clips(app, &etat, pid);
            surveiller_replays(app, &etat, pid);
            abaisser_fond(&etat, &procs);
            true
        }
        None => {
            let (profils, reglages) = {
                let d = etat.stock.donnees.lock().unwrap();
                (d.profils.clone(), d.reglages.clone())
            };
            if !reglages.boost_auto && !reglages.mesure_fps {
                return false;
            }
            for p in procs.iter() {
                if let Some(profil) = profils.iter().find(|pr| pr.actif && pr.exes.iter().any(|e| e.eq_ignore_ascii_case(&p.nom))) {
                    demarrer(app, &etat, profil, p.pid, &p.nom, &procs);
                    return true;
                }
            }
            false
        }
    }
}

fn demarrer(app: &AppHandle, etat: &Etat, profil: &crate::config::Profil, pid: u32, exe: &str, procs: &[systeme::Processus]) {
    let reglages = etat.stock.donnees.lock().unwrap().reglages.clone();
    let boost = reglages.boost_auto && profil.boost;
    let mut actions = Vec::new();

    let alim = systeme::alimentation();
    if alim.sur_batterie {
        actions.push("Sur batterie : branche le chargeur, Windows bride le processeur et la carte graphique".into());
    }
    // Overclocking : mis de côté pour les jeux où il est désactivé, sinon suivi pour détecter les plantages
    let mut oc_suspendu = None;
    let mut oc_actif = false;
    if let Some(actuel) = crate::nvidia::oc_actuel().filter(|&(c, m)| c != 0 || m != 0) {
        if profil.oc {
            oc_actif = true;
        } else if crate::nvidia::appliquer_oc(0, 0).is_ok() {
            oc_suspendu = Some(actuel);
            actions.push("Overclocking mis en pause pour ce jeu".into());
        }
    }

    if boost {
        // Plan d'alimentation (ou, sur les PC en veille moderne, mode d'alimentation de Windows 11) :
        // l'état d'avant est mémorisé sur disque, au cas où l'app serait coupée en pleine partie.
        let (plan_perso, plan_max) = {
            let d = etat.stock.donnees.lock().unwrap();
            (d.plan_perso.clone(), d.plan_max.clone())
        };
        let actuel = systeme::plan_actif().unwrap_or_default();
        let deja = if reglages.boost_max {
            actuel == systeme::PLAN_OPTIMAL || Some(&actuel) == plan_max.as_ref()
        } else {
            systeme::est_plan_perf(&actuel, &[plan_perso.clone(), plan_max.clone()])
        };
        if !deja {
            let essai = if reglages.boost_max {
                systeme::passer_en_performances(systeme::PLAN_OPTIMAL, plan_max.as_deref(), "Performances optimales")
            } else {
                systeme::passer_en_performances(systeme::PLAN_PERF, plan_perso.as_deref(), "Performances élevées")
            };
            match essai {
                Ok(p) => {
                    let mut d = etat.stock.donnees.lock().unwrap();
                    if let Some(cree) = p.plan_cree {
                        if reglages.boost_max { d.plan_max = Some(cree) } else { d.plan_perso = Some(cree) }
                    }
                    if p.retour.is_some() {
                        d.retour_avant_jeu = p.retour;
                        actions.push(format!("Alimentation : {}", p.libelle));
                    }
                    drop(d);
                    etat.stock.sauver();
                }
                Err(e) => actions.push(format!("Alimentation : {e}")),
            }
        }
        if reglages.boost_max {
            if systeme::minuteur_precis(true) {
                actions.push("Minuteur système à 0,5 ms".into());
            }
            match systeme::vider_memoire_veille() {
                Ok(o) => actions.push(format!("Mémoire en veille vidée ({:.1} Go libérés)", o as f64 / 1073741824.0)),
                Err(e) => actions.push(format!("Mémoire en veille : {e}")),
            }
        }
        if profil.priorite_haute {
            actions.push(if systeme::priorite_haute(pid) {
                "Priorité haute donnée au jeu".into()
            } else {
                "Priorité refusée par l'anti-triche du jeu".into()
            });
        }
        if profil.gpu_perf {
            if let Some(chemin) = systeme::chemin_exe(pid) {
                if tweaks::gpu_performance(&chemin).is_ok() {
                    actions.push("Carte graphique la plus puissante choisie".into());
                }
            }
        }
        let mut fermees = Vec::new();
        for p in procs {
            if reglages.fermer_applis.iter().any(|a| a.eq_ignore_ascii_case(&p.nom)) && systeme::terminer(p.pid) && !fermees.contains(&p.nom) {
                fermees.push(p.nom.clone());
            }
        }
        if !fermees.is_empty() {
            actions.push(format!("Fermé : {}", fermees.join(", ")));
        }
    }

    let capture = if reglages.mesure_fps && etat.presentmon.exists() {
        let partage = etat.fps_live.clone();
        let app2 = app.clone();
        match fps::demarrer(&etat.presentmon, exe, move |f| {
            *partage.lock().unwrap() = Some(f);
            let _ = app2.emit("fps", f);
        }) {
            Ok(c) => Some(c),
            Err(e) => {
                actions.push(format!("Compteur FPS indisponible : {e}"));
                None
            }
        }
    } else {
        None
    };

    *etat.session.lock().unwrap() = Some(EnCours {
        profil: profil.nom.clone(),
        exe: exe.to_string(),
        pid,
        debut: Instant::now(),
        debut_unix: SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        boost,
        actions,
        capture,
        derniere_purge: None,
        purges: 0,
        temp_max: None,
        temp: None,
        oc_suspendu,
        oc_actif,
        abaisses: Vec::new(),
        vus: Vec::new(),
        presence: None,
        enregistreur: None,
        replays_vus: Vec::new(),
        debut_systeme: SystemTime::now(),
        pseudo_r6: None,
        clips_manuels: Vec::new(),
        clips_lances: false,
        replays_cherches: false,
        rencontres_relues: false,
        clips_erreur: None,
        dossiers_replays: Vec::new(),
    });
    if reglages.discord {
        demarrer_discord(etat, profil, boost);
    }
    abaisser_fond(etat, procs);
    // Partie jouée avec l'overclocking : marque effacée à la fin de la partie (voir oc_au_demarrage)
    if oc_actif {
        etat.stock.donnees.lock().unwrap().oc_partie = Some(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()));
        etat.stock.sauver();
    }
    // Résolution propre au jeu (ex. 4:3 étiré), appliquée même sans boost
    if let Some(r) = &profil.resolution {
        let message = match crate::ecrans::mode_pour_jeu(r.largeur, r.hauteur, r.etirer) {
            Ok((id, avertissement)) => {
                etat.stock.donnees.lock().unwrap().ecran_jeu = Some(id);
                etat.stock.sauver();
                let base = format!("Résolution {} × {}{}", r.largeur, r.hauteur, if r.etirer { " étirée" } else { "" });
                match avertissement {
                    Some(a) => format!("{base} ({a})"),
                    None => base,
                }
            }
            Err(e) => format!("Résolution : {e}"),
        };
        if let Some(s) = etat.session.lock().unwrap().as_mut() {
            s.actions.push(message);
        }
    }
    if reglages.couleurs_en_jeu {
        let couleurs = etat.stock.donnees.lock().unwrap().couleurs.clone();
        if !couleurs.is_empty() {
            let erreurs = crate::ecrans::appliquer(&couleurs);
            etat.stock.donnees.lock().unwrap().vibrance_avant = crate::ecrans::vibrances_modifiees();
            etat.stock.sauver();
            if let Some(s) = etat.session.lock().unwrap().as_mut() {
                s.actions.push(if erreurs.is_empty() { "Couleurs de jeu appliquées".into() } else { erreurs.join(" ; ") });
            }
        }
    }
    if reglages.overlay && reglages.mesure_fps {
        crate::fenetres::ouvrir_overlay(app);
    }
    if reglages.tableau_r6 && exe.to_lowercase().starts_with("rainbowsix") {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::MOD_CONTROL;
        let pris = etat.touche_tableau.lock().unwrap().as_ref().is_some_and(|r| r.changer(MOD_CONTROL, 0x58));
        if let Some(s) = etat.session.lock().unwrap().as_mut() {
            s.actions.push(if pris { "Tableau de la partie : Ctrl+X".into() } else { "Ctrl+X déjà utilisé par une autre appli (stats.cc ?) : tableau sans touche".into() });
        }
    }
    // Après le changement de résolution du jeu : la capture démarre sur le bon mode d'écran
    lancer_clips_si_besoin(app, etat, pid);
    crate::fenetres::infobulle(app, &format!("FPS Cagoule : {}{}", profil.nom, if boost { " boosté" } else { "" }));
    let _ = app.emit("jeu", etat.info_jeu());
}

/// Fin de partie (ou fermeture de l'app) : on remet tout comme avant et on enregistre la session.
pub fn terminer(app: &AppHandle, etat: &Etat) {
    let Some(s) = etat.session.lock().unwrap().take() else { return };
    if let Some(r) = etat.touche_tableau.lock().unwrap().as_ref() {
        r.changer(0, 0);
    }
    crate::fenetres::fermer_tableau(app);
    restaurer_plan(etat);
    restaurer_couleurs(etat);
    restaurer_resolution(etat);
    systeme::minuteur_precis(false);
    if etat.stock.donnees.lock().unwrap().oc_partie.take().is_some() {
        etat.stock.sauver();
    }
    crate::amd::fin_de_partie();
    for &(pid, classe) in &s.abaisses {
        systeme::remettre_priorite(pid, classe);
    }
    if let Some((c, m)) = s.oc_suspendu {
        let _ = crate::nvidia::appliquer_oc(c, m);
    }
    if s.oc_actif {
        verifier_plantage_oc(app, etat, s.debut.elapsed().as_secs() + 30, &s.exe);
    }
    *etat.fps_live.lock().unwrap() = None;
    crate::fenetres::fermer_overlay(app);
    crate::fenetres::infobulle(app, "FPS Cagoule");

    if let Some(capture) = s.capture {
        let acc = capture.terminer();
        if let Some(st) = stats::calculer(acc.principale()) {
            let (courbe, pas_s) = stats::courbe(acc.principale(), 300);
            etat.stock.ajouter_session(Session {
                jeu: s.profil.clone(),
                debut: s.debut_unix,
                duree_s: s.debut.elapsed().as_secs(),
                boost: s.boost,
                images: st.images,
                moyenne: st.moyenne,
                low1: st.low1,
                low01: st.low01,
                temp_gpu: s.temp_max,
                courbe,
                pas_s,
            });
        }
    }
    // Carte graphique à 85 °C ou plus : message à la prochaine ouverture de l'interface
    if let Some(t) = s.temp_max.filter(|t| *t >= SEUIL_TEMP) {
        etat.stock.donnees.lock().unwrap().alerte_temp = Some(crate::config::AlerteTemp { jeu: s.profil.clone(), temp: t });
        etat.stock.sauver();
        let _ = app.emit("alerte_temp", ());
    }
    let _ = app.emit("jeu", None::<InfoJeu>);
}

/// Démarre l'enregistrement à côté (FFmpeg met quelques secondes à confirmer qu'il tourne)
/// Démarre l'enregistrement une fois par partie, dès que les clips sont activés et FFmpeg installé
/// (aussi quand ils sont activés en pleine partie)
fn lancer_clips_si_besoin(app: &AppHandle, etat: &Etat, pid: u32) {
    if !etat.stock.donnees.lock().unwrap().reglages.clips || !crate::clips::installe(&etat.stock) {
        return;
    }
    let (jeu, exe) = {
        let mut session = etat.session.lock().unwrap();
        match session.as_mut().filter(|s| s.pid == pid && !s.clips_lances && s.enregistreur.is_none()) {
            Some(s) => {
                s.clips_lances = true;
                (s.profil.clone(), s.exe.clone())
            }
            None => return,
        }
    };
    demarrer_clips(app, pid, &jeu, &exe);
}

fn demarrer_clips(app: &AppHandle, pid: u32, jeu: &str, exe: &str) {
    let app = app.clone();
    let jeu = jeu.to_string();
    let exe = exe.to_string();
    let _ = thread::Builder::new().name("clips".into()).spawn(move || {
        let etat = app.state::<Arc<Etat>>();
        let resultat = crate::clips::demarrer(&etat.stock, &jeu, Some(&exe));
        let mut session = etat.session.lock().unwrap();
        match (session.as_mut(), resultat) {
            // La partie est toujours là : l'enregistrement lui appartient (sinon il s'arrête en étant lâché)
            (Some(s), Ok(e)) if s.pid == pid => {
                crate::clips::noter(&etat.stock, &format!("Enregistrement de {jeu} démarré ({:?})", e.encodeur));
                s.actions.push(format!("Clips : touche prête ({})", match e.encodeur {
                    crate::clips::Encodeur::Logiciel => "encodage par le processeur",
                    _ => "encodage par la carte graphique",
                }));
                s.enregistreur = Some(e);
            }
            (Some(s), Err(e)) if s.pid == pid => {
                crate::clips::noter(&etat.stock, &format!("Enregistrement de {jeu} impossible : {e}"));
                s.clips_erreur = Some(e.clone());
                s.actions.push(e);
            }
            _ => {}
        }
    });
}

/// L'enregistrement s'est arrêté, ou l'image ne vient plus (le jeu a changé la résolution ou passé en plein écran) :
/// on le relance en arrière-plan, 30 fois au plus par partie
const RELANCES_MAX: u32 = 30;

fn surveiller_clips(app: &AppHandle, etat: &Etat, pid: u32) {
    let mort = {
        let mut session = etat.session.lock().unwrap();
        match session.as_mut() {
            Some(s) => {
                let arrete = s.enregistreur.as_mut().is_some_and(|e| {
                    if e.relances >= RELANCES_MAX {
                        return false;
                    }
                    let mort = !e.vivant();
                    if mort || e.image_figee() {
                        e.raison = if mort { "FFmpeg arrêté" } else { "image figée" };
                        true
                    } else if !e.fenetre && !e.fenetre_reessayee && s.debut.elapsed() > Duration::from_secs(45) {
                        // La fenêtre du jeu n'existait pas encore au lancement : un essai de plus, une fois en jeu
                        e.fenetre_reessayee = true;
                        e.raison = "essai de la capture de fenêtre";
                        true
                    } else {
                        false
                    }
                });
                if arrete {
                    s.enregistreur.take()
                } else {
                    None
                }
            }
            None => None,
        }
    };
    let Some(mut enregistreur) = mort else { return };
    let app = app.clone();
    let _ = thread::Builder::new().name("clips".into()).spawn(move || {
        let etat = app.state::<Arc<Etat>>();
        let r = enregistreur.relancer(&etat.stock);
        let derniere = enregistreur.derniere_erreur.lock().unwrap().clone().unwrap_or_default();
        crate::clips::noter(
            &etat.stock,
            &format!("Capture relancée ({}e fois, {}{}){}", enregistreur.relances, enregistreur.raison, if derniere.is_empty() { String::new() } else { format!(" : {derniere}") }, r.as_ref().err().map(|e| format!(" — relance ratée : {e}")).unwrap_or_default()),
        );
        let mut session = etat.session.lock().unwrap();
        if let Some(s) = session.as_mut().filter(|s| s.pid == pid) {
            if enregistreur.relances >= RELANCES_MAX || r.is_err() {
                let detail = enregistreur.derniere_erreur.lock().unwrap().clone().or(r.err()).unwrap_or_else(|| "pas d'image".into());
                let message = format!("la capture de l'écran s'arrête sans cesse ({detail})");
                s.actions.push(format!("Clips : {message}"));
                s.clips_erreur = Some(message);
            } else {
                s.clips_erreur = None;
            }
            // Même raté, il est remis en place : la surveillance réessaiera au prochain passage
            s.enregistreur = Some(enregistreur);
        }
    });
}

/// Fins de manche dans R6 : le replay (.rec) de chaque manche est lu pour la fenêtre des dégâts et les clips
/// automatiques (2 kills ou plus du joueur donnent un clip « 3K · Ace · Clubhouse » qui part 15 s avant le premier kill)
fn surveiller_replays(app: &AppHandle, etat: &Etat, pid: u32) {
    let (clips_auto, degats) = {
        let d = etat.stock.donnees.lock().unwrap();
        (d.reglages.clips_auto, d.reglages.degats_fin_manche)
    };
    if !clips_auto && !degats {
        return;
    }
    let infos = {
        let session = etat.session.lock().unwrap();
        session.as_ref().filter(|s| s.exe.to_lowercase().starts_with("rainbowsix")).map(|s| {
            (s.debut_systeme, s.replays_vus.clone(), s.enregistreur.as_ref().filter(|_| clips_auto).map(|e| (e.dossier.clone(), e.jeu.clone())))
        })
    };
    let Some((depuis, vus, enregistrement)) = infos else { return };
    if enregistrement.is_none() && !degats {
        return;
    }
    // Dossiers des replays cherchés une fois par partie (puis tant qu'aucun n'existe : R6 le crée à la première manche)
    let dossiers = {
        let deja = etat.session.lock().unwrap().as_ref().map(|s| s.dossiers_replays.clone()).unwrap_or_default();
        if deja.is_empty() {
            use windows_sys::Win32::UI::Shell::FOLDERID_Documents;
            let Some(documents) = crate::jeux_config::dossier_connu(&FOLDERID_Documents) else { return };
            let exe = systeme::chemin_exe(pid).map(PathBuf::from);
            // R6 peut tourner depuis une copie (AppData\Local\ubisoft\r6s\multiexe) : les replays restent dans
            // le dossier d'installation, cherché aussi dans Ubisoft Connect et les bibliothèques Steam
            let mut installs = crate::bibliotheques::dossiers_ubisoft();
            installs.extend(crate::bibliotheques::dossiers_steam());
            let trouves = crate::replays_r6::dossiers_replays(&documents, exe.as_deref(), &installs);
            if let Some(s) = etat.session.lock().unwrap().as_mut().filter(|s| s.pid == pid) {
                // Noté une fois (puis seulement quand un dossier apparaît)
                if !s.replays_cherches || !trouves.is_empty() {
                    crate::clips::noter(&etat.stock, &format!("Partie R6 ({}) : dossiers des replays {:?}", exe.as_deref().map(|p| p.display().to_string()).unwrap_or_default(), trouves));
                    s.replays_cherches = true;
                }
                s.dossiers_replays = trouves.clone();
            }
            trouves
        } else {
            deja
        }
    };
    relire_anciennes_parties(app, etat, pid, &dossiers);
    for (fichier, modifie) in crate::replays_r6::manches_recentes(&dossiers, depuis) {
        // Fichier encore en cours d'écriture : on attendra le prochain passage
        let age = SystemTime::now().duration_since(modifie).unwrap_or_default();
        if vus.contains(&fichier) || age < Duration::from_secs(4) {
            continue;
        }
        if let Some(s) = etat.session.lock().unwrap().as_mut().filter(|s| s.pid == pid) {
            s.replays_vus.push(fichier.clone());
        }
        let (app, enregistrement) = (app.clone(), enregistrement.clone());
        let _ = thread::Builder::new().name("clip-auto".into()).spawn(move || {
            let etat = app.state::<Arc<Etat>>();
            let nom = fichier.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let octets = match std::fs::read(&fichier) {
                Ok(o) => o,
                Err(e) => return crate::clips::noter(&etat.stock, &format!("{nom} : illisible ({e})")),
            };
            let (manche, flux) = match crate::replays_r6::lire_avec_flux(&octets) {
                Ok(m) => m,
                Err(e) => return crate::clips::noter(&etat.stock, &format!("{nom} : replay non lu ({e})")),
            };
            drop(octets);
            // Dégâts de la manche : calculés à part (moins d'une seconde) pour ne pas retarder le clip
            if degats {
                let (app, manche, nom) = (app.clone(), manche.clone(), nom.clone());
                let partie = fichier.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let _ = thread::Builder::new().name("degats".into()).spawn(move || {
                    let etat = app.state::<Arc<Etat>>();
                    let debut = Instant::now();
                    let bilan = crate::degats_r6::bilan(&flux, &manche, &mut etat.degats_historique.lock().unwrap());
                    match bilan {
                        Some(mut b) => {
                            {
                                let mut r = etat.rencontres.lock().unwrap();
                                let date = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
                                r.noter(&partie, date, &manche);
                                r.noter_manche(&partie, &b);
                                r.sauver();
                            }
                            b.partie = partie;
                            rafraichir_ubisoft(&app, b.joueurs.iter().map(|j| j.profil.clone()).collect());
                            let inconnus: u32 = b.coups.iter().filter(|c| c.tireur.is_none()).map(|c| c.degats).sum();
                            let total: u32 = b.coups.iter().map(|c| c.degats).sum();
                            crate::clips::noter(&etat.stock, &format!("{nom} : dégâts lus en {:.1} s ({} coups, {inconnus}/{total} PV sans tireur)", debut.elapsed().as_secs_f64(), b.coups.len()));
                            {
                                let mut liste = etat.degats.lock().unwrap();
                                liste.retain(|x| !(x.partie == b.partie && x.manche == b.manche));
                                liste.push(b);
                                // Les 40 dernières manches au plus (une partie dépasse rarement 12 manches)
                                while liste.len() > 40 {
                                    liste.remove(0);
                                }
                            }
                            let _ = app.emit("degats", ());
                            if etat.stock.donnees.lock().unwrap().reglages.degats_fin_manche {
                                crate::fenetres::ouvrir_degats(&app);
                            }
                        }
                        None => crate::clips::noter(&etat.stock, &format!("{nom} : dégâts illisibles")),
                    }
                });
            }
            let Some((tampon, jeu)) = enregistrement else { return };
            crate::clips::noter(&etat.stock, &format!("{nom} : compte {:?}, {} kill(s), titre {:?}", manche.moi, manche.mes_kills().len(), crate::replays_r6::titre_clip(&manche)));
            let manuels = {
                let mut session = etat.session.lock().unwrap();
                match session.as_mut().filter(|s| s.pid == pid) {
                    Some(s) => {
                        if let Some(pseudo) = &manche.moi {
                            s.pseudo_r6 = Some(pseudo.clone());
                        }
                        s.clips_manuels.clone()
                    }
                    None => vec![],
                }
            };
            // Repères des kills, KO et morts sur les clips faits à la touche pendant cette manche
            for (clip, fin, secondes) in manuels {
                let reperes = reperes_manche(&manche, modifie, fin, secondes);
                if !reperes.is_empty() {
                    crate::clips::ajouter_marqueurs(&etat.stock, &clip, &reperes);
                }
            }
            let Some(titre) = crate::replays_r6::titre_clip(&manche) else { return };
            // Seulement les multi-kills choisis (2K, 3K, 4K, ACE)
            let kills = manche.mes_kills().len().min(5) as u8;
            if !etat.stock.donnees.lock().unwrap().reglages.clips_auto_kills.contains(&kills) {
                return crate::clips::noter(&etat.stock, &format!("{nom} : {kills} kills, pas dans les clips auto choisis"));
            }
            let secondes = crate::replays_r6::duree_clip(&manche, modifie, SystemTime::now());
            let fin = SystemTime::now();
            match crate::clips::sauver(&etat.stock, &tampon, &jeu, secondes, Some(&titre)) {
                Ok((chemin, duree)) => {
                    crate::clips::noter(&etat.stock, &format!("{nom} : clip auto « {titre} » ({duree:.0} s) → {}", chemin.display()));
                    crate::clips::ajouter_marqueurs(&etat.stock, &chemin, &reperes_manche(&manche, modifie, fin, duree));
                    signaler_clip(&app, &etat.stock, &chemin);
                    poster_auto(&app, &etat.stock, &chemin, &titre, kills);
                }
                Err(e) => {
                    crate::clips::noter(&etat.stock, &format!("{nom} : clip auto « {titre} » raté ({e})"));
                    let _ = app.emit("clip_erreur", format!("clip auto « {titre} » raté ({e})"));
                }
            }
        });
    }
}

/// Historique des rencontres : les parties déjà présentes dans les dossiers de replays (juste le début de leur
/// premier replay, quelques ms chacune), une fois par session, en arrière-plan et sans se presser
fn relire_anciennes_parties(app: &AppHandle, etat: &Etat, pid: u32, dossiers: &[PathBuf]) {
    if dossiers.is_empty() {
        return;
    }
    {
        let mut session = etat.session.lock().unwrap();
        match session.as_mut().filter(|s| s.pid == pid) {
            Some(s) if !s.rencontres_relues => s.rencontres_relues = true,
            _ => return,
        }
    }
    let a_relire = crate::rencontres::parties_a_relire(dossiers, &etat.rencontres.lock().unwrap());
    if a_relire.is_empty() {
        return;
    }
    let app = app.clone();
    let _ = thread::Builder::new().name("rencontres".into()).spawn(move || {
        let etat = app.state::<Arc<Etat>>();
        let mut lues = 0;
        for (partie, fichier, date) in &a_relire {
            let Ok(octets) = std::fs::read(fichier) else { continue };
            if let Ok(m) = crate::replays_r6::lire_debut(&octets) {
                lues += usize::from(etat.rencontres.lock().unwrap().noter(partie, *date, &m));
            }
            thread::sleep(Duration::from_millis(150));
        }
        etat.rencontres.lock().unwrap().sauver();
        crate::clips::noter(&etat.stock, &format!("Rencontres : {lues} anciennes parties relues sur {}", a_relire.len()));
    });
}

/// Repères d'une manche qui tombent dans un clip couvrant les `secondes` avant `fin`
fn reperes_manche(manche: &crate::replays_r6::Manche, ecriture: SystemTime, fin: SystemTime, secondes: f64) -> Vec<crate::clips::Marqueur> {
    let Some(debut) = fin.checked_sub(Duration::from_secs_f64(secondes)) else { return vec![] };
    manche
        .mes_evenements()
        .into_iter()
        .filter_map(|e| {
            let quand = crate::replays_r6::instant(manche, e.ecoule, ecriture)?;
            let t = quand.duration_since(debut).ok()?.as_secs_f64();
            (t <= secondes).then(|| crate::clips::Marqueur { t, genre: e.genre.into(), detail: e.detail, headshot: e.headshot })
        })
        .collect()
}

#[derive(serde::Serialize, Clone)]
struct PosteAuto {
    chemin: String,
    titre: String,
    lien: Option<String>,
    erreur: Option<String>,
}

/// Clip auto choisi pour Discord (3K, 4K, ACE…) : partagé et posté tout seul dans le salon réglé
fn poster_auto(app: &AppHandle, stock: &crate::config::Stockage, chemin: &std::path::Path, titre: &str, kills: u8) {
    let voulu = {
        let r = &stock.donnees.lock().unwrap().reglages;
        r.discord_auto_kills.contains(&kills) && r.partage_discord.as_deref().is_some_and(crate::liens::webhook_valide)
    };
    if !voulu {
        return;
    }
    let chemin_texte = chemin.to_string_lossy().to_string();
    let (lien, erreur) = match crate::liens::partager(stock, &chemin_texte, titre, |_| {}) {
        Ok(p) => match p.discord {
            Some(Err(e)) => (Some(p.lien), Some(format!("lien créé mais pas posté sur Discord ({e})"))),
            _ => (Some(p.lien), None),
        },
        Err(e) => (None, Some(e)),
    };
    crate::clips::noter(stock, &format!("Discord auto « {titre} » : {}", erreur.as_deref().unwrap_or("posté")));
    let _ = app.emit("clip_poste", PosteAuto { chemin: chemin_texte, titre: titre.into(), lien, erreur });
}

/// Clip enregistré : prévient l'interface, et dit si le dossier choisi (serveur) était injoignable
fn signaler_clip(app: &AppHandle, stock: &crate::config::Stockage, chemin: &std::path::Path) {
    let _ = app.emit("clip", chemin.to_string_lossy().to_string());
    if !chemin.starts_with(crate::clips::dossier_clips(stock)) {
        let _ = app.emit("clip_secours", ());
    }
}

/// Touche des clips : sauvegarde les dernières secondes de la partie en cours
/// Clips auto de R6 qui ne peuvent pas marcher pendant cette partie : pourquoi (None = tout va bien ou trop tôt)
pub fn alerte_replays(etat: &Etat) -> Option<String> {
    if !etat.stock.donnees.lock().unwrap().reglages.clips_auto {
        return None;
    }
    let session = etat.session.lock().unwrap();
    let s = session.as_ref().filter(|s| s.exe.to_lowercase().starts_with("rainbowsix") && s.enregistreur.is_some())?;
    if s.replays_vus.is_empty() && s.debut.elapsed() > Duration::from_secs(8 * 60) {
        Some("aucun replay de R6 depuis le début de la partie : active l'enregistrement des replays dans les options du jeu, sur ce compte".into())
    } else if !s.replays_vus.is_empty() && s.pseudo_r6.is_none() {
        Some("les replays sont lus, mais ton compte n'y est pas reconnu : envoie-en un pour que je regarde".into())
    } else {
        None
    }
}

pub fn touche_clips(app: &AppHandle) {
    use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONHAND, MB_OK};
    let etat = app.state::<Arc<Etat>>();
    let infos = etat
        .session
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|s| s.enregistreur.as_ref().map(|e| (e.dossier.clone(), e.jeu.clone(), s.pseudo_r6.clone())));
    let Some((dossier, jeu, pseudo)) = infos else {
        unsafe { MessageBeep(MB_ICONHAND) };
        let _ = app.emit("clip_erreur", pourquoi_pas_de_clip(&etat));
        return;
    };
    let duree = etat.stock.donnees.lock().unwrap().reglages.clips_duree_s.clamp(10, 180);
    // Dans R6, le compte joué est ajouté au nom : « Rainbow Six Siege · Pseudo 2026-11-17 21h42m05 »
    let titre = pseudo.map(|p| format!("{jeu} · {p} {}", crate::clips::date_texte()));
    match crate::clips::sauver(&etat.stock, &dossier, &jeu, duree, titre.as_deref()) {
        Ok((chemin, duree_reelle)) => {
            unsafe { MessageBeep(MB_OK) };
            if let Some(s) = etat.session.lock().unwrap().as_mut() {
                s.clips_manuels.push((chemin.clone(), SystemTime::now(), duree_reelle));
            }
            signaler_clip(app, &etat.stock, &chemin);
        }
        Err(e) => {
            unsafe { MessageBeep(MB_ICONHAND) };
            let _ = app.emit("clip_erreur", e);
        }
    }
}

/// Explication quand la touche des clips ne peut rien garder
fn pourquoi_pas_de_clip(etat: &Etat) -> String {
    let actif = etat.stock.donnees.lock().unwrap().reglages.clips;
    let session = etat.session.lock().unwrap();
    match session.as_ref() {
        None => "aucun jeu détecté. La touche marche pendant une partie d'un jeu de ta liste.".into(),
        Some(_) if !actif => "les clips sont coupés : active « Enregistrer mes parties » dans Clips.".into(),
        Some(_) if !crate::clips::installe(&etat.stock) => "l'outil d'enregistrement n'est pas installé : ouvre Clips pour l'installer.".into(),
        Some(s) => match &s.clips_erreur {
            Some(e) => format!("l'enregistrement n'a pas pu démarrer ({e})."),
            None => "l'enregistrement démarre, réessaie dans quelques secondes.".into(),
        },
    }
}

fn texte_discord(boost: bool, fps: Option<f64>) -> String {
    let base = if boost { "Boosté par FPS Cagoule" } else { "FPS mesurés par FPS Cagoule" };
    match fps {
        Some(f) if f > 0.0 => format!("{} · {} FPS", if boost { "Boosté" } else { "En jeu" }, f.round()),
        _ => base.into(),
    }
}

/// Statut Discord : jaquette Steam du jeu si on la connaît, sinon le logo de l'app
fn demarrer_discord(etat: &Etat, profil: &crate::config::Profil, boost: bool) {
    let jeux = crate::bibliotheques::sources(false);
    let image = crate::images::meilleur(profil, &jeux)
        .and_then(|j| j.steam)
        .map(|id| format!("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{id}/library_600x900.jpg"));
    let mut session = etat.session.lock().unwrap();
    if let Some(s) = session.as_mut() {
        s.presence = crate::discord::demarrer(&profil.nom, texte_discord(boost, None), s.debut_unix, image);
    }
}

fn maj_discord(etat: &Etat) {
    let fps = *etat.fps_live.lock().unwrap();
    if let Some(s) = etat.session.lock().unwrap().as_ref() {
        if let Some(p) = &s.presence {
            p.etat(texte_discord(s.boost, fps));
        }
    }
}

/// Température de la carte graphique à partir de laquelle on prévient (°C)
const SEUIL_TEMP: i32 = 85;

pub fn restaurer_plan(etat: &Etat) {
    let (ancien, retour) = {
        let mut d = etat.stock.donnees.lock().unwrap();
        (d.plan_avant_jeu.take(), d.retour_avant_jeu.take())
    };
    if let Some(guid) = &ancien {
        systeme::activer_plan_guid(guid);
    }
    if let Some(r) = &retour {
        systeme::remettre(r);
    }
    if ancien.is_some() || retour.is_some() {
        etat.stock.sauver();
    }
}

pub fn restaurer_couleurs(etat: &Etat) {
    crate::ecrans::restaurer();
    let avant = std::mem::take(&mut etat.stock.donnees.lock().unwrap().vibrance_avant);
    if !avant.is_empty() {
        // Cas d'une app coupée en pleine partie : la rampe gamma est déjà revenue, pas la vibrance
        crate::ecrans::restaurer_vibrances(&avant);
        etat.stock.sauver();
    }
}

pub fn restaurer_resolution(etat: &Etat) {
    let ecran = etat.stock.donnees.lock().unwrap().ecran_jeu.take();
    if let Some(id) = ecran {
        crate::ecrans::retablir_mode(&id);
        etat.stock.sauver();
    }
}

/// Pendant une partie boostée, vide la mémoire en veille quand la mémoire libre devient basse
/// (comme l'outil ISLC) : utile surtout avec 16 Go de RAM ou moins, ou en Boost max.
fn entretenir_memoire(etat: &Etat) {
    const GO: u64 = 1 << 30;
    let boost_max = etat.stock.donnees.lock().unwrap().reglages.boost_max;
    let mut session = etat.session.lock().unwrap();
    let Some(s) = session.as_mut() else { return };
    let total = systeme::memoire_totale();
    if !s.boost || !(boost_max || total <= 17 * GO) {
        return;
    }
    if s.derniere_purge.is_some_and(|t| t.elapsed() < Duration::from_secs(60)) || systeme::memoire_libre() >= (total / 10).max(GO) {
        return;
    }
    if systeme::vider_memoire_veille().is_ok() {
        s.derniere_purge = Some(Instant::now());
        s.purges += 1;
        let texte = format!("Mémoire en veille vidée pendant la partie ({} fois)", s.purges);
        match s.actions.iter_mut().find(|a| a.starts_with("Mémoire en veille vidée pendant")) {
            Some(a) => *a = texte,
            None => s.actions.push(texte),
        }
    }
}

/// Pendant une partie boostée : les programmes de fond de la liste passent après le jeu.
/// Les nouveaux processus (onglets du navigateur…) sont pris au passage, toutes les 3 s.
fn abaisser_fond(etat: &Etat, procs: &[systeme::Processus]) {
    let (actif, liste) = {
        let d = etat.stock.donnees.lock().unwrap();
        (d.reglages.priorite_basse, d.reglages.priorite_basse_applis.clone())
    };
    let mut session = etat.session.lock().unwrap();
    let Some(s) = session.as_mut() else { return };
    if !actif || !s.boost || liste.is_empty() {
        return;
    }
    let avant = s.abaisses.len();
    for p in procs {
        if p.pid == s.pid || s.vus.contains(&p.pid) || !liste.iter().any(|a| a.eq_ignore_ascii_case(&p.nom)) {
            continue;
        }
        s.vus.push(p.pid);
        if let Some(classe) = systeme::priorite_basse(p.pid) {
            s.abaisses.push((p.pid, classe));
        }
    }
    if s.abaisses.len() != avant {
        let mut noms: Vec<&str> = s.abaisses.iter().filter_map(|(pid, _)| procs.iter().find(|p| p.pid == *pid).map(|p| p.nom.as_str())).collect();
        noms.sort_unstable_by_key(|n| n.to_lowercase());
        noms.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        let texte = format!("Passent après le jeu : {}", noms.join(", "));
        match s.actions.iter_mut().find(|a| a.starts_with("Passent après le jeu")) {
            Some(a) => *a = texte,
            None => s.actions.push(texte),
        }
    }
}

/// Température de la carte graphique pendant la partie (NVIDIA, un appel NVAPI toutes les 3 s)
fn suivre_temperature(app: &AppHandle, etat: &Etat) {
    let Some(t) = crate::nvidia::temperature().or_else(crate::amd::temperature_en_jeu) else { return };
    let mut session = etat.session.lock().unwrap();
    let Some(s) = session.as_mut() else { return };
    s.temp = Some(t);
    s.temp_max = Some(s.temp_max.map_or(t, |m| m.max(t)));
    drop(session);
    let _ = app.emit("temp_gpu", t);
}

/// Après une partie avec overclocking : si le pilote graphique ou le jeu a planté, l'OC baisse d'un cran
/// (-25 MHz GPU, -100 MHz mémoire), comme le bouton « Baisser d'un cran ».
fn verifier_plantage_oc(app: &AppHandle, etat: &Etat, secondes: u64, exe: &str) {
    let (protection, oc) = {
        let d = etat.stock.donnees.lock().unwrap();
        (d.reglages.oc_protection, d.oc.clone())
    };
    let Some(mut oc) = oc.filter(|_| protection) else { return };
    let Some(quoi) = systeme::plantage_depuis(secondes, exe) else { return };
    oc.coeur_mhz = baisser(oc.coeur_mhz, 25);
    oc.memoire_mhz = baisser(oc.memoire_mhz, 100);
    if crate::nvidia::appliquer_oc(oc.coeur_mhz, oc.memoire_mhz).is_err() {
        return;
    }
    let cause = if quoi == "pilote" { "Le pilote graphique a planté" } else { "Le jeu a planté" };
    let texte = format!(
        "{cause} pendant ta partie avec l'overclocking : il a été baissé d'un cran (GPU {:+} MHz, mémoire {:+} MHz).",
        oc.coeur_mhz, oc.memoire_mhz
    );
    let mut d = etat.stock.donnees.lock().unwrap();
    d.oc = Some(oc);
    d.oc_baisse_auto = Some(texte);
    drop(d);
    etat.stock.sauver();
    // Interface ouverte : message tout de suite (sinon il est montré à la prochaine ouverture)
    let _ = app.emit("oc_baisse", ());
}

/// Retire `pas` à un décalage positif, sans passer sous zéro
pub fn baisser(v: i32, pas: i32) -> i32 {
    if v > 0 { (v - pas).max(0) } else { v }
}

const IFEO: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options";

/// Priorité haute au lancement des jeux, appliquée par Windows lui-même (PerfOptions\CpuPriorityClass) :
/// contrairement au changement de priorité en cours de partie, l'anti-triche ne peut pas la refuser.
/// Seules les entrées créées par l'app sont retirées.
pub fn synchroniser_priorites(stock: &Stockage) {
    use crate::config::PrioriteCree;
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS};
    use winreg::RegKey;

    let (voulus, mut crees) = {
        let d = stock.donnees.lock().unwrap();
        let mut voulus: Vec<String> = Vec::new();
        if d.reglages.boost_auto {
            for p in d.profils.iter().filter(|p| p.actif && p.boost && p.priorite_haute) {
                for e in &p.exes {
                    let e = e.to_lowercase();
                    if !voulus.contains(&e) {
                        voulus.push(e);
                    }
                }
            }
        }
        (voulus, d.priorites.clone())
    };
    let Ok(racine) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(IFEO, KEY_ALL_ACCESS) else { return };
    let avant = crees.len();
    let mut change = false;

    let a_ajouter: Vec<String> = voulus.iter().filter(|e| !crees.iter().any(|c| &c.exe == *e)).cloned().collect();
    for exe in &a_ajouter {
        let existait = racine.open_subkey(exe).is_ok();
        let Ok((perf, _)) = racine.create_subkey(format!(r"{exe}\PerfOptions")) else { continue };
        // Déjà réglé par l'utilisateur ou un autre outil : on n'y touche pas
        if perf.get_value::<u32, _>("CpuPriorityClass").is_ok() {
            continue;
        }
        if perf.set_value("CpuPriorityClass", &3u32).is_ok() {
            crees.push(PrioriteCree { exe: exe.clone(), cle_creee: !existait });
            change = true;
        }
    }
    crees.retain(|c| {
        if voulus.contains(&c.exe) {
            return true;
        }
        let chemin = format!(r"{}\PerfOptions", c.exe);
        if let Ok(perf) = racine.open_subkey_with_flags(&chemin, KEY_ALL_ACCESS) {
            let _ = perf.delete_value("CpuPriorityClass");
            if perf.query_info().is_ok_and(|i| i.values == 0 && i.sub_keys == 0) {
                let _ = racine.delete_subkey(&chemin);
            }
        }
        if c.cle_creee {
            if let Ok(k) = racine.open_subkey(&c.exe) {
                if k.query_info().is_ok_and(|i| i.values == 0 && i.sub_keys == 0) {
                    let _ = racine.delete_subkey(&c.exe);
                }
            }
        }
        false
    });
    if change || crees.len() != avant {
        stock.donnees.lock().unwrap().priorites = crees;
        stock.sauver();
    }
}

/// Au lancement de l'app : réapplique l'overclocking demandé, sauf si le PC s'est arrêté brutalement
/// pendant une partie avec l'OC (la marque de partie est restée et Windows a redémarré depuis).
pub fn oc_au_demarrage(etat: &Etat) {
    let mut d = etat.stock.donnees.lock().unwrap();
    if let Some(debut) = d.oc_partie.take() {
        if systeme::demarrage_windows() > debut {
            if let Some(oc) = d.oc.as_mut() {
                oc.au_demarrage = false;
            }
            d.oc_coupe_apres_plantage = true;
            drop(d);
            etat.stock.sauver();
            return;
        }
    }
    let oc = d.oc.clone();
    drop(d);
    if let Some(oc) = oc.filter(|o| o.au_demarrage) {
        let _ = crate::nvidia::appliquer_oc(oc.coeur_mhz, oc.memoire_mhz);
        if let Some(p) = oc.puissance_pct {
            let _ = crate::nvidia::appliquer_puissance(p);
        }
    }
}
