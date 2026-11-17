// Partage des clips par un lien : le clip est copié dans un dossier publié sur le web (ex. le serveur de la maison,
// servi par Caddy), avec une petite page qui le lit et l'affiche en aperçu dans Discord.

use crate::clips;
use crate::config::Stockage;
use std::fs;
use std::path::{Path, PathBuf};

fn cle(clip: &Path) -> String {
    clips::chemin_simple(clip).to_lowercase()
}

/// Dossier et adresse du partage, s'ils sont réglés
fn reglage(stock: &Stockage) -> Option<(PathBuf, String)> {
    let r = &stock.donnees.lock().unwrap().reglages;
    let dossier = r.partage_dossier.clone().filter(|d| !d.trim().is_empty())?;
    let url = r.partage_url.clone().filter(|u| !u.trim().is_empty())?;
    Some((PathBuf::from(crate::partage::resoudre(dossier.trim())), url.trim().trim_end_matches('/').to_string()))
}

/// Lien du clip s'il est partagé
pub fn lien(stock: &Stockage, clip: &Path) -> Option<String> {
    let id = stock.donnees.lock().unwrap().partages.get(&cle(clip)).cloned()?;
    let (_, url) = reglage(stock)?;
    Some(format!("{url}/{id}"))
}

/// Identifiant difficile à deviner (10 caractères, chiffres et minuscules)
fn identifiant() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut h = RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    let mut n = h.finish() ^ (RandomState::new().build_hasher().finish() << 1);
    let alphabet = b"0123456789abcdefghijkmnpqrstuvwxyz";
    (0..10)
        .map(|_| {
            let c = alphabet[(n % alphabet.len() as u64) as usize] as char;
            n /= alphabet.len() as u64;
            c
        })
        .collect()
}

fn echapper(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Page du lien : lecteur, et balises lues par Discord pour montrer la vidéo dans la conversation
pub fn page(titre: &str, url: &str, id: &str) -> String {
    let t = echapper(titre);
    format!(
        r##"<!doctype html>
<html lang="fr">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{t} · FPS Cagoule</title>
<meta name="robots" content="noindex">
<meta name="theme-color" content="#2f6fed">
<meta property="og:site_name" content="FPS Cagoule">
<meta property="og:title" content="{t}">
<meta property="og:type" content="video.other">
<meta property="og:url" content="{url}/{id}">
<meta property="og:image" content="{url}/{id}.jpg">
<meta property="og:video" content="{url}/{id}.mp4">
<meta property="og:video:secure_url" content="{url}/{id}.mp4">
<meta property="og:video:type" content="video/mp4">
<meta property="og:video:width" content="1280">
<meta property="og:video:height" content="720">
<meta name="twitter:card" content="summary_large_image">
<style>
  :root {{ color-scheme: dark; }}
  body {{ margin: 0; min-height: 100vh; display: grid; place-items: center; background: radial-gradient(circle at 50% 0, #14213d, #0b0d12 60%); color: #e8ecf5; font: 15px/1.4 system-ui, sans-serif; }}
  main {{ width: min(1280px, 100% - 32px); }}
  video {{ width: 100%; border-radius: 14px; background: #000; box-shadow: 0 0 40px rgba(47, 111, 237, .35); display: block; }}
  h1 {{ font-size: 20px; margin: 16px 0 4px; }}
  p {{ margin: 0; color: #8b93a7; font-size: 13px; }}
</style>
</head>
<body>
<main>
  <video src="{id}.mp4" poster="{id}.jpg" controls autoplay muted playsinline></video>
  <h1>{t}</h1>
  <p>Partagé avec FPS Cagoule</p>
</main>
</body>
</html>
"##
    )
}

/// Adresse d'un webhook Discord valide (salon → Intégrations → Webhooks → Copier l'URL)
pub fn webhook_valide(url: &str) -> bool {
    ["https://discord.com/api/webhooks/", "https://discordapp.com/api/webhooks/", "https://ptb.discord.com/api/webhooks/", "https://canary.discord.com/api/webhooks/"]
        .iter()
        .any(|d| url.starts_with(d) && url.len() > d.len() + 10)
}

/// Poste le clip dans le salon Discord du webhook : Discord montre la vidéo grâce à la page du lien
fn poster_discord(webhook: &str, titre: &str, lien: &str) -> Result<(), String> {
    let corps = serde_json::json!({
        "username": "FPS Cagoule",
        "content": format!("🎬 **{titre}**\n{lien}"),
        "allowed_mentions": { "parse": [] },
    })
    .to_string();
    let script = format!(
        "[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12;\n$c = [Text.Encoding]::UTF8.GetBytes({});\nInvoke-RestMethod -UseBasicParsing -TimeoutSec 20 -Method Post -ContentType 'application/json; charset=utf-8' -Uri {} -Body $c | Out-Null",
        crate::ps::chaine(&corps),
        crate::ps::chaine(webhook)
    );
    crate::ps::executer(&script).map(|_| ())
}

#[derive(serde::Serialize)]
pub struct Partage {
    pub lien: String,
    /// Posté dans Discord : None = pas de webhook réglé, Some(Ok) = posté, Some(Err) = raté
    pub discord: Option<Result<(), String>>,
}

/// Partage le clip (ou redonne son lien s'il l'est déjà) ; un nouveau partage est posté dans Discord si un webhook est réglé
pub fn partager(stock: &Stockage, chemin: &str, titre: &str, progression: impl Fn(f64)) -> Result<Partage, String> {
    let source = clips::clip_valide(stock, chemin)?;
    // Un seul partage à la fois pour un même clip (double-clic…)
    let cle_source = cle(&source);
    {
        let mut en_cours = EN_COURS.lock().unwrap();
        if en_cours.contains(&cle_source) {
            return Err("Ce clip est déjà en train d'être partagé.".into());
        }
        en_cours.push(cle_source.clone());
    }
    let r = partager_un(stock, &source, titre, progression);
    EN_COURS.lock().unwrap().retain(|c| c != &cle_source);
    r
}

/// Clips en train d'être partagés
static EN_COURS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Copies provisoires abandonnées (app fermée pendant un partage) : effacées au bout d'une heure
fn nettoyer(dossier: &Path) {
    let Ok(l) = fs::read_dir(dossier) else { return };
    for e in l.flatten() {
        let nom = e.file_name().to_string_lossy().to_string();
        let vieux = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|d| d.as_secs() > 3600);
        if nom.starts_with('.') && nom.ends_with(".mp4") && vieux {
            let _ = fs::remove_file(e.path());
        }
    }
}

fn partager_un(stock: &Stockage, source: &Path, titre: &str, progression: impl Fn(f64)) -> Result<Partage, String> {
    let source = source.to_path_buf();
    let (dossier, url) = reglage(stock).ok_or("Règle d'abord le partage : dossier publié et adresse du site.")?;
    if let Some(id) = stock.donnees.lock().unwrap().partages.get(&cle(&source)).cloned() {
        if dossier.join(format!("{id}.mp4")).is_file() {
            return Ok(Partage { lien: format!("{url}/{id}"), discord: None });
        }
    }
    fs::create_dir_all(&dossier).map_err(|_| "Dossier de partage injoignable (serveur éteint ?).".to_string())?;
    nettoyer(&dossier);
    let id = identifiant();
    // Copie sous un nom provisoire : le lien ne marche qu'une fois la vidéo complète
    let provisoire = dossier.join(format!(".{id}.mp4"));
    if let Err(e) = clips::version_web(stock, &source, &provisoire, progression) {
        let _ = fs::remove_file(&provisoire);
        return Err(e);
    }
    clips::image_partage(stock, &source, &dossier.join(format!("{id}.jpg")));
    fs::write(dossier.join(format!("{id}.html")), page(titre, &url, &id)).map_err(|e| e.to_string())?;
    fs::rename(&provisoire, dossier.join(format!("{id}.mp4"))).map_err(|e| e.to_string())?;
    stock.donnees.lock().unwrap().partages.insert(cle(&source), id.clone());
    stock.sauver();
    let lien = format!("{url}/{id}");
    let webhook = stock.donnees.lock().unwrap().reglages.partage_discord.clone().filter(|w| webhook_valide(w));
    let discord = webhook.map(|w| poster_discord(&w, titre, &lien));
    Ok(Partage { lien, discord })
}

/// Reposte un clip déjà partagé dans Discord
pub fn reposter(stock: &Stockage, chemin: &str, titre: &str) -> Result<(), String> {
    let source = clips::clip_valide(stock, chemin)?;
    let lien = lien(stock, &source).ok_or("Ce clip n'est pas partagé.")?;
    let webhook = stock.donnees.lock().unwrap().reglages.partage_discord.clone().filter(|w| webhook_valide(w)).ok_or("Aucun salon Discord réglé.")?;
    poster_discord(&webhook, titre, &lien)
}

/// Retire le clip du web : le lien ne marche plus
pub fn retirer(stock: &Stockage, chemin: &str) -> Result<(), String> {
    let source = clips::clip_valide(stock, chemin)?;
    let id = stock.donnees.lock().unwrap().partages.remove(&cle(&source));
    stock.sauver();
    if let (Some(id), Some((dossier, _))) = (id, reglage(stock)) {
        for ext in ["mp4", "jpg", "html"] {
            let _ = fs::remove_file(dossier.join(format!("{id}.{ext}")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiants_et_page() {
        let (a, b) = (identifiant(), identifiant());
        assert_eq!(a.len(), 10);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
        let p = page("3K · Kaid · <Oregon>", "https://clips.exemple.ch", "abc");
        assert!(p.contains(r#"content="https://clips.exemple.ch/abc.mp4""#));
        assert!(p.contains("3K · Kaid · &lt;Oregon&gt;"));
        assert!(webhook_valide("https://discord.com/api/webhooks/1234567890/abcdefghijklmnop"));
        assert!(!webhook_valide("https://exemple.ch/api/webhooks/123"));
    }
}
