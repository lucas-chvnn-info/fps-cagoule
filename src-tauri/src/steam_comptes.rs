// Comptes Steam mémorisés sur le PC (config\loginusers.vdf) et changement de compte avant de lancer un jeu,
// comme les « account switchers » : Steam est fermé, le compte choisi devient celui de la connexion
// automatique, puis Steam est relancé avec le jeu. Seuls les comptes dont Steam a gardé la connexion marchent.

use crate::bibliotheques::steam_racine;
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
use winreg::RegKey;

/// Écart entre un SteamID64 et l'identifiant de compte 32 bits
const BASE_STEAMID: u64 = 76_561_197_960_265_728;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct CompteSteam {
    /// SteamID64
    pub id: String,
    /// Identifiant de connexion
    pub identifiant: String,
    /// Pseudo affiché
    pub pseudo: String,
    /// Photo de profil en data URL (cache de Steam)
    pub avatar: Option<String>,
    /// Compte connecté en ce moment
    pub actif: bool,
    /// Steam a gardé la connexion : pas besoin du mot de passe
    pub memorise: bool,
}

fn fichier_comptes() -> Option<PathBuf> {
    Some(steam_racine()?.join(r"config\loginusers.vdf"))
}

/// Comptes du fichier loginusers.vdf : (SteamID64, [(clé, valeur)])
fn lire_comptes(texte: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut comptes: Vec<(String, Vec<(String, String)>)> = Vec::new();
    let mut profondeur = 0;
    let mut dernier_nom: Option<String> = None;
    for ligne in texte.lines() {
        let l = ligne.trim();
        if l == "{" {
            profondeur += 1;
            if profondeur == 2 {
                if let Some(id) = dernier_nom.take() {
                    comptes.push((id, Vec::new()));
                }
            }
            continue;
        }
        if l == "}" {
            profondeur -= 1;
            continue;
        }
        let morceaux: Vec<&str> = l.split('"').collect();
        if morceaux.len() == 3 {
            // "7656119…" seul sur sa ligne : début d'un compte
            dernier_nom = Some(morceaux[1].to_string());
        } else if morceaux.len() >= 5 && profondeur == 2 {
            if let Some(c) = comptes.last_mut() {
                c.1.push((morceaux[1].to_string(), morceaux[3].to_string()));
            }
        }
    }
    comptes
}

/// Le compte choisi devient le plus récent et se connecte tout seul ; seules les clés déjà présentes changent.
fn choisir_dans_vdf(texte: &str, choisi: &str) -> String {
    let mut profondeur = 0;
    let mut compte = String::new();
    let mut dernier_nom = String::new();
    let mut sortie = Vec::new();
    for ligne in texte.split('\n') {
        let l = ligne.trim();
        let mut nouvelle = ligne.to_string();
        if l == "{" {
            profondeur += 1;
            if profondeur == 2 {
                compte = dernier_nom.clone();
            }
        } else if l == "}" {
            profondeur -= 1;
        } else {
            let morceaux: Vec<&str> = l.split('"').collect();
            if morceaux.len() == 3 {
                dernier_nom = morceaux[1].to_string();
            } else if morceaux.len() >= 5 && profondeur == 2 {
                let cible = compte == choisi;
                let valeur = match morceaux[1] {
                    "MostRecent" | "mostrecent" => Some(if cible { "1" } else { "0" }),
                    "AllowAutoLogin" if cible => Some("1"),
                    "WantsOfflineMode" if cible => Some("0"),
                    _ => None,
                };
                if let Some(v) = valeur {
                    let mut m: Vec<String> = ligne.split('"').map(str::to_string).collect();
                    m[3] = v.to_string();
                    nouvelle = m.join("\"");
                }
            }
        }
        sortie.push(nouvelle);
    }
    sortie.join("\n")
}

fn cle_steam(acces: u32) -> Option<RegKey> {
    RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(r"Software\Valve\Steam", acces).ok()
}

/// Compte connecté dans Steam en ce moment (SteamID64), None si Steam est fermé ou déconnecté
fn compte_actif() -> Option<String> {
    let actif: u32 = cle_steam(KEY_READ)?.open_subkey_with_flags("ActiveProcess", KEY_READ).ok()?.get_value("ActiveUser").ok()?;
    (actif != 0).then(|| (BASE_STEAMID + actif as u64).to_string())
}

fn avatar(id: &str) -> Option<String> {
    let octets = fs::read(steam_racine()?.join(r"config\avatarcache").join(format!("{id}.png"))).ok()?;
    octets.starts_with(&[0x89, b'P', b'N', b'G']).then(|| format!("data:image/png;base64,{}", crate::ps::base64(&octets)))
}

pub fn comptes() -> Vec<CompteSteam> {
    let Some(texte) = fichier_comptes().and_then(|f| fs::read_to_string(f).ok()) else { return vec![] };
    let actif = compte_actif();
    let mut liste: Vec<CompteSteam> = lire_comptes(&texte)
        .into_iter()
        .filter(|(id, _)| id.len() == 17 && id.chars().all(|c| c.is_ascii_digit()))
        .map(|(id, cles)| {
            let val = |k: &str| cles.iter().find(|(c, _)| c.eq_ignore_ascii_case(k)).map(|(_, v)| v.clone()).unwrap_or_default();
            CompteSteam {
                avatar: avatar(&id),
                actif: actif.as_deref() == Some(id.as_str()),
                identifiant: val("AccountName"),
                pseudo: Some(val("PersonaName")).filter(|p| !p.is_empty()).unwrap_or_else(|| val("AccountName")),
                memorise: val("RememberPassword") == "1",
                id,
            }
        })
        .collect();
    liste.sort_by(|a, b| a.pseudo.to_lowercase().cmp(&b.pseudo.to_lowercase()));
    liste
}

fn steam_ouvert() -> bool {
    crate::systeme::processus().iter().any(|p| p.nom.eq_ignore_ascii_case("steam.exe"))
}

/// Prépare Steam pour le compte choisi. Renvoie vrai si Steam a dû être fermé pour changer de compte.
pub fn basculer(id: &str) -> Result<bool, String> {
    let compte = comptes().into_iter().find(|c| c.id == id).ok_or("Compte Steam introuvable sur ce PC")?;
    if compte.actif {
        return Ok(false);
    }
    if !compte.memorise {
        return Err(format!("Steam n'a pas gardé la connexion de {} : connecte-toi une fois dans Steam en cochant « Se souvenir de moi ».", compte.pseudo));
    }
    let racine = steam_racine().ok_or("Steam n'est pas installé")?;
    let ferme = steam_ouvert();
    if ferme {
        let _ = crate::systeme::executer(&racine.join("steam.exe").to_string_lossy(), &["-shutdown"]);
        let limite = Instant::now() + Duration::from_secs(25);
        while steam_ouvert() {
            if Instant::now() > limite {
                return Err("Steam ne s'est pas fermé : ferme-le toi-même puis réessaie.".into());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        // Steam finit d'écrire ses fichiers juste après sa fermeture
        std::thread::sleep(Duration::from_millis(800));
    }
    let cle = cle_steam(KEY_SET_VALUE).ok_or("Registre de Steam inaccessible")?;
    cle.set_value("AutoLoginUser", &compte.identifiant).map_err(|e| e.to_string())?;
    cle.set_value("RememberPassword", &1u32).map_err(|e| e.to_string())?;
    let fichier = fichier_comptes().ok_or("Steam n'est pas installé")?;
    let texte = fs::read_to_string(&fichier).map_err(|e| e.to_string())?;
    fs::write(&fichier, choisir_dans_vdf(&texte, id)).map_err(|e| e.to_string())?;
    Ok(ferme)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VDF: &str = "\"users\"\r\n{\r\n\t\"76561198000000001\"\r\n\t{\r\n\t\t\"AccountName\"\t\t\"lucas_main\"\r\n\t\t\"PersonaName\"\t\t\"Cagoule\"\r\n\t\t\"RememberPassword\"\t\t\"1\"\r\n\t\t\"WantsOfflineMode\"\t\t\"0\"\r\n\t\t\"AllowAutoLogin\"\t\t\"1\"\r\n\t\t\"MostRecent\"\t\t\"1\"\r\n\t\t\"Timestamp\"\t\t\"1790000000\"\r\n\t}\r\n\t\"76561198000000002\"\r\n\t{\r\n\t\t\"AccountName\"\t\t\"lucas_smurf\"\r\n\t\t\"PersonaName\"\t\t\"Smurf\"\r\n\t\t\"RememberPassword\"\t\t\"1\"\r\n\t\t\"WantsOfflineMode\"\t\t\"1\"\r\n\t\t\"AllowAutoLogin\"\t\t\"0\"\r\n\t\t\"MostRecent\"\t\t\"0\"\r\n\t\t\"Timestamp\"\t\t\"1780000000\"\r\n\t}\r\n}\r\n";

    #[test]
    fn lecture_des_comptes() {
        let c = lire_comptes(VDF);
        assert_eq!(c.len(), 2);
        assert_eq!(c[1].0, "76561198000000002");
        assert!(c[1].1.contains(&("PersonaName".to_string(), "Smurf".to_string())));
    }

    #[test]
    fn changement_de_compte() {
        let t = choisir_dans_vdf(VDF, "76561198000000002");
        let c = lire_comptes(&t);
        let val = |i: usize, k: &str| c[i].1.iter().find(|(x, _)| x == k).unwrap().1.clone();
        assert_eq!((val(0, "MostRecent"), val(1, "MostRecent")), ("0".into(), "1".into()));
        assert_eq!((val(1, "AllowAutoLogin"), val(1, "WantsOfflineMode")), ("1".into(), "0".into()));
        // Le reste du fichier ne bouge pas (fins de ligne, autres clés)
        assert_eq!(val(0, "AllowAutoLogin"), "1");
        assert_eq!(t.matches("\r\n").count(), VDF.matches("\r\n").count());
        assert_eq!(t.len(), VDF.len());
    }
}
