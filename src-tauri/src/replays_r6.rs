// Lecture des replays de Rainbow Six Siege (fichiers .rec du « Match Replay », un par manche) : map, joueurs,
// opérateurs et killfeed. Format repris de r6-dissect (licence MIT, Benjamin Ryan), limité à ce qu'il faut
// pour les clips, et sans jamais planter sur un opérateur ou une map d'une nouvelle saison : ils restent « inconnus ».

use serde::Serialize;
use std::io::Read;

/// Opérateurs connus : (identifiant dans les replays, nom, attaque ?). Repris de r6-dissect (licence MIT, Benjamin Ryan).
pub const OPERATEURS: &[(u64, &str, Option<bool>)] = &[
    (104189664390, "Ace", Some(true)),
    (104189662071, "Alibi", Some(false)),
    (104189663607, "Amaru", Some(true)),
    (104189664704, "Aruni", Some(false)),
    (92270642656, "Ash", Some(true)),
    (378305069945, "Azami", Some(false)),
    (92270642526, "Bandit", Some(false)),
    (92270642136, "Blackbeard", Some(true)),
    (92270642539, "Blitz", Some(true)),
    (288200866821, "Brava", Some(true)),
    (92270642474, "Buck", Some(true)),
    (92270644215, "Capitão", Some(true)),
    (92270642682, "Castle", Some(false)),
    (92270644241, "Caveira", Some(false)),
    (104189662280, "Clash", Some(false)),
    (374667787816, "Deimos", Some(true)),
    (374667787937, "Denari", Some(false)),
    (92270644007, "Doc", Some(false)),
    (92270644267, "Dokkaebi", Some(true)),
    (92270642214, "Echo", Some(false)),
    (92270644163, "Ela", Some(false)),
    (288200867339, "Fenrir", Some(false)),
    (104189661965, "Finka", Some(true)),
    (328397386974, "Flores", Some(true)),
    (92270642500, "Frost", Some(false)),
    (92270642032, "Fuze", Some(true)),
    (92270642084, "Glaz", Some(true)),
    (104189663698, "Goyo", Some(false)),
    (174977508808, "Gridlock", Some(true)),
    (374667788042, "Grim", Some(true)),
    (92270642240, "Hibana", Some(true)),
    (92270642578, "IQ", Some(true)),
    (104189664038, "Iana", Some(true)),
    (92270644345, "Jackal", Some(true)),
    (92270642604, "Jäger", Some(false)),
    (161289666230, "Kaid", Some(false)),
    (104189663920, "Kali", Some(true)),
    (92270641980, "Kapkan", Some(false)),
    (92270642266, "Lesion", Some(false)),
    (104189661861, "Lion", Some(true)),
    (104189662175, "Maestro", Some(false)),
    (104189662384, "Maverick", Some(true)),
    (104189664273, "Melusi", Some(false)),
    (92270644319, "Mira", Some(false)),
    (92270644033, "Montagne", Some(true)),
    (174977508820, "Mozzie", Some(false)),
    (92270642318, "Mute", Some(false)),
    (104189663024, "Nøkk", Some(true)),
    (161289666248, "Nomad", Some(true)),
    (104189664155, "Oryx", Some(false)),
    (288200867444, "Osa", Some(true)),
    (92270642708, "Pulse", Some(false)),
    (395943091136, "Ram", Some(true)),
    (386098331923, "Rauora", Some(true)),
    (359656345734, "Recrue", None),
    (92270644059, "Rook", Some(false)),
    (384797789346, "Sens", Some(true)),
    (409899350403, "Sentry", Some(false)),
    (386098331713, "Skopos", Some(false)),
    (92270642344, "Sledge", Some(true)),
    (92270642396, "Smoke", Some(false)),
    (391752120891, "Solis", Some(false)),
    (409899350463, "Striker", Some(true)),
    (291437347686, "Tachanka", Some(false)),
    (92270642422, "Thatcher", Some(true)),
    (92270642760, "Thermite", Some(true)),
    (373711624351, "Thorn", Some(false)),
    (288200867351, "Thunderbird", Some(false)),
    (288200867549, "Tubarão", Some(false)),
    (92270644111, "Twitch", Some(true)),
    (92270642188, "Valkyrie", Some(false)),
    (92270644293, "Vigil", Some(false)),
    (104189663803, "Wamai", Some(false)),
    (104189662920, "Warden", Some(false)),
    (92270642292, "Ying", Some(true)),
    (291191151607, "Zero", Some(true)),
    (92270644189, "Zofia", Some(true)),
];

/// Maps connues : (worldid, nom)
pub const MAPS: &[(u64, &str)] = &[
    (837214085, "Clubhouse"),
    (1378191338, "Kafe"),
    (1460220617, "Kanal"),
    (1767965020, "Yacht"),
    (2609218856, "Presidential Plane"),
    (2609221242, "Consulate"),
    (2697268122, "Bartlett"),
    (42090092951, "Coastline"),
    (53627213396, "Tower"),
    (88107330328, "Villa"),
    (126196841359, "Fortress"),
    (127951053400, "Hereford"),
    (199824623654, "Theme Park"),
    (231702797556, "Oregon"),
    (237873412352, "House"),
    (259816839773, "Chalet"),
    (270063334510, "Stadium Bravo"),
    (276279025182, "Skyscraper"),
    (305979357167, "Border"),
    (329867321446, "Favela"),
    (355496559878, "Bank"),
    (362605108559, "Outback"),
    (365284490964, "Emerald Plains"),
    (378595635123, "Nighthaven Labs"),
    (379218689149, "Consulate"),
    (388073319671, "Lair"),
    (405306299908, "Stadium"),
    (407193663917, "Clubhouse"),
    (407558616688, "Chalet"),
    (407987100456, "Border"),
    (413779563590, "Bank"),
    (413845419788, "Kafe"),
    (417890697769, "Lair"),
    (418119057546, "Nighthaven Labs"),
    (418126004176, "Consulate"),
];

/// Versions du jeu (« code » de l'en-tête) à partir desquelles le format change
const Y9S1_UPDATE3: u64 = 8_211_379;
const Y9S3: u64 = 8_506_016;

const ZSTD: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];
const SEPARATEUR: [u8; 7] = [0; 7];
const JOUEUR: &[u8] = &[0x22, 0x07, 0x94, 0x9B, 0xDC];
const CHANGEMENT_OPERATEUR: &[u8] = &[0x22, 0xA9, 0x26, 0x0B, 0xE4];
const CHRONO: &[u8] = &[0x1F, 0x07, 0xEF, 0xC9];
const KILLFEED: &[u8] = &[0x59, 0x34, 0xE5, 0x8B, 0x04];
const INDICE_KILL: [u8; 5] = [0x22, 0xD9, 0x13, 0x3C, 0xBA];

pub fn nom_operateur(id: u64) -> Option<&'static str> {
    OPERATEURS.iter().find(|(i, _, _)| *i == id).map(|(_, n, _)| *n)
}

pub fn nom_map(id: u64) -> Option<&'static str> {
    MAPS.iter().find(|(i, _)| *i == id).map(|(_, n)| *n)
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Joueur {
    pub pseudo: String,
    /// 0 ou 1
    pub equipe: u8,
    /// Identifiant de l'opérateur dans le replay (0 = pas encore choisi)
    pub operateur_id: u64,
    pub operateur: Option<String>,
    /// Identifiant Ubisoft du profil (sert à reconnaître le joueur qui enregistre)
    pub profil: String,
    #[serde(skip)]
    id_entete: u64,
    #[serde(skip)]
    id_dissect: Vec<u8>,
    #[serde(skip)]
    id_ui: u64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Kill {
    /// Joueur crédité du kill (celui qui a mis à terre, si la victime a été achevée ensuite)
    pub tueur: String,
    pub victime: String,
    pub headshot: bool,
    /// Secondes restantes au chrono de la manche au moment de la mort
    pub chrono: u32,
    /// Chrono de la mise à terre (KO) quand la victime a d'abord été mise à terre
    pub ko: Option<u32>,
}

/// Moment de la manche à marquer sur la vidéo
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Evenement {
    pub chrono: u32,
    /// « kill », « ko » ou « mort »
    pub genre: &'static str,
    /// Victime (kill, KO) ou tueur (mort)
    pub detail: String,
    pub headshot: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct Manche {
    pub version: String,
    pub code: u64,
    pub map_id: u64,
    pub map: Option<String>,
    pub manche: u32,
    /// Pseudo du joueur qui a enregistré le replay (toi)
    pub moi: Option<String>,
    pub joueurs: Vec<Joueur>,
    pub kills: Vec<Kill>,
    /// Chrono à la fin de l'enregistrement de la manche
    pub chrono_fin: u32,
}

impl Manche {
    /// Kills du joueur qui enregistre, dans l'ordre
    pub fn mes_kills(&self) -> Vec<&Kill> {
        let Some(moi) = &self.moi else { return vec![] };
        self.kills.iter().filter(|k| &k.tueur == moi).collect()
    }

    /// Mes KO, mes kills et ma mort, dans l'ordre de la manche
    pub fn mes_evenements(&self) -> Vec<Evenement> {
        let Some(moi) = &self.moi else { return vec![] };
        let mut e = Vec::new();
        for k in &self.kills {
            if &k.tueur == moi {
                if let Some(ko) = k.ko {
                    e.push(Evenement { chrono: ko, genre: "ko", detail: k.victime.clone(), headshot: false });
                }
                e.push(Evenement { chrono: k.chrono, genre: "kill", detail: k.victime.clone(), headshot: k.headshot });
            } else if &k.victime == moi {
                e.push(Evenement { chrono: k.chrono, genre: "mort", detail: k.tueur.clone(), headshot: k.headshot });
            }
        }
        // Le chrono descend : le premier moment a le plus grand chrono
        e.sort_by(|a, b| b.chrono.cmp(&a.chrono));
        e
    }

    pub fn mon_operateur(&self) -> Option<&Joueur> {
        let moi = self.moi.as_ref()?;
        self.joueurs.iter().find(|j| &j.pseudo == moi)
    }
}

/// Curseur sur les données décompressées
struct Curseur<'a> {
    b: &'a [u8],
    pos: usize,
}

impl Curseur<'_> {
    fn octets(&mut self, n: usize) -> Option<&[u8]> {
        let fin = self.pos.checked_add(n)?;
        let s = self.b.get(self.pos..fin)?;
        self.pos = fin;
        Some(s)
    }
    fn sauter(&mut self, n: usize) -> Option<()> {
        self.octets(n).map(|_| ())
    }
    fn octet(&mut self) -> Option<u8> {
        self.octets(1).map(|s| s[0])
    }
    fn texte(&mut self) -> Option<String> {
        let n = self.octet()? as usize;
        self.octets(n).map(|s| String::from_utf8_lossy(s).into_owned())
    }
    /// Entier précédé d'un octet de taille (ignoré)
    fn u32(&mut self) -> Option<u32> {
        self.sauter(1)?;
        self.octets(4).map(|s| u32::from_le_bytes(s.try_into().unwrap()))
    }
    fn u64(&mut self) -> Option<u64> {
        self.sauter(1)?;
        self.octets(8).map(|s| u64::from_le_bytes(s.try_into().unwrap()))
    }
    /// Avance juste après le prochain motif
    fn chercher(&mut self, motif: &[u8]) -> Option<()> {
        let reste = self.b.get(self.pos..)?;
        let i = reste.windows(motif.len()).position(|w| w == motif)?;
        self.pos += i + motif.len();
        Some(())
    }
    fn texte_entete(&mut self) -> Option<String> {
        let n = self.octet()? as usize;
        if self.octets(7)? != SEPARATEUR {
            return None;
        }
        self.octets(n).map(|s| String::from_utf8_lossy(s).into_owned())
    }
}

/// Décompresse les blocs zstd qui suivent l'en-tête
fn decompresser(b: &[u8]) -> Result<Vec<u8>, String> {
    let mut sortie = Vec::new();
    let mut pos = 0;
    while let Some(i) = b.get(pos..).and_then(|r| r.windows(4).position(|w| w == ZSTD)) {
        let debut = pos + i;
        let mut source = &b[debut..];
        let avant = source.len();
        match ruzstd::decoding::StreamingDecoder::new(&mut source) {
            Ok(mut d) => {
                let mut bloc = Vec::new();
                let lu = d.read_to_end(&mut bloc);
                drop(d);
                let consomme = avant - source.len();
                if lu.is_ok() || !bloc.is_empty() {
                    sortie.extend_from_slice(&bloc);
                }
                // Bloc illisible : on cherche le suivant juste après ce faux départ
                pos = if lu.is_ok() && consomme > 4 { debut + consomme } else { debut + 4 };
            }
            Err(_) => pos = debut + 4,
        }
    }
    if sortie.is_empty() {
        Err("Replay vide ou illisible".into())
    } else {
        Ok(sortie)
    }
}

/// Lit une manche (.rec)
pub fn lire(fichier: &[u8]) -> Result<Manche, String> {
    if !fichier.starts_with(b"dissect") {
        return Err("Ce n'est pas un replay de Rainbow Six Siege".into());
    }
    let mut c = Curseur { b: fichier, pos: 7 };
    // Fin du préambule : deux suites de 7 octets nuls
    let (mut nuls, mut suites) = (0, 0);
    while suites != 2 {
        if c.octet().ok_or("En-tête tronqué")? == 0 {
            if nuls != 6 {
                nuls += 1;
            } else {
                nuls = 0;
                suites += 1;
            }
        } else {
            nuls = 0;
        }
    }
    // En-tête : paires clé / valeur, les joueurs à la suite les uns des autres
    let mut proprietes: Vec<(String, String)> = Vec::new();
    let mut joueurs: Vec<Joueur> = Vec::new();
    let mut dans_joueur = false;
    loop {
        let k = c.texte_entete().ok_or("En-tête illisible")?;
        let v = c.texte_entete().ok_or("En-tête illisible")?;
        if k == "playerid" {
            dans_joueur = true;
            joueurs.push(Joueur { pseudo: String::new(), equipe: 0, operateur_id: 0, operateur: None, profil: String::new(), id_entete: v.parse().unwrap_or(0), id_dissect: vec![], id_ui: 0 });
        } else if k == "playlistcategory" || k == "id" {
            dans_joueur = false;
        }
        match (dans_joueur, k.as_str(), joueurs.last_mut()) {
            (true, "playername", Some(j)) => j.pseudo = v.clone(),
            (true, "team", Some(j)) => j.equipe = v.parse().unwrap_or(0),
            (true, "profileid", Some(j)) => j.profil = v.clone(),
            _ => {}
        }
        let fin = k == "teamscore1";
        proprietes.push((k, v));
        if fin {
            break;
        }
    }
    let prop = |k: &str| proprietes.iter().find(|(c, _)| c == k).map(|(_, v)| v.as_str());
    let code: u64 = prop("code").and_then(|v| v.parse().ok()).unwrap_or(0);
    if code < Y9S3 {
        return Err("Replay d'une saison trop ancienne (avant Y9S3)".into());
    }
    let map_id: u64 = prop("worldid").and_then(|v| v.parse().ok()).unwrap_or(0);
    let id_moi: u64 = prop("recordingplayerid").and_then(|v| v.parse().ok()).unwrap_or(0);
    let profil_moi = prop("recordingprofileid").unwrap_or("").to_string();
    let mut manche = Manche {
        version: prop("version").unwrap_or("").into(),
        code,
        map_id,
        map: nom_map(map_id).map(String::from),
        manche: prop("roundnumber").and_then(|v| v.parse::<u32>().ok()).map_or(0, |n| n + 1),
        moi: None,
        joueurs,
        kills: Vec::new(),
        chrono_fin: 0,
    };
    let donnees = decompresser(&fichier[c.pos..])?;
    lire_paquets(&donnees, &mut manche, !profil_moi.is_empty());
    manche.joueurs.retain(|j| !j.pseudo.is_empty());
    for j in &mut manche.joueurs {
        j.operateur = nom_operateur(j.operateur_id).map(String::from);
    }
    manche.moi = manche
        .joueurs
        .iter()
        .find(|j| (!profil_moi.is_empty() && j.profil == profil_moi) || (id_moi != 0 && j.id_entete == id_moi))
        .map(|j| j.pseudo.clone());
    Ok(manche)
}

/// Parcourt les paquets dans l'ordre : joueurs (opérateur), changements d'opérateur, chrono, killfeed
fn lire_paquets(b: &[u8], m: &mut Manche, profils: bool) {
    let motifs: [&[u8]; 4] = [JOUEUR, CHANGEMENT_OPERATEUR, CHRONO, KILLFEED];
    let mut chrono = 0u32;
    let mut joueurs_lus = 0;
    let mut i = 0;
    while i < b.len() {
        let Some(n) = motifs.iter().position(|motif| b[i..].starts_with(motif)) else {
            i += 1;
            continue;
        };
        let mut c = Curseur { b, pos: i + motifs[n].len() };
        match n {
            0 => {
                joueurs_lus += 1;
                let _ = lire_joueur(&mut c, m, joueurs_lus, profils);
            }
            1 => {
                let _ = lire_changement(&mut c, m);
            }
            2 => {
                if let Some(t) = c.u32() {
                    chrono = t;
                    m.chrono_fin = t;
                }
            }
            _ => {
                let _ = lire_kill(&mut c, m, chrono);
            }
        }
        i += 1;
    }
}

fn lire_joueur(c: &mut Curseur, m: &mut Manche, rang: usize, profils: bool) -> Option<()> {
    let pseudo = c.texte()?;
    c.chercher(&[0x40, 0xF2, 0x15, 0x04])?;
    c.sauter(8)?;
    if c.octet()? == 0x9D {
        return None; // paquet en double
    }
    let op = c.u64()?;
    if op == 0 || c.octet()? != 0x22 {
        return None;
    }
    c.chercher(&[0x33, 0xD8, 0x3D, 0x4F, 0x23])?;
    let id = c.octets(4)?.to_vec();
    c.chercher(&[0xAF, 0x98, 0x99, 0xCA])?;
    let apparition = c.texte()?;
    if apparition.is_empty() {
        c.sauter(10)?;
        if c.octet()? != 0x1B {
            return None;
        }
    }
    c.chercher(&[0x38, 0xDF, 0xEE, 0x88])?;
    c.sauter(13)?;
    let id_ui = c.u64()?;
    let mut profil = String::new();
    if profils {
        c.chercher(&[0x8A, 0x50, 0x9B, 0xD0])?;
        profil = c.texte()?;
    }
    let equipe = u8::from(rang > 5);
    match m.joueurs.iter_mut().find(|j| j.pseudo == pseudo || (!j.id_dissect.is_empty() && j.id_dissect == id)) {
        Some(j) => {
            j.pseudo = pseudo;
            j.operateur_id = op;
            j.id_dissect = id;
            j.id_ui = id_ui;
            if !profil.is_empty() {
                j.profil = profil;
            }
        }
        None if !pseudo.is_empty() => m.joueurs.push(Joueur { pseudo, equipe, operateur_id: op, operateur: None, profil, id_entete: 0, id_dissect: id, id_ui }),
        None => {}
    }
    Some(())
}

/// Attaquant qui change d'opérateur pendant la préparation
fn lire_changement(c: &mut Curseur, m: &mut Manche) -> Option<()> {
    let op = c.u64()?;
    c.sauter(402)?;
    let id_ui = c.u64()?;
    if let Some(j) = m.joueurs.iter_mut().find(|j| j.id_ui == id_ui && id_ui != 0) {
        j.operateur_id = op;
    }
    Some(())
}

fn lire_kill(c: &mut Curseur, m: &mut Manche, chrono: u32) -> Option<()> {
    if m.code < Y9S1_UPDATE3 {
        return None;
    }
    c.sauter(38)?;
    if c.octet()? != 0 || c.octets(5)? != INDICE_KILL {
        return None;
    }
    let tueur = c.texte()?;
    c.sauter(15)?;
    let victime = c.texte()?;
    if tueur.is_empty() || victime.is_empty() {
        return None; // mort sans tueur (chute, explosion de soi-même…)
    }
    c.sauter(56)?;
    let headshot = c.octet()? == 1;
    // Même victime une seconde fois : la première était une mise à terre (KO), celle-ci l'achève.
    // Le kill reste à celui qui l'a mise à terre (règle du jeu) ; un paquet en double (même chrono) est ignoré.
    match m.kills.iter_mut().find(|k| k.victime == victime) {
        Some(k) if k.chrono != chrono && k.ko.is_none() => {
            k.ko = Some(k.chrono);
            k.chrono = chrono;
            k.headshot |= headshot;
        }
        Some(_) => {}
        None => m.kills.push(Kill { tueur, victime, headshot, chrono, ko: None }),
    }
    Some(())
}

/// Délai supposé entre le dernier chrono de la manche et l'écriture du replay (à confirmer sur de vraies parties)
pub const DECALAGE_ECRITURE_S: f64 = 2.0;

/// Instant réel d'un moment de la manche, d'après la date d'écriture du replay. None si le chrono est passé
/// au compte à rebours du défuseur entre-temps (les deux chronos ne se comparent pas).
pub fn instant(m: &Manche, chrono: u32, ecriture: std::time::SystemTime) -> Option<std::time::SystemTime> {
    if chrono < m.chrono_fin {
        return None;
    }
    let recul = (chrono - m.chrono_fin) as f64 + DECALAGE_ECRITURE_S;
    ecriture.checked_sub(std::time::Duration::from_secs_f64(recul))
}

/// Secondes à garder pour couvrir tous mes kills de la manche : du premier kill moins 15 s jusqu'à la fin,
/// plus le temps écoulé depuis l'écriture du replay. Si le chrono est passé au compte à rebours du défuseur,
/// l'écart n'est plus calculable : on garde 2 min.
pub fn duree_clip(m: &Manche, depuis_ecriture_s: u32) -> u32 {
    let premier = m.mes_kills().first().map_or(0, |k| k.chrono);
    let ecart = if premier >= m.chrono_fin { premier - m.chrono_fin } else { 105 };
    (ecart + 15 + 8 + depuis_ecriture_s).clamp(20, 240)
}

/// Dossiers « MatchReplay » de Rainbow Six Siege (un par compte Ubisoft) sous Documents\My Games
pub fn dossiers_replays(documents: &std::path::Path) -> Vec<std::path::PathBuf> {
    let racine = documents.join("My Games").join("Rainbow Six - Siege");
    std::fs::read_dir(racine)
        .map(|l| l.flatten().map(|e| e.path().join("MatchReplay")).filter(|p| p.is_dir()).collect())
        .unwrap_or_default()
}

/// Replays des manches (.rec) modifiés après `depuis`, avec leur date de modification
pub fn manches_recentes(documents: &std::path::Path, depuis: std::time::SystemTime) -> Vec<(std::path::PathBuf, std::time::SystemTime)> {
    let mut liste = Vec::new();
    for d in dossiers_replays(documents) {
        let Ok(parties) = std::fs::read_dir(d) else { continue };
        for partie in parties.flatten().filter(|e| e.path().is_dir()) {
            let Ok(m) = partie.metadata().and_then(|m| m.modified()) else { continue };
            if m < depuis {
                continue;
            }
            let Ok(fichiers) = std::fs::read_dir(partie.path()) else { continue };
            for f in fichiers.flatten() {
                let p = f.path();
                if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("rec")) {
                    if let Ok(t) = f.metadata().and_then(|m| m.modified()) {
                        if t >= depuis {
                            liste.push((p, t));
                        }
                    }
                }
            }
        }
    }
    liste.sort_by_key(|(_, t)| *t);
    liste
}

/// Nom du clip : « 3K · Ace · Clubhouse · Pseudo », « ACE · Ash · Bank · Pseudo » (aucun pour moins de 2 kills).
/// Le pseudo est celui du compte Ubisoft qui a joué la manche.
pub fn titre_clip(m: &Manche) -> Option<String> {
    let n = m.mes_kills().len();
    if n < 2 {
        return None;
    }
    let kills = if n >= 5 { "ACE".to_string() } else { format!("{n}K") };
    let op = m.mon_operateur().and_then(|j| j.operateur.clone()).unwrap_or_else(|| "Nouvel opérateur".into());
    let map = m.map.clone().unwrap_or_else(|| "Nouvelle map".into());
    let pseudo = m.moi.as_deref().unwrap_or_default();
    Some(format!("{kills} · {op} · {map} · {pseudo}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manche(kills: &[(&str, u32)], op: u64, chrono_fin: u32) -> Manche {
        Manche {
            version: "Y11S3".into(),
            code: 9_500_000,
            map_id: 407193663917,
            map: nom_map(407193663917).map(String::from),
            manche: 5,
            moi: Some("Cagoule".into()),
            joueurs: vec![Joueur { pseudo: "Cagoule".into(), equipe: 0, operateur_id: op, operateur: nom_operateur(op).map(String::from), profil: String::new(), id_entete: 0, id_dissect: vec![], id_ui: 0 }],
            kills: kills.iter().enumerate().map(|(i, (t, c))| Kill { tueur: t.to_string(), victime: format!("ennemi{i}"), headshot: false, chrono: *c, ko: None }).collect(),
            chrono_fin,
        }
    }

    #[test]
    fn titres_des_clips() {
        assert_eq!(titre_clip(&manche(&[("Cagoule", 150), ("Autre", 140), ("Cagoule", 120), ("Cagoule", 100)], 104189664390, 60)).as_deref(), Some("3K · Ace · Clubhouse · Cagoule"));
        assert_eq!(titre_clip(&manche(&[("Cagoule", 150); 5], 92270642656, 0)).as_deref(), Some("ACE · Ash · Clubhouse · Cagoule"));
        // Opérateur d'une nouvelle saison : pas de plantage, nom provisoire
        assert_eq!(titre_clip(&manche(&[("Cagoule", 150), ("Cagoule", 140)], 444310693746, 0)).as_deref(), Some("2K · Nouvel opérateur · Clubhouse · Cagoule"));
        assert_eq!(titre_clip(&manche(&[("Cagoule", 150)], 104189664390, 0)), None);
    }

    #[test]
    fn ko_et_evenements() {
        let mut m = manche(&[("Cagoule", 150), ("Ennemi", 90)], 0, 0);
        m.kills[1].victime = "Cagoule".into();
        m.kills[0].ko = Some(160);
        let e = m.mes_evenements();
        assert_eq!(e.iter().map(|x| (x.genre, x.chrono)).collect::<Vec<_>>(), vec![("ko", 160), ("kill", 150), ("mort", 90)]);
        assert_eq!(e[2].detail, "Ennemi");
    }

    #[test]
    fn duree_des_clips() {
        // Premier kill à 2:30, fin à 1:00 : 90 s + 15 s avant + 8 s de marge + 3 s depuis l'écriture
        assert_eq!(duree_clip(&manche(&[("Cagoule", 150), ("Cagoule", 120)], 0, 60), 3), 116);
        // Chrono du défuseur après une pose : 2 min par défaut
        assert_eq!(duree_clip(&manche(&[("Cagoule", 30), ("Cagoule", 20)], 0, 40), 0), 128);
    }

    #[test]
    fn fichier_invalide() {
        assert!(lire(b"pas un replay").is_err());
        assert!(lire(b"dissect").is_err());
    }

    /// Vrai replay public (Y10S3, partagé sur le dépôt de r6-dissect) : joueurs, opérateurs et killfeed
    #[test]
    fn replay_public() {
        let zip = std::env::temp_dir().join("fps-cagoule-replay-test.zip");
        let script = format!(
            "Invoke-WebRequest -UseBasicParsing -TimeoutSec 120 -Uri 'https://github.com/user-attachments/files/23219645/Match-2025-10-29_20-08-15-22388-R07.zip' -OutFile {z}
Add-Type -AssemblyName System.IO.Compression.FileSystem
$a = [IO.Compression.ZipFile]::OpenRead({z}); try {{ $e = $a.Entries[0]; $f = $e.Open(); $m = New-Object IO.MemoryStream; $f.CopyTo($m); [Convert]::ToBase64String($m.ToArray()) }} finally {{ $a.Dispose() }}",
            z = crate::ps::chaine(&zip.to_string_lossy())
        );
        let texte = crate::ps::executer(&script).expect("téléchargement du replay public");
        let _ = std::fs::remove_file(&zip);
        let octets = crate::images::decoder_base64_pub(&texte).expect("base64");
        let m = lire(&octets).expect("lecture du replay");
        assert_eq!((m.map.as_deref(), m.manche), (Some("Chalet"), 7));
        assert_eq!(m.joueurs.len(), 10);
        assert!(m.joueurs.iter().all(|j| j.operateur.is_some()), "{:?}", m.joueurs);
        assert!(m.kills.len() >= 8 && m.kills.iter().any(|k| k.headshot), "{:?}", m.kills);
        assert!(m.kills.iter().any(|k| k.tueur == "Nxtgen.WW" && k.victime == "IIIIIlIlI_IllIl" && k.headshot));
    }
}
