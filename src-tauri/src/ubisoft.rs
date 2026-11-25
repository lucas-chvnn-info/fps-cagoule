// Stats Ubisoft des joueurs d'une partie de R6 : rang, pic de la saison, niveau, KD et victoires de la saison.
// API non officielle utilisée par les trackers (adresses et identifiants d'application repris de siegeapi,
// github.com/CNDRD/siegeapi, licence MIT). Connexion avec un compte Ubisoft sans double authentification
// (un compte secondaire suffit) : l'e-mail et le mot de passe restent dans le Gestionnaire d'identification de
// Windows, ils passent à PowerShell par l'entrée standard et ne partent que vers public-ubiservices.ubi.com.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const APPID: &str = "4391c956-8943-48eb-8859-07b0778f47b9";
const APPID_RANGS: &str = "e3d5ea9e-50bd-43b7-88bf-39794f4e3d40";
const ESPACE_R6: &str = "0d2ae42d-4c27-4cb7-af6c-2099062302bb";
const AGENT: &str = "UbiServices_SDK_2020.Release.58_PC64_ansi_static";
const CIBLE: &str = "FPS Cagoule/Ubisoft";
/// Les sessions d'Ubisoft durent ~3 h : on en rouvre une un peu avant
const DUREE_SESSION: Duration = Duration::from_secs(2 * 3600 + 45 * 60);

pub const DOUBLE_AUTH: &str = "Ce compte a la double authentification : utilise un compte Ubisoft secondaire sans double authentification (gratuit, il n'a pas besoin du jeu).";

/// Rangs du classement actuel (Ranked 3.0 : 8 paliers de 5 divisions, Champion compris), du plus bas au plus haut :
/// identifiant = position. En Ranked 2.0 (saisons 28 à 39), 36 était « Champion » tout court.
pub const RANGS: [&str; 41] = [
    "Non classé", "Cuivre 5", "Cuivre 4", "Cuivre 3", "Cuivre 2", "Cuivre 1", "Bronze 5", "Bronze 4", "Bronze 3", "Bronze 2", "Bronze 1",
    "Argent 5", "Argent 4", "Argent 3", "Argent 2", "Argent 1", "Or 5", "Or 4", "Or 3", "Or 2", "Or 1", "Platine 5", "Platine 4", "Platine 3",
    "Platine 2", "Platine 1", "Émeraude 5", "Émeraude 4", "Émeraude 3", "Émeraude 2", "Émeraude 1", "Diamant 5", "Diamant 4", "Diamant 3",
    "Diamant 2", "Diamant 1", "Champion 5", "Champion 4", "Champion 3", "Champion 2", "Champion 1",
];
const RANG_MAX: u32 = 40;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct StatsUbi {
    /// Rang actuel, pic de la saison et pic général (meilleur rang vu par l'app, toutes saisons) : identifiants de RANGS
    pub rang: u32,
    pub points: u32,
    pub pic_saison: u32,
    pub pic_general: u32,
    pub niveau: Option<u32>,
    /// Saison en cours, mode classé (ou partie rapide si aucune partie classée)
    pub kills: u32,
    pub morts: u32,
    pub victoires: u32,
    pub defaites: u32,
    pub abandons: u32,
    pub classe: bool,
    /// Saison Ubisoft du rang (28 et plus : classement actuel ; avant : ancien système)
    #[serde(default)]
    pub saison: u32,
}

struct Session {
    ticket: String,
    id: String,
    debut: Instant,
}

static SESSIONS: Mutex<(Option<Session>, Option<Session>)> = Mutex::new((None, None));

fn valide(s: &Option<Session>) -> bool {
    s.as_ref().is_some_and(|s| s.debut.elapsed() < DUREE_SESSION)
}

/// Ouvre une session Ubisoft. `autorisation` (secret) passe par l'entrée standard.
fn ouvrir(autorisation: &str, appid: &str) -> Result<(Session, Value), String> {
    let script = format!(
        r#"[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
try {{
  $r = Invoke-RestMethod -UseBasicParsing -TimeoutSec 20 -Method Post -Uri 'https://public-ubiservices.ubi.com/v3/profiles/sessions' -UserAgent '{AGENT}' -ContentType 'application/json' -Headers @{{ Authorization = $secret.Trim(); 'Ubi-AppId' = '{APPID_X}' }} -Body '{{"rememberMe":true}}'
  $r | ConvertTo-Json -Compress -Depth 6
}} catch {{
  $code = 0; try {{ $code = [int]$_.Exception.Response.StatusCode }} catch {{}}
  @{{ erreur = $code }} | ConvertTo-Json -Compress
}}"#,
        APPID_X = appid
    );
    let texte = crate::ps::executer_avec_secret(&script, autorisation)?;
    let v: Value = serde_json::from_str(&texte).map_err(|_| "Réponse d'Ubisoft illisible".to_string())?;
    if let Some(code) = v.get("erreur").and_then(Value::as_i64) {
        return Err(match code {
            401 => "E-mail ou mot de passe refusé par Ubisoft.".into(),
            429 => "Ubisoft limite les connexions : réessaie dans quelques minutes.".into(),
            0 => "Ubisoft injoignable (connexion Internet ?).".into(),
            c => format!("Connexion à Ubisoft refusée (code {c})."),
        });
    }
    let ticket = v.get("ticket").and_then(Value::as_str).unwrap_or("");
    if ticket.is_empty() {
        return Err(if v.get("twoFactorAuthenticationTicket").is_some_and(|t| !t.is_null()) { DOUBLE_AUTH.into() } else { "Connexion à Ubisoft refusée.".into() });
    }
    let id = v.get("sessionId").and_then(Value::as_str).unwrap_or("").to_string();
    Ok((Session { ticket: ticket.to_string(), id, debut: Instant::now() }, v))
}

/// Requête GET avec la session (le jeton passe par l'entrée standard)
fn lire(url: &str, s: &Session, appid: &str) -> Result<Value, String> {
    let script = format!(
        r#"[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
try {{
  $r = Invoke-RestMethod -UseBasicParsing -TimeoutSec 20 -Uri {url} -UserAgent '{AGENT}' -Headers @{{ Authorization = 'Ubi_v1 t=' + $secret.Trim(); 'Ubi-AppId' = '{appid}'; 'Ubi-SessionId' = '{id}'; 'Ubi-LocaleCode' = 'en-US' }}
  $r | ConvertTo-Json -Compress -Depth 12
}} catch {{
  $code = 0; try {{ $code = [int]$_.Exception.Response.StatusCode }} catch {{}}
  @{{ erreur = $code }} | ConvertTo-Json -Compress
}}"#,
        url = crate::ps::chaine(url),
        id = s.id.replace('\'', "")
    );
    let texte = crate::ps::executer_avec_secret(&script, &s.ticket)?;
    let v: Value = serde_json::from_str(&texte).map_err(|_| "Réponse d'Ubisoft illisible".to_string())?;
    match v.get("erreur").and_then(Value::as_i64) {
        Some(c) => Err(format!("erreur {c}")),
        None => Ok(v),
    }
}

// --- Identifiants dans le Gestionnaire d'identification de Windows ---

fn large(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn memoriser(email: &str, mot_de_passe: &str) -> bool {
    use windows_sys::Win32::Security::Credentials::{CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC};
    let (cible, nom) = (large(CIBLE), large(email));
    let blob: Vec<u8> = mot_de_passe.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut c: CREDENTIALW = unsafe { std::mem::zeroed() };
    c.Type = CRED_TYPE_GENERIC;
    c.TargetName = cible.as_ptr() as *mut u16;
    c.UserName = nom.as_ptr() as *mut u16;
    c.CredentialBlob = blob.as_ptr() as *mut u8;
    c.CredentialBlobSize = blob.len() as u32;
    c.Persist = CRED_PERSIST_LOCAL_MACHINE;
    unsafe { CredWriteW(&c, 0) != 0 }
}

fn identifiants() -> Option<(String, String)> {
    use windows_sys::Win32::Security::Credentials::{CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC};
    let cible = large(CIBLE);
    let mut p: *mut CREDENTIALW = std::ptr::null_mut();
    if unsafe { CredReadW(cible.as_ptr(), CRED_TYPE_GENERIC, 0, &mut p) } == 0 || p.is_null() {
        return None;
    }
    let r = unsafe {
        let c = &*p;
        let nom = if c.UserName.is_null() {
            String::new()
        } else {
            let n = (0..).take_while(|&i| *c.UserName.add(i) != 0).count();
            String::from_utf16_lossy(std::slice::from_raw_parts(c.UserName, n))
        };
        let octets = std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize);
        let unites: Vec<u16> = octets.chunks_exact(2).map(|x| u16::from_le_bytes([x[0], x[1]])).collect();
        (nom, String::from_utf16_lossy(&unites))
    };
    unsafe { CredFree(p as *const _) };
    Some(r)
}

fn oublier() {
    use windows_sys::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};
    let cible = large(CIBLE);
    unsafe { CredDeleteW(cible.as_ptr(), CRED_TYPE_GENERIC, 0) };
}

fn basique(email: &str, mot_de_passe: &str) -> String {
    format!("Basic {}", crate::ps::base64(format!("{email}:{mot_de_passe}").as_bytes()))
}

/// Se connecte et garde les identifiants. Renvoie le pseudo Ubisoft du compte.
pub fn connecter(email: &str, mot_de_passe: &str) -> Result<String, String> {
    let (s, v) = ouvrir(&basique(email.trim(), mot_de_passe), APPID)?;
    if !memoriser(email.trim(), mot_de_passe) {
        return Err("Impossible de garder les identifiants dans Windows.".into());
    }
    let pseudo = v.get("nameOnPlatform").and_then(Value::as_str).unwrap_or("").to_string();
    *SESSIONS.lock().unwrap() = (Some(s), None);
    Ok(pseudo)
}

pub fn deconnecter() {
    oublier();
    *SESSIONS.lock().unwrap() = (None, None);
}

/// Sessions valides (la seconde sert aux rangs), rouvertes avec les identifiants gardés si besoin
fn sessions() -> Result<(), String> {
    let mut s = SESSIONS.lock().unwrap();
    if !valide(&s.0) {
        let (email, mdp) = identifiants().ok_or("Pas de compte Ubisoft connecté.")?;
        s.0 = Some(ouvrir(&basique(&email, &mdp), APPID)?.0);
        s.1 = None;
    }
    if !valide(&s.1) {
        let base = format!("Ubi_v1 t={}", s.0.as_ref().unwrap().ticket);
        s.1 = Some(ouvrir(&base, APPID_RANGS)?.0);
    }
    Ok(())
}

/// Stats des profils (identifiants Ubisoft des replays). Un profil absent de la réponse (nom caché…) n'y est pas.
pub fn stats(profils: &[String]) -> Result<HashMap<String, StatsUbi>, String> {
    let profils: Vec<String> = profils.iter().filter(|p| !p.is_empty() && !p.starts_with("pseudo:")).map(|p| p.to_lowercase()).collect();
    if profils.is_empty() {
        return Ok(HashMap::new());
    }
    sessions()?;
    let liste = profils.join(",");
    let (rangs, niveaux) = {
        let s = SESSIONS.lock().unwrap();
        let rangs = lire(&format!("https://public-ubiservices.ubi.com/v2/spaces/{ESPACE_R6}/title/r6s/skill/full_profiles?profile_ids={liste}&platform_families=pc"), s.1.as_ref().unwrap(), APPID_RANGS);
        let niveaux = lire(&format!("https://public-ubiservices.ubi.com/v1/profiles/stats?profileIds={liste}&spaceId={ESPACE_R6}&statNames=PClearanceLevel"), s.0.as_ref().unwrap(), APPID);
        (rangs, niveaux)
    };
    // Session refusée : on la rouvrira à la prochaine demande
    if rangs.as_ref().is_err_and(|e| e.contains("401")) {
        *SESSIONS.lock().unwrap() = (None, None);
    }
    let mut out: HashMap<String, StatsUbi> = HashMap::new();
    lire_rangs(&rangs?, &mut out);
    if let Ok(n) = niveaux {
        for p in n.get("profiles").and_then(Value::as_array).into_iter().flatten() {
            let id = p.get("profileId").and_then(Value::as_str).unwrap_or("").to_lowercase();
            let niveau = p.pointer("/stats/PClearanceLevel/value").and_then(|v| v.as_str().and_then(|t| t.parse().ok()).or(v.as_u64().map(|x| x as u32)));
            if let (Some(s), Some(n)) = (out.get_mut(&id), niveau) {
                s.niveau = Some(n);
            }
        }
    }
    Ok(out)
}

// --- Serveur FPS Cagoule (r6-api) : un compte Ubisoft gardé sur le serveur, rien à configurer dans l'app ---

/// Requête GET au serveur des stats (pas de secret : rien ne passe par l'entrée standard)
fn serveur(base: &str, chemin: &str) -> Result<Value, String> {
    let url = format!("{}{chemin}", base.trim_end_matches('/'));
    if !url.starts_with("https://") {
        return Err("Adresse du serveur des stats invalide.".into());
    }
    let script = format!(
        r#"[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
try {{
  Invoke-RestMethod -UseBasicParsing -TimeoutSec 25 -Uri {} -Headers @{{ 'X-FPS-Cagoule' = '1' }} | ConvertTo-Json -Compress -Depth 8
}} catch {{
  $code = 0; try {{ $code = [int]$_.Exception.Response.StatusCode }} catch {{}}
  @{{ erreur_serveur = $code }} | ConvertTo-Json -Compress
}}"#,
        crate::ps::chaine(&url)
    );
    let texte = crate::ps::executer(&script)?;
    let v: Value = serde_json::from_str(if texte.is_empty() { "null" } else { &texte }).map_err(|_| "Réponse du serveur des stats illisible".to_string())?;
    match v.get("erreur_serveur").and_then(Value::as_i64) {
        Some(429) => Err("Serveur des stats : trop de demandes, réessaie dans une minute.".into()),
        Some(0) => Err("Serveur des stats injoignable.".into()),
        Some(c) => Err(format!("Serveur des stats : erreur {c}.")),
        None => Ok(v),
    }
}

/// Stats de profils via le serveur (20 par requête)
pub fn stats_serveur(base: &str, profils: &[String]) -> Result<HashMap<String, Option<StatsUbi>>, String> {
    let mut out = HashMap::new();
    let valides: Vec<String> = profils.iter().filter(|p| !p.is_empty() && !p.starts_with("pseudo:")).map(|p| p.to_lowercase()).collect();
    for paquet in valides.chunks(20) {
        let v = serveur(base, &format!("/v1/stats?profils={}", paquet.join(",")))?;
        for p in paquet {
            out.insert(p.clone(), v.get(p).and_then(|s| serde_json::from_value(s.clone()).ok()));
        }
    }
    Ok(out)
}

pub fn chercher_serveur(base: &str, pseudo: &str) -> Result<Vec<(String, String)>, String> {
    let encode: String = pseudo.trim().bytes().map(|o| if o.is_ascii_alphanumeric() || b"-_.~".contains(&o) { (o as char).to_string() } else { format!("%{o:02X}") }).collect();
    let v = serveur(base, &format!("/v1/chercher?pseudo={encode}"))?;
    // ConvertTo-Json transforme un tableau d'un seul élément en objet : on accepte les deux formes
    let liste: Vec<Value> = match v {
        Value::Array(a) if a.first().is_some_and(|x| x.is_string()) => vec![Value::Array(a)],
        Value::Array(a) => a,
        _ => vec![],
    };
    Ok(liste.into_iter().filter_map(|x| Some((x.get(0)?.as_str()?.to_string(), x.get(1)?.as_str()?.to_string()))).collect())
}

pub fn saisons_serveur(base: &str, profil: &str) -> Vec<StatsUbi> {
    match serveur(base, &format!("/v1/saisons?profil={}", profil.to_lowercase())) {
        Ok(Value::Array(a)) => a.into_iter().filter_map(|x| serde_json::from_value(x).ok()).collect(),
        Ok(v @ Value::Object(_)) => serde_json::from_value(v).ok().into_iter().collect(),
        _ => vec![],
    }
}

/// Comptes Ubisoft (PC) dont le pseudo est exactement `pseudo` : (profil, pseudo)
pub fn chercher(pseudo: &str) -> Result<Vec<(String, String)>, String> {
    let pseudo = pseudo.trim();
    if pseudo.is_empty() {
        return Ok(vec![]);
    }
    sessions()?;
    let encode: String = pseudo.bytes().map(|o| if o.is_ascii_alphanumeric() || b"-_.~".contains(&o) { (o as char).to_string() } else { format!("%{o:02X}") }).collect();
    let v = {
        let s = SESSIONS.lock().unwrap();
        lire(&format!("https://public-ubiservices.ubi.com/v3/profiles?nameOnPlatform={encode}&platformType=uplay"), s.0.as_ref().unwrap(), APPID)?
    };
    Ok(v.get("profiles")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| Some((p.get("profileId")?.as_str()?.to_lowercase(), p.get("nameOnPlatform")?.as_str()?.to_string())))
        .collect())
}

/// Réponse de full_profiles : rang et pic de la saison (classé), KD et victoires (classé, sinon partie rapide)
pub fn lire_rangs(v: &Value, out: &mut HashMap<String, StatsUbi>) {
    let tableaux = v.pointer("/platform_families_full_profiles/0/board_ids_full_profiles").and_then(Value::as_array);
    let mut rapide: HashMap<String, StatsUbi> = HashMap::new();
    for t in tableaux.into_iter().flatten() {
        let tableau = t.get("board_id").and_then(Value::as_str).unwrap_or("");
        if tableau != "ranked" && tableau != "standard" {
            continue;
        }
        for fp in t.get("full_profiles").and_then(Value::as_array).into_iter().flatten() {
            let n = |chemin: &str| fp.pointer(chemin).and_then(Value::as_u64).unwrap_or(0) as u32;
            let id = fp.pointer("/profile/id").and_then(Value::as_str).unwrap_or("").to_lowercase();
            if id.is_empty() {
                continue;
            }
            let s = StatsUbi {
                rang: n("/profile/rank").min(RANG_MAX),
                points: n("/profile/rank_points"),
                pic_saison: n("/profile/max_rank").min(RANG_MAX),
                pic_general: 0,
                niveau: None,
                kills: n("/season_statistics/kills"),
                morts: n("/season_statistics/deaths"),
                victoires: n("/season_statistics/match_outcomes/wins"),
                defaites: n("/season_statistics/match_outcomes/losses"),
                abandons: n("/season_statistics/match_outcomes/abandons"),
                classe: tableau == "ranked",
                saison: n("/profile/season_id"),
            };
            if tableau == "ranked" {
                out.insert(id, s);
            } else {
                rapide.insert(id, s);
            }
        }
    }
    // Aucune partie classée cette saison : KD et victoires de la partie rapide, rang tel quel
    for (id, r) in rapide {
        let s = out.entry(id).or_insert_with(|| StatsUbi { classe: false, ..r.clone() });
        if s.victoires + s.defaites + s.abandons == 0 {
            (s.kills, s.morts, s.victoires, s.defaites, s.abandons, s.classe) = (r.kills, r.morts, r.victoires, r.defaites, r.abandons, false);
        }
    }
}

/// Historique par saison : Ubisoft ne donne que la saison en cours, l'app garde donc la dernière photo de chaque
/// saison de chaque joueur vu (ubisoft-saisons.json : profil → saison → stats)
fn noter_saisons(dossier: &Path, stats: &HashMap<String, StatsUbi>) {
    let chemin = dossier.join("ubisoft-saisons.json");
    let mut toutes: HashMap<String, std::collections::BTreeMap<u32, StatsUbi>> = std::fs::read_to_string(&chemin).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    for (id, s) in stats.iter().filter(|(_, s)| s.saison > 0) {
        toutes.entry(id.clone()).or_default().insert(s.saison, s.clone());
    }
    if let Ok(t) = serde_json::to_string(&toutes) {
        let tmp = chemin.with_extension("tmp");
        if std::fs::write(&tmp, t).is_ok() {
            let _ = std::fs::rename(&tmp, &chemin);
        }
    }
}

/// Saisons connues d'un profil, la plus récente d'abord
pub fn saisons(dossier: &Path, profil: &str) -> Vec<StatsUbi> {
    let toutes: HashMap<String, std::collections::BTreeMap<u32, StatsUbi>> =
        std::fs::read_to_string(dossier.join("ubisoft-saisons.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    toutes.get(&profil.to_lowercase()).map(|m| m.values().rev().cloned().collect()).unwrap_or_default()
}

/// Pic général : meilleur rang jamais vu par l'app pour chaque joueur (ubisoft-pics.json), mis à jour ici,
/// avec l'historique des saisons
pub fn pics_generaux(dossier: &Path, stats: &mut HashMap<String, StatsUbi>) {
    noter_saisons(dossier, stats);
    let chemin = dossier.join("ubisoft-pics.json");
    let mut pics: HashMap<String, u32> = std::fs::read_to_string(&chemin).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut change = false;
    for (id, s) in stats.iter_mut() {
        let pic = pics.entry(id.clone()).or_default();
        if s.pic_saison.max(s.rang) > *pic {
            *pic = s.pic_saison.max(s.rang);
            change = true;
        }
        s.pic_general = *pic;
    }
    if change {
        if let Ok(t) = serde_json::to_string(&pics) {
            let _ = std::fs::write(&chemin, t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lecture_des_rangs() {
        let v: Value = serde_json::from_str(
            r#"{"platform_families_full_profiles":[{"platform_family":"pc","board_ids_full_profiles":[
            {"board_id":"ranked","full_profiles":[
              {"profile":{"id":"AAA","rank":19,"rank_points":2850,"max_rank":21,"season_id":40},"season_statistics":{"kills":120,"deaths":100,"match_outcomes":{"wins":12,"losses":8,"abandons":1}}},
              {"profile":{"id":"bbb","rank":0,"rank_points":1000,"max_rank":0,"season_id":40},"season_statistics":{"kills":0,"deaths":0,"match_outcomes":{"wins":0,"losses":0,"abandons":0}}}]},
            {"board_id":"standard","full_profiles":[
              {"profile":{"id":"bbb","rank":0,"max_rank":0},"season_statistics":{"kills":50,"deaths":40,"match_outcomes":{"wins":6,"losses":4,"abandons":0}}}]},
            {"board_id":"event","full_profiles":[]}]}]}"#,
        )
        .unwrap();
        let mut out = HashMap::new();
        lire_rangs(&v, &mut out);
        let a = &out["aaa"];
        assert_eq!((RANGS[a.rang as usize], RANGS[a.pic_saison as usize], a.kills, a.victoires, a.classe), ("Or 2", "Platine 5", 120, 12, true));
        // Pas de partie classée : KD et victoires de la partie rapide
        let b = &out["bbb"];
        assert_eq!((b.kills, b.victoires, b.classe), (50, 6, false));
    }
}
