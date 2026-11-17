// Clips (comme Medal) : pendant les parties, FFmpeg filme l'écran (Desktop Duplication, encodage par la carte
// graphique) en morceaux de 2 s sur le disque, avec le son du PC. La touche choisie recolle les dernières
// 30 s, 1 min ou 3 min en un .mp4 dans Vidéos\FPS Cagoule. Rien ne tourne en dehors des parties.
// FFmpeg (9.0.2, build « essentials » de Gyan Doshi) n'est téléchargé que si l'utilisateur active les clips.

use crate::config::Stockage;
use crate::gpu;
use crate::systeme::CREATE_NO_WINDOW;
use serde::Serialize;
use std::fs;
use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const FFMPEG_URL: &str = "https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip";
const FFMPEG_SHA256: &str = "60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba";
const FFMPEG_TAILLE: u64 = 114_768_076;
const FFMPEG_DANS_ZIP: &str = "ffmpeg-9.0.2-essentials_build/bin/ffmpeg.exe";
/// Durée d'un morceau ; le tampon garde 4 min + une marge (une manche entière pour les clips automatiques)
const MORCEAU_S: u32 = 2;
const MORCEAUX: u32 = 240 / MORCEAU_S + 4;
/// Taille d'un paquet MPEG-TS : un morceau en cours d'écriture est coupé au dernier paquet complet
const PAQUET_TS: usize = 188;

fn outils(stock: &Stockage) -> PathBuf {
    stock.dossier().join("outils")
}

pub fn ffmpeg(stock: &Stockage) -> PathBuf {
    outils(stock).join("ffmpeg.exe")
}

pub fn installe(stock: &Stockage) -> bool {
    ffmpeg(stock).is_file()
}

/// Dossier des clips : Vidéos\FPS Cagoule
pub fn dossier_clips() -> PathBuf {
    use windows_sys::Win32::UI::Shell::FOLDERID_Videos;
    crate::jeux_config::dossier_connu(&FOLDERID_Videos)
        .unwrap_or_else(|| PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Videos"))
        .join("FPS Cagoule")
}

fn tampon(stock: &Stockage) -> PathBuf {
    stock.dossier().join("tampon-clips")
}

fn miniatures(stock: &Stockage) -> PathBuf {
    stock.dossier().join("miniatures")
}

/// Télécharge FFmpeg, vérifie son empreinte et ne garde que ffmpeg.exe. `progression(reçu, total)`.
pub fn installer(stock: &Stockage, progression: impl Fn(u64, u64)) -> Result<(), String> {
    if installe(stock) {
        return Ok(());
    }
    let dossier = outils(stock);
    fs::create_dir_all(&dossier).map_err(|e| e.to_string())?;
    let zip = dossier.join("ffmpeg.zip");
    let script = format!(
        r#"[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
Invoke-WebRequest -UseBasicParsing -TimeoutSec 600 -Uri {url} -OutFile {zip}
$flux = [IO.File]::OpenRead({zip})
try {{ $empreinte = -join ([Security.Cryptography.SHA256]::Create().ComputeHash($flux) | ForEach-Object {{ $_.ToString('X2') }}) }} finally {{ $flux.Dispose() }}
if ($empreinte -ne {hash}) {{ Remove-Item -LiteralPath {zip}; throw 'Fichier FFmpeg corrompu' }}
Add-Type -AssemblyName System.IO.Compression.FileSystem
$z = [IO.Compression.ZipFile]::OpenRead({zip})
try {{ [IO.Compression.ZipFileExtensions]::ExtractToFile($z.GetEntry({entree}), {exe}, $true) }} finally {{ $z.Dispose() }}
Remove-Item -LiteralPath {zip}"#,
        url = crate::ps::chaine(FFMPEG_URL),
        zip = crate::ps::chaine(&zip.to_string_lossy()),
        hash = crate::ps::chaine(&FFMPEG_SHA256.to_uppercase()),
        entree = crate::ps::chaine(FFMPEG_DANS_ZIP),
        exe = crate::ps::chaine(&ffmpeg(stock).to_string_lossy()),
    );
    // PowerShell télécharge ; on suit la taille du fichier pour la barre de progression
    let fini = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let fini2 = fini.clone();
    let travail = std::thread::spawn(move || {
        let r = crate::ps::executer(&script);
        fini2.store(true, std::sync::atomic::Ordering::Relaxed);
        r
    });
    while !fini.load(std::sync::atomic::Ordering::Relaxed) {
        progression(fs::metadata(&zip).map(|m| m.len()).unwrap_or(0), FFMPEG_TAILLE);
        std::thread::sleep(Duration::from_millis(400));
    }
    let r = travail.join().map_err(|_| "Installation interrompue".to_string())?;
    let _ = fs::remove_file(&zip);
    r.map(|_| ()).map_err(|e| format!("Téléchargement de FFmpeg impossible : {e}"))?;
    progression(FFMPEG_TAILLE, FFMPEG_TAILLE);
    if installe(stock) {
        Ok(())
    } else {
        Err("FFmpeg introuvable après l'installation".into())
    }
}

/// Encodeur vidéo : celui de la carte graphique, sinon le processeur (libx264, plus lourd)
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Encodeur {
    Nvidia,
    Amd,
    Intel,
    Logiciel,
}

fn encodeur_conseille() -> Encodeur {
    match gpu::principale().map(|c| c.fabricant) {
        Some("nvidia") => Encodeur::Nvidia,
        Some("amd") => Encodeur::Amd,
        Some("intel") => Encodeur::Intel,
        _ => Encodeur::Logiciel,
    }
}

/// Arguments FFmpeg de l'enregistrement. `base_us` : instant de référence (µs depuis 1970) commun au son et à l'image.
pub fn arguments(source_video: &str, audio: Option<crate::audio::Format>, encodeur: Encodeur, base_us: i64, sortie: &Path) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error", "-y"].iter().map(|s| s.to_string()).collect();
    if let Some(f) = audio {
        a.extend(
            [
                "-thread_queue_size", "4096", "-use_wallclock_as_timestamps", "1", "-f", f.ffmpeg(),
                "-ar", &f.frequence.to_string(), "-ac", &f.canaux.to_string(), "-i", "pipe:0",
            ]
            .iter()
            .map(|s| s.to_string()),
        );
    }
    a.extend(["-thread_queue_size", "64", "-f", "lavfi", "-i", source_video].iter().map(|s| s.to_string()));
    let v = if audio.is_some() { 1 } else { 0 };
    // Images de la capture d'écran (D3D11) : directes pour NVIDIA et AMD, converties pour Intel et le processeur
    let capture_ecran = source_video.starts_with("ddagrab");
    let conversion = match encodeur {
        _ if !capture_ecran => "format=yuv420p,",
        Encodeur::Nvidia | Encodeur::Amd => "",
        Encodeur::Intel => "hwmap=derive_device=qsv,format=qsv,",
        Encodeur::Logiciel => "hwdownload,format=bgra,format=yuv420p,",
    };
    // Son et image horodatés avec la même horloge (temps réel), à partir de base_us
    let mut graphe = format!("[{v}:v]{conversion}setpts=(RTCTIME-{base_us})/(1000000*TB)[v]");
    if audio.is_some() {
        graphe.push_str(&format!(";[0:a]asetpts=PTS-({base_us}/1000000)/TB,aresample=async=1000[a]"));
    }
    a.extend(["-copyts".to_string(), "-filter_complex".to_string(), graphe, "-map".into(), "[v]".into()]);
    if audio.is_some() {
        a.extend(["-map", "[a]", "-c:a", "aac", "-b:a", "160k"].iter().map(|s| s.to_string()));
    }
    let gop = (60 * MORCEAU_S).to_string();
    let video: Vec<&str> = match encodeur {
        Encodeur::Nvidia => vec!["-c:v", "h264_nvenc", "-preset", "p4", "-tune", "ll", "-rc", "cbr", "-b:v", "20M", "-bf", "0"],
        Encodeur::Amd => vec!["-c:v", "h264_amf", "-usage", "lowlatency", "-quality", "speed", "-rc", "cbr", "-b:v", "20M"],
        Encodeur::Intel => vec!["-c:v", "h264_qsv", "-preset", "veryfast", "-b:v", "20M"],
        Encodeur::Logiciel => vec!["-c:v", "libx264", "-preset", "ultrafast", "-tune", "zerolatency", "-crf", "23"],
    };
    a.extend(video.iter().map(|s| s.to_string()));
    a.extend(
        [
            "-g", &gop, "-f", "segment", "-segment_time", &MORCEAU_S.to_string(), "-segment_format", "mpegts",
            "-segment_wrap", &MORCEAUX.to_string(), "-reset_timestamps", "0",
        ]
        .iter()
        .map(|s| s.to_string()),
    );
    a.push(sortie.join("m%03d.ts").to_string_lossy().into());
    a
}

/// Capture d'écran de FFmpeg : écran principal à 60 images/s, sans le curseur
pub const ECRAN: &str = "ddagrab=output_idx=0:framerate=60:draw_mouse=0";

pub struct Enregistreur {
    enfant: Child,
    _son: Option<crate::audio::Capture>,
    pub dossier: PathBuf,
    pub jeu: String,
    pub encodeur: Encodeur,
    /// Horloge commune gardée d'un redémarrage à l'autre : les morceaux restent bout à bout
    base_us: i64,
    /// Redémarrages pendant la partie (changement de résolution, plein écran…)
    pub relances: u32,
}

impl Enregistreur {
    /// FFmpeg tourne encore
    pub fn vivant(&mut self) -> bool {
        matches!(self.enfant.try_wait(), Ok(None))
    }

    /// Relance FFmpeg après un arrêt (la capture d'écran s'arrête quand la résolution change)
    pub fn relancer(&mut self, stock: &Stockage) -> Result<(), String> {
        let _ = self.enfant.kill();
        let _ = self.enfant.wait();
        self.relances += 1;
        let (enfant, son) = lancer_ffmpeg(stock, self.encodeur, &self.dossier, self.base_us)?;
        self.enfant = enfant;
        self._son = son;
        Ok(())
    }
}

impl Drop for Enregistreur {
    fn drop(&mut self) {
        let _ = self.enfant.kill();
        let _ = self.enfant.wait();
        let _ = fs::remove_dir_all(&self.dossier);
    }
}

fn lancer_ffmpeg(stock: &Stockage, encodeur: Encodeur, dossier: &Path, base_us: i64) -> Result<(Child, Option<crate::audio::Capture>), String> {
    let son = crate::audio::ouvrir().ok();
    let args = arguments(ECRAN, son.as_ref().map(|(f, _)| *f), encodeur, base_us, dossier);
    let mut enfant = Command::new(ffmpeg(stock))
        .args(&args)
        .stdin(if son.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW | 0x0000_4000) // BELOW_NORMAL : le jeu passe d'abord
        .spawn()
        .map_err(|e| format!("FFmpeg ne démarre pas : {e}"))?;
    let capture = match (son, enfant.stdin.take()) {
        (Some((_, demarrer)), Some(entree)) => Some(demarrer(Box::new(entree))),
        _ => None,
    };
    // FFmpeg qui s'arrête tout de suite : encodeur ou capture refusés
    let limite = Instant::now() + Duration::from_secs(4);
    while Instant::now() < limite {
        if let Ok(Some(_)) = enfant.try_wait() {
            let mut erreur = String::new();
            if let Some(mut e) = enfant.stderr.take() {
                let _ = e.read_to_string(&mut erreur);
            }
            return Err(erreur.lines().last().unwrap_or("FFmpeg s'est arrêté").to_string());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    // Les messages d'erreur ne sont plus lus : on évite que FFmpeg se bloque sur un tuyau plein
    if let Some(mut e) = enfant.stderr.take() {
        std::thread::spawn(move || std::io::copy(&mut e, &mut std::io::sink()));
    }
    Ok((enfant, capture))
}

/// Démarre l'enregistrement d'une partie (encodeur de la carte graphique, sinon le processeur)
pub fn demarrer(stock: &Stockage, jeu: &str) -> Result<Enregistreur, String> {
    let dossier = tampon(stock);
    let _ = fs::remove_dir_all(&dossier);
    fs::create_dir_all(&dossier).map_err(|e| e.to_string())?;
    let mut erreurs = Vec::new();
    let base_us = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_micros() as i64);
    for encodeur in [encodeur_conseille(), Encodeur::Logiciel] {
        match lancer_ffmpeg(stock, encodeur, &dossier, base_us) {
            Ok((enfant, son)) => return Ok(Enregistreur { enfant, _son: son, dossier, jeu: jeu.into(), encodeur, base_us, relances: 0 }),
            Err(e) => erreurs.push(e),
        }
        if encodeur == Encodeur::Logiciel {
            break;
        }
    }
    Err(format!("Enregistrement impossible : {}", erreurs.join(" ; ")))
}

/// Nom de fichier sans caractères interdits par Windows
pub fn nom_fichier(texte: &str) -> String {
    let n: String = texte.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { ' ' } else { c }).collect();
    n.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn date_texte() -> String {
    // Heure locale via PowerShell serait lourde : on demande l'heure à Windows directement
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    let mut t = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut t) };
    format!("{:04}-{:02}-{:02} {:02}h{:02}m{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

/// Morceaux du tampon couvrant les `secondes` dernières secondes, du plus ancien au plus récent
fn morceaux_recents(dossier: &Path, secondes: u32) -> Vec<PathBuf> {
    let mut liste: Vec<(SystemTime, PathBuf)> = fs::read_dir(dossier)
        .map(|l| {
            l.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "ts"))
                .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
                .collect()
        })
        .unwrap_or_default();
    liste.sort_by_key(|(t, _)| *t);
    let n = (secondes.div_ceil(MORCEAU_S) + 1) as usize;
    liste.into_iter().rev().take(n).rev().map(|(_, p)| p).collect()
}

/// Sauvegarde les `secondes` dernières secondes de la partie. Renvoie le chemin du clip.
pub fn sauver(stock: &Stockage, dossier_tampon: &Path, jeu: &str, secondes: u32, titre: Option<&str>) -> Result<PathBuf, String> {
    let morceaux = morceaux_recents(dossier_tampon, secondes);
    if morceaux.is_empty() {
        return Err("Rien d'enregistré pour l'instant".into());
    }
    // Les morceaux MPEG-TS se recollent bout à bout (horodatage continu)
    let brut = stock.dossier().join("clip-en-cours.ts");
    {
        let mut f = fs::File::create(&brut).map_err(|e| e.to_string())?;
        for m in &morceaux {
            if let Ok(o) = fs::read(m) {
                let complet = o.len() - o.len() % PAQUET_TS;
                f.write_all(&o[..complet]).map_err(|e| e.to_string())?;
            }
        }
    }
    let dossier_jeu = dossier_clips().join(nom_fichier(jeu));
    fs::create_dir_all(&dossier_jeu).map_err(|e| e.to_string())?;
    let nom = nom_fichier(&titre.map(String::from).unwrap_or_else(|| format!("{jeu} {}", date_texte())));
    let mut sortie = dossier_jeu.join(format!("{nom}.mp4"));
    let mut n = 2;
    while sortie.exists() {
        sortie = dossier_jeu.join(format!("{nom} ({n}).mp4"));
        n += 1;
    }
    let statut = Command::new(ffmpeg(stock))
        .args(["-hide_banner", "-loglevel", "error", "-y", "-sseof", &format!("-{secondes}"), "-i"])
        .arg(&brut)
        .args(["-c", "copy", "-bsf:a", "aac_adtstoasc", "-movflags", "+faststart"])
        .arg(&sortie)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| e.to_string())?;
    let _ = fs::remove_file(&brut);
    if !statut.success() || !sortie.is_file() {
        return Err("Le clip n'a pas pu être créé".into());
    }
    miniature(stock, &sortie);
    Ok(sortie)
}

fn cle_miniature(clip: &Path) -> String {
    // Nom stable par chemin de clip
    let mut h: u64 = 1469598103934665603;
    for b in clip.to_string_lossy().to_lowercase().bytes() {
        h = (h ^ b as u64).wrapping_mul(1099511628211);
    }
    format!("{h:016x}.jpg")
}

fn miniature(stock: &Stockage, clip: &Path) {
    let dossier = miniatures(stock);
    let _ = fs::create_dir_all(&dossier);
    let _ = Command::new(ffmpeg(stock))
        .args(["-hide_banner", "-loglevel", "error", "-y", "-ss", "1", "-i"])
        .arg(clip)
        .args(["-frames:v", "1", "-vf", "scale=480:-2", "-q:v", "4"])
        .arg(dossier.join(cle_miniature(clip)))
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

#[derive(Serialize)]
pub struct Clip {
    pub chemin: String,
    pub nom: String,
    pub jeu: String,
    /// Secondes depuis 1970
    pub date: u64,
    pub octets: u64,
    pub miniature: Option<String>,
}

/// Clips du dossier Vidéos\FPS Cagoule, du plus récent au plus ancien
pub fn liste(stock: &Stockage) -> Vec<Clip> {
    let mut clips = Vec::new();
    let Ok(jeux) = fs::read_dir(dossier_clips()) else { return clips };
    for jeu in jeux.flatten().filter(|e| e.path().is_dir()) {
        let Ok(fichiers) = fs::read_dir(jeu.path()) else { continue };
        for f in fichiers.flatten() {
            let p = f.path();
            if !p.extension().is_some_and(|x| x.eq_ignore_ascii_case("mp4")) {
                continue;
            }
            let Ok(m) = f.metadata() else { continue };
            let image = fs::read(miniatures(stock).join(cle_miniature(&p))).ok();
            clips.push(Clip {
                nom: p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                jeu: jeu.file_name().to_string_lossy().to_string(),
                date: m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs()),
                octets: m.len(),
                miniature: image.map(|o| format!("data:image/jpeg;base64,{}", crate::ps::base64(&o))),
                chemin: p.to_string_lossy().to_string(),
            });
        }
    }
    clips.sort_by(|a, b| b.date.cmp(&a.date));
    clips
}

/// Chemin d'un clip venu de l'interface, accepté seulement s'il est dans Vidéos\FPS Cagoule
pub fn clip_valide(chemin: &str) -> Result<PathBuf, String> {
    let p = fs::canonicalize(chemin).map_err(|_| "Clip introuvable".to_string())?;
    let racine = fs::canonicalize(dossier_clips()).map_err(|_| "Dossier des clips introuvable".to_string())?;
    if p.starts_with(&racine) && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("mp4")) {
        Ok(p)
    } else {
        Err("Chemin refusé".into())
    }
}

pub fn supprimer(stock: &Stockage, chemin: &str) -> Result<(), String> {
    let p = clip_valide(chemin)?;
    let _ = fs::remove_file(miniatures(stock).join(cle_miniature(Path::new(chemin))));
    fs::remove_file(p).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noms_de_fichiers() {
        assert_eq!(nom_fichier("3K · Ace · Clubhouse"), "3K · Ace · Clubhouse");
        assert_eq!(nom_fichier("R6: Siege / test?"), "R6 Siege test");
    }

    /// Vrai FFmpeg sur le PC Windows de GitHub : téléchargement vérifié, enregistrement avec son, clip sauvegardé
    #[test]
    fn ffmpeg_reel() {
        // Hors du dossier temporaire : le test du nettoyage le vide en même temps
        let racine = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("test-clips");
        let stock = Stockage::ouvrir(racine.join("donnees"));
        installer(&stock, |_, _| {}).expect("installation de FFmpeg");
        let dossier = racine.join("tampon");
        let _ = fs::remove_dir_all(&dossier);
        fs::create_dir_all(&dossier).unwrap();
        // Image de test + silence envoyé comme le ferait la capture du son
        let format = crate::audio::Format { frequence: 48000, canaux: 2, flottant: true };
        let base = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_micros() as i64;
        let args = arguments("testsrc2=size=640x360:rate=60", Some(format), Encodeur::Logiciel, base, &dossier);
        let mut enfant = Command::new(ffmpeg(&stock)).args(&args).stdin(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let mut entree = enfant.stdin.take().unwrap();
        let debut = Instant::now();
        let silence = vec![0u8; 480 * 8];
        while debut.elapsed() < Duration::from_secs(9) {
            if entree.write_all(&silence).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let clip = sauver(&stock, &dossier, "Test FPS Cagoule", 4, None);
        let _ = enfant.kill();
        let mut erreur = String::new();
        let _ = enfant.stderr.take().unwrap().read_to_string(&mut erreur);
        let clip = clip.unwrap_or_else(|e| panic!("{e} / FFmpeg : {erreur}"));
        // FFmpeg décrit le clip : il faut une vidéo et un son
        let sortie = Command::new(ffmpeg(&stock)).arg("-i").arg(&clip).output().unwrap();
        let infos = String::from_utf8_lossy(&sortie.stderr).to_string();
        println!("{}", infos.lines().filter(|l| l.contains("Duration") || l.contains("Stream")).collect::<Vec<_>>().join("\n"));
        assert!(infos.contains("Video: h264") && infos.contains("Audio: aac"), "{infos}");
        assert!(liste(&stock).iter().any(|c| Path::new(&c.chemin) == clip && c.miniature.is_some()), "miniature absente");
        supprimer(&stock, &clip.to_string_lossy()).unwrap();
        let _ = fs::remove_dir_all(&dossier);
        // Vraie capture de l'écran : le PC de GitHub n'a pas forcément de bureau utilisable, on affiche seulement le résultat
        match demarrer(&stock, "Test écran") {
            Ok(e) => {
                std::thread::sleep(Duration::from_secs(5));
                println!("Capture de l'écran : OK ({:?}), morceaux : {}", e.encodeur, morceaux_recents(&e.dossier, 30).len());
            }
            Err(e) => println!("Capture de l'écran indisponible sur ce PC : {e}"),
        }
    }

    #[test]
    fn arguments_nvidia_avec_son() {
        let f = crate::audio::Format { frequence: 48000, canaux: 2, flottant: true };
        let a = arguments(ECRAN, Some(f), Encodeur::Nvidia, 1_000_000, Path::new(r"C:\t"));
        let texte = a.join(" ");
        assert!(texte.contains("-f f32le -ar 48000 -ac 2 -i pipe:0"));
        assert!(texte.contains("[1:v]setpts=(RTCTIME-1000000)/(1000000*TB)[v]"));
        assert!(texte.contains("h264_nvenc") && texte.contains("-segment_wrap 124"));
        assert!(a.last().unwrap().ends_with(r"m%03d.ts"));
    }
}
