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
use std::sync::{Arc, Mutex};

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
    /// Standard 12, haute 20, ultra 40 Mbit/s, toujours à 60 images/s : la capture de Windows plafonne vers
    /// 65 images vraiment nouvelles par seconde en jeu (mesuré), le 120 donnait des fichiers lourds sans gain.
    /// `ips` est gardé pour les anciens réglages (120) mais ignoré.
    pub fn depuis(niveau: &str, ips: u32) -> Self {
        let _ = ips;
        let ips = 60;
        let (base, crf) = match niveau {
            "standard" => (12, 26),
            "ultra" => (40, 19),
            _ => (20, 23),
        };
        Qualite { debit_mbps: if ips == 120 { base * 3 / 2 } else { base }, ips, crf }
    }
}

/// Capture d'écran de FFmpeg : écran principal, sans le curseur
/// Capture de la fenêtre du jeu (Windows Graphics Capture, comme OBS) : reçoit chaque image du jeu, là où la copie
/// de l'écran en rate dans les parties chargées. `exe` : exécutable du jeu (RainbowSix.exe…).
pub fn source_fenetre(exe: &str, ips: u32) -> String {
    // Motif sans caractères spéciaux pour FFmpeg : « . » remplace tout ce qui n'est pas lettre, chiffre, _ ou -
    let motif: String = exe.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c.to_ascii_lowercase() } else { '.' }).collect();
    // R6 : sa vraie fenêtre de jeu (pas celle de lancement)
    let classe = if motif.starts_with("rainbowsix") { ":window_class=R6Game" } else { "" };
    format!("gfxcapture=window_exe=(?i){motif}${classe}:max_framerate={ips}:capture_cursor=0:display_border=0")
}

pub fn source_ecran(ips: u32) -> String {
    format!("ddagrab=output_idx=0:framerate={ips}:draw_mouse=0")
}

/// Heure du son : comptée en échantillons depuis le départ, pas à l'arrivée de chaque paquet dans le tuyau.
/// À l'arrivée, un paquet en retard de 0,1 à 0,25 s (le jeu occupe le PC) faisait croire à un trou :
/// FFmpeg le comblait par du silence puis jetait le rattrapage → son haché (442 blancs dans un clip de 3 min).
/// Recalage sur l'horloge du PC (registre 0 = décalage) : plus tôt dès qu'un paquet arrive en avance sur le compte,
/// plus tard seulement au-delà de 0,5 s de retard (son vraiment perdu), jamais pour un simple paquet en retard.
pub fn filtre_son(base_us: i64) -> String {
    format!("asetpts='st(1,PTS-N/SR/TB);if(eq(N,0)+lt(ld(1),ld(0))+gt(ld(1)-ld(0),0.5/TB),st(0,ld(1)));N/SR/TB+ld(0)-({base_us}/1000000)/TB'")
}

/// Arguments FFmpeg de l'enregistrement. `base_us` : instant de référence (µs depuis 1970) commun au son et à l'image.
/// `audio` : une entrée par piste son (format, et « pipe:0 » ou le nom d'un tuyau nommé).
pub fn arguments(source_video: &str, audio: &[(crate::audio::Format, String)], encodeur: Encodeur, qualite: Qualite, base_us: i64, sortie: &Path) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error", "-y"].iter().map(|s| s.to_string()).collect();
    for (f, entree) in audio {
        a.extend(
            [
                "-thread_queue_size", "4096", "-use_wallclock_as_timestamps", "1", "-f", f.ffmpeg(),
                "-ar", &f.frequence.to_string(), "-ac", &f.canaux.to_string(), "-i", entree,
            ]
            .iter()
            .map(|s| s.to_string()),
        );
    }
    // Heure de chaque image prise à la capture (horloge du PC, comme le son), pas plus loin dans le traitement :
    // sinon les images qui attendent un instant repartent groupées avec presque la même heure, FFmpeg en jette une
    // sur deux et recopie la précédente (~32 vraies images/s mesurées, contre 60 pour la capture seule)
    a.extend(["-thread_queue_size", "64", "-use_wallclock_as_timestamps", "1", "-f", "lavfi", "-i", source_video].iter().map(|s| s.to_string()));
    let v = audio.len();
    // Images de la capture d'écran (D3D11) : directes pour NVIDIA et AMD, converties pour Intel et le processeur
    // Images de la carte graphique (D3D11), écran entier ou fenêtre du jeu
    let capture_ecran = source_video.starts_with("ddagrab") || source_video.starts_with("gfxcapture");
    let conversion = match encodeur {
        _ if !capture_ecran => "format=yuv420p,",
        Encodeur::Nvidia | Encodeur::Amd => "",
        Encodeur::Intel => "hwmap=derive_device=qsv,format=qsv,",
        Encodeur::Logiciel => "hwdownload,format=bgra,format=yuv420p,",
    };
    // Son et image horodatés avec la même horloge (temps réel), ramenée à base_us.
    // Chacun dans son propre circuit (-filter:v / -filter:a) : réunis dans un seul graphe, l'image attendait le son
    // (qui arrive par paquets) et repartait par à-coups : ~30 images/s au lieu de 60 (reproduit : 278 contre 479 en 8 s)
    a.extend(["-copyts".to_string(), "-map".into(), format!("{v}:v"), "-filter:v".into(), format!("{conversion}setpts=PTS-({base_us}/1000000)/TB")]);
    if !audio.is_empty() {
        // Une piste par source (son du PC, Discord, micro), chacune horodatée de la même façon
        for i in 0..audio.len() {
            a.extend(["-map".to_string(), format!("{i}:a")]);
        }
        a.extend(["-filter:a".to_string(), filtre_son(base_us)]);
        a.extend(["-c:a", "aac", "-b:a", "160k"].iter().map(|s| s.to_string()));
    }
    // Une image clé toutes les 2 s : les morceaux du tampon commencent tous par une image complète
    let gop = (qualite.ips * MORCEAU_S).to_string();
    let debit = format!("{}M", qualite.debit_mbps);
    let crf = qualite.crf.to_string();
    // Un clip n'est pas du direct : réglages « qualité » (profil High, images B, analyse en avance, quantification
    // adaptative) plutôt que « faible latence » (profil Baseline), bien plus net à débit égal, surtout en mouvement.
    // Débit visé, et jusqu'à 1,5 fois plus dans les scènes chargées.
    let maxi = format!("{}M", qualite.debit_mbps * 3 / 2);
    let tampon = format!("{}M", qualite.debit_mbps * 2);
    let video: Vec<&str> = match encodeur {
        Encodeur::Nvidia => vec![
            "-c:v", "h264_nvenc", "-preset", "p5", "-tune", "hq", "-profile:v", "high", "-rc", "vbr", "-cq", &crf, "-b:v", &debit, "-maxrate", &maxi,
            "-bufsize", &tampon, "-bf", "2", "-spatial-aq", "1", "-rc-lookahead", "16",
        ],
        Encodeur::Amd => vec![
            "-c:v", "h264_amf", "-usage", "transcoding", "-quality", "quality", "-profile:v", "high", "-rc", "vbr_peak", "-b:v", &debit, "-maxrate", &maxi,
        ],
        Encodeur::Intel => vec!["-c:v", "h264_qsv", "-preset", "medium", "-profile:v", "high", "-b:v", &debit, "-maxrate", &maxi],
        Encodeur::Logiciel => vec!["-c:v", "libx264", "-preset", "superfast", "-profile:v", "high", "-crf", &crf, "-maxrate", &maxi, "-bufsize", &tampon],
    };
    a.extend(video.iter().map(|s| s.to_string()));
    // Les images gardent leur heure : pas de recalage forcé sur 60 i/s (qui jette et recopie des images)
    a.extend(["-fps_mode", "vfr"].iter().map(|s| s.to_string()));
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
    _son: Vec<crate::audio::Capture>,
    /// Pistes son, choisies au début de la partie et gardées jusqu'au bout (mêmes pistes dans tous les morceaux)
    sons: Sons,
    pub dossier: PathBuf,
    pub jeu: String,
    pub encodeur: Encodeur,
    qualite: Qualite,
    /// Horloge commune gardée d'un redémarrage à l'autre : les morceaux restent bout à bout
    base_us: i64,
    /// Redémarrages pendant la partie (changement de résolution, plein écran…)
    pub relances: u32,
    /// Morceau le plus récent vu à la dernière surveillance, et depuis quand c'est lui
    dernier_morceau: Option<PathBuf>,
    depuis: Instant,
    /// Image arrêtée : le dernier morceau ne contient plus que du son
    fige: bool,
    /// Dernier message d'erreur de FFmpeg (capture d'écran perdue…)
    pub derniere_erreur: Arc<Mutex<Option<String>>>,
    /// Raison de la dernière relance (pour le journal)
    pub raison: &'static str,
    /// Exécutable du jeu : capture de sa fenêtre quand c'est possible
    exe: Option<String>,
    /// Capture en cours : fenêtre du jeu (true) ou écran entier
    pub fenetre: bool,
    /// Nouvel essai de la capture de fenêtre déjà fait (la fenêtre n'existait pas encore au lancement)
    pub fenetre_reessayee: bool,
}

impl Enregistreur {
    /// FFmpeg tourne encore
    pub fn vivant(&mut self) -> bool {
        matches!(self.enfant.try_wait(), Ok(None))
    }

    /// L'image ne vient plus : Windows coupe la capture d'écran quand le jeu passe en plein écran
    /// ou change de résolution, et FFmpeg continue avec le son seul. On le voit au tampon :
    /// un nouveau morceau arrive toutes les 2 s tant qu'il y a des images, sinon le même grossit.
    pub fn image_figee(&mut self) -> bool {
        let recent = morceaux_recents(&self.dossier, 0).pop();
        if recent.is_some() && recent != self.dernier_morceau {
            self.dernier_morceau = recent;
            self.depuis = Instant::now();
            return false;
        }
        let limite = if recent.is_none() { 15 } else { 4 * MORCEAU_S as u64 };
        self.fige = self.depuis.elapsed() > Duration::from_secs(limite);
        self.fige
    }

    /// Relance FFmpeg après un arrêt (la capture d'écran s'arrête quand la résolution change)
    pub fn relancer(&mut self, stock: &Stockage) -> Result<(), String> {
        let _ = self.enfant.kill();
        let _ = self.enfant.wait();
        self.relances += 1;
        // Morceau sans image (que du son) : inutilisable dans un clip
        if self.fige {
            if let Some(p) = self.dernier_morceau.take() {
                let _ = fs::remove_file(p);
            }
        }
        self.fige = false;
        self.dernier_morceau = None;
        self.depuis = Instant::now();
        let mut derniere = String::new();
        for fenetre in sources(self.exe.is_some()) {
            let source = source_de(fenetre, self.exe.as_deref(), self.qualite.ips);
            match lancer_ffmpeg(stock, &source, &self.sons, self.encodeur, self.qualite, &self.dossier, self.base_us, &self.derniere_erreur) {
                Ok((enfant, son)) => {
                    self.enfant = enfant;
                    self._son = son;
                    self.fenetre = fenetre;
                    noter(stock, &format!("Capture : {}", if fenetre { "fenêtre du jeu" } else { "écran entier" }));
                    return Ok(());
                }
                Err(e) => derniere = e,
            }
        }
        Err(derniere)
    }
}

impl Drop for Enregistreur {
    fn drop(&mut self) {
        let _ = self.enfant.kill();
        let _ = self.enfant.wait();
        let _ = fs::remove_dir_all(&self.dossier);
    }
}

/// Sources essayées dans l'ordre : écran entier, puis fenêtre du jeu (si connue) en secours.
/// La capture de fenêtre ne donnait pas plus d'images avec R6 et perdait la fenêtre quand le jeu la recrée.
fn sources(avec_fenetre: bool) -> Vec<bool> {
    if avec_fenetre {
        vec![false, true]
    } else {
        vec![false]
    }
}

fn source_de(fenetre: bool, exe: Option<&str>, ips: u32) -> String {
    match (fenetre, exe) {
        (true, Some(e)) => source_fenetre(e, ips),
        _ => source_ecran(ips),
    }
}

/// Piste son d'un clip
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Piste {
    Pc,
    Discord,
    Micro,
}

impl Piste {
    pub fn titre(&self) -> &'static str {
        match self {
            Piste::Pc => "Jeu et PC",
            Piste::Discord => "Discord",
            Piste::Micro => "Micro",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sons {
    pub pistes: Vec<Piste>,
    /// Discord laissé hors du son du PC : sur sa propre piste, ou pas voulu du tout
    pub sans_discord: bool,
}

/// Pistes à enregistrer d'après le réglage (« pc », « discord », « micro »). Discord fermé : pas de piste Discord.
pub fn sons_choisis(choix: &[String], discord_ouvert: bool) -> Sons {
    let veut = |n: &str| choix.iter().any(|c| c == n);
    let mut pistes = Vec::new();
    if veut("pc") {
        pistes.push(Piste::Pc);
    }
    if veut("discord") && discord_ouvert {
        pistes.push(Piste::Discord);
    }
    if veut("micro") {
        pistes.push(Piste::Micro);
    }
    Sons { sans_discord: !veut("discord") || pistes.contains(&Piste::Discord), pistes }
}

static NUMERO_TUYAU: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Ouvre les sources de son des pistes. Une piste qui ne s'ouvre pas reste muette (mêmes pistes partout),
/// sauf s'il n'y en a qu'une : pas de son du tout, comme avant.
fn ouvrir_sons(stock: &Stockage, sons: &Sons) -> Vec<(crate::audio::Format, impl FnOnce(Box<dyn Write + Send>, Option<String>) -> crate::audio::Capture)> {
    use crate::audio::Source;
    let discord = if sons.sans_discord || sons.pistes.contains(&Piste::Discord) { crate::audio::pid_discord() } else { None };
    let mut ouvertes = Vec::new();
    for piste in &sons.pistes {
        let source = match piste {
            Piste::Pc => Source::Pc { sauf: if sons.sans_discord { discord } else { None } },
            Piste::Discord => discord.map_or(Source::Silence, Source::Programme),
            Piste::Micro => Source::Micro,
        };
        let mut o = crate::audio::ouvrir(source);
        if let (Err(e), Source::Pc { sauf: Some(_) }) = (&o, source) {
            noter(stock, &format!("Son du PC sans Discord impossible ({e}) : Discord reste dans le son du PC"));
            o = crate::audio::ouvrir(Source::Pc { sauf: None });
        }
        match o {
            Ok(o) => ouvertes.push(o),
            Err(e) => {
                noter(stock, &format!("Piste « {} » muette : {e}", piste.titre()));
                if sons.pistes.len() == 1 {
                    break;
                }
                match crate::audio::ouvrir(Source::Silence) {
                    Ok(o) => ouvertes.push(o),
                    Err(_) => return Vec::new(),
                }
            }
        }
    }
    ouvertes
}

#[allow(clippy::too_many_arguments)]
fn lancer_ffmpeg(
    stock: &Stockage,
    source: &str,
    sons: &Sons,
    encodeur: Encodeur,
    qualite: Qualite,
    dossier: &Path,
    base_us: i64,
    derniere_erreur: &Arc<Mutex<Option<String>>>,
) -> Result<(Child, Vec<crate::audio::Capture>), String> {
    let ouvertes = ouvrir_sons(stock, sons);
    // Première piste par stdin, les suivantes par des tuyaux nommés (FFmpeg les lit comme des fichiers)
    let mut entrees = Vec::new();
    let mut tuyaux = Vec::new();
    for (i, (f, _)) in ouvertes.iter().enumerate() {
        if i == 0 {
            entrees.push((*f, "pipe:0".to_string()));
        } else {
            let nom = format!(r"\\.\pipe\fps-cagoule-son-{}-{}", std::process::id(), NUMERO_TUYAU.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
            let t = crate::audio::Tuyau::creer(&nom).map_err(|e| format!("Tuyau du son : {e}"))?;
            tuyaux.push((t, nom.clone()));
            entrees.push((*f, nom));
        }
    }
    let args = arguments(source, &entrees, encodeur, qualite, base_us, dossier);
    let mut enfant = Command::new(ffmpeg(stock))
        .args(&args)
        .stdin(if ouvertes.is_empty() { Stdio::null() } else { Stdio::piped() })
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        // Priorité haute, comme le jeu : FPS Cagoule passe le jeu en priorité haute, et une capture restée en
        // « normale » passait après lui (une image sur deux ratée à 200 FPS). Elle prend très peu de processeur :
        // l'encodage se fait sur la puce vidéo de la carte graphique.
        .creation_flags(CREATE_NO_WINDOW | 0x0000_0080) // HIGH_PRIORITY_CLASS
        .spawn()
        .map_err(|e| format!("FFmpeg ne démarre pas : {e}"))?;
    crate::systeme::attacher_a_l_app(&enfant);
    let mut capture = Vec::new();
    let mut tuyaux = tuyaux.into_iter();
    for (i, (_, demarrer)) in ouvertes.into_iter().enumerate() {
        if i == 0 {
            if let Some(entree) = enfant.stdin.take() {
                capture.push(demarrer(Box::new(entree), None));
            }
        } else if let Some((t, nom)) = tuyaux.next() {
            capture.push(demarrer(Box::new(t), Some(nom)));
        }
    }
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
    // Comme OBS : priorité haute sur la carte graphique, sinon le jeu (200 FPS) la monopolise et la capture rate une
    // image sur deux. Demandée maintenant que FFmpeg utilise la carte : avant, Windows répond « paramètre invalide ».
    {
        let pid = enfant.id();
        let stock_journal = stock.dossier().to_path_buf();
        std::thread::spawn(move || {
            let mut code = None;
            for _ in 0..5 {
                code = crate::systeme::priorite_gpu_haute_pid(pid);
                if code == Some(0) {
                    break;
                }
                std::thread::sleep(Duration::from_secs(2));
            }
            noter_dans(&stock_journal, &format!("Priorité de la capture sur la carte graphique : {}", match code {
                Some(0) => "haute (acceptée)".to_string(),
                Some(c) => format!("refusée par Windows (code {:#x})", c as u32),
                None => "indisponible".to_string(),
            }));
        });
    }
    // Messages d'erreur lus au fil de l'eau (FFmpeg ne se bloque pas sur un tuyau plein) : on garde le dernier
    if let Some(e) = enfant.stderr.take() {
        let derniere = derniere_erreur.clone();
        std::thread::spawn(move || {
            use std::io::BufRead;
            for ligne in std::io::BufReader::new(e).lines().map_while(Result::ok) {
                if !ligne.trim().is_empty() {
                    *derniere.lock().unwrap() = Some(ligne.trim().chars().take(200).collect());
                }
            }
        });
    }
    Ok((enfant, capture))
}

/// Démarre l'enregistrement d'une partie (encodeur de la carte graphique, sinon le processeur)
pub fn demarrer(stock: &Stockage, jeu: &str, exe: Option<&str>) -> Result<Enregistreur, String> {
    let dossier = tampon(stock);
    let _ = fs::remove_dir_all(&dossier);
    fs::create_dir_all(&dossier).map_err(|e| e.to_string())?;
    let mut erreurs = Vec::new();
    let base_us = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_micros() as i64);
    let qualite = {
        let r = &stock.donnees.lock().unwrap().reglages;
        Qualite::depuis(&r.clips_qualite, r.clips_ips)
    };
    let derniere_erreur = Arc::new(Mutex::new(None));
    let choix = stock.donnees.lock().unwrap().reglages.clips_sons.clone();
    let sons = sons_choisis(&choix, crate::audio::pid_discord().is_some());
    // Noms des pistes, lus à la sauvegarde d'un clip
    let _ = fs::write(dossier.join("pistes.txt"), sons.pistes.iter().map(|p| p.titre()).collect::<Vec<_>>().join("\n"));
    noter(stock, &format!("Pistes son : {}", if sons.pistes.is_empty() { "aucune".to_string() } else { sons.pistes.iter().map(|p| p.titre()).collect::<Vec<_>>().join(", ") }));
    for encodeur in [encodeur_conseille(), Encodeur::Logiciel] {
        for fenetre in sources(exe.is_some()) {
            let source = source_de(fenetre, exe, qualite.ips);
            match lancer_ffmpeg(stock, &source, &sons, encodeur, qualite, &dossier, base_us, &derniere_erreur) {
                Ok((enfant, son)) => {
                    noter(stock, &format!("Capture : {}", if fenetre { "fenêtre du jeu" } else { "écran entier" }));
                    return Ok(Enregistreur {
                        enfant,
                        _son: son,
                        sons: sons.clone(),
                        dossier,
                        jeu: jeu.into(),
                        encodeur,
                        qualite,
                        base_us,
                        relances: 0,
                        dernier_morceau: None,
                        depuis: Instant::now(),
                        fige: false,
                        derniere_erreur,
                        raison: "",
                        exe: exe.map(String::from),
                        fenetre,
                        // Pas de nouvel essai de la fenêtre en cours de partie : l'écran entier est le choix voulu
                        fenetre_reessayee: true,
                    });
                }
                Err(e) => {
                    noter(stock, &format!("Capture {} refusée : {e}", if fenetre { "de la fenêtre" } else { "de l'écran" }));
                    erreurs.push(e)
                }
            }
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

/// Journal des clips (%APPDATA%\ch.lucas-chvnn.fps-cagoule\journal-clips.txt, bouton « Journal » de la page Clips) : replays trouvés et lus, clips auto faits ou ratés.
/// Petit fichier texte (gardé sous 300 Ko) pour comprendre un clip auto qui ne vient pas.
pub fn noter(stock: &Stockage, message: &str) {
    noter_dans(&stock.dossier(), message)
}

/// Comme `noter`, depuis un fil qui n'a que le dossier de l'app
pub fn noter_dans(dossier: &Path, message: &str) {
    let f = dossier.join("journal-clips.txt");
    if fs::metadata(&f).map(|m| m.len() > 300_000).unwrap_or(false) {
        let _ = fs::rename(&f, dossier.join("journal-clips.ancien.txt"));
    }
    if let Ok(mut fichier) = fs::OpenOptions::new().create(true).append(true).open(&f) {
        let _ = writeln!(fichier, "{} {message}", date_texte());
    }
}

/// Premier horodatage (90 kHz) de l'image et du son dans un flux MPEG-TS
pub fn premiers_pts(ts: &[u8]) -> (Option<u64>, Option<u64>) {
    let (mut video, mut son) = (None, None);
    for p in ts.chunks_exact(PAQUET_TS).take(200_000) {
        if p[0] != 0x47 || p[1] & 0x40 == 0 {
            continue; // pas un début de paquet PES
        }
        let mut debut = 4;
        if p[3] & 0x20 != 0 {
            debut += 1 + p[4] as usize;
        }
        let Some(pes) = p.get(debut..) else { continue };
        if pes.len() < 14 || pes[..3] != [0, 0, 1] || pes[7] & 0x80 == 0 {
            continue;
        }
        let b = &pes[9..14];
        let pts = (((b[0] as u64 >> 1) & 7) << 30) | ((b[1] as u64) << 22) | ((b[2] as u64 >> 1) << 15) | ((b[3] as u64) << 7) | (b[4] as u64 >> 1);
        match pes[3] {
            0xE0..=0xEF if video.is_none() => video = Some(pts),
            0xC0..=0xDF if son.is_none() => son = Some(pts),
            _ => {}
        }
        if video.is_some() && son.is_some() {
            break;
        }
    }
    (video, son)
}

/// Premier horodatage (90 kHz) de l'image et de chacune des `pistes` pistes son d'un flux MPEG-TS.
/// Toutes les pistes son ont le même numéro de flux PES (0xC0) : on les distingue par leur PID, attribués dans
/// l'ordre des pistes.
pub fn premiers_pts_pistes(ts: &[u8], pistes: usize) -> (Option<u64>, Vec<Option<u64>>) {
    let mut video = None;
    let mut sons: Vec<(u16, u64)> = Vec::new();
    for p in ts.chunks_exact(PAQUET_TS).take(400_000) {
        if p[0] != 0x47 || p[1] & 0x40 == 0 {
            continue;
        }
        let pid = (((p[1] & 0x1F) as u16) << 8) | p[2] as u16;
        let mut debut = 4;
        if p[3] & 0x20 != 0 {
            debut += 1 + p[4] as usize;
        }
        let Some(pes) = p.get(debut..) else { continue };
        if pes.len() < 14 || pes[..3] != [0, 0, 1] || pes[7] & 0x80 == 0 {
            continue;
        }
        let b = &pes[9..14];
        let pts = (((b[0] as u64 >> 1) & 7) << 30) | ((b[1] as u64) << 22) | ((b[2] as u64 >> 1) << 15) | ((b[3] as u64) << 7) | (b[4] as u64 >> 1);
        match pes[3] {
            0xE0..=0xEF if video.is_none() => video = Some(pts),
            0xC0..=0xDF if !sons.iter().any(|(q, _)| *q == pid) => sons.push((pid, pts)),
            _ => {}
        }
        if video.is_some() && sons.len() >= pistes {
            break;
        }
    }
    sons.sort_by_key(|(pid, _)| *pid);
    let mut r: Vec<Option<u64>> = sons.into_iter().map(|(_, t)| Some(t)).collect();
    r.resize(pistes, None);
    (video, r)
}

/// Mélange des pistes [s0], [s1]… : piste [mix] (volumes appliqués, sans saturer) et copies intactes [p0], [p1]…
pub fn filtre_mix(pistes: usize, volumes: &[f64]) -> String {
    let mut f = String::new();
    for i in 0..pistes {
        let v = volumes.get(i).copied().unwrap_or(1.0).clamp(0.0, 4.0);
        f.push_str(&format!("[s{i}]asplit=2[p{i}][q{i}];[q{i}]volume={v:.2}[r{i}];"));
    }
    for i in 0..pistes {
        f.push_str(&format!("[r{i}]"));
    }
    f.push_str(&format!("amix=inputs={pistes}:normalize=0:duration=longest,alimiter=limit=0.95:level=0:latency=1[mix]"));
    f
}

/// Réglages FFmpeg des pistes d'un clip à plusieurs sons : mélange d'abord (lu partout : galerie, Discord, Windows),
/// puis chaque son à part (pour régler les volumes dans l'éditeur). `volumes` en fraction, gardés dans le fichier.
fn sorties_pistes(cmd: &mut Command, titres: &[String], volumes: &[f64]) {
    cmd.args(["-map", "[mix]"]);
    for i in 0..titres.len() {
        cmd.args(["-map", &format!("[p{i}]")]);
    }
    cmd.args(["-c:a", "aac", "-b:a", "160k", "-metadata:s:a:0", "title=Tout", "-disposition:a:0", "default"]);
    for (i, t) in titres.iter().enumerate() {
        cmd.args([format!("-metadata:s:a:{}", i + 1), format!("title={t}"), format!("-disposition:a:{}", i + 1), "0".into()]);
    }
    let v: Vec<String> = (0..titres.len()).map(|i| format!("{}", (volumes.get(i).copied().unwrap_or(1.0) * 100.0).round() as i64)).collect();
    cmd.args(["-metadata".to_string(), format!("comment=sons:{}", v.join(","))]);
}

/// Réglage du son pour qu'il commence avec la première image (sinon décalé dans certains lecteurs, ex. celui de Windows).
/// `decalage` : avance du son sur l'image, en secondes (négatif = le son commence après).
pub fn alignement_son(decalage: f64) -> Option<String> {
    if decalage > 0.02 {
        Some(format!("atrim=start={decalage:.3}"))
    } else if decalage < -0.02 {
        Some("aresample=async=1:first_pts=0".into())
    } else {
        None
    }
}

/// Sauvegarde les `secondes` dernières secondes de la partie (en morceaux entiers : le clip commence sur une image
/// complète, le son aligné dessus). Renvoie le chemin du clip et sa durée réelle.
pub fn sauver(stock: &Stockage, dossier_tampon: &Path, jeu: &str, secondes: u32, titre: Option<&str>) -> Result<(PathBuf, f64), String> {
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
    // Chaque morceau commence par une image complète : pas de coupe au milieu, l'image démarre tout de suite.
    // Le son, lui, peut commencer un peu avant ou après : on l'aligne sur la première image.
    let titres: Vec<String> = fs::read_to_string(dossier_tampon.join("pistes.txt"))
        .map(|t| t.lines().filter(|l| !l.trim().is_empty()).map(String::from).collect())
        .unwrap_or_default();
    let octets = fs::read(&brut).unwrap_or_default();
    let (pts_image, pts_son) = premiers_pts(&octets);
    let (_, pts_pistes) = premiers_pts_pistes(&octets, titres.len());
    drop(octets);
    let mut cmd = Command::new(ffmpeg(stock));
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-i"]).arg(&brut).args(["-map", "0:v:0", "-c:v", "copy"]);
    if titres.len() >= 2 && pts_pistes.iter().all(|p| p.is_some()) {
        // Plusieurs sons : chacun aligné sur la première image, puis mélangés
        let mut graphe = String::new();
        for (i, pts) in pts_pistes.iter().enumerate() {
            let decalage = match (pts_image, pts) {
                (Some(v), Some(a)) => (v as f64 - *a as f64) / 90_000.0,
                _ => 0.0,
            };
            graphe.push_str(&format!("[0:a:{i}]{}[s{i}];", alignement_son(decalage).unwrap_or_else(|| "anull".into())));
        }
        graphe.push_str(&filtre_mix(titres.len(), &[]));
        cmd.args(["-filter_complex", &graphe]);
        sorties_pistes(&mut cmd, &titres, &[]);
    } else if pts_son.is_some() {
        cmd.args(["-map", "0:a:0"]);
        let decalage = match (pts_image, pts_son) {
            (Some(v), Some(a)) => (v as f64 - a as f64) / 90_000.0,
            _ => 0.0,
        };
        match alignement_son(decalage) {
            Some(filtre) => cmd.args(["-af", &filtre, "-c:a", "aac", "-b:a", "160k"]),
            None => cmd.args(["-c:a", "copy", "-bsf:a", "aac_adtstoasc"]),
        };
    }
    let statut = cmd
        .args(["-avoid_negative_ts", "make_zero", "-movflags", "+faststart"])
        .arg(&sortie)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| e.to_string())?;
    let _ = fs::remove_file(&brut);
    if !statut.success() || !sortie.is_file() {
        return Err("Le clip n'a pas pu être créé".into());
    }
    // Sans image (capture d'écran coupée par Windows) : pas de clip « que du son »
    let (image, duree) = infos_video(stock, &sortie);
    if !image {
        let _ = fs::remove_file(&sortie);
        return Err("l'image du jeu n'a pas été capturée (la capture redémarre, réessaie dans quelques secondes)".into());
    }
    miniature(stock, &sortie);
    Ok((sortie, if duree > 0.0 { duree } else { secondes as f64 }))
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

/// Repères rangés à côté du clip (dossier caché « .fps-cagoule »), pour des clips importés (Medal…)
/// ou venus d'un autre PC : ils suivent le clip sur le serveur
fn marqueurs_voisins(clip: &Path) -> PathBuf {
    let nom = clip.file_name().map(|n| format!("{}.json", n.to_string_lossy())).unwrap_or_default();
    clip.parent().unwrap_or(Path::new("")).join(".fps-cagoule").join(nom)
}

/// Repères du clip : ceux du PC ou ceux rangés à côté du clip, les plus récents l'emportent
/// (des repères corrigés posés à côté du clip remplacent d'anciens repères du PC)
pub fn marqueurs(stock: &Stockage, clip: &Path) -> Vec<Marqueur> {
    let lire = |f: &Path| {
        let date = fs::metadata(f).and_then(|m| m.modified()).ok()?;
        let liste = fs::read_to_string(f).ok().and_then(|t| serde_json::from_str::<Vec<Marqueur>>(&t).ok())?;
        Some((date, liste))
    };
    match (lire(&fichier_marqueurs(stock, clip)), lire(&marqueurs_voisins(clip))) {
        (Some((dl, l)), Some((dv, v))) => {
            if dv > dl {
                v
            } else {
                l
            }
        }
        (Some((_, l)), None) => l,
        (None, Some((_, v))) => v,
        (None, None) => vec![],
    }
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

/// Copie du clip pour le web (navigateurs, Discord). Un clip H.264 ou HEVC pas trop lourd (cas normal, Medal compris)
/// est simplement copié : d'un dossier du serveur à un autre, Windows fait faire la copie au serveur lui-même
/// (rien ne passe par le PC). Seul un clip trop lourd (Ultra 120 i/s…) est réencodé en H.264 ~8 Mb/s, sur le PC,
/// puis copié une fois.
pub fn version_web(stock: &Stockage, source: &Path, sortie: &Path, progression: impl Fn(f64)) -> Result<(), String> {
    let infos = Command::new(ffmpeg(stock)).arg("-i").arg(source).creation_flags(CREATE_NO_WINDOW).output().map_err(|e| e.to_string())?;
    let infos = String::from_utf8_lossy(&infos.stderr).to_string();
    let duree = infos
        .split("Duration: ")
        .nth(1)
        .and_then(|r| r.split(',').next())
        .and_then(|t| {
            let p: Vec<f64> = t.trim().split(':').filter_map(|x| x.parse().ok()).collect();
            (p.len() == 3).then(|| p[0] * 3600.0 + p[1] * 60.0 + p[2])
        })
        .unwrap_or(0.0);
    let debit_kbs: u64 = infos.split("bitrate: ").nth(1).and_then(|r| r.split_whitespace().next()).and_then(|n| n.parse().ok()).unwrap_or(0);
    let lisible = infos.contains("Video: h264") || infos.contains("Video: hevc");
    if lisible && debit_kbs > 0 && debit_kbs <= 25_000 {
        progression(0.5);
        fs::copy(source, sortie).map_err(|e| format!("Copie vers le serveur impossible : {e}"))?;
        progression(1.0);
        return Ok(());
    }
    // H.264 un peu lourd : piste vidéo gardée telle quelle ; sinon réencodé
    let copie = infos.contains("Video: h264") && debit_kbs > 0 && debit_kbs <= 16_000;
    let son = infos.contains("Audio:");
    let local = stock.dossier().join("partage-en-cours");
    let _ = fs::create_dir_all(&local);
    let temp = local.join(format!("{}.mp4", cle_clip(source, "tmp").trim_end_matches(".tmp")));
    let mut erreur = String::new();
    for encodeur in [encodeur_conseille(), Encodeur::Logiciel] {
        let video: Vec<&str> = match (copie, encodeur) {
            (true, _) => vec!["-c:v", "copy"],
            (_, Encodeur::Nvidia) => vec!["-c:v", "h264_nvenc", "-preset", "p5", "-rc", "vbr", "-cq", "26", "-b:v", "8M", "-maxrate", "12M", "-bufsize", "16M"],
            (_, Encodeur::Amd) => vec!["-c:v", "h264_amf", "-quality", "quality", "-rc", "vbr_peak", "-b:v", "8M", "-maxrate", "12M"],
            (_, Encodeur::Intel) => vec!["-c:v", "h264_qsv", "-preset", "medium", "-b:v", "8M", "-maxrate", "12M", "-pix_fmt", "nv12"],
            (_, Encodeur::Logiciel) => vec!["-c:v", "libx264", "-preset", "veryfast", "-crf", "23", "-maxrate", "10M", "-bufsize", "20M", "-pix_fmt", "yuv420p"],
        };
        let mut cmd = Command::new(ffmpeg(stock));
        cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-nostats", "-progress", "pipe:1", "-i"]).arg(source).args(["-map", "0:v:0"]);
        if son {
            cmd.args(["-map", "0:a:0", "-c:a", "aac", "-b:a", "160k"]);
        }
        let mut enfant = cmd
            .args(&video)
            .args(["-movflags", "+faststart"])
            .arg(&temp)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| e.to_string())?;
        crate::systeme::attacher_a_l_app(&enfant);
        if let Some(out) = enfant.stdout.take() {
            use std::io::BufRead;
            for ligne in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                if let Some(us) = ligne.strip_prefix("out_time_us=").and_then(|v| v.trim().parse::<f64>().ok()) {
                    if duree > 0.0 {
                        progression((us / 1e6 / duree * 0.9).clamp(0.0, 0.9));
                    }
                }
            }
        }
        let mut sortie_err = String::new();
        if let Some(mut e) = enfant.stderr.take() {
            let _ = e.read_to_string(&mut sortie_err);
        }
        let ok = enfant.wait().map(|s| s.success()).unwrap_or(false);
        if ok && temp.is_file() {
            progression(0.92);
            let r = fs::copy(&temp, sortie).map(|_| ()).map_err(|e| format!("Copie vers le serveur impossible : {e}"));
            let _ = fs::remove_file(&temp);
            if r.is_err() {
                let _ = fs::remove_file(sortie);
            }
            progression(1.0);
            return r;
        }
        let _ = fs::remove_file(&temp);
        erreur = sortie_err.lines().last().unwrap_or("").to_string();
        if copie || encodeur == Encodeur::Logiciel {
            break;
        }
    }
    Err(format!("Copie du clip impossible : {erreur}"))
}

/// Version légère d'un clip pour l'aperçu dans Discord : 720p, environ 75 Mo au plus (le débit dépend de la durée),
/// seulement le son « Tout ». Préparée sur le PC puis copiée vers `sortie`. `progression(0..1)`.
pub fn version_discord(stock: &Stockage, source: &Path, sortie: &Path, progression: impl Fn(f64)) -> Result<(), String> {
    let duree = infos_video(stock, source).1.max(1.0);
    let son = !pistes_son(stock, source).0.is_empty();
    let kbps = ((75.0 * 8.0 * 1024.0 / duree) as u64).saturating_sub(130).clamp(700, 6000);
    let (debit, maxi, tampon) = (format!("{kbps}k"), format!("{}k", kbps * 5 / 4), format!("{}k", kbps * 2));
    let local = stock.dossier().join("partage-en-cours");
    let _ = fs::create_dir_all(&local);
    let temp = local.join(format!("{}-discord.mp4", cle_clip(source, "tmp").trim_end_matches(".tmp")));
    let mut erreur = String::new();
    for encodeur in [encodeur_conseille(), Encodeur::Logiciel] {
        let video: Vec<&str> = match encodeur {
            Encodeur::Nvidia => vec!["-c:v", "h264_nvenc", "-preset", "p5", "-rc", "vbr", "-b:v", &debit, "-maxrate", &maxi, "-bufsize", &tampon],
            Encodeur::Amd => vec!["-c:v", "h264_amf", "-quality", "quality", "-rc", "vbr_peak", "-b:v", &debit, "-maxrate", &maxi],
            Encodeur::Intel => vec!["-c:v", "h264_qsv", "-preset", "medium", "-b:v", &debit, "-maxrate", &maxi, "-pix_fmt", "nv12"],
            Encodeur::Logiciel => vec!["-c:v", "libx264", "-preset", "veryfast", "-b:v", &debit, "-maxrate", &maxi, "-bufsize", &tampon, "-pix_fmt", "yuv420p"],
        };
        let mut cmd = Command::new(ffmpeg(stock));
        cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-nostats", "-progress", "pipe:1", "-i"]).arg(source).args(["-map", "0:v:0", "-vf", "scale=-2:720"]);
        if son {
            cmd.args(["-map", "0:a:0", "-c:a", "aac", "-b:a", "128k"]);
        }
        let mut enfant = cmd
            .args(&video)
            .args(["-movflags", "+faststart"])
            .arg(&temp)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| e.to_string())?;
        crate::systeme::attacher_a_l_app(&enfant);
        if let Some(out) = enfant.stdout.take() {
            use std::io::BufRead;
            for ligne in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                if let Some(us) = ligne.strip_prefix("out_time_us=").and_then(|v| v.trim().parse::<f64>().ok()) {
                    progression((us / 1e6 / duree * 0.9).clamp(0.0, 0.9));
                }
            }
        }
        let mut sortie_err = String::new();
        if let Some(mut e) = enfant.stderr.take() {
            let _ = e.read_to_string(&mut sortie_err);
        }
        let ok = enfant.wait().map(|s| s.success()).unwrap_or(false);
        if ok && temp.is_file() {
            let r = fs::copy(&temp, sortie).map(|_| ()).map_err(|e| format!("Copie vers le serveur impossible : {e}"));
            let _ = fs::remove_file(&temp);
            if r.is_err() {
                let _ = fs::remove_file(sortie);
            }
            progression(1.0);
            return r;
        }
        let _ = fs::remove_file(&temp);
        erreur = sortie_err.lines().last().unwrap_or("").to_string();
    }
    Err(format!("Version légère impossible : {erreur}"))
}

/// Grande image du clip pour l'aperçu des liens (Discord…)
pub fn image_partage(stock: &Stockage, source: &Path, sortie: &Path) {
    let _ = Command::new(ffmpeg(stock))
        .args(["-hide_banner", "-loglevel", "error", "-y", "-ss", "1", "-i"])
        .arg(source)
        .args(["-frames:v", "1", "-vf", "scale=1280:-2", "-q:v", "3"])
        .arg(sortie)
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
    pub marqueurs: Vec<Marqueur>,
    /// Lien public si le clip est partagé
    pub lien: Option<String>,
}

/// Nom montré dans la galerie : sans le « (2) » ajouté pour que deux clips du même nom puissent coexister
pub fn nom_affiche(nom: &str) -> String {
    if let Some(debut) = nom.strip_suffix(')').and_then(|n| n.rfind(" (").map(|i| (i, n))) {
        let (i, n) = debut;
        if n[i + 2..].chars().all(|c| c.is_ascii_digit()) && i + 2 < n.len() {
            return n[..i].to_string();
        }
    }
    nom.to_string()
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
                nom: p.file_stem().map(|s| nom_affiche(&s.to_string_lossy())).unwrap_or_default(),
                jeu,
                date: m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs()),
                octets: m.len(),
                miniature: image.map(|o| format!("data:image/jpeg;base64,{}", crate::ps::base64(&o))),
                marqueurs: marqueurs(stock, &p),
                lien: crate::liens::lien(stock, &p),
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
    let _ = fs::remove_file(marqueurs_voisins(&p));
    fs::remove_file(p).map_err(|e| e.to_string())
}

/// Le clip a-t-il une piste vidéo
/// Le clip a-t-il une piste vidéo, et sa durée (s)
fn infos_video(stock: &Stockage, clip: &Path) -> (bool, f64) {
    let Ok(o) = Command::new(ffmpeg(stock)).arg("-i").arg(clip).creation_flags(CREATE_NO_WINDOW).output() else { return (false, 0.0) };
    let t = String::from_utf8_lossy(&o.stderr);
    let duree = t
        .split("Duration: ")
        .nth(1)
        .and_then(|r| r.split(',').next())
        .and_then(|d| {
            let p: Vec<f64> = d.trim().split(':').filter_map(|x| x.parse().ok()).collect();
            (p.len() == 3).then(|| p[0] * 3600.0 + p[1] * 60.0 + p[2])
        })
        .unwrap_or(0.0);
    (t.contains("Video:"), duree)
}

/// Pistes son décrites par FFmpeg (`ffmpeg -i`) : nom de chaque piste (vide s'il n'en a pas), et volumes gardés
/// dans le fichier (« sons:100,50,… », en fraction). Clip à plusieurs sons : « Tout » d'abord, puis chaque son.
pub fn lire_pistes(infos: &str) -> (Vec<String>, Vec<f64>) {
    let mut titres: Vec<String> = Vec::new();
    let mut volumes = Vec::new();
    let mut dans_son = false;
    for ligne in infos.lines() {
        let t = ligne.trim();
        if t.starts_with("Stream #") {
            dans_son = t.contains(": Audio:");
            if dans_son {
                titres.push(String::new());
            }
            continue;
        }
        let Some((cle, valeur)) = t.split_once(':') else { continue };
        let (cle, valeur) = (cle.trim(), valeur.trim());
        if dans_son && (cle == "title" || cle == "name") {
            if let Some(d) = titres.last_mut().filter(|d| d.is_empty()) {
                *d = valeur.to_string();
            }
        }
        if cle == "comment" {
            if let Some(v) = valeur.strip_prefix("sons:") {
                volumes = v.split(',').filter_map(|x| x.trim().parse::<f64>().ok()).map(|x| x / 100.0).collect();
            }
        }
    }
    (titres, volumes)
}

/// Pistes son d'un clip et volumes gardés (le PC peut ne pas avoir de sortie audio : aucune piste)
pub fn pistes_son(stock: &Stockage, clip: &Path) -> (Vec<String>, Vec<f64>) {
    Command::new(ffmpeg(stock))
        .arg("-i")
        .arg(clip)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| lire_pistes(&String::from_utf8_lossy(&o.stderr)))
        .unwrap_or_default()
}

/// Sons séparés d'un clip (sans la piste « Tout ») : vide s'il n'a qu'un son
fn sons_separes(titres: &[String]) -> Vec<String> {
    if titres.len() >= 3 {
        titres[1..].to_vec()
    } else {
        Vec::new()
    }
}

/// Sons séparés d'un clip, copiés à part pour les écouter dans l'éditeur avec chacun son volume.
/// Renvoie leurs noms, leurs fichiers et les volumes gardés dans le clip.
pub fn extraire_pistes(stock: &Stockage, chemin: &str) -> Result<(Vec<String>, Vec<PathBuf>, Vec<f64>), String> {
    let source = clip_valide(stock, chemin)?;
    let (titres, volumes) = pistes_son(stock, &source);
    let separes = sons_separes(&titres);
    if separes.is_empty() {
        return Ok((vec![], vec![], vec![]));
    }
    let dossier = stock.dossier().join("pistes");
    // Pistes d'un clip ouvert avant : effacées (celles encore lues sont laissées)
    if let Ok(l) = fs::read_dir(&dossier) {
        for e in l.flatten() {
            let _ = fs::remove_file(e.path());
        }
    }
    fs::create_dir_all(&dossier).map_err(|e| e.to_string())?;
    let id = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let fichiers: Vec<PathBuf> = (1..=separes.len()).map(|k| dossier.join(format!("{id}-{k}.m4a"))).collect();
    let mut cmd = Command::new(ffmpeg(stock));
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-i"]).arg(&source);
    for (k, f) in fichiers.iter().enumerate() {
        cmd.args(["-map", &format!("0:a:{}", k + 1), "-c", "copy", "-vn"]).arg(f);
    }
    let ok = cmd.creation_flags(CREATE_NO_WINDOW).status().map(|s| s.success()).unwrap_or(false);
    if !ok || fichiers.iter().any(|f| !f.is_file()) {
        return Err("Sons du clip illisibles".into());
    }
    let mut v = volumes;
    v.resize(separes.len(), 1.0);
    Ok((separes, fichiers, v))
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

/// Comme `filtre_montage`, pour un clip à plusieurs sons : chaque son est coupé pareil, puis mélangé
/// avec ses `volumes` ([v], [mix], et chaque son [p0], [p1]…)
pub fn filtre_montage_pistes(morceaux: &[(f64, f64)], pistes: usize, volumes: &[f64]) -> String {
    let mut f = String::new();
    for (i, (a, b)) in morceaux.iter().enumerate() {
        f.push_str(&format!("[0:v]trim=start={a:.3}:end={b:.3},setpts=PTS-STARTPTS[v{i}];"));
        for k in 0..pistes {
            f.push_str(&format!("[0:a:{}]atrim=start={a:.3}:end={b:.3},asetpts=PTS-STARTPTS[a{i}_{k}];", k + 1));
        }
    }
    for i in 0..morceaux.len() {
        f.push_str(&format!("[v{i}]"));
        for k in 0..pistes {
            f.push_str(&format!("[a{i}_{k}]"));
        }
    }
    f.push_str(&format!("concat=n={}:v=1:a={pistes}[v]", morceaux.len()));
    for k in 0..pistes {
        f.push_str(&format!("[s{k}]"));
    }
    f.push(';');
    f.push_str(&filtre_mix(pistes, volumes));
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
/// Exporte le montage. `remplacer` : le montage prend la place du clip d'origine (même nom, même date)
/// `volumes` : volume de chaque son séparé (clip à plusieurs sons), en fraction ; sinon ceux gardés dans le clip.
pub fn exporter(stock: &Stockage, chemin: &str, morceaux: &[(f64, f64)], remplacer: bool, volumes: Option<&[f64]>, progression: impl Fn(f64)) -> Result<PathBuf, String> {
    let source = clip_valide(stock, chemin)?;
    let mut m: Vec<(f64, f64)> = morceaux.iter().copied().filter(|(a, b)| b - a >= 0.2 && *a >= 0.0).collect();
    m.sort_by(|x, y| x.0.total_cmp(&y.0));
    if m.is_empty() {
        return Err("Garde au moins un passage".into());
    }
    let total: f64 = m.iter().map(|(a, b)| b - a).sum();
    let (titres, volumes_clip) = pistes_son(stock, &source);
    let son = !titres.is_empty();
    let separes = sons_separes(&titres);
    let volumes: Vec<f64> = volumes.map(|v| v.to_vec()).unwrap_or(volumes_clip);
    // Clip gardé en entier, seuls les volumes changent : l'image est recopiée telle quelle (quelques secondes)
    let duree = infos_video(stock, &source).1;
    let entier = !separes.is_empty() && m.len() == 1 && m[0].0 <= 0.05 && duree > 0.0 && m[0].1 >= duree - 0.05;
    let nom = source.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "clip".into());
    let dossier = source.parent().ok_or("Dossier introuvable")?;
    let mut sortie = dossier.join(format!("{nom} (montage).mp4"));
    let mut n = 2;
    while sortie.exists() {
        sortie = dossier.join(format!("{nom} (montage {n}).mp4"));
        n += 1;
    }
    let mut derniere_erreur = String::new();
    let encodeurs: Vec<Option<Encodeur>> = if entier { vec![None] } else { vec![Some(encodeur_conseille()), Some(Encodeur::Logiciel)] };
    for encodeur in encodeurs {
        let video: Vec<&str> = match encodeur {
            None => vec!["-c:v", "copy"],
            // Débit plafonné (~12 Mb/s, 20 au plus) : sinon un clip HEVC de Medal devient un fichier de 1 Go
            Some(Encodeur::Nvidia) => vec!["-c:v", "h264_nvenc", "-preset", "p5", "-rc", "vbr", "-cq", "22", "-b:v", "12M", "-maxrate", "20M", "-bufsize", "30M"],
            Some(Encodeur::Amd) => vec!["-c:v", "h264_amf", "-quality", "quality", "-rc", "vbr_peak", "-b:v", "12M", "-maxrate", "20M"],
            Some(Encodeur::Intel) => vec!["-c:v", "h264_qsv", "-preset", "medium", "-b:v", "12M", "-maxrate", "20M", "-pix_fmt", "nv12"],
            Some(Encodeur::Logiciel) => vec!["-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-maxrate", "20M", "-bufsize", "30M", "-pix_fmt", "yuv420p"],
        };
        let mut cmd = Command::new(ffmpeg(stock));
        cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-nostats", "-progress", "pipe:1", "-i"]).arg(&source);
        if entier {
            // Mêmes sons, nouveaux volumes
            let mut graphe: String = (0..separes.len()).map(|k| format!("[0:a:{}]anull[s{k}];", k + 1)).collect();
            graphe.push_str(&filtre_mix(separes.len(), &volumes));
            cmd.args(["-filter_complex", &graphe, "-map", "0:v:0"]);
            sorties_pistes(&mut cmd, &separes, &volumes);
        } else if !separes.is_empty() {
            cmd.args(["-filter_complex", &filtre_montage_pistes(&m, separes.len(), &volumes), "-map", "[v]"]);
            sorties_pistes(&mut cmd, &separes, &volumes);
        } else {
            cmd.args(["-filter_complex", &filtre_montage(&m, son), "-map", "[v]"]);
            if son {
                cmd.args(["-map", "[a]", "-c:a", "aac", "-b:a", "160k"]);
            }
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
        crate::systeme::attacher_a_l_app(&enfant);
        // Progression : FFmpeg écrit « out_time_us=… » sur sa sortie
        if let Some(out) = enfant.stdout.take() {
            use std::io::BufRead;
            for ligne in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                if let Some(us) = ligne.strip_prefix("out_time_us=").and_then(|v| v.trim().parse::<f64>().ok()) {
                    progression((us / 1e6 / total.max(0.1)).clamp(0.0, 1.0));
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
            let reperes: Vec<Marqueur> = marqueurs(stock, &source)
                .into_iter()
                .filter_map(|mut r| deplacer_repere(r.t, &m).map(|t| {
                    r.t = t;
                    r
                }))
                .collect();
            let fin = if remplacer { remplacer_original(stock, &source, &sortie) } else { sortie };
            miniature(stock, &fin);
            ajouter_marqueurs(stock, &fin, &reperes);
            return Ok(fin);
        }
        let _ = fs::remove_file(&sortie);
        derniere_erreur = erreur.lines().last().unwrap_or("").to_string();
    }
    Err(format!("Export impossible : {derniere_erreur}"))
}

/// Met le montage à la place du clip d'origine (même nom, même date dans la galerie).
/// Si Windows refuse (clip encore ouvert…), le montage reste à côté et l'original n'est pas touché.
fn remplacer_original(stock: &Stockage, original: &Path, montage: &Path) -> PathBuf {
    let date = fs::metadata(original).and_then(|m| m.modified()).ok();
    if fs::rename(montage, original).is_err() {
        return montage.to_path_buf();
    }
    let _ = fs::remove_file(fichier_marqueurs(stock, original));
    let _ = fs::remove_file(marqueurs_voisins(original));
    let _ = fs::remove_file(miniatures(stock).join(cle_miniature(original)));
    if let (Some(d), Ok(f)) = (date, fs::File::options().write(true).open(original)) {
        let _ = f.set_modified(d);
    }
    original.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trois sons (PC, Discord, micro) : stdin et deux tuyaux nommés vers FFmpeg, puis clip « Tout » + 3 sons séparés,
    /// volumes changés sans recoder l'image, montage, et sons copiés pour l'éditeur
    fn trois_pistes(stock: &Stockage, dossier: &Path) {
        fs::create_dir_all(dossier).unwrap();
        fs::write(dossier.join("pistes.txt"), "Jeu et PC\nDiscord\nMicro").unwrap();
        let f = crate::audio::Format { frequence: 48000, canaux: 2, flottant: false };
        let (n1, n2) = (format!(r"\\.\pipe\fps-cagoule-test-{}-1", std::process::id()), format!(r"\\.\pipe\fps-cagoule-test-{}-2", std::process::id()));
        let t1 = crate::audio::Tuyau::creer(&n1).expect("tuyau 1");
        let t2 = crate::audio::Tuyau::creer(&n2).expect("tuyau 2");
        let base = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_micros() as i64;
        let args = arguments("testsrc2=size=640x360:rate=60,realtime", &[(f, "pipe:0".into()), (f, n1), (f, n2)], Encodeur::Logiciel, Qualite::depuis("haute", 60), base, dossier);
        let mut enfant = Command::new(ffmpeg(stock)).args(&args).stdin(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let entree: Box<dyn Write + Send> = Box::new(enfant.stdin.take().unwrap());
        // Chaque piste : un son différent (440, 660, 880 Hz), 10 ms à la fois, au rythme réel
        let fils: Vec<_> = [(entree, 440.0), (Box::new(t1) as Box<dyn Write + Send>, 660.0), (Box::new(t2) as Box<dyn Write + Send>, 880.0)]
            .into_iter()
            .map(|(mut sortie, hz): (Box<dyn Write + Send>, f64)| {
                std::thread::spawn(move || {
                    let debut = Instant::now();
                    let mut n: u64 = 0;
                    while debut.elapsed() < Duration::from_secs(9) {
                        let mut bloc = Vec::with_capacity(480 * 4);
                        for _ in 0..480 {
                            let v = ((n as f64 * hz * std::f64::consts::TAU / 48000.0).sin() * 8000.0) as i16;
                            bloc.extend_from_slice(&v.to_le_bytes());
                            bloc.extend_from_slice(&v.to_le_bytes());
                            n += 1;
                        }
                        if sortie.write_all(&bloc).is_err() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    sortie
                })
            })
            .collect();
        // Sons gardés ouverts jusqu'à la sauvegarde, comme pendant une vraie partie
        let sorties: Vec<_> = fils.into_iter().map(|f| f.join().unwrap()).collect();
        let clip = sauver(stock, dossier, "Test pistes", 4, None);
        drop(sorties);
        let _ = enfant.kill();
        let mut erreur = String::new();
        let _ = enfant.stderr.take().unwrap().read_to_string(&mut erreur);
        let (clip, _) = clip.unwrap_or_else(|e| panic!("{e} / FFmpeg : {erreur}"));
        let (titres, volumes) = pistes_son(stock, &clip);
        println!("Pistes du clip : {titres:?} {volumes:?}");
        assert_eq!(titres, vec!["Tout", "Jeu et PC", "Discord", "Micro"]);
        // Les trois sons sont bien là (pas du silence) : niveau de chaque piste
        for k in 0..4 {
            let o = Command::new(ffmpeg(stock)).arg("-i").arg(&clip).args(["-map", &format!("0:a:{k}"), "-af", "volumedetect", "-f", "null", "-"]).output().unwrap();
            let t = String::from_utf8_lossy(&o.stderr).to_string();
            let niveau = t.split("mean_volume: ").nth(1).and_then(|r| r.split(' ').next()).and_then(|v| v.parse::<f64>().ok()).unwrap_or(-99.0);
            println!("Piste {k} : {niveau} dB");
            assert!(niveau > -40.0, "piste {k} muette : {t}");
        }
        // Discord coupé, micro à moitié : seule la piste « Tout » change, l'image est recopiée
        let duree = infos_video(stock, &clip).1;
        let r = exporter(stock, &clip.to_string_lossy(), &[(0.0, duree)], true, Some(&[1.0, 0.0, 0.5]), |_| {}).expect("nouveaux volumes");
        assert_eq!(r, clip);
        let (titres, volumes) = pistes_son(stock, &clip);
        assert_eq!((titres.len(), volumes), (4, vec![1.0, 0.0, 0.5]));
        // Montage : les sons séparés restent
        let m = exporter(stock, &clip.to_string_lossy(), &[(0.0, 1.0), (2.0, 3.0)], false, None, |_| {}).expect("montage à plusieurs sons");
        assert_eq!(pistes_son(stock, &m), (vec!["Tout".to_string(), "Jeu et PC".into(), "Discord".into(), "Micro".into()], vec![1.0, 0.0, 0.5]));
        let leger = dossier.join("leger.mp4");
        version_discord(stock, &clip, &leger, |_| {}).expect("version légère pour Discord");
        let (titres_leger, _) = pistes_son(stock, &leger);
        assert_eq!(titres_leger.len(), 1, "une seule piste son dans la version légère");
        assert!(fs::metadata(&leger).unwrap().len() < crate::liens::LIMITE_DISCORD);
        let _ = fs::remove_file(&leger);
        let (separes, fichiers, v) = extraire_pistes(stock, &m.to_string_lossy()).expect("sons pour l'éditeur");
        assert_eq!((separes.len(), fichiers.iter().filter(|f| f.is_file()).count(), v.len()), (3, 3, 3));
        supprimer(stock, &m.to_string_lossy()).unwrap();
        supprimer(stock, &clip.to_string_lossy()).unwrap();
        let _ = fs::remove_dir_all(dossier);
    }

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
        // « realtime » : la mire avance au rythme réel, comme une vraie capture
        let args = arguments("testsrc2=size=640x360:rate=60,realtime", &[(format, "pipe:0".into())], Encodeur::Logiciel, Qualite::depuis("haute", 60), base, &dossier);
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
        let (clip, duree) = clip.unwrap_or_else(|e| panic!("{e} / FFmpeg : {erreur}"));
        // Morceaux entiers : au moins les 4 s demandées, au plus un morceau de plus
        assert!((3.5..=8.5).contains(&duree), "durée {duree}");
        // FFmpeg décrit le clip : il faut une vidéo et un son
        let sortie = Command::new(ffmpeg(&stock)).arg("-i").arg(&clip).output().unwrap();
        let infos = String::from_utf8_lossy(&sortie.stderr).to_string();
        println!("{}", infos.lines().filter(|l| l.contains("Duration") || l.contains("Stream")).collect::<Vec<_>>().join("\n"));
        assert!(infos.contains("Video: h264") && infos.contains("Audio: aac"), "{infos}");
        assert!(liste(&stock, || {}).iter().any(|c| Path::new(&c.chemin) == clip && c.miniature.is_some()), "miniature absente");
        // Montage : deux passages gardés, repère déplacé
        ajouter_marqueurs(&stock, &clip, &[Marqueur { t: 3.0, genre: "kill".into(), detail: "x".into(), headshot: true }]);
        let montage = exporter(&stock, &clip.to_string_lossy(), &[(0.0, 1.0), (2.5, 4.0)], false, None, |_| {}).expect("export du montage");
        assert_eq!(marqueurs(&stock, &montage).first().map(|m| m.t), Some(1.5));
        // Montage qui remplace le clip : même chemin, plus de « (montage 2) » à côté
        let remplace = exporter(&stock, &montage.to_string_lossy(), &[(0.0, 1.0)], true, None, |_| {}).expect("montage à la place");
        assert_eq!(remplace, montage);
        assert!(!montage.with_file_name(format!("{} (montage).mp4", montage.file_stem().unwrap().to_string_lossy())).exists());
        supprimer(&stock, &montage.to_string_lossy()).unwrap();
        supprimer(&stock, &clip.to_string_lossy()).unwrap();
        let _ = fs::remove_dir_all(&dossier);
        trois_pistes(&stock, &dossier);
        // Méthodes de capture disponibles dans ce FFmpeg (ddagrab = Desktop Duplication, gfxcapture = Windows Graphics Capture)
        let filtres = Command::new(ffmpeg(&stock)).args(["-hide_banner", "-filters"]).output().unwrap();
        let filtres = String::from_utf8_lossy(&filtres.stdout);
        println!("ddagrab : {}, gfxcapture : {}", filtres.contains("ddagrab"), filtres.contains("gfxcapture"));
        if filtres.contains("gfxcapture") {
            let aide = Command::new(ffmpeg(&stock)).args(["-hide_banner", "-h", "filter=gfxcapture"]).output().unwrap();
            println!("{}", String::from_utf8_lossy(&aide.stdout));
        }
        // Capture de fenêtre : la syntaxe doit passer (sur ce PC, pas de jeu : on vise l'Explorateur et une fenêtre absente)
        for exe in ["explorer.exe", "RainbowSix.exe"] {
            let source = source_fenetre(exe, 30);
            let r = Command::new(ffmpeg(&stock)).args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", &source, "-t", "2", "-f", "null", "-"]).output().unwrap();
            let err = String::from_utf8_lossy(&r.stderr);
            println!("Capture de fenêtre {exe} ({source}) : {} {}", r.status, err.lines().last().unwrap_or(""));
            assert!(!err.contains("Option not found") && !err.contains("Error parsing") && !err.contains("No such filter"), "{err}");
        }
        // Vraie capture de l'écran : le PC de GitHub n'a pas forcément de bureau utilisable, on affiche seulement le résultat
        match demarrer(&stock, "Test écran", None) {
            Ok(e) => {
                std::thread::sleep(Duration::from_secs(5));
                println!("Capture de l'écran : OK ({:?}), morceaux : {}", e.encodeur, morceaux_recents(&e.dossier, 30).len());
            }
            Err(e) => println!("Capture de l'écran indisponible sur ce PC : {e}"),
        }
    }

    #[test]
    fn reperes_a_cote_du_clip() {
        let d = std::env::temp_dir().join("fps-cagoule-test-voisins");
        let _ = fs::remove_dir_all(&d);
        let clip = d.join("Rainbow Six Siege").join("3K · Kaid · Oregon.mp4");
        fs::create_dir_all(clip.parent().unwrap().join(".fps-cagoule")).unwrap();
        fs::write(&clip, b"x").unwrap();
        fs::write(marqueurs_voisins(&clip), r#"[{"t":4.5,"genre":"kill","detail":"x","headshot":true}]"#).unwrap();
        let stock = Stockage::ouvrir(d.join("donnees"));
        assert_eq!(marqueurs(&stock, &clip).len(), 1);
        // Le dossier caché n'est pas pris pour un jeu
        assert_eq!(videos(&d).len(), 1);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn son_aligne_sur_l_image() {
        // Paquet TS minimal : début de PES vidéo (0xE0) avec PTS = 90 000 (1 s), puis son (0xC0) à 0,9 s
        fn paquet(flux: u8, pts: u64) -> Vec<u8> {
            let mut p = vec![0x47, 0x40, 0x11, 0x10, 0, 0, 1, flux, 0, 0, 0x80, 0x80, 5];
            p.extend([0x21 | ((pts >> 29) & 0x0E) as u8, (pts >> 22) as u8, 0x01 | ((pts >> 14) & 0xFE) as u8, (pts >> 7) as u8, 0x01 | ((pts << 1) & 0xFE) as u8]);
            p.resize(PAQUET_TS, 0xFF);
            p
        }
        let mut ts = paquet(0xE0, 90_000);
        ts.extend(paquet(0xC0, 81_000));
        assert_eq!(premiers_pts(&ts), (Some(90_000), Some(81_000)));
        // Son 0,1 s en avance : on en coupe le début ; en retard : on complète avec du silence ; sinon rien
        assert_eq!(alignement_son(0.1).as_deref(), Some("atrim=start=0.100"));
        assert_eq!(alignement_son(-0.3).as_deref(), Some("aresample=async=1:first_pts=0"));
        assert_eq!(alignement_son(0.005), None);
    }

    #[test]
    fn noms_sans_numero_de_doublon() {
        assert_eq!(nom_affiche("4K · Kali · Kanal (5)"), "4K · Kali · Kanal");
        assert_eq!(nom_affiche("4K · Kali · Kanal"), "4K · Kali · Kanal");
        assert_eq!(nom_affiche("Partie (finale)"), "Partie (finale)");
        assert_eq!(nom_affiche("Clip ()"), "Clip ()");
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
    fn pistes_choisies() {
        let c = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // Par défaut : son du PC et Discord à part
        assert_eq!(sons_choisis(&c(&["pc", "discord"]), true), Sons { pistes: vec![Piste::Pc, Piste::Discord], sans_discord: true });
        // Discord fermé : pas de piste Discord, et rien à retirer du son du PC
        assert_eq!(sons_choisis(&c(&["pc", "discord"]), false), Sons { pistes: vec![Piste::Pc], sans_discord: false });
        // Discord pas voulu : retiré du son du PC
        assert_eq!(sons_choisis(&c(&["pc", "micro"]), true), Sons { pistes: vec![Piste::Pc, Piste::Micro], sans_discord: true });
        assert!(sons_choisis(&[], true).pistes.is_empty());
    }

    #[test]
    fn lecture_des_pistes() {
        let infos = "  Metadata:\n    comment         : sons:100,50,0\n  Duration: 00:00:06.06\n  Stream #0:0[0x1](und): Video: h264\n    Metadata:\n      handler_name    : VideoHandler\n  Stream #0:1[0x2](und): Audio: aac\n    Metadata:\n      handler_name    : SoundHandler\n      name            : Tout\n  Stream #0:2[0x3](und): Audio: aac\n    Metadata:\n      name            : Jeu et PC\n  Stream #0:3[0x4](und): Audio: aac\n    Metadata:\n      title           : Discord\n  Stream #0:4[0x5](und): Audio: aac\n";
        let (titres, volumes) = lire_pistes(infos);
        assert_eq!(titres, vec!["Tout", "Jeu et PC", "Discord", ""]);
        assert_eq!(volumes, vec![1.0, 0.5, 0.0]);
        assert_eq!(sons_separes(&titres), vec!["Jeu et PC", "Discord", ""]);
        // Ancien clip : une seule piste, pas de sons séparés
        assert!(sons_separes(&lire_pistes("  Stream #0:1: Audio: aac\n").0).is_empty());
    }

    #[test]
    fn filtres_des_pistes() {
        assert_eq!(
            filtre_mix(2, &[1.0, 0.5]),
            "[s0]asplit=2[p0][q0];[q0]volume=1.00[r0];[s1]asplit=2[p1][q1];[q1]volume=0.50[r1];[r0][r1]amix=inputs=2:normalize=0:duration=longest,alimiter=limit=0.95:level=0:latency=1[mix]"
        );
        let f = filtre_montage_pistes(&[(0.0, 5.0), (8.0, 10.0)], 2, &[]);
        assert!(f.starts_with("[0:v]trim=start=0.000:end=5.000,setpts=PTS-STARTPTS[v0];[0:a:1]atrim=start=0.000:end=5.000,asetpts=PTS-STARTPTS[a0_0];[0:a:2]atrim"));
        assert!(f.contains("[v0][a0_0][a0_1][v1][a1_0][a1_1]concat=n=2:v=1:a=2[v][s0][s1];[s0]asplit"));
        // Trois sources : stdin puis deux tuyaux nommés, l'image en dernier
        let fm = crate::audio::Format { frequence: 48000, canaux: 2, flottant: false };
        let a = arguments(&source_ecran(60), &[(fm, "pipe:0".into()), (fm, r"\\.\pipe\a".into()), (fm, r"\\.\pipe\b".into())], Encodeur::Nvidia, Qualite::depuis("haute", 60), 0, Path::new(r"C:\t")).join(" ");
        assert!(a.contains(r"-f s16le -ar 48000 -ac 2 -i \\.\pipe\b"));
        assert!(a.contains("-map 3:v -filter:v setpts") && a.contains("-map 0:a -map 1:a -map 2:a -filter:a asetpts="));
    }

    #[test]
    fn arguments_nvidia_avec_son() {
        let f = crate::audio::Format { frequence: 48000, canaux: 2, flottant: true };
        let a = arguments(&source_ecran(60), &[(f, "pipe:0".into())], Encodeur::Nvidia, Qualite::depuis("ultra", 120), 1_000_000, Path::new(r"C:\t"));
        let texte = a.join(" ");
        assert!(texte.contains("-f f32le -ar 48000 -ac 2 -i pipe:0"));
        assert!(texte.contains("-map 1:v -filter:v setpts=PTS-(1000000/1000000)/TB") && texte.contains("-map 0:a -filter:a asetpts='st(1,PTS-N/SR/TB)"));
        assert!(!texte.contains("aresample=async=1000"));
        assert!(!texte.contains("filter_complex"));
        assert!(texte.contains("-use_wallclock_as_timestamps 1 -f lavfi") && texte.contains("-fps_mode vfr"));
        assert!(texte.contains("h264_nvenc") && texte.contains("-segment_wrap 124"));
        // 120 i/s demandé par un ancien réglage : ramené à 60
        assert!(texte.contains("framerate=60") && texte.contains("-b:v 40M") && texte.contains("-g 120"));
        // Réglages « qualité » : profil High, images B, débit max 1,5 fois le débit visé
        assert_eq!(source_fenetre("RainbowSix.exe", 60), "gfxcapture=window_exe=(?i)rainbowsix.exe$:window_class=R6Game:max_framerate=60:capture_cursor=0:display_border=0");
        assert_eq!(source_fenetre("FortniteClient-Win64-Shipping.exe", 120), "gfxcapture=window_exe=(?i)fortniteclient-win64-shipping.exe$:max_framerate=120:capture_cursor=0:display_border=0");
        assert!(texte.contains("-profile:v high") && texte.contains("-tune hq") && texte.contains("-bf 2") && texte.contains("-maxrate 60M"));
        assert!(!texte.contains("-tune ll") && !texte.contains("cbr"));
        assert_eq!(Qualite::depuis("standard", 60), Qualite { debit_mbps: 12, ips: 60, crf: 26 });
        assert_eq!(Qualite::depuis("inconnu", 90).debit_mbps, 20);
        assert!(a.last().unwrap().ends_with(r"m%03d.ts"));
    }
}
