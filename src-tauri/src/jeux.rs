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
}

pub struct Etat {
    pub stock: Stockage,
    pub session: Mutex<Option<EnCours>>,
    pub fps_live: Arc<Mutex<Option<f64>>>,
    pub presentmon: PathBuf,
}

#[derive(Serialize, Clone)]
pub struct InfoJeu {
    pub nom: String,
    pub exe: String,
    pub depuis_s: u64,
    pub boost: bool,
    pub actions: Vec<String>,
    pub fps: Option<f64>,
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

    if boost {
        // Plan d'alimentation : l'ancien est mémorisé sur disque, au cas où l'app serait coupée en pleine partie
        let plan_perso = etat.stock.donnees.lock().unwrap().plan_perso.clone();
        if let Some(avant) = systeme::plan_actif().filter(|g| !systeme::est_plan_perf(g, plan_perso.as_deref())) {
            match systeme::activer_plan_perf(plan_perso.as_deref()) {
                Ok(cree) => {
                    let mut d = etat.stock.donnees.lock().unwrap();
                    d.plan_avant_jeu = Some(avant);
                    if cree.is_some() {
                        d.plan_perso = cree;
                    }
                    drop(d);
                    etat.stock.sauver();
                    actions.push("Plan Performances élevées activé".into());
                }
                Err(e) => actions.push(format!("Plan d'alimentation : {e}")),
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
    });
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
    crate::fenetres::infobulle(app, &format!("FPS Cagoule : {}{}", profil.nom, if boost { " boosté" } else { "" }));
    let _ = app.emit("jeu", etat.info_jeu());
}

/// Fin de partie (ou fermeture de l'app) : on remet tout comme avant et on enregistre la session.
pub fn terminer(app: &AppHandle, etat: &Etat) {
    let Some(s) = etat.session.lock().unwrap().take() else { return };
    restaurer_plan(etat);
    restaurer_couleurs(etat);
    *etat.fps_live.lock().unwrap() = None;
    crate::fenetres::fermer_overlay(app);
    crate::fenetres::infobulle(app, "FPS Cagoule");

    if let Some(capture) = s.capture {
        let acc = capture.terminer();
        if let Some(st) = stats::calculer(acc.principale()) {
            etat.stock.ajouter_session(Session {
                jeu: s.profil,
                debut: s.debut_unix,
                duree_s: s.debut.elapsed().as_secs(),
                boost: s.boost,
                images: st.images,
                moyenne: st.moyenne,
                low1: st.low1,
                low01: st.low01,
            });
        }
    }
    let _ = app.emit("jeu", None::<InfoJeu>);
}

pub fn restaurer_plan(etat: &Etat) {
    let avant = etat.stock.donnees.lock().unwrap().plan_avant_jeu.take();
    if let Some(guid) = avant {
        systeme::activer_plan(&guid);
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
