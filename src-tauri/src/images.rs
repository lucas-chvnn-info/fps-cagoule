// Jaquettes des jeux pour la page « Mes jeux ». Cherchées seulement quand la page s'ouvre, jamais en arrière-plan :
// d'abord sur le PC (cache de Steam, catalogue du launcher Epic, icône du jeu), puis sur Steam pour les autres.
// Gardées dans « images » (une seule recherche par jeu ; nouvel essai après 7 jours si rien n'a été trouvé).

use crate::bibliotheques::{self, JeuTrouve};
use crate::config::{Profil, Stockage};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Taille maximale d'une image gardée (une jaquette Steam fait 40 à 80 Ko)
const TAILLE_MAX: usize = 3 << 20;
const REESSAI: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Serialize)]
pub struct Carte {
    pub nom: String,
    /// Image en data URL
    pub image: Option<String>,
    /// « perso » (choisie par l'utilisateur), « jaquette » ou « icone »
    pub genre: Option<&'static str>,
    /// Trouvé dans une bibliothèque du PC
    pub installe: bool,
    /// Bouton « Jouer » possible
    pub jouable: bool,
    /// Jeu Steam (choix du compte)
    pub steam: bool,
}

/// Nom comparable : minuscules et chiffres seulement, sans « Tom Clancy's »
pub fn normaliser(nom: &str) -> String {
    let n: String = nom.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect();
    n.strip_prefix("tomclancys").map(String::from).unwrap_or(n)
}

fn cle(nom: &str) -> String {
    let n = normaliser(nom);
    if n.is_empty() {
        // Nom sans lettres latines : on garde un nom de fichier stable quand même
        format!("jeu{}", nom.chars().map(|c| c as u32).fold(0u32, |a, c| a.wrapping_mul(31).wrapping_add(c)))
    } else {
        n
    }
}

/// À quel point un jeu installé correspond à un profil (0 = pas du tout)
fn ressemblance(profil: &Profil, j: &JeuTrouve) -> u8 {
    let (a, b) = (normaliser(&profil.nom), normaliser(&j.nom));
    if a.is_empty() || b.is_empty() {
        return 0;
    }
    if a == b {
        return 3;
    }
    let exe = profil.exes.iter().any(|e| j.exes.iter().any(|x| x.eq_ignore_ascii_case(e)) || Path::new(&j.dossier).join(e).is_file());
    if exe {
        return 2;
    }
    // « Overwatch » pour « Overwatch 2 », « Siege X » pour « Siege », mais pas « Minecraft Dungeons » pour « Minecraft »
    let (court, long) = if a.len() <= b.len() { (&a, &b) } else { (&b, &a) };
    u8::from(proche(court, long))
}

/// Même nom à un suffixe court près (« 2 », « X », « HD »)
fn proche(court: &str, long: &str) -> bool {
    court.len() >= 6 && long.starts_with(court) && long.len() <= court.len() + 3
}

pub(crate) fn meilleur<'a>(profil: &Profil, jeux: &'a [JeuTrouve]) -> Option<&'a JeuTrouve> {
    jeux.iter().map(|j| (ressemblance(profil, j), j)).filter(|(r, _)| *r > 0).max_by_key(|(r, _)| *r).map(|(_, j)| j)
}

fn type_image(octets: &[u8]) -> Option<&'static str> {
    match octets {
        [0xFF, 0xD8, 0xFF, ..] => Some("image/jpeg"),
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("image/webp"),
        [0, 0, 1, 0, ..] => Some("image/x-icon"),
        _ => None,
    }
}

fn decoder_base64(texte: &str) -> Option<Vec<u8>> {
    let mut sortie = Vec::with_capacity(texte.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in texte.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\r' | b'\n' | b' ' => continue,
            _ => return None,
        };
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            sortie.push((acc >> bits) as u8);
        }
    }
    Some(sortie)
}

fn dossier(stock: &Stockage) -> PathBuf {
    stock.dossier().join("images")
}

fn fichier(stock: &Stockage, nom: &str, genre: &str) -> PathBuf {
    dossier(stock).join(format!("{}.{genre}", cle(nom)))
}

fn data_url(chemin: &Path) -> Option<String> {
    let octets = fs::read(chemin).ok()?;
    let mime = type_image(&octets)?;
    Some(format!("data:{mime};base64,{}", crate::ps::base64(&octets)))
}

/// Garde une image venue du disque si c'en est bien une, pas trop lourde
fn garder(source: &Path, cible: &Path) -> bool {
    match fs::read(source) {
        Ok(o) if o.len() <= TAILLE_MAX && type_image(&o).is_some() => fs::write(cible, o).is_ok(),
        _ => false,
    }
}

/// Jaquette verticale déjà téléchargée par Steam (ancienne et nouvelle organisation du cache)
fn cache_steam(appid: u32) -> Option<PathBuf> {
    let cache = bibliotheques::steam_racine()?.join(r"appcache\librarycache");
    let ancien = cache.join(format!("{appid}_library_600x900.jpg"));
    if ancien.is_file() {
        return Some(ancien);
    }
    let mut pile = vec![(cache.join(appid.to_string()), 0)];
    while let Some((d, profondeur)) = pile.pop() {
        for e in fs::read_dir(&d).ok()?.flatten() {
            let p = e.path();
            if p.is_dir() && profondeur < 2 {
                pile.push((p, profondeur + 1));
            } else if e.file_name().to_string_lossy().eq_ignore_ascii_case("library_600x900.jpg") {
                return Some(p);
            }
        }
    }
    None
}

fn url_sure(url: &str) -> bool {
    let Some(reste) = url.strip_prefix("https://") else { return false };
    let hote = reste.split(['/', '?']).next().unwrap_or("").to_lowercase();
    ["epicgames.com", "unrealengine.com"].iter().any(|d| hote == *d || hote.ends_with(&format!(".{d}")))
}

/// Jaquettes du catalogue du launcher Epic (fichier local, JSON en base64) : id du catalogue → URL
fn catalogue_epic_lire(contenu: &str) -> HashMap<String, String> {
    let Some(octets) = decoder_base64(contenu.trim()) else { return HashMap::new() };
    let Ok(serde_json::Value::Array(elements)) = serde_json::from_slice(&octets) else { return HashMap::new() };
    let mut liste = HashMap::new();
    for e in elements {
        let (Some(id), Some(images)) = (e["id"].as_str(), e["keyImages"].as_array()) else { continue };
        let url = ["DieselGameBoxTall", "OfferImageTall", "DieselGameBox", "OfferImageWide", "Thumbnail"].iter().find_map(|genre| {
            images.iter().find(|i| i["type"].as_str() == Some(genre)).and_then(|i| i["url"].as_str()).filter(|u| url_sure(u))
        });
        if let Some(u) = url {
            // Le CDN d'Epic redimensionne : 400 px de large suffisent
            let u = if u.contains('?') { u.to_string() } else { format!("{u}?w=400&h=600&resize=1") };
            liste.insert(id.to_string(), u);
        }
    }
    liste
}

fn catalogue_epic() -> HashMap<String, String> {
    let racine = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
    fs::read_to_string(Path::new(&racine).join(r"Epic\EpicGamesLauncher\Data\Catalog\catcache.bin")).map(|t| catalogue_epic_lire(&t)).unwrap_or_default()
}

/// Parmi les résultats de la recherche Steam (id, nom), celui qui est vraiment ce jeu
fn choisir_resultat(nom: &str, resultats: &[(u32, String)]) -> Option<u32> {
    let a = normaliser(nom);
    if a.is_empty() {
        return None;
    }
    resultats.iter().find(|(_, n)| normaliser(n) == a).or_else(|| {
        resultats.iter().find(|(_, n)| proche(&a, &normaliser(n)))
    }).map(|(id, _)| *id)
}

fn url_steam(appid: u32) -> String {
    format!("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{appid}/library_600x900.jpg")
}

/// Recherche Steam pour chaque nom : liste (id, nom) des premières applis trouvées
fn rechercher_steam(noms: &[String]) -> Vec<Vec<(u32, String)>> {
    if noms.is_empty() {
        return vec![];
    }
    let liste = serde_json::to_string(noms).unwrap_or_default();
    let script = format!(
        r#"[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$noms = ConvertFrom-Json {}
$sortie = New-Object System.Collections.ArrayList
foreach ($n in $noms) {{
  $items = @()
  try {{
    $r = Invoke-RestMethod -UseBasicParsing -TimeoutSec 10 ('https://store.steampowered.com/api/storesearch/?l=english&cc=US&term=' + [uri]::EscapeDataString($n))
    $items = @($r.items | Where-Object {{ $_.type -eq 'app' }} | Select-Object -First 8 | ForEach-Object {{ [pscustomobject]@{{ id = [int]$_.id; nom = [string]$_.name }} }})
  }} catch {{ }}
  [void]$sortie.Add($items)
}}
ConvertTo-Json -Compress -Depth 4 -InputObject $sortie"#,
        crate::ps::chaine(&liste)
    );
    let Ok(v) = crate::ps::json(&script) else { return vec![] };
    v.as_array()
        .map(|l| {
            l.iter()
                .map(|r| {
                    r.as_array()
                        .map(|items| items.iter().filter_map(|i| Some((i["id"].as_u64()? as u32, i["nom"].as_str()?.to_string()))).collect())
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Télécharge chaque image (première URL qui marche) dans son fichier
fn telecharger(travaux: &[(PathBuf, Vec<String>)]) {
    if travaux.is_empty() {
        return;
    }
    let liste: Vec<serde_json::Value> = travaux.iter().map(|(f, urls)| serde_json::json!({ "f": f.to_string_lossy(), "u": urls })).collect();
    let script = format!(
        r#"[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$travaux = ConvertFrom-Json {}
foreach ($t in $travaux) {{
  foreach ($u in @($t.u)) {{
    try {{ Invoke-WebRequest -UseBasicParsing -TimeoutSec 15 -Uri $u -OutFile ($t.f + '.tmp'); Move-Item -Force ($t.f + '.tmp') $t.f; break }}
    catch {{ Remove-Item -ErrorAction SilentlyContinue ($t.f + '.tmp') }}
  }}
}}"#,
        crate::ps::chaine(&serde_json::Value::Array(liste).to_string())
    );
    let _ = crate::ps::executer(&script);
}

fn recent(chemin: &Path) -> bool {
    fs::metadata(chemin).and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok()).is_some_and(|d| d < REESSAI)
}

/// Cherche les jaquettes qui manquent. `forcer` : nom d'un jeu à rechercher même s'il a déjà été cherché récemment.
pub fn chercher(stock: &Stockage, profils: &[Profil], forcer: Option<&str>) {
    let _ = fs::create_dir_all(dossier(stock));
    let jeux = bibliotheques::sources(false);
    let mut epic: Option<HashMap<String, String>> = None;
    let mut travaux: Vec<(PathBuf, Vec<String>)> = Vec::new();
    let mut a_chercher: Vec<&Profil> = Vec::new();
    for p in profils {
        let jaquette = fichier(stock, &p.nom, "jaquette");
        let aucune = fichier(stock, &p.nom, "aucune");
        if jaquette.is_file() || fichier(stock, &p.nom, "perso").is_file() {
            continue;
        }
        if recent(&aucune) && forcer != Some(p.nom.as_str()) {
            continue;
        }
        let j = meilleur(p, &jeux);
        if let Some(ico) = j.and_then(|j| j.icone.as_deref()) {
            garder(Path::new(ico), &fichier(stock, &p.nom, "icone"));
        }
        let mut urls = Vec::new();
        if let Some(appid) = j.and_then(|j| j.steam) {
            if cache_steam(appid).is_some_and(|c| garder(&c, &jaquette)) {
                continue;
            }
            urls.push(url_steam(appid));
        }
        if let Some(id) = j.and_then(|j| j.epic.as_deref()) {
            if let Some(u) = epic.get_or_insert_with(catalogue_epic).get(id) {
                urls.push(u.clone());
            }
        }
        if urls.is_empty() {
            a_chercher.push(p);
        } else {
            travaux.push((jaquette, urls));
        }
    }
    let noms: Vec<String> = a_chercher.iter().map(|p| p.nom.clone()).collect();
    let trouves = rechercher_steam(&noms);
    // Pas de réponse du tout : sans doute hors ligne, on réessaiera à la prochaine ouverture
    let hors_ligne = !noms.is_empty() && trouves.is_empty();
    for (p, resultats) in a_chercher.iter().zip(trouves) {
        if let Some(appid) = choisir_resultat(&p.nom, &resultats) {
            travaux.push((fichier(stock, &p.nom, "jaquette"), vec![url_steam(appid)]));
        }
    }
    telecharger(&travaux);
    for p in profils {
        let jaquette = fichier(stock, &p.nom, "jaquette");
        // Réponse qui n'est pas une image (page d'erreur…) : jetée
        if jaquette.is_file() && fs::read(&jaquette).map(|o| type_image(&o).is_none() || o.len() > TAILLE_MAX).unwrap_or(true) {
            let _ = fs::remove_file(&jaquette);
        }
        let aucune = fichier(stock, &p.nom, "aucune");
        if jaquette.is_file() {
            let _ = fs::remove_file(&aucune);
        } else if !hors_ligne && !fichier(stock, &p.nom, "perso").is_file() && (!recent(&aucune) || forcer == Some(p.nom.as_str())) {
            let _ = fs::write(&aucune, b"");
        }
    }
}

/// Image et état de chaque jeu de la liste (lecture du disque seulement)
pub fn cartes(stock: &Stockage, profils: &[Profil]) -> Vec<Carte> {
    let jeux = bibliotheques::sources(false);
    profils
        .iter()
        .map(|p| {
            let (image, genre) = [("perso", "perso"), ("jaquette", "jaquette"), ("icone", "icone")]
                .iter()
                .find_map(|(f, g)| data_url(&fichier(stock, &p.nom, f)).map(|d| (Some(d), Some(*g))))
                .unwrap_or((None, None));
            let lancement = crate::lancement::jouable(p, &jeux);
            Carte {
                nom: p.nom.clone(),
                image,
                genre,
                installe: lancement.is_some(),
                jouable: lancement.is_some_and(|(_, j)| j),
                steam: lancement.is_some_and(|(s, _)| s.is_some()),
            }
        })
        .collect()
}

/// Image choisie par l'utilisateur (data URL envoyée par l'interface, déjà réduite)
pub fn choisir(stock: &Stockage, nom: &str, donnees: &str) -> Result<(), String> {
    let base64 = donnees.split_once(";base64,").map(|(_, b)| b).ok_or("Image illisible")?;
    let octets = decoder_base64(base64).ok_or("Image illisible")?;
    if octets.len() > TAILLE_MAX || type_image(&octets).is_none() {
        return Err("Choisis une image JPG, PNG ou WebP de moins de 3 Mo.".into());
    }
    fs::create_dir_all(dossier(stock)).map_err(|e| e.to_string())?;
    fs::write(fichier(stock, nom, "perso"), octets).map_err(|e| e.to_string())
}

pub fn effacer_perso(stock: &Stockage, nom: &str) {
    let _ = fs::remove_file(fichier(stock, nom, "perso"));
    // La jaquette d'origine est recherchée de nouveau tout de suite
    let _ = fs::remove_file(fichier(stock, nom, "aucune"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profil(nom: &str, exes: &[&str]) -> Profil {
        Profil { nom: nom.into(), exes: exes.iter().map(|e| e.to_string()).collect(), ..Default::default() }
    }

    fn trouve(nom: &str, exes: &[&str]) -> JeuTrouve {
        JeuTrouve::nouveau(nom.into(), exes.iter().map(|e| e.to_string()).collect(), "Steam", String::new())
    }

    #[test]
    fn noms() {
        assert_eq!(normaliser("Tom Clancy's Rainbow Six® Siege"), "rainbowsixsiege");
        assert_eq!(normaliser("VALORANT"), "valorant");
        assert_eq!(cle("Counter-Strike 2"), "counterstrike2");
        assert!(cle("原神").starts_with("jeu"));
    }

    #[test]
    fn correspondances() {
        assert_eq!(meilleur(&profil("Overwatch 2", &[]), &[trouve("Overwatch", &[])]).map(|j| j.nom.as_str()), Some("Overwatch"));
        let jeux = [trouve("Minecraft Dungeons", &[]), trouve("Tom Clancy's Rainbow Six Siege", &[]), trouve("Grand Theft Auto V", &["GTA5.exe"])];
        assert_eq!(meilleur(&profil("Rainbow Six Siege", &[]), &jeux).map(|j| j.nom.as_str()), Some("Tom Clancy's Rainbow Six Siege"));
        assert_eq!(meilleur(&profil("GTA V", &["GTA5.exe"]), &jeux).map(|j| j.nom.as_str()), Some("Grand Theft Auto V"));
        assert!(meilleur(&profil("Minecraft", &["Minecraft.Windows.exe"]), &jeux).is_none());
    }

    #[test]
    fn recherche_steam() {
        let r = vec![(377560, "Tom Clancy's Rainbow Six® Siege - Ultra HD Texture Pack".to_string()), (359550, "Tom Clancy's Rainbow Six® Siege".to_string())];
        assert_eq!(choisir_resultat("Rainbow Six Siege", &r), Some(359550));
        assert_eq!(choisir_resultat("Apex Legends", &[(1172470, "Apex Legends™".into())]), Some(1172470));
        assert_eq!(choisir_resultat("Fortnite", &[(1, "Fortnite Skins Wallpaper Pack Deluxe".into())]), None);
    }

    /// Vraie recherche et vrai téléchargement (PC Windows de GitHub)
    #[test]
    fn steam_en_ligne() {
        let r = rechercher_steam(&["Rainbow Six Siege".into(), "Counter-Strike 2".into()]);
        assert_eq!(r.len(), 2, "réponse : {r:?}");
        assert_eq!(choisir_resultat("Rainbow Six Siege", &r[0]), Some(359550), "{:?}", r[0]);
        assert_eq!(choisir_resultat("Counter-Strike 2", &r[1]), Some(730), "{:?}", r[1]);
        let f = std::env::temp_dir().join("fps-cagoule-jaquette.jpg");
        let _ = fs::remove_file(&f);
        telecharger(&[(f.clone(), vec!["https://exemple.invalid/x.jpg".into(), url_steam(730)])]);
        let octets = fs::read(&f).expect("jaquette téléchargée");
        assert_eq!(type_image(&octets), Some("image/jpeg"));
        let _ = fs::remove_file(&f);
    }

    #[test]
    fn epic_et_images() {
        let json = r#"[{"id":"abc","keyImages":[{"type":"Thumbnail","url":"https://cdn1.epicgames.com/t.jpg"},{"type":"DieselGameBoxTall","url":"https://cdn1.epicgames.com/tall.jpg"}]},
            {"id":"pirate","keyImages":[{"type":"DieselGameBoxTall","url":"https://exemple.com/x.jpg"}]}]"#;
        let liste = catalogue_epic_lire(&crate::ps::base64(json.as_bytes()));
        assert_eq!(liste.get("abc").map(String::as_str), Some("https://cdn1.epicgames.com/tall.jpg?w=400&h=600&resize=1"));
        assert!(!liste.contains_key("pirate"));
        assert!(!url_sure("https://epicgames.com.pirate.net/x.jpg"));
        assert_eq!(decoder_base64(&crate::ps::base64(b"FPS Cagoule!")).unwrap(), b"FPS Cagoule!");
        assert_eq!(type_image(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(type_image(b"<html>"), None);
    }
}
