// Réglages dans les jeux : modifie leurs fichiers de configuration (seulement les clés déjà présentes),
// après une copie de sauvegarde. On garde ce qui aide à voir (distance d'affichage, textures) et on
// coupe ce qui coûte des FPS ou gêne la visibilité (flou, V-Sync, ombres lourdes, effets).

use crate::config::Stockage;
use crate::edition::{self, Changement, Format};
use serde::Serialize;
use std::path::{Path, PathBuf};
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
use winreg::RegKey;

struct Reglage {
    section: Option<&'static str>,
    cle: &'static str,
    valeur: &'static str,
    libelle: &'static str,
}

const fn r(section: Option<&'static str>, cle: &'static str, valeur: &'static str, libelle: &'static str) -> Reglage {
    Reglage { section, cle, valeur, libelle }
}

#[derive(Clone, Copy)]
enum Base {
    LocalAppData,
    AppData,
    Documents,
    PartiesEnregistrees,
    Steam,
}

struct Jeu {
    id: &'static str,
    nom: &'static str,
    exes: &'static [&'static str],
    format: Format,
    /// Emplacements possibles du fichier (base + chemin) ; « * » = un dossier par compte
    chemins: &'static [(Base, &'static str)],
    reglages: &'static [Reglage],
    note: &'static str,
}

const SG: Option<&str> = Some("ScalabilityGroups");
const RL: Option<&str> = Some("SystemSettings");
const R6: Option<&str> = Some("CUSTOM_PRESET_SETTINGS");
const OW: Option<&str> = Some("Render.13");

const JEUX: &[Jeu] = &[
    Jeu {
        id: "fortnite",
        nom: "Fortnite",
        exes: &["FortniteClient-Win64-Shipping.exe"],
        format: Format::Ini,
        chemins: &[(Base::LocalAppData, r"FortniteGame\Saved\Config\WindowsClient\GameUserSettings.ini")],
        reglages: &[
            r(None, "bUseVSync", "False", "V-Sync désactivée"),
            r(None, "bMotionBlur", "False", "Flou de mouvement coupé"),
            r(None, "bShowGrass", "False", "Herbe masquée (meilleure visibilité)"),
            r(SG, "sg.ShadowQuality", "0", "Ombres coupées"),
            r(SG, "sg.GlobalIlluminationQuality", "0", "Illumination globale (Lumen) coupée"),
            r(SG, "sg.ReflectionQuality", "0", "Réflexions au minimum"),
            r(SG, "sg.PostProcessQuality", "0", "Post-traitement au minimum"),
            r(SG, "sg.EffectsQuality", "0", "Effets au minimum"),
            r(SG, "sg.FoliageQuality", "0", "Végétation au minimum"),
        ],
        note: "Distance d'affichage et textures gardées. Pour encore plus de FPS, choisis le mode de rendu « Performances » dans le jeu.",
    },
    Jeu {
        id: "valorant",
        nom: "Valorant",
        exes: &["VALORANT-Win64-Shipping.exe"],
        format: Format::Ini,
        chemins: &[(Base::LocalAppData, r"VALORANT\Saved\Config\*\Windows\GameUserSettings.ini")],
        reglages: &[
            r(None, "bUseVSync", "False", "V-Sync désactivée"),
            r(SG, "sg.ShadowQuality", "0", "Ombres au minimum"),
            r(SG, "sg.PostProcessQuality", "0", "Post-traitement au minimum"),
            r(SG, "sg.EffectsQuality", "0", "Effets au minimum"),
            r(SG, "sg.FoliageQuality", "0", "Végétation au minimum"),
        ],
        note: "Appliqué à tous tes comptes Valorant de ce PC. Si tu changes un réglage graphique dans le jeu, il réécrit le fichier.",
    },
    Jeu {
        id: "r6",
        nom: "Rainbow Six Siege",
        exes: &["RainbowSix.exe", "RainbowSix_Vulkan.exe", "RainbowSix_DX12.exe"],
        format: Format::Ini,
        chemins: &[(Base::Documents, r"My Games\Rainbow Six - Siege\*\GameSettings.ini")],
        reglages: &[
            r(None, "VSync", "0", "V-Sync désactivée"),
            r(None, "MaxGPUBufferedFrame", "1", "Une seule image en attente (moins de latence)"),
            r(None, "RawInputMouseKeyboard", "1", "Entrée brute de la souris"),
            r(R6, "MotionBlur", "0", "Flou de mouvement coupé"),
            r(R6, "LensEffects", "0", "Effets d'objectif coupés"),
            r(R6, "DepthOfField", "0", "Profondeur de champ coupée"),
            r(R6, "AO", "0", "Occlusion ambiante coupée"),
        ],
        note: "Ombres, textures et niveau de détail gardés (les ombres aident à repérer les ennemis). Les effets ne s'appliquent que si tes graphismes sont en « Personnalisé ».",
    },
    Jeu {
        id: "cs2",
        nom: "Counter-Strike 2",
        exes: &["cs2.exe"],
        format: Format::Valve,
        chemins: &[(Base::Steam, r"userdata\*\730\local\cfg\cs2_video.txt")],
        reglages: &[
            r(None, "setting.mat_vsync", "0", "V-Sync désactivée"),
            r(None, "setting.r_low_latency", "1", "NVIDIA Reflex activé"),
            r(None, "setting.shaderquality", "0", "Détails des shaders au minimum"),
        ],
        note: "Anticrénelage (MSAA) et textures gardés pour la netteté.",
    },
    Jeu {
        id: "apex",
        nom: "Apex Legends",
        exes: &["r5apex.exe", "r5apex_dx12.exe"],
        format: Format::Valve,
        chemins: &[(Base::PartiesEnregistrees, r"Respawn\Apex\local\videoconfig.txt")],
        reglages: &[
            r(None, "setting.mat_vsync_mode", "0", "V-Sync désactivée"),
            r(None, "setting.mat_backbuffer_count", "0", "Moins d'images en attente (moins de latence)"),
            r(None, "setting.volumetric_lighting", "0", "Éclairage volumétrique coupé"),
            r(None, "setting.ssao_enabled", "0", "Occlusion ambiante coupée"),
            r(None, "setting.mat_depthfeather_enable", "0", "Adoucissement des particules coupé"),
            r(None, "setting.cl_ragdoll_maxcount", "0", "Corps des ennemis tués retirés"),
        ],
        note: "Apex réécrit parfois ce fichier au lancement : si les réglages reviennent, réapplique-les ici.",
    },
    Jeu {
        id: "rocket-league",
        nom: "Rocket League",
        exes: &["RocketLeague.exe"],
        format: Format::Ini,
        chemins: &[(Base::Documents, r"My Games\Rocket League\TAGame\Config\TASystemSettings.ini")],
        reglages: &[
            r(RL, "UseVsync", "False", "V-Sync désactivée"),
            r(RL, "MotionBlur", "False", "Flou de mouvement coupé"),
            r(RL, "AllowRadialBlur", "False", "Flou radial coupé"),
            r(RL, "DepthOfField", "False", "Profondeur de champ coupée"),
            r(RL, "Bloom", "False", "Bloom coupé"),
            r(RL, "LensFlares", "False", "Reflets d'objectif coupés"),
            r(RL, "AmbientOcclusion", "False", "Occlusion ambiante coupée"),
        ],
        note: "Ombres dynamiques gardées : elles aident à placer la balle en l'air.",
    },
    Jeu {
        id: "marvel-rivals",
        nom: "Marvel Rivals",
        exes: &["Marvel-Win64-Shipping.exe"],
        format: Format::Ini,
        chemins: &[(Base::LocalAppData, r"Marvel\Saved\Config\Windows\GameUserSettings.ini")],
        reglages: &[
            r(None, "bUseVSync", "False", "V-Sync désactivée"),
            r(SG, "sg.ShadowQuality", "0", "Ombres au minimum"),
            r(SG, "sg.GlobalIlluminationQuality", "0", "Illumination globale au minimum"),
            r(SG, "sg.ReflectionQuality", "0", "Réflexions au minimum"),
            r(SG, "sg.PostProcessQuality", "0", "Post-traitement au minimum"),
            r(SG, "sg.EffectsQuality", "0", "Effets au minimum"),
        ],
        note: "Textures et distance d'affichage gardées.",
    },
    Jeu {
        id: "the-finals",
        nom: "The Finals",
        exes: &["Discovery.exe"],
        format: Format::Ini,
        chemins: &[(Base::LocalAppData, r"Discovery\Saved\Config\WindowsClient\GameUserSettings.ini")],
        reglages: &[
            r(None, "bUseVSync", "False", "V-Sync désactivée"),
            r(SG, "sg.ShadowQuality", "0", "Ombres au minimum"),
            r(SG, "sg.GlobalIlluminationQuality", "0", "Illumination globale au minimum"),
            r(SG, "sg.ReflectionQuality", "0", "Réflexions au minimum"),
            r(SG, "sg.PostProcessQuality", "0", "Post-traitement au minimum"),
            r(SG, "sg.EffectsQuality", "0", "Effets au minimum"),
        ],
        note: "Textures et distance d'affichage gardées.",
    },
    Jeu {
        id: "overwatch",
        nom: "Overwatch 2",
        exes: &["Overwatch.exe"],
        format: Format::Ini,
        chemins: &[(Base::Documents, r"Overwatch\Settings\Settings_v0.ini")],
        reglages: &[
            r(OW, "TripleBufferingEnabled", "0", "Triple tampon coupé (moins de latence)"),
            r(OW, "SSAOEnabled", "0", "Occlusion ambiante coupée"),
            r(OW, "LocalReflections", "0", "Réflexions locales coupées"),
            r(OW, "SSLRDetailLevel", "0", "Réflexions à l'écran au minimum"),
            r(OW, "RefractionDetail", "0", "Réfraction au minimum"),
            r(OW, "LocalFogDetail", "0", "Brouillard au minimum (meilleure visibilité)"),
        ],
        note: "Ombres, textures et effets des capacités gardés. Si tu changes les graphismes dans le jeu, il réécrit le fichier.",
    },
    Jeu {
        id: "gta-v",
        nom: "GTA V",
        exes: &["GTA5.exe", "GTA5_Enhanced.exe"],
        format: Format::XmlValeur,
        chemins: &[(Base::Documents, r"Rockstar Games\GTAV Enhanced\settings.xml"), (Base::Documents, r"Rockstar Games\GTA V\settings.xml")],
        reglages: &[
            r(None, "VSync", "0", "V-Sync désactivée"),
            r(None, "MotionBlurStrength", "0.000000", "Flou de mouvement coupé"),
            r(None, "DoF", "false", "Profondeur de champ coupée"),
            r(None, "SSAO", "0", "Occlusion ambiante coupée"),
            r(None, "ReflectionMSAA", "0", "Anticrénelage des reflets coupé"),
            r(None, "Lighting_FogVolumes", "false", "Volumes de brouillard coupés"),
        ],
        note: "Textures et distance d'affichage gardées. Édition Legacy et Enhanced.",
    },
    Jeu {
        id: "minecraft",
        nom: "Minecraft",
        exes: &["Minecraft.Windows.exe"],
        format: Format::DeuxPoints,
        chemins: &[
            (Base::AppData, r"Minecraft Bedrock\Users\*\games\com.mojang\minecraftpe\options.txt"),
            (Base::LocalAppData, r"Packages\Microsoft.MinecraftUWP_8wekyb3d8bbwe\LocalState\games\com.mojang\minecraftpe\options.txt"),
        ],
        reglages: &[
            r(None, "gfx_vsync", "0", "V-Sync désactivée"),
            r(None, "graphics_mode", "0", "Graphismes simples (au lieu de « Vibrant Visuals »)"),
            r(None, "gfx_toggleclouds", "0", "Nuages coupés"),
            r(None, "gfx_bubble_particles", "0", "Particules de bulles coupées"),
        ],
        note: "Minecraft pour Windows (Bedrock). Distance d'affichage gardée.",
    },
];

fn dossier_connu(id: &windows_sys::core::GUID) -> Option<PathBuf> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::SHGetKnownFolderPath;
    unsafe {
        let mut p: *mut u16 = std::ptr::null_mut();
        if SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut p) != 0 || p.is_null() {
            return None;
        }
        let n = (0..).take_while(|&i| *p.add(i) != 0).count();
        let chemin = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
        CoTaskMemFree(p as *const _);
        Some(PathBuf::from(chemin))
    }
}

fn base(b: Base) -> Option<PathBuf> {
    use windows_sys::Win32::UI::Shell::{FOLDERID_Documents, FOLDERID_SavedGames};
    match b {
        Base::LocalAppData => std::env::var("LOCALAPPDATA").ok().map(PathBuf::from),
        Base::AppData => std::env::var("APPDATA").ok().map(PathBuf::from),
        // Dossiers connus : ils peuvent être déplacés (OneDrive…)
        Base::Documents => dossier_connu(&FOLDERID_Documents),
        Base::PartiesEnregistrees => dossier_connu(&FOLDERID_SavedGames),
        Base::Steam => RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(r"Software\Valve\Steam", KEY_READ)
            .and_then(|k| k.get_value::<String, _>("SteamPath"))
            .ok()
            .map(|p| PathBuf::from(p.replace('/', "\\"))),
    }
}

/// Fichiers de configuration du jeu (un par compte quand le motif contient « * »).
fn fichiers(jeu: &Jeu) -> Vec<PathBuf> {
    let mut trouves: Vec<PathBuf> = jeu.chemins.iter().flat_map(|(b, motif)| fichiers_motif(*b, motif)).collect();
    // Le plus récemment utilisé d'abord (c'est lui qu'on affiche)
    trouves.sort_by_key(|p| std::cmp::Reverse(std::fs::metadata(p).and_then(|m| m.modified()).ok()));
    trouves
}

fn fichiers_motif(b: Base, motif: &str) -> Vec<PathBuf> {
    let Some(racine) = base(b) else { return vec![] };
    let mut chemins = vec![racine];
    for morceau in motif.split('\\') {
        chemins = chemins
            .into_iter()
            .flat_map(|c| -> Vec<PathBuf> {
                if morceau == "*" {
                    std::fs::read_dir(&c).map(|l| l.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default()
                } else {
                    vec![c.join(morceau)]
                }
            })
            .collect();
    }
    chemins.into_iter().filter(|p| p.is_file()).collect()
}

fn changements(jeu: &Jeu) -> Vec<Changement<'static>> {
    jeu.reglages.iter().map(|r| Changement { section: r.section, cle: r.cle, valeur: r.valeur }).collect()
}

fn egal(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

#[derive(Serialize)]
pub struct EtatReglage {
    pub libelle: &'static str,
    pub actuel: Option<String>,
    pub cible: &'static str,
}

#[derive(Serialize)]
pub struct EtatJeu {
    pub id: &'static str,
    pub nom: &'static str,
    pub note: &'static str,
    pub exes: &'static [&'static str],
    /// Nombre de fichiers trouvés (0 = jeu pas installé ou jamais lancé)
    pub fichiers: usize,
    pub en_cours: bool,
    /// Réglages présents dans le fichier le plus récent
    pub reglages: Vec<EtatReglage>,
    pub optimise: bool,
    pub sauvegarde: bool,
}

fn jeu_lance(jeu: &Jeu) -> bool {
    crate::systeme::processus().iter().any(|p| jeu.exes.iter().any(|e| e.eq_ignore_ascii_case(&p.nom)))
}

fn lire_fichier(chemin: &Path) -> Result<(String, edition::Encodage), String> {
    std::fs::read(chemin).map(|o| edition::decoder(&o)).map_err(|e| format!("{} : {e}", chemin.display()))
}

pub fn liste(stock: &Stockage) -> Vec<EtatJeu> {
    let sauvegardes = stock.donnees.lock().unwrap().configs_jeux.clone();
    JEUX.iter()
        .map(|jeu| {
            let f = fichiers(jeu);
            let ch = changements(jeu);
            let mut optimise = !f.is_empty();
            let mut reglages = Vec::new();
            for (i, chemin) in f.iter().enumerate() {
                let Ok((texte, _)) = lire_fichier(chemin) else { continue };
                let valeurs = edition::lire(&texte, jeu.format, &ch);
                if valeurs.iter().all(Option::is_none) {
                    continue;
                }
                optimise &= valeurs.iter().zip(jeu.reglages).all(|(v, r)| v.as_deref().map_or(true, |v| egal(v, r.valeur)));
                if i == 0 || reglages.is_empty() {
                    reglages = valeurs
                        .into_iter()
                        .zip(jeu.reglages)
                        .filter_map(|(v, r)| v.map(|v| EtatReglage { libelle: r.libelle, actuel: Some(v), cible: r.valeur }))
                        .collect();
                }
            }
            EtatJeu {
                id: jeu.id,
                nom: jeu.nom,
                note: jeu.note,
                exes: jeu.exes,
                fichiers: f.len(),
                en_cours: !f.is_empty() && jeu_lance(jeu),
                optimise: optimise && !reglages.is_empty(),
                reglages,
                sauvegarde: sauvegardes.contains_key(jeu.id),
            }
        })
        .collect()
}

/// Écrit le fichier en gardant son encodage ; un fichier en lecture seule le redevient ensuite.
fn ecrire_fichier(chemin: &Path, octets: &[u8]) -> Result<(), String> {
    let lecture_seule = std::fs::metadata(chemin).map(|m| m.permissions().readonly()).unwrap_or(false);
    let changer = |ro: bool| {
        if let Ok(m) = std::fs::metadata(chemin) {
            let mut p = m.permissions();
            p.set_readonly(ro);
            let _ = std::fs::set_permissions(chemin, p);
        }
    };
    if lecture_seule {
        changer(false);
    }
    let tmp = chemin.with_extension("fpscagoule.tmp");
    let r = std::fs::write(&tmp, octets).and_then(|_| std::fs::rename(&tmp, chemin)).map_err(|e| format!("{} : {e}", chemin.display()));
    if lecture_seule {
        changer(true);
    }
    r
}

/// Applique le préréglage à un fichier. Renvoie le nombre de valeurs changées.
pub fn optimiser_fichier(chemin: &Path, format: Format, ch: &[Changement]) -> Result<usize, String> {
    let (texte, encodage) = lire_fichier(chemin)?;
    let avant = edition::lire(&texte, format, ch);
    let nouveau = edition::appliquer(&texte, format, ch);
    let n = avant.iter().zip(ch).filter(|(v, c)| v.as_deref().is_some_and(|v| !egal(v, c.valeur))).count();
    if n > 0 {
        ecrire_fichier(chemin, &edition::encoder(&nouveau, encodage))?;
    }
    Ok(n)
}

fn trouver(id: &str) -> Result<&'static Jeu, String> {
    JEUX.iter().find(|j| j.id == id).ok_or_else(|| "Jeu inconnu".into())
}

pub fn appliquer(stock: &Stockage, id: &str) -> Result<usize, String> {
    let jeu = trouver(id)?;
    if jeu_lance(jeu) {
        return Err(format!("Ferme {} d'abord : il réécrit ses réglages en quittant.", jeu.nom));
    }
    let f = fichiers(jeu);
    if f.is_empty() {
        return Err(format!("Fichier de réglages de {} introuvable : lance le jeu une fois, puis réessaie.", jeu.nom));
    }
    // Copie d'origine, seulement la première fois
    if !stock.donnees.lock().unwrap().configs_jeux.contains_key(id) {
        let dossier = stock.dossier().join("sauvegardes-jeux").join(id);
        std::fs::create_dir_all(&dossier).map_err(|e| e.to_string())?;
        let mut copies = Vec::new();
        for (i, chemin) in f.iter().enumerate() {
            let copie = dossier.join(format!("{i}-{}", chemin.file_name().unwrap_or_default().to_string_lossy()));
            std::fs::copy(chemin, &copie).map_err(|e| format!("Sauvegarde impossible : {e}"))?;
            copies.push((chemin.to_string_lossy().to_string(), copie.to_string_lossy().to_string()));
        }
        stock.donnees.lock().unwrap().configs_jeux.insert(id.to_string(), copies);
        stock.sauver();
    }
    let ch = changements(jeu);
    f.iter().map(|c| optimiser_fichier(c, jeu.format, &ch)).sum()
}

pub fn restaurer(stock: &Stockage, id: &str) -> Result<(), String> {
    let jeu = trouver(id)?;
    if jeu_lance(jeu) {
        return Err(format!("Ferme {} d'abord.", jeu.nom));
    }
    let copies = stock.donnees.lock().unwrap().configs_jeux.get(id).cloned().ok_or("Aucune sauvegarde pour ce jeu")?;
    for (origine, copie) in &copies {
        let octets = std::fs::read(copie).map_err(|e| format!("Sauvegarde illisible : {e}"))?;
        ecrire_fichier(Path::new(origine), &octets)?;
    }
    for (_, copie) in &copies {
        let _ = std::fs::remove_file(copie);
    }
    stock.donnees.lock().unwrap().configs_jeux.remove(id);
    stock.sauver();
    Ok(())
}
