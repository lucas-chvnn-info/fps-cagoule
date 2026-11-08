// Nettoyage façon CCleaner : on analyse d'abord (taille par catégorie), puis on supprime ce qui est coché.
// Les fichiers en cours d'utilisation sont ignorés, les liens et jonctions ne sont jamais suivis.
// Pas de « nettoyeur de registre » : le gain est nul et le risque réel.

use crate::ps;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use windows_sys::Win32::UI::Shell::{SHEmptyRecycleBinW, SHQueryRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHQUERYRBINFO};

#[derive(Serialize)]
pub struct Categorie {
    pub id: &'static str,
    pub titre: &'static str,
    pub description: &'static str,
    /// Cochée par défaut
    pub defaut: bool,
    pub octets: u64,
    pub fichiers: u64,
}

#[derive(Serialize, Default)]
pub struct Bilan {
    pub id: String,
    pub octets: u64,
    pub fichiers: u64,
}

struct Cible {
    dossier: PathBuf,
    /// Âge minimal des fichiers supprimés (pour ne pas toucher à ce qui sert en ce moment)
    age_min: Duration,
    /// Préfixe du nom (ex. thumbcache_) et extensions acceptées ; vide = tout
    prefixe: &'static str,
    extensions: &'static [&'static str],
    /// Descendre dans les sous-dossiers
    recursif: bool,
    /// Supprimer les sous-dossiers devenus vides (pas dans les dossiers de Windows qui doivent exister)
    vider_dossiers: bool,
}

fn cible(dossier: PathBuf) -> Cible {
    Cible { dossier, age_min: Duration::ZERO, prefixe: "", extensions: &[], recursif: true, vider_dossiers: true }
}

fn env(nom: &str) -> PathBuf {
    PathBuf::from(std::env::var(nom).unwrap_or_default())
}

const JOUR: Duration = Duration::from_secs(24 * 3600);

struct Definition {
    id: &'static str,
    titre: &'static str,
    description: &'static str,
    defaut: bool,
}

const DEFINITIONS: [Definition; 13] = [
    Definition { id: "temp", titre: "Fichiers temporaires", description: "Fichiers laissés par Windows et les programmes (installations, mises à jour…), de plus de 24 h.", defaut: true },
    Definition { id: "corbeille", titre: "Corbeille", description: "Vide la corbeille de tous tes disques.", defaut: true },
    Definition { id: "navigateurs", titre: "Cache des navigateurs", description: "Chrome, Edge, Brave, Opera, Opera GX, Vivaldi et Firefox. Tes mots de passe, ton historique et tes connexions aux sites sont conservés. Ferme ton navigateur pour tout nettoyer.", defaut: true },
    Definition { id: "discord", titre: "Cache de Discord", description: "Images et vidéos déjà vues dans Discord, retéléchargées au besoin. Ferme Discord pour tout nettoyer.", defaut: true },
    Definition { id: "steam", titre: "Cache web de Steam", description: "Pages de la boutique et de la communauté Steam. Tes jeux et sauvegardes ne sont pas touchés.", defaut: true },
    Definition { id: "rapports", titre: "Rapports d'erreurs et vidages mémoire", description: "Rapports de plantage de Windows et des programmes, et fichiers de vidage (.dmp), parfois de plusieurs Go.", defaut: true },
    Definition { id: "maj-windows", titre: "Anciens téléchargements de Windows Update", description: "Mises à jour déjà installées, de plus de 10 jours. Windows les retélécharge si besoin.", defaut: true },
    Definition { id: "optimisation-distribution", titre: "Cache d'optimisation de la distribution", description: "Copies de mises à jour gardées par Windows pour les partager avec d'autres PC.", defaut: true },
    Definition { id: "journaux", titre: "Journaux de Windows", description: "Fichiers journaux (.log, .etl) de plus de 7 jours.", defaut: true },
    Definition { id: "miniatures", titre: "Cache des miniatures", description: "Aperçus des images et vidéos dans l'Explorateur. Ils sont recréés à l'ouverture des dossiers (un peu plus lent la première fois).", defaut: false },
    Definition { id: "shaders-directx", titre: "Cache de shaders DirectX", description: "Les jeux recompilent leurs shaders au prochain lancement : quelques saccades la première fois. Utile si un jeu saccade après une mise à jour de pilote.", defaut: false },
    Definition { id: "shaders-pilotes", titre: "Caches de shaders NVIDIA, AMD et Intel", description: "Même effet que le cache DirectX : à vider seulement en cas de saccades ou de bugs graphiques après une mise à jour.", defaut: false },
    Definition { id: "crash-jeux", titre: "Rapports de plantage des jeux", description: "Rapports d'Unreal Engine (Fortnite, Valorant…) et vidages de Steam.", defaut: true },
];

/// Dossiers de cache d'un navigateur basé sur Chromium (tous les profils).
fn caches_chromium(donnees: &Path, caches: &mut Vec<Cible>) {
    for global in ["ShaderCache", "GrShaderCache", "GraphiteDawnCache"] {
        caches.push(cible(donnees.join(global)));
    }
    let Ok(entrees) = std::fs::read_dir(donnees) else { return };
    for e in entrees.flatten() {
        let profil = e.path();
        let nom = e.file_name().to_string_lossy().to_string();
        if nom == "Default" || nom.starts_with("Profile ") || nom == "Guest Profile" || profil.join("Cache").is_dir() {
            for c in ["Cache", "Code Cache", "GPUCache", "DawnGraphiteCache", "DawnWebGPUCache"] {
                caches.push(cible(profil.join(c)));
            }
        }
    }
}

fn cibles(id: &str) -> Vec<Cible> {
    let local = env("LOCALAPPDATA");
    let roaming = env("APPDATA");
    let windir = env("SystemRoot");
    let donnees_prog = env("ProgramData");
    let x86 = env("ProgramFiles(x86)");
    match id {
        "temp" => vec![Cible { age_min: JOUR, ..cible(std::env::temp_dir()) }, Cible { age_min: JOUR, ..cible(windir.join("Temp")) }],
        "navigateurs" => {
            let mut c = Vec::new();
            for base in [r"Google\Chrome\User Data", r"Microsoft\Edge\User Data", r"BraveSoftware\Brave-Browser\User Data", r"Vivaldi\User Data"] {
                caches_chromium(&local.join(base), &mut c);
            }
            for opera in ["Opera Stable", "Opera GX Stable"] {
                let base = local.join("Opera Software").join(opera);
                for d in ["Cache", "Code Cache", "GPUCache"] {
                    c.push(cible(base.join(d)));
                }
            }
            if let Ok(profils) = std::fs::read_dir(local.join(r"Mozilla\Firefox\Profiles")) {
                for p in profils.flatten() {
                    c.push(cible(p.path().join("cache2")));
                }
            }
            c
        }
        "discord" => ["discord", "discordptb", "discordcanary"]
            .iter()
            .flat_map(|d| ["Cache", "Code Cache", "GPUCache"].map(|c| cible(roaming.join(d).join(c))))
            .collect(),
        "steam" => vec![cible(local.join(r"Steam\htmlcache")), cible(x86.join(r"Steam\appcache\httpcache"))],
        "rapports" => vec![
            cible(donnees_prog.join(r"Microsoft\Windows\WER\ReportArchive")),
            cible(donnees_prog.join(r"Microsoft\Windows\WER\ReportQueue")),
            cible(donnees_prog.join(r"Microsoft\Windows\WER\Temp")),
            cible(local.join("CrashDumps")),
            cible(windir.join("Minidump")),
            cible(windir.join("LiveKernelReports")),
            Cible { extensions: &["dmp"], recursif: false, ..cible(windir.clone()) },
        ],
        "maj-windows" => vec![Cible { age_min: JOUR * 10, vider_dossiers: false, ..cible(windir.join(r"SoftwareDistribution\Download")) }],
        "optimisation-distribution" => {
            vec![cible(windir.join(r"ServiceProfiles\NetworkService\AppData\Local\Microsoft\Windows\DeliveryOptimization\Cache"))]
        }
        "journaux" => vec![Cible { age_min: JOUR * 7, extensions: &["log", "etl", "old"], vider_dossiers: false, ..cible(windir.join("Logs")) }],
        "miniatures" => vec![Cible { prefixe: "thumbcache_", recursif: false, ..cible(local.join(r"Microsoft\Windows\Explorer")) }],
        "shaders-directx" => vec![cible(local.join("D3DSCache"))],
        "shaders-pilotes" => vec![
            cible(local.join(r"NVIDIA\DXCache")),
            cible(local.join(r"NVIDIA\GLCache")),
            cible(donnees_prog.join(r"NVIDIA Corporation\NV_Cache")),
            cible(local.join(r"AMD\DxCache")),
            cible(local.join(r"AMD\DxcCache")),
            cible(local.join(r"AMD\VkCache")),
            cible(local.join(r"AMD\GLCache")),
            cible(local.join(r"Intel\ShaderCache")),
        ],
        "crash-jeux" => vec![
            cible(local.join(r"CrashReportClient\Saved\Logs")),
            cible(local.join(r"FortniteGame\Saved\Crashes")),
            cible(local.join(r"VALORANT\Saved\Crashes")),
            cible(x86.join(r"Steam\dumps")),
        ],
        _ => vec![],
    }
}

fn concerne(c: &Cible, chemin: &Path, meta: &std::fs::Metadata, maintenant: SystemTime) -> bool {
    let nom = chemin.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !c.prefixe.is_empty() && !nom.starts_with(c.prefixe) {
        return false;
    }
    if !c.extensions.is_empty() && !c.extensions.iter().any(|e| nom.ends_with(&format!(".{e}"))) {
        return false;
    }
    c.age_min.is_zero() || meta.modified().is_ok_and(|m| maintenant.duration_since(m).is_ok_and(|age| age >= c.age_min))
}

/// Parcourt (et supprime si `supprimer`) les fichiers d'un dossier. Renvoie (octets, fichiers).
fn parcourir(c: &Cible, dossier: &Path, supprimer: bool, maintenant: SystemTime, bilan: &mut (u64, u64)) {
    let Ok(entrees) = std::fs::read_dir(dossier) else { return };
    for e in entrees.flatten() {
        let chemin = e.path();
        // symlink_metadata : les liens et jonctions ne sont jamais suivis
        let Ok(meta) = std::fs::symlink_metadata(&chemin) else { continue };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            if c.recursif {
                parcourir(c, &chemin, supprimer, maintenant, bilan);
                if supprimer && c.vider_dossiers {
                    let _ = std::fs::remove_dir(&chemin); // seulement s'il est vide
                }
            }
        } else if concerne(c, &chemin, &meta, maintenant) && (!supprimer || std::fs::remove_file(&chemin).is_ok()) {
            bilan.0 += meta.len();
            bilan.1 += 1;
        }
    }
}

fn corbeille() -> (u64, u64) {
    let mut info = SHQUERYRBINFO { cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32, i64Size: 0, i64NumItems: 0 };
    if unsafe { SHQueryRecycleBinW(std::ptr::null(), &mut info) } == 0 {
        (info.i64Size.max(0) as u64, info.i64NumItems.max(0) as u64)
    } else {
        (0, 0)
    }
}

fn mesurer(id: &str, supprimer: bool) -> (u64, u64) {
    if id == "corbeille" {
        let avant = corbeille();
        if supprimer && avant.1 > 0 {
            unsafe { SHEmptyRecycleBinW(std::ptr::null_mut(), std::ptr::null(), SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND) };
            let apres = corbeille();
            return (avant.0.saturating_sub(apres.0), avant.1.saturating_sub(apres.1));
        }
        return avant;
    }
    if id == "optimisation-distribution" && supprimer {
        // Le cache appartient au service : on passe par la commande prévue par Windows
        let avant = mesurer(id, false);
        let _ = ps::executer("Delete-DeliveryOptimizationCache -Force");
        let apres = mesurer(id, false);
        return (avant.0.saturating_sub(apres.0), avant.1.saturating_sub(apres.1));
    }
    let maintenant = SystemTime::now();
    let mut bilan = (0, 0);
    for c in cibles(id) {
        parcourir(&c, &c.dossier, supprimer, maintenant, &mut bilan);
    }
    bilan
}

pub fn analyser() -> Vec<Categorie> {
    // Chaque catégorie dans son fil : l'analyse prend le temps de la plus lente
    let fils: Vec<_> = DEFINITIONS.iter().map(|d| (d, std::thread::spawn(move || mesurer(d.id, false)))).collect();
    fils.into_iter()
        .map(|(d, fil)| {
            let (octets, fichiers) = fil.join().unwrap_or_default();
            Categorie { id: d.id, titre: d.titre, description: d.description, defaut: d.defaut, octets, fichiers }
        })
        .collect()
}

pub fn nettoyer(ids: &[String]) -> Vec<Bilan> {
    let fils: Vec<_> = ids
        .iter()
        .filter(|id| DEFINITIONS.iter().any(|d| d.id == id.as_str()))
        .map(|id| {
            let id = id.clone();
            std::thread::spawn(move || {
                let (octets, fichiers) = mesurer(&id, true);
                Bilan { id, octets, fichiers }
            })
        })
        .collect();
    fils.into_iter().filter_map(|f| f.join().ok()).collect()
}

/// Optimise les disques comme l'outil « Optimiser les lecteurs » de Windows :
/// TRIM pour les SSD (rapide), défragmentation pour les disques durs (peut être long).
pub fn optimiser_disques() -> Result<serde_json::Value, String> {
    ps::json(
        r#"$res = @()
Get-Volume | Where-Object { $_.DriveLetter -and $_.DriveType -eq 'Fixed' -and $_.FileSystem -eq 'NTFS' } | ForEach-Object {
  $l = $_.DriveLetter
  $type = "$((Get-Partition -DriveLetter $l -ErrorAction SilentlyContinue | Get-Disk | Get-PhysicalDisk | Select-Object -First 1).MediaType)"
  try {
    if ($type -eq 'HDD') { Optimize-Volume -DriveLetter $l -Defrag -ErrorAction Stop } else { Optimize-Volume -DriveLetter $l -ReTrim -ErrorAction Stop }
    $res += [ordered]@{ lecteur = "$($l):"; type = $type; ok = $true; erreur = $null }
  } catch {
    $res += [ordered]@{ lecteur = "$($l):"; type = $type; ok = $false; erreur = $_.Exception.Message }
  }
}
ConvertTo-Json -Compress -Depth 3 -InputObject @($res)"#,
    )
}
