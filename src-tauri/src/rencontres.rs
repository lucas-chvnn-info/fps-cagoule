// Joueurs croisés dans Rainbow Six Siege : historique local (rencontres.json) tiré des replays.
// Pour chaque partie : le compte qui joue (tu peux en avoir plusieurs) et chaque autre joueur, avec ou contre toi.
// Sert à « Contre 3× · Avec 1× » et aux groupes (joueurs souvent dans la même équipe) de la fenêtre des dégâts.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Au-delà, les plus vieilles parties sont oubliées (~300 Ko de JSON)
const MAX_PARTIES: usize = 3000;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct JoueurVu {
    /// Profil Ubisoft (ou « pseudo:… » si le replay ne le donne pas)
    pub profil: String,
    pub pseudo: String,
    /// Dans l'équipe du compte qui joue
    pub avec: bool,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct PartieVue {
    /// Dossier du match dans « MatchReplay »
    pub partie: String,
    pub date_ms: u64,
    /// Compte qui joue : profil et pseudo
    pub moi: String,
    pub moi_pseudo: String,
    pub joueurs: Vec<JoueurVu>,
}

#[derive(Serialize, Clone, Default, Debug, PartialEq)]
pub struct AutreCompte {
    pub pseudo: String,
    pub avec: u32,
    pub contre: u32,
}

/// Ce qu'on sait d'un joueur du lobby, hors partie en cours
#[derive(Serialize, Clone, Default, Debug, PartialEq)]
pub struct Resume {
    /// Parties avec / contre lui sur le compte qui joue
    pub avec: u32,
    pub contre: u32,
    /// Et sur tes autres comptes
    pub autres_comptes: Vec<AutreCompte>,
    /// Autres joueurs du lobby déjà vus dans la même équipe que lui : profil → nombre de parties
    pub ensemble: HashMap<String, u32>,
}

pub struct Rencontres {
    chemin: PathBuf,
    pub parties: Vec<PartieVue>,
}

pub fn cle(profil: &str, pseudo: &str) -> String {
    if profil.is_empty() {
        format!("pseudo:{}", pseudo.to_lowercase())
    } else {
        profil.to_lowercase()
    }
}

impl Rencontres {
    pub fn ouvrir(dossier: &Path) -> Self {
        let chemin = dossier.join("rencontres.json");
        let parties = std::fs::read_to_string(&chemin).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Rencontres { chemin, parties }
    }

    pub fn connue(&self, partie: &str) -> bool {
        self.parties.iter().any(|p| p.partie == partie)
    }

    /// Ajoute les joueurs d'une manche à sa partie (les remplaçants arrivés en cours de partie aussi). Vrai si changé.
    pub fn noter(&mut self, partie: &str, date_ms: u64, m: &crate::replays_r6::Manche) -> bool {
        let Some(moi) = m.moi.as_ref().and_then(|n| m.joueurs.iter().find(|j| &j.pseudo == n)) else { return false };
        let mien = cle(&moi.profil, &moi.pseudo);
        let joueurs: Vec<JoueurVu> = m
            .joueurs
            .iter()
            .filter(|j| j.pseudo != moi.pseudo && !j.pseudo.is_empty())
            .map(|j| JoueurVu { profil: cle(&j.profil, &j.pseudo), pseudo: j.pseudo.clone(), avec: j.equipe == moi.equipe })
            .collect();
        let pos = match self.parties.iter().position(|p| p.partie == partie) {
            Some(i) => i,
            None => {
                self.parties.push(PartieVue { partie: partie.into(), date_ms, moi: mien, moi_pseudo: moi.pseudo.clone(), joueurs: vec![] });
                if self.parties.len() > MAX_PARTIES {
                    self.parties.sort_by_key(|p| p.date_ms);
                    self.parties.drain(..self.parties.len() - MAX_PARTIES);
                }
                self.parties.iter().position(|p| p.partie == partie).unwrap_or(0)
            }
        };
        let p = &mut self.parties[pos];
        let mut change = p.joueurs.is_empty();
        for j in joueurs {
            if !p.joueurs.iter().any(|x| x.profil == j.profil) {
                p.joueurs.push(j);
                change = true;
            }
        }
        change
    }

    pub fn sauver(&self) {
        if let Ok(texte) = serde_json::to_string(&self.parties) {
            let tmp = self.chemin.with_extension("tmp");
            if std::fs::write(&tmp, texte).is_ok() {
                let _ = std::fs::rename(&tmp, &self.chemin);
            }
        }
    }

    /// Ce qu'on sait de chaque joueur du lobby (`profils`), hors partie en cours, vu du compte `mien`
    pub fn resume(&self, partie_actuelle: &str, mien: &str, profils: &[String]) -> HashMap<String, Resume> {
        let mut out: HashMap<String, Resume> = profils.iter().map(|p| (p.clone(), Resume::default())).collect();
        for partie in self.parties.iter().filter(|p| p.partie != partie_actuelle) {
            // Équipe de chaque joueur dans cette partie : true = celle du compte qui jouait (lui compris)
            let cote = |profil: &str| -> Option<bool> {
                if profil == partie.moi {
                    Some(true)
                } else {
                    partie.joueurs.iter().find(|j| j.profil == profil).map(|j| j.avec)
                }
            };
            for p in profils {
                let Some(c) = cote(p) else { continue };
                let r = out.get_mut(p).unwrap();
                if p != mien {
                    if partie.moi == mien {
                        if c {
                            r.avec += 1;
                        } else {
                            r.contre += 1;
                        }
                    } else if partie.moi != *p {
                        let autre = match r.autres_comptes.iter_mut().find(|a| a.pseudo == partie.moi_pseudo) {
                            Some(a) => a,
                            None => {
                                r.autres_comptes.push(AutreCompte { pseudo: partie.moi_pseudo.clone(), avec: 0, contre: 0 });
                                r.autres_comptes.last_mut().unwrap()
                            }
                        };
                        if c {
                            autre.avec += 1;
                        } else {
                            autre.contre += 1;
                        }
                    }
                }
                for q in profils.iter().filter(|q| *q != p) {
                    if cote(q) == Some(c) {
                        *r.ensemble.entry(q.clone()).or_default() += 1;
                    }
                }
            }
        }
        out
    }
}

/// Parties des dossiers « MatchReplay » pas encore dans l'historique : (dossier du match, premier replay, date)
pub fn parties_a_relire(dossiers: &[PathBuf], connues: &Rencontres) -> Vec<(String, PathBuf, u64)> {
    let mut out = Vec::new();
    for d in dossiers {
        let Ok(liste) = std::fs::read_dir(d) else { continue };
        for partie in liste.flatten().filter(|e| e.path().is_dir()) {
            let nom = partie.file_name().to_string_lossy().to_string();
            if connues.connue(&nom) {
                continue;
            }
            let Ok(fichiers) = std::fs::read_dir(partie.path()) else { continue };
            let mut recs: Vec<PathBuf> = fichiers.flatten().map(|f| f.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("rec"))).collect();
            recs.sort();
            let Some(premier) = recs.into_iter().next() else { continue };
            let date = std::fs::metadata(&premier)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as u64);
            out.push((nom, premier, date));
        }
    }
    out.sort_by_key(|x| x.2);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn partie(nom: &str, moi: &str, joueurs: &[(&str, bool)]) -> PartieVue {
        PartieVue {
            partie: nom.into(),
            date_ms: 0,
            moi: moi.into(),
            moi_pseudo: moi.to_uppercase(),
            joueurs: joueurs.iter().map(|(p, a)| JoueurVu { profil: (*p).into(), pseudo: p.to_uppercase(), avec: *a }).collect(),
        }
    }

    #[test]
    fn avec_contre_et_autres_comptes() {
        let r = Rencontres {
            chemin: PathBuf::new(),
            parties: vec![
                partie("m1", "moi", &[("kalls", true), ("x", false), ("y", false)]),
                partie("m2", "moi", &[("kalls", true), ("x", true)]),
                partie("m3", "luc4s", &[("x", false), ("y", false)]),
                partie("actuelle", "moi", &[("x", false), ("y", false)]),
            ],
        };
        let lobby = vec!["moi".to_string(), "kalls".into(), "x".into(), "y".into()];
        let s = r.resume("actuelle", "moi", &lobby);
        assert_eq!((s["x"].avec, s["x"].contre), (1, 1));
        assert_eq!(s["x"].autres_comptes, vec![AutreCompte { pseudo: "LUC4S".into(), avec: 0, contre: 1 }]);
        assert_eq!((s["kalls"].avec, s["kalls"].contre), (2, 0));
        // Ensemble : x et y dans la même équipe en m1 et m3 ; kalls avec moi en m1 et m2
        assert_eq!(s["x"].ensemble.get("y"), Some(&2));
        assert_eq!(s["kalls"].ensemble.get("moi"), Some(&2));
        assert_eq!(s["moi"].avec + s["moi"].contre, 0);
    }
}
