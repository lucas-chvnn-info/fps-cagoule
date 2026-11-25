// Dégâts d'une manche de Rainbow Six Siege, lus dans le replay (.rec) : qui a fait combien à qui.
//
// Le flux décompressé contient, après un instantané, des sections de blocs `tick:u32 taille:u32 0:u32` ;
// la section 0 est le flux des objets (`n:u16` puis paquets `objet:u32 aux:u32 taille:u32 données`) avec la
// position (x, y, z) et l'orientation du corps de chaque joueur. Structure décrite par heat3d_ext_r6
// (github.com/Holdi601/3DHeat-Ext-R6, licence MIT), réécrite ici.
// Les PV de chaque joueur sont une propriété (`23 objet 00000000 25 26 76 c9 taille valeur`) datée par son bloc.
// Le tireur n'est écrit nulle part : le coup final vient du killfeed, les autres coups vont à l'ennemi qui visait
// la victime à cet instant (regard = cap du corps + 90°). Vérifié sur 279 kills : 96 % justes quand la visée est nette.

use serde::Serialize;
use std::collections::HashMap;

const NOM: &[u8] = &[0x22, 0x07, 0x94, 0x9B, 0xDC];
const ID_JOUEUR: &[u8] = &[0xEE, 0xD4, 0x45, 0xC8, 0x08];
const PV: &[u8] = &[0x00, 0x00, 0x00, 0x00, 0x25, 0x26, 0x76, 0xC9];
const KILLFEED: &[u8] = &[0x59, 0x34, 0xE5, 0x8B, 0x04];
const POSITION: &[u8] = &[0x73, 0x85, 0xFE];

/// Incertitude ajoutée par tick d'ancienneté de la position du tireur (au-delà de 8 ticks), en mètres
const PENALITE: f32 = 0.03;
/// Écart maximum entre la ligne de visée et la victime pour une visée sûre (m)
const SEUIL: f32 = 1.5;

#[derive(Serialize, Clone, Debug, Default)]
pub struct JoueurDegats {
    pub pseudo: String,
    pub equipe: u8,
    pub operateur: Option<String>,
    /// Dégâts infligés aux ennemis
    pub inflige: u32,
    /// Dégâts reçus
    pub recu: u32,
    pub kills: u32,
    pub mort: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct Coup {
    pub tireur: Option<String>,
    pub victime: String,
    pub degats: u32,
    /// « feed » (coup final), « visée », « suite » ou « inconnu »
    pub certitude: &'static str,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Bilan {
    pub map: Option<String>,
    pub manche: u32,
    pub moi: Option<String>,
    pub joueurs: Vec<JoueurDegats>,
    pub coups: Vec<Coup>,
}

fn u16_a(b: &[u8], p: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(p..p + 2)?.try_into().ok()?))
}
fn u32_a(b: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(p..p + 4)?.try_into().ok()?))
}
fn f32_a(b: &[u8], p: usize) -> Option<f32> {
    Some(f32::from_le_bytes(b.get(p..p + 4)?.try_into().ok()?))
}

/// Prochaine occurrence de `motif` dans `b[debut..fin]`
fn trouver(b: &[u8], motif: &[u8], debut: usize, fin: usize) -> Option<usize> {
    let fin = fin.min(b.len());
    if debut >= fin || motif.is_empty() {
        return None;
    }
    let premier = motif[0];
    let mut i = debut;
    while i + motif.len() <= fin {
        let k = b[i..fin - motif.len() + 1].iter().position(|&x| x == premier)?;
        i += k;
        if &b[i..i + motif.len()] == motif {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[derive(Clone, Copy)]
struct Bloc {
    section: usize,
    tick: u32,
    debut: usize,
    fin: usize,
}

struct Flux<'a> {
    b: &'a [u8],
    debut: usize,
    /// (début, fin) de chaque section
    sections: Vec<(usize, usize)>,
    blocs: Vec<Bloc>,
}

/// Sections à partir de `p` : en-tête de 33 octets puis blocs jusqu'au bloc vide
fn sections(b: &[u8], mut p: usize, limite: usize) -> (Vec<(usize, usize)>, Vec<Bloc>) {
    let (mut secs, mut blocs) = (Vec::new(), Vec::new());
    let limite = limite.min(b.len());
    while p + 45 <= limite {
        let (mut q, mut n, mut dernier) = (p + 33, 0, None::<u32>);
        while q + 12 <= limite {
            let (Some(tick), Some(taille), Some(zero)) = (u32_a(b, q), u32_a(b, q + 4), u32_a(b, q + 8)) else { break };
            let taille = taille as usize;
            if zero != 0 || q + 12 + taille > limite || dernier.is_some_and(|d| tick < d) {
                break;
            }
            dernier = Some(tick);
            n += 1;
            if taille == 0 {
                q += 12;
                break;
            }
            blocs.push(Bloc { section: secs.len(), tick, debut: q + 12, fin: q + 12 + taille });
            q += 12 + taille;
        }
        if n == 0 {
            break;
        }
        secs.push((p, q));
        p = q;
    }
    (secs, blocs)
}

/// Les paquets d'un bloc du flux des objets le remplissent-ils exactement ?
fn remplit(b: &[u8], debut: usize, fin: usize) -> bool {
    let Some(n) = u16_a(b, debut) else { return false };
    let mut q = debut + 2;
    for _ in 0..n {
        let Some(t) = u32_a(b, q + 8) else { return false };
        if q + 12 > fin {
            return false;
        }
        q += 12 + t as usize;
    }
    q == fin
}

impl<'a> Flux<'a> {
    fn ouvrir(b: &'a [u8]) -> Option<Self> {
        let mut p = trouver(b, POSITION, 0, b.len());
        while let Some(i) = p {
            if let Some(s) = i.checked_sub(1 + 12 + 2 + 12 + 33) {
                let (secs, blocs) = sections(b, s, s + 4_000_000);
                if !secs.is_empty() && secs[0].0 == s && blocs.iter().filter(|k| k.section == 0).count() >= 3 && blocs.iter().take(3).all(|k| remplit(b, k.debut, k.fin)) {
                    let (sections, blocs) = sections(b, s, b.len());
                    return Some(Flux { b, debut: s, sections, blocs });
                }
            }
            p = trouver(b, POSITION, i + 3, b.len());
        }
        None
    }

    /// Tick du bloc qui contient `pos` (None dans l'instantané)
    fn tick(&self, pos: usize) -> Option<u32> {
        let i = self.blocs.partition_point(|k| k.debut <= pos).checked_sub(1)?;
        let k = self.blocs[i];
        (pos < k.fin).then_some(k.tick)
    }

    fn paquets(&self, k: Bloc) -> Vec<(u32, usize, usize)> {
        let b = self.b;
        let mut out = Vec::new();
        let Some(n) = u16_a(b, k.debut) else { return out };
        let mut q = k.debut + 2;
        for _ in 0..n {
            let (Some(obj), Some(taille)) = (u32_a(b, q), u32_a(b, q + 8)) else { break };
            let taille = taille as usize;
            if q + 12 + taille > k.fin {
                break;
            }
            out.push((obj, q + 12, taille));
            q += 12 + taille;
        }
        out
    }
}

/// Position du corps à un tick : (tick, x, y, z, cap en degrés ou NaN)
#[derive(Clone, Copy)]
struct Point {
    tick: u32,
    x: f32,
    y: f32,
    z: f32,
    cap: f32,
}

struct Trace {
    points: Vec<Point>,
    plus_gros: usize,
}

fn traces(f: &Flux) -> HashMap<u32, Trace> {
    let b = f.b;
    let mut out: HashMap<u32, Trace> = HashMap::new();
    for k in f.blocs.iter().filter(|k| k.section == 0) {
        for (obj, q, n) in f.paquets(*k) {
            if n < 18 || b.get(q + 1..q + 4) != Some(POSITION) || b[q + 4] & 0x80 == 0 {
                continue;
            }
            let (Some(x), Some(y), Some(z)) = (f32_a(b, q + 6), f32_a(b, q + 10), f32_a(b, q + 14)) else { continue };
            if !(x.is_finite() && y.is_finite() && z.is_finite()) || x.abs().max(y.abs()).max(z.abs()) > 5000.0 || (x == 0.0 && y == 0.0) {
                continue;
            }
            let mut cap = f32::NAN;
            if n >= 38 {
                if let (Some(a), Some(c), Some(qz), Some(qw)) = (f32_a(b, q + 22), f32_a(b, q + 26), f32_a(b, q + 30), f32_a(b, q + 34)) {
                    if (qz.hypot(qw) - 1.0).abs() < 0.02 && a.abs() < 0.05 && c.abs() < 0.05 {
                        cap = (2.0 * qz.atan2(qw)).to_degrees();
                    }
                }
            }
            let t = out.entry(obj).or_insert(Trace { points: Vec::new(), plus_gros: 0 });
            t.points.push(Point { tick: k.tick, x, y, z, cap });
            t.plus_gros = t.plus_gros.max(n);
        }
    }
    out
}

struct Acteur {
    pseudo: String,
    equipe: u8,
    pid: Option<u64>,
    corps: Option<u32>,
    pv_obj: Option<u32>,
    pv_depart: Option<u32>,
}

/// Dernière position connue avant `t`, avec le dernier cap valide (40 points en arrière au plus)
fn etat(points: &[Point], t: u32) -> Option<Point> {
    let i = points.partition_point(|p| p.tick <= t).checked_sub(1)?;
    let mut e = points[i];
    let mut k = i;
    while e.cap.is_nan() && k > 0 && i - k < 40 {
        k -= 1;
        e.cap = points[k].cap;
    }
    Some(e)
}

/// Bilan des dégâts d'une manche, d'après le flux décompressé du replay et la manche déjà lue
pub fn bilan(donnees: &[u8], m: &crate::replays_r6::Manche) -> Option<Bilan> {
    let f = Flux::ouvrir(donnees)?;
    let b = f.b;
    let tr = traces(&f);
    let s0 = *f.sections.first()?;

    // Joueurs dans l'ordre de l'instantané (bloc « nom ») : identifiant 64 bits
    let mut noms: Vec<(usize, String)> = Vec::new();
    let mut p = trouver(b, NOM, 0, f.debut);
    while let Some(i) = p {
        if let Some(&n) = b.get(i + 5) {
            if let Some(s) = b.get(i + 6..i + 6 + n as usize) {
                if n > 0 {
                    noms.push((i, String::from_utf8_lossy(s).into_owned()));
                }
            }
        }
        p = trouver(b, NOM, i + 5, f.debut);
    }
    let mut acteurs: Vec<Acteur> = Vec::new();
    for (k, (i, nom)) in noms.iter().enumerate() {
        if acteurs.iter().any(|a| &a.pseudo == nom) {
            continue;
        }
        let fin = noms.get(k + 1).map(|x| x.0).unwrap_or((i + 8000).min(f.debut));
        let pid = trouver(b, ID_JOUEUR, *i, fin).and_then(|q| Some(u64::from_le_bytes(b.get(q + 5..q + 13)?.try_into().ok()?)));
        // Équipes : les 5 premiers blocs puis les 5 suivants (vérifié sur 53 manches)
        let equipe = u8::from(acteurs.len() >= 5);
        acteurs.push(Acteur { pseudo: nom.clone(), equipe, pid, corps: None, pv_obj: None, pv_depart: None });
    }
    if acteurs.len() < 2 {
        return None;
    }

    // Corps : l'objet mobile dont un paquet porte l'identifiant du joueur et dépasse 800 octets
    let blocs0: Vec<Bloc> = f.blocs.iter().copied().filter(|k| k.section == 0).collect();
    for a in acteurs.iter_mut() {
        let Some(pid) = a.pid else { continue };
        let cle = pid.to_le_bytes();
        let mut p = trouver(b, &cle, s0.0, s0.1);
        while let Some(i) = p {
            if let Some(n) = blocs0.partition_point(|k| k.debut <= i).checked_sub(1) {
                if i < blocs0[n].fin {
                    if let Some((obj, _, _)) = f.paquets(blocs0[n]).into_iter().find(|(_, q, t)| *q <= i && i < q + t) {
                        if tr.get(&obj).is_some_and(|t| t.plus_gros >= 800) {
                            a.corps = Some(obj);
                            break;
                        }
                    }
                }
            }
            p = trouver(b, &cle, i + 1, s0.1);
        }
    }

    // PV : l'objet mis à jour juste avant le bloc du joueur dans l'instantané, puis ses changements datés
    let mut maj: Vec<(u32, u32, u32)> = Vec::new(); // (tick, objet, valeur)
    let mut p = trouver(b, PV, 0, b.len());
    while let Some(i) = p {
        p = trouver(b, PV, i + 8, b.len());
        let (Some(debut), Some(&n)) = (i.checked_sub(5), b.get(i + 8)) else { continue };
        if b[debut] != 0x23 || b[i - 1] != 0xF0 || !matches!(n, 1 | 2 | 4) {
            continue;
        }
        let obj = u32_a(b, debut + 1).unwrap_or(0);
        let Some(v) = b.get(i + 9..i + 9 + n as usize) else { continue };
        let v = v.iter().rev().fold(0u32, |acc, &x| (acc << 8) | x as u32);
        match f.tick(debut) {
            Some(t) => maj.push((t, obj, v)),
            None => {
                if let Some((_, nom)) = noms.iter().find(|(q, _)| *q > debut) {
                    if let Some(a) = acteurs.iter_mut().find(|a| &a.pseudo == nom && a.pv_obj.is_none()) {
                        a.pv_obj = Some(obj);
                        a.pv_depart = Some(v);
                    }
                }
            }
        }
    }
    maj.sort_by_key(|x| x.0);

    // Killfeed daté : premier kill de chaque victime
    let mut feed: Vec<(u32, String, String)> = Vec::new();
    let mut p = trouver(b, KILLFEED, f.debut, b.len());
    while let Some(i) = p {
        p = trouver(b, KILLFEED, i + 5, b.len());
        let j = i + 5 + 38;
        if b.get(j) != Some(&0) {
            continue;
        }
        let lire = |q: usize| -> Option<(String, usize)> {
            let n = *b.get(q)? as usize;
            Some((String::from_utf8_lossy(b.get(q + 1..q + 1 + n)?).into_owned(), q + 1 + n))
        };
        let Some((tueur, k)) = lire(j + 6) else { continue };
        let Some((victime, _)) = lire(k + 15) else { continue };
        let Some(t) = f.tick(i) else { continue };
        if !victime.is_empty() && !feed.iter().any(|x| x.2 == victime) {
            feed.push((t, tueur, victime));
        }
    }

    // Attribution de chaque perte de PV
    let vide: Vec<Point> = Vec::new();
    let points = |a: &Acteur| a.corps.and_then(|o| tr.get(&o)).map(|t| &t.points).unwrap_or(&vide);
    let mut prec: HashMap<u32, u32> = HashMap::new();
    let mut mort_t: HashMap<String, u32> = HashMap::new();
    let mut coups: Vec<(u32, Coup)> = Vec::new();
    for (t, obj, v) in maj {
        let Some(vi) = acteurs.iter().position(|a| a.pv_obj == Some(obj)) else { continue };
        let victime = &acteurs[vi];
        let Some(avant) = prec.insert(obj, v).or(victime.pv_depart) else { continue };
        if v >= avant {
            continue;
        }
        let tueur_feed = feed.iter().find(|x| x.2 == victime.pseudo && x.0 + 5 >= t && x.0 < t + 400).map(|x| x.1.clone());
        // Ennemis vivants (ou morts depuis moins de 20 ticks) qui visaient la victime : (score, écart latéral, pseudo)
        let mut cands: Vec<(f32, f32, &str)> = Vec::new();
        if let Some(cible) = etat(points(victime), t) {
            for a in acteurs.iter().filter(|a| a.equipe != victime.equipe) {
                if mort_t.get(&a.pseudo).is_some_and(|&m| t > m + 20) {
                    continue;
                }
                let Some(e) = etat(points(a), t) else { continue };
                if e.cap.is_nan() {
                    continue;
                }
                let d = ((cible.x - e.x).powi(2) + (cible.y - e.y).powi(2) + (cible.z - e.z).powi(2)).sqrt();
                let angle = (cible.y - e.y).atan2(cible.x - e.x).to_degrees();
                let ecart = ((e.cap + 90.0 - angle + 180.0).rem_euclid(360.0) - 180.0).abs();
                let lateral = d * ecart.min(90.0).to_radians().sin();
                let age = t.saturating_sub(e.tick).saturating_sub(8) as f32;
                cands.push((lateral + PENALITE * age, lateral, a.pseudo.as_str()));
            }
        }
        cands.sort_by(|x, y| x.0.total_cmp(&y.0));
        let nette = cands.first().is_some_and(|c| c.0 <= SEUIL) && (cands.len() < 2 || cands[1].0 - cands[0].0 > 0.4);
        let alignes: Vec<_> = cands.iter().filter(|c| c.1 <= 1.0).collect();
        let (tireur, certitude) = if v == 0 && tueur_feed.is_some() {
            (tueur_feed.clone(), "feed")
        } else if nette {
            (Some(cands[0].2.to_string()), "visée")
        } else if let Some(k) = tueur_feed.as_ref().filter(|k| cands.iter().any(|c| c.2 == k.as_str() && c.0 <= 3.0 * SEUIL)) {
            (Some(k.clone()), "visée")
        } else if cands.first().is_some_and(|c| c.0 <= SEUIL) {
            (Some(cands[0].2.to_string()), "visée")
        } else if alignes.len() == 1 {
            (Some(alignes[0].2.to_string()), "visée")
        } else if let Some((_, c)) = coups.iter().rev().find(|(tc, c)| c.victime == victime.pseudo && c.tireur.is_some() && t - tc <= 60) {
            (c.tireur.clone(), "suite")
        } else {
            (None, "inconnu")
        };
        let victime_nom = victime.pseudo.clone();
        if v == 0 {
            mort_t.insert(victime_nom.clone(), t);
        }
        coups.push((t, Coup { tireur, victime: victime_nom, degats: avant - v, certitude }));
    }

    let coups: Vec<Coup> = coups.into_iter().map(|(_, c)| c).collect();
    let joueurs = acteurs
        .iter()
        .map(|a| JoueurDegats {
            pseudo: a.pseudo.clone(),
            equipe: a.equipe,
            operateur: m.joueurs.iter().find(|j| j.pseudo == a.pseudo).and_then(|j| j.operateur.clone()),
            inflige: coups.iter().filter(|c| c.tireur.as_deref() == Some(a.pseudo.as_str())).map(|c| c.degats).sum(),
            recu: coups.iter().filter(|c| c.victime == a.pseudo).map(|c| c.degats).sum(),
            kills: m.kills.iter().filter(|k| k.tueur == a.pseudo).count() as u32,
            mort: m.kills.iter().any(|k| k.victime == a.pseudo) || mort_t.contains_key(&a.pseudo),
        })
        .collect();
    Some(Bilan { map: m.map.clone(), manche: m.manche, moi: m.moi.clone(), joueurs, coups })
}
