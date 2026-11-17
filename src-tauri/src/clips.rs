// Clips (comme Medal) : pendant les parties, FFmpeg filme l'écran (Desktop Duplication, encodage par la carte
// graphique) en morceaux de 2 s sur le disque, avec le son du PC. La touche choisie recolle les dernières
// 30 s, 1 min ou 3 min en un .mp4 dans Vidéos\FPS Cagoule. Rien ne tourne en dehors des parties.
// FFmpeg (9.0.2, build « essentials » de Gyan Doshi) n'est téléchargé que si l'utilisateur active les clips.

use crate::config::Stockage;
use crate::gpu;
use crate::systeme::CREATE_NO_WINDOW;
use serde::{Deserialize, Serialize};
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

/// Dossier des clips choisi par l'utilisateur, sinon Vidéos\FPS Cagoule
pub fn dossier_clips(stock: &Stockage) -> PathBuf {
    stock.donnees.lock().unwrap().reglages.clips_dossier.clone().filter(|d| !d.trim().is_empty()).map(|d| PathBuf::from(crate::partage::resoudre(&d))).unwrap_or_else(dossier_defaut)
}

/// Dossiers où chercher les clips : celui choisi et Vidéos\FPS Cagoule (clips d'avant un changement, ou de secours)
pub fn dossiers_clips(stock: &Stockage) -> Vec<PathBuf> {
    let mut l = vec![dossier_clips(stock)];
    let defaut = dossier_defaut();
    if !l.contains(&defaut) {
        l.push(defaut);
    }
    l
}

/// Le dossier accepte l'écriture (dossier réseau joignable…)
pub fn accessible(dossier: &Path) -> bool {
    essai_ecriture(dossier).is_ok()
}

/// Essaie d'écrire un petit fichier dans le dossier (l'erreur de Windows dit pourquoi ça échoue)
pub fn essai_ecriture(dossier: &Path) -> std::io::Result<()> {
    let essai = dossier.join(".fps-cagoule-essai");
    fs::create_dir_all(dossier)?;
    let r = fs::write(&essai, b"ok");
    let _ = fs::remove_file(essai);
    r
}

/// Fenêtre Windows de choix d'un dossier, avec un champ où taper un chemin réseau (\\serveur\partage),
/// même quand Windows ne montre pas les serveurs du réseau. `fenetre` : fenêtre de l'app, pour s'ouvrir devant elle.
pub fn choisir_dossier(fenetre: isize) -> Option<String> {
    // BIF_RETURNONLYFSDIRS | BIF_EDITBOX | BIF_NEWDIALOGSTYLE | BIF_SHAREABLE
    let script = format!(
        r#"$s = New-Object -ComObject Shell.Application
$d = $s.BrowseForFolder({fenetre}, 'Dossier des clips. Pour un serveur, tape son chemin, par exemple \\192.168.1.10\partage', 0x8051, 0)
if ($d) {{ $d.Self.Path }}"#
    );
    crate::ps::executer(&script).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && !s.starts_with("::"))
}

pub fn dossier_defaut() -> PathBuf {
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

/// Qualité de l'enregistrement : débit (Mbit/s) et images par seconde
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Qualite {
    pub debit_mbps: u32,
    pub ips: u32,
    /// Qualité constante de l'encodeur du processeur (plus petit = meilleur)
    pub crf: u32,
}

impl Qualite {
    /// Standard 12, haute 20, ultra 40 Mbit/s à 60 images/s ; ×1,5 à 120 images/s
    pub fn depuis(niveau: &str, ips: u32) -> Self {
        let ips = if ips >= 120 { 120 } else { 60 };
        let (base, crf) = match niveau {
            "standard" => (12, 26),
            "ultra" => (40, 19),
            _ => (20, 23),
        };
        Qualite { debit_mbps: if ips == 120 { base * 3 / 2 } else { base }, ips, crf }
    }
}

/// Capture d'écran de FFmpeg : écran principal, sans le curseur
pub fn source_ecran(ips: u32) -> String {
    format!("ddagrab=output_idx=0:framerate={ips}:draw_mouse=0")
}

/// Arguments FFmpeg de l'enregistrement. `base_us` : instant de référence (µs depuis 1970) commun au son et à l'image.
pub fn arguments(source_video: &str, audio: Option<crate::audio::Format>, encodeur: Encodeur, qualite: Qualite, base_us: i64, sortie: &Path) -> Vec<String> {
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
    // Une image clé toutes les 2 s : les morceaux du tampon commencent tous par une image complète
    let gop = (qualite.ips * MORCEAU_S).to_string();
    let debit = format!("{}M", qualite.debit_mbps);
    let crf = qualite.crf.to_string();
    let video: Vec<&str> = match encodeur {
        Encodeur::Nvidia => vec!["-c:v", "h264_nvenc", "-preset", "p4", "-tune", "ll", "-rc", "cbr", "-b:v", &debit, "-bf", "0"],
        Encodeur::Amd => vec!["-c:v", "h264_amf", "-usage", "lowlatency", "-quality", "speed", "-rc", "cbr", "-b:v", &debit],
        Encodeur::Intel => vec!["-c:v", "h264_qsv", "-preset", "veryfast", "-b:v", &debit],
        Encodeur::Logiciel => vec!["-c:v", "libx264", "-preset", "ultrafast", "-tune", "zerolatency", "-crf", &crf],
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


pub struct Enregistreur {
    enfant: Child,
    _son: Option<crate::audio::Capture>,
    pub dossier: PathBuf,
    pub jeu: String,
    pub encodeur: Encodeur,
    qualite: Qualite,
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
        let (enfant, son) = lancer_ffmpeg(stock, self.encodeur, self.qualite, &self.dossier, self.base_us)?;
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

fn lancer_ffmpeg(stock: &Stockage, encodeur: Encodeur, qualite: Qualite, dossier: &Path, base_us: i64) -> Result<(Child, Option<crate::audio::Capture>), String> {
    let son = crate::audio::ouvrir().ok();
    let args = arguments(&source_ecran(qualite.ips), son.as_ref().map(|(f, _)| *f), encodeur, qualite, base_us, dossier);
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
    let qualite = {
        let r = &stock.donnees.lock().unwrap().reglages;
        Qualite::depuis(&r.clips_qualite, r.clips_ips)
    };
    for encodeur in [encodeur_conseille(), Encodeur::Logiciel] {
        match lancer_ffmpeg(stock, encodeur, qualite, &dossier, base_us) {
            Ok((enfant, son)) => return Ok(Enregistreur { enfant, _son: son, dossier, jeu: jeu.into(), encodeur, qualite, base_us, relances: 0 }),
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

pub fn date_texte() -> String {
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
    // Dossier choisi (serveur…) ; s'il est injoignable, le clip reste sur le PC
    let mut dossier_jeu = dossier_clips(stock).join(nom_fichier(jeu));
    if fs::create_dir_all(&dossier_jeu).is_err() {
        dossier_jeu = dossier_defaut().join(nom_fichier(jeu));
        fs::create_dir_all(&dossier_jeu).map_err(|e| e.to_string())?;
    }
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

/// Chemin sous sa forme courante : « \\?\C:\… » (chemin canonique de Windows) → « C:\… », « \\?\UNC\serveur\… » → « \\serveur\… »
pub fn chemin_simple(p: &Path) -> String {
    let t = p.to_string_lossy();
    if let Some(r) = t.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{r}")
    } else {
        t.strip_prefix(r"\\?\").unwrap_or(&t).to_string()
    }
}

fn cle_clip(clip: &Path, extension: &str) -> String {
    // Nom stable par chemin de clip, quelle que soit sa forme
    let mut h: u64 = 1469598103934665603;
    for b in chemin_simple(clip).to_lowercase().bytes() {
        h = (h ^ b as u64).wrapping_mul(1099511628211);
    }
    format!("{h:016x}.{extension}")
}

fn cle_miniature(clip: &Path) -> String {
    cle_clip(clip, "jpg")
}

/// Repère sur la vidéo : kill, KO ou mort, à `t` secondes du début du clip
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Marqueur {
    pub t: f64,
    pub genre: String,
    pub detail: String,
    pub headshot: bool,
}

fn fichier_marqueurs(stock: &Stockage, clip: &Path) -> PathBuf {
    stock.dossier().join("clips-infos").join(cle_clip(clip, "json"))
}

pub fn marqueurs(stock: &Stockage, clip: &Path) -> Vec<Marqueur> {
    fs::read_to_string(fichier_marqueurs(stock, clip)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Ajoute des repères au clip (sans doublon à moins d'une demi-seconde du même genre)
pub fn ajouter_marqueurs(stock: &Stockage, clip: &Path, nouveaux: &[Marqueur]) {
    let mut liste = marqueurs(stock, clip);
    for m in nouveaux {
        if !liste.iter().any(|x| x.genre == m.genre && (x.t - m.t).abs() < 0.5) {
            liste.push(m.clone());
        }
    }
    liste.sort_by(|a, b| a.t.total_cmp(&b.t));
    let f = fichier_marqueurs(stock, clip);
    let _ = fs::create_dir_all(f.parent().unwrap());
    if let Ok(t) = serde_json::to_string(&liste) {
        let _ = fs::write(f, t);
    }
}

fn miniature(stock: &Stockage, clip: &Path) {
    miniature_vers(&ffmpeg(stock), &miniatures(stock), clip);
}

/// Image du clip à 1 s (ffmpeg.exe, dossier des miniatures) ; vrai si elle a été faite
fn miniature_vers(ffmpeg: &Path, dossier: &Path, clip: &Path) -> bool {
    let _ = fs::create_dir_all(dossier);
    let sortie = dossier.join(cle_miniature(clip));
    let _ = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-ss", "1", "-i"])
        .arg(clip)
        .args(["-frames:v", "1", "-vf", "scale=480:-2", "-q:v", "4"])
        .arg(&sortie)
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    sortie.is_file()
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
    pub marqueurs: Vec<Marqueur>,
}

/// Vidéos .mp4 d'un dossier et de ses sous-dossiers (4 niveaux), avec le jeu :
/// celui qui contient la vidéo (Clips\R6\…, ou Medal\Clips\R6\… recopié tel quel), « Autres » à la racine
fn videos(racine: &Path) -> Vec<(PathBuf, String)> {
    let mut trouves = Vec::new();
    let mut pile = vec![(racine.to_path_buf(), 0u8, None::<String>)];
    let mut vus = 0usize;
    while let Some((d, profondeur, jeu)) = pile.pop() {
        let Ok(entrees) = fs::read_dir(&d) else { continue };
        for e in entrees.flatten() {
            vus += 1;
            if vus > 20_000 {
                return trouves;
            }
            let nom = e.file_name().to_string_lossy().to_string();
            if nom.starts_with('.') || nom.starts_with('$') {
                continue;
            }
            let p = e.path();
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                if profondeur < 4 {
                    pile.push((p, profondeur + 1, Some(nom)));
                }
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("mp4")) {
                trouves.push((p, jeu.clone().unwrap_or_else(|| "Autres".into())));
            }
        }
    }
    trouves
}

static MINIATURES_EN_COURS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Vidéos dont la miniature a déjà été tentée (fichier abîmé…) : on n'y revient pas
static MINIATURES_ESSAYEES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Clips des dossiers des clips, du plus récent au plus ancien.
/// Les miniatures manquantes (vidéos copiées à la main, Medal…) sont faites en fond, puis `fini` est appelé.
pub fn liste(stock: &Stockage, fini: impl FnOnce() + Send + 'static) -> Vec<Clip> {
    let mut clips = Vec::new();
    let mut sans_miniature = Vec::new();
    let mut deja = std::collections::HashSet::new();
    for racine in dossiers_clips(stock) {
        for (p, jeu) in videos(&racine) {
            if !deja.insert(chemin_simple(&p).to_lowercase()) {
                continue;
            }
            let Ok(m) = fs::metadata(&p) else { continue };
            let image = fs::read(miniatures(stock).join(cle_miniature(&p))).ok();
            if image.is_none() && !MINIATURES_ESSAYEES.lock().unwrap().contains(&chemin_simple(&p)) {
                sans_miniature.push(p.clone());
            }
            clips.push(Clip {
                nom: p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                jeu,
                date: m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs()),
                octets: m.len(),
                miniature: image.map(|o| format!("data:image/jpeg;base64,{}", crate::ps::base64(&o))),
                marqueurs: marqueurs(stock, &p),
                chemin: p.to_string_lossy().to_string(),
            });
        }
    }
    clips.sort_by(|a, b| b.date.cmp(&a.date));
    use std::sync::atomic::Ordering;
    if !sans_miniature.is_empty() && installe(stock) && !MINIATURES_EN_COURS.swap(true, Ordering::SeqCst) {
        let (outil, dossier) = (ffmpeg(stock), miniatures(stock));
        let _ = std::thread::Builder::new().name("miniatures".into()).spawn(move || {
            // Les plus récentes d'abord, 60 par passage pour rester léger
            let mut faites = 0;
            for p in sans_miniature.iter().take(60) {
                MINIATURES_ESSAYEES.lock().unwrap().push(chemin_simple(p));
                faites += miniature_vers(&outil, &dossier, p) as usize;
            }
            MINIATURES_EN_COURS.store(false, Ordering::SeqCst);
            if faites > 0 {
                fini();
            }
        });
    }
    clips
}

/// Chemin d'un clip venu de l'interface, accepté seulement s'il est dans un dossier des clips
pub fn clip_valide(stock: &Stockage, chemin: &str) -> Result<PathBuf, String> {
    let p = fs::canonicalize(chemin).map_err(|_| "Clip introuvable".to_string())?;
    let dans_un_dossier = dossiers_clips(stock).iter().filter_map(|d| fs::canonicalize(d).ok()).any(|r| p.starts_with(r));
    if dans_un_dossier && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("mp4")) {
        Ok(PathBuf::from(chemin_simple(&p)))
    } else {
        Err("Chemin refusé".into())
    }
}

pub fn supprimer(stock: &Stockage, chemin: &str) -> Result<(), String> {
    let p = clip_valide(stock, chemin)?;
    let _ = fs::remove_file(miniatures(stock).join(cle_miniature(Path::new(chemin))));
    let _ = fs::remove_file(fichier_marqueurs(stock, Path::new(chemin)));
    fs::remove_file(p).map_err(|e| e.to_string())
}

/// Le clip a-t-il une piste son (le PC peut ne pas avoir de sortie audio)
fn a_du_son(stock: &Stockage, clip: &Path) -> bool {
    Command::new(ffmpeg(stock))
        .arg("-i")
        .arg(clip)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stderr).contains("Audio:"))
        .unwrap_or(false)
}

/// Filtre FFmpeg qui garde les morceaux [début, fin] (en secondes) et les recolle
pub fn filtre_montage(morceaux: &[(f64, f64)], son: bool) -> String {
    let mut f = String::new();
    for (i, (a, b)) in morceaux.iter().enumerate() {
        f.push_str(&format!("[0:v]trim=start={a:.3}:end={b:.3},setpts=PTS-STARTPTS[v{i}];"));
        if son {
            f.push_str(&format!("[0:a]atrim=start={a:.3}:end={b:.3},asetpts=PTS-STARTPTS[a{i}];"));
        }
    }
    for i in 0..morceaux.len() {
        f.push_str(&format!("[v{i}]"));
        if son {
            f.push_str(&format!("[a{i}]"));
        }
    }
    f.push_str(&format!("concat=n={}:v=1:a={}[v]", morceaux.len(), u8::from(son)));
    if son {
        f.push_str("[a]");
    }
    f
}

/// Position d'un repère après le montage (None s'il tombe dans un passage retiré)
pub fn deplacer_repere(t: f64, morceaux: &[(f64, f64)]) -> Option<f64> {
    let mut avant = 0.0;
    for (a, b) in morceaux {
        if t >= *a && t <= *b {
            return Some(avant + t - a);
        }
        avant += b - a;
    }
    None
}

/// Exporte le montage (morceaux gardés) dans un nouveau fichier « … (montage).mp4 ». `progression(0..1)`.
pub fn exporter(stock: &Stockage, chemin: &str, morceaux: &[(f64, f64)], progression: impl Fn(f64)) -> Result<PathBuf, String> {
    let source = clip_valide(stock, chemin)?;
    let mut m: Vec<(f64, f64)> = morceaux.iter().copied().filter(|(a, b)| b - a >= 0.2 && *a >= 0.0).collect();
    m.sort_by(|x, y| x.0.total_cmp(&y.0));
    if m.is_empty() {
        return Err("Garde au moins un passage".into());
    }
    let total: f64 = m.iter().map(|(a, b)| b - a).sum();
    let son = a_du_son(stock, &source);
    let nom = source.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "clip".into());
    let dossier = source.parent().ok_or("Dossier introuvable")?;
    let mut sortie = dossier.join(format!("{nom} (montage).mp4"));
    let mut n = 2;
    while sortie.exists() {
        sortie = dossier.join(format!("{nom} (montage {n}).mp4"));
        n += 1;
    }
    let mut derniere_erreur = String::new();
    for encodeur in [encodeur_conseille(), Encodeur::Logiciel] {
        let video: Vec<&str> = match encodeur {
            Encodeur::Nvidia => vec!["-c:v", "h264_nvenc", "-preset", "p5", "-rc", "vbr", "-cq", "21", "-b:v", "0"],
            Encodeur::Amd => vec!["-c:v", "h264_amf", "-quality", "quality", "-rc", "cqp", "-qp_i", "20", "-qp_p", "22"],
            Encodeur::Intel => vec!["-c:v", "h264_qsv", "-global_quality", "21", "-pix_fmt", "nv12"],
            Encodeur::Logiciel => vec!["-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-pix_fmt", "yuv420p"],
        };
        let mut cmd = Command::new(ffmpeg(stock));
        cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-nostats", "-progress", "pipe:1", "-i"])
            .arg(&source)
            .args(["-filter_complex", &filtre_montage(&m, son), "-map", "[v]"]);
        if son {
            cmd.args(["-map", "[a]", "-c:a", "aac", "-b:a", "160k"]);
        }
        let mut enfant = cmd
            .args(&video)
            .args(["-movflags", "+faststart"])
            .arg(&sortie)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| e.to_string())?;
        // Progression : FFmpeg écrit « out_time_us=… » sur sa sortie
        if let Some(out) = enfant.stdout.take() {
            use std::io::BufRead;
            for ligne in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                if let Some(us) = ligne.strip_prefix("out_time_us=").and_then(|v| v.trim().parse::<f64>().ok()) {
                    progression((us / 1e6 / total).clamp(0.0, 1.0));
                }
            }
        }
        let mut erreur = String::new();
        if let Some(mut e) = enfant.stderr.take() {
            let _ = e.read_to_string(&mut erreur);
        }
        let ok = enfant.wait().map(|s| s.success()).unwrap_or(false);
        if ok && sortie.is_file() {
            progression(1.0);
            miniature(stock, &sortie);
            let reperes: Vec<Marqueur> = marqueurs(stock, &source)
                .into_iter()
                .filter_map(|mut r| deplacer_repere(r.t, &m).map(|t| {
                    r.t = t;
                    r
                }))
                .collect();
            ajouter_marqueurs(stock, &sortie, &reperes);
            return Ok(sortie);
        }
        let _ = fs::remove_file(&sortie);
        derniere_erreur = erreur.lines().last().unwrap_or("").to_string();
        if encodeur == Encodeur::Logiciel {
            break;
        }
    }
    Err(format!("Export impossible : {derniere_erreur}"))
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
        let args = arguments("testsrc2=size=640x360:rate=60", Some(format), Encodeur::Logiciel, Qualite::depuis("haute", 60), base, &dossier);
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
        assert!(liste(&stock, || {}).iter().any(|c| Path::new(&c.chemin) == clip && c.miniature.is_some()), "miniature absente");
        // Montage : deux passages gardés, repère déplacé
        ajouter_marqueurs(&stock, &clip, &[Marqueur { t: 3.0, genre: "kill".into(), detail: "x".into(), headshot: true }]);
        let montage = exporter(&stock, &clip.to_string_lossy(), &[(0.0, 1.0), (2.5, 4.0)], |_| {}).expect("export du montage");
        assert_eq!(marqueurs(&stock, &montage).first().map(|m| m.t), Some(1.5));
        supprimer(&stock, &montage.to_string_lossy()).unwrap();
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
    fn videos_dans_les_sous_dossiers() {
        let d = std::env::temp_dir().join("fps-cagoule-test-videos");
        let _ = fs::remove_dir_all(&d);
        for f in ["racine.mp4", r"Rainbow Six Siege\a.mp4", r"Medal\Clips\R6\b.MP4", r".cache\c.mp4", r"Rainbow Six Siege\notes.txt"] {
            let p = d.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, b"x").unwrap();
        }
        let mut v: Vec<(String, String)> = videos(&d).into_iter().map(|(p, j)| (p.file_name().unwrap().to_string_lossy().to_string(), j)).collect();
        v.sort();
        assert_eq!(v, [("a.mp4".into(), "Rainbow Six Siege".into()), ("b.MP4".into(), "R6".into()), ("racine.mp4".into(), "Autres".into())]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn chemins() {
        assert_eq!(chemin_simple(Path::new(r"\\?\C:\Vidéos\a.mp4")), r"C:\Vidéos\a.mp4");
        assert_eq!(chemin_simple(Path::new(r"\\?\UNC\192.168.1.204\fichiers\a.mp4")), r"\\192.168.1.204\fichiers\a.mp4");
        assert_eq!(cle_clip(Path::new(r"\\?\C:\V\A.mp4"), "json"), cle_clip(Path::new(r"C:\V\a.mp4"), "json"));
    }

    #[test]
    fn montage() {
        let m = [(0.0, 5.0), (8.0, 10.5)];
        assert_eq!(
            filtre_montage(&m, true),
            "[0:v]trim=start=0.000:end=5.000,setpts=PTS-STARTPTS[v0];[0:a]atrim=start=0.000:end=5.000,asetpts=PTS-STARTPTS[a0];[0:v]trim=start=8.000:end=10.500,setpts=PTS-STARTPTS[v1];[0:a]atrim=start=8.000:end=10.500,asetpts=PTS-STARTPTS[a1];[v0][a0][v1][a1]concat=n=2:v=1:a=1[v][a]"
        );
        assert_eq!(deplacer_repere(9.0, &m), Some(6.0));
        assert_eq!(deplacer_repere(6.0, &m), None);
    }

    #[test]
    fn arguments_nvidia_avec_son() {
        let f = crate::audio::Format { frequence: 48000, canaux: 2, flottant: true };
        let a = arguments(&source_ecran(120), Some(f), Encodeur::Nvidia, Qualite::depuis("ultra", 120), 1_000_000, Path::new(r"C:\t"));
        let texte = a.join(" ");
        assert!(texte.contains("-f f32le -ar 48000 -ac 2 -i pipe:0"));
        assert!(texte.contains("[1:v]setpts=(RTCTIME-1000000)/(1000000*TB)[v]"));
        assert!(texte.contains("h264_nvenc") && texte.contains("-segment_wrap 124"));
        assert!(texte.contains("framerate=120") && texte.contains("-b:v 60M") && texte.contains("-g 240"));
        assert_eq!(Qualite::depuis("standard", 60), Qualite { debit_mbps: 12, ips: 60, crf: 26 });
        assert_eq!(Qualite::depuis("inconnu", 90).debit_mbps, 20);
        assert!(a.last().unwrap().ends_with(r"m%03d.ts"));
    }
}
