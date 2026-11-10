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
    });
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
    crate::fenetres::infobulle(app, &format!("FPS Cagoule : {}{}", profil.nom, if boost { " boosté" } else { "" }));
    let _ = app.emit("jeu", etat.info_jeu());
}

/// Fin de partie (ou fermeture de l'app) : on remet tout comme avant et on enregistre la session.
pub fn terminer(app: &AppHandle, etat: &Etat) {
    let Some(s) = etat.session.lock().unwrap().take() else { return };
    restaurer_plan(etat);
    restaurer_couleurs(etat);
    restaurer_resolution(etat);
    systeme::minuteur_precis(false);
    if etat.stock.donnees.lock().unwrap().oc_partie.take().is_some() {
        etat.stock.sauver();
    }
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
            etat.stock.ajouter_session(Session {
                jeu: s.profil,
                debut: s.debut_unix,
                duree_s: s.debut.elapsed().as_secs(),
                boost: s.boost,
                images: st.images,
                moyenne: st.moyenne,
                low1: st.low1,
                low01: st.low01,
                temp_gpu: s.temp_max,
            });
        }
    }
    let _ = app.emit("jeu", None::<InfoJeu>);
}

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
    let Some(t) = crate::nvidia::temperature() else { return };
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
