// Benchmark : processeur (1 cœur, tous les cœurs), mémoire (débit, latence) et disque.
// La carte graphique est testée dans l'interface (WebGL). Rien ne tourne en dehors d'un test lancé.
// Les points sont relatifs au PC de référence (le Windows de GitHub Actions : AMD EPYC 7763, 4 cœurs) = 1000.

use serde::Serialize;
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/* ---------- Processeur ---------- */

/// Une unité de travail : un carré de fractale de Mandelbrot (calcul flottant, branches)
/// et un crible de nombres premiers (entiers, mémoire cache).
fn unite(graine: u64) -> u64 {
    let mut total = 0u64;
    let dx = (graine % 7) as f64 * 0.001;
    for y in 0..24 {
        for x in 0..24 {
            let (cr, ci) = (-2.0 + x as f64 * 0.11 + dx, -1.2 + y as f64 * 0.1);
            let (mut zr, mut zi, mut n) = (0.0f64, 0.0f64, 0u32);
            while n < 96 && zr * zr + zi * zi < 4.0 {
                let t = zr * zr - zi * zi + cr;
                zi = 2.0 * zr * zi + ci;
                zr = t;
                n += 1;
            }
            total += n as u64;
        }
    }
    let mut crible = [true; 4096];
    let mut premiers = 0u64;
    for i in 2..4096 {
        if crible[i] {
            premiers += 1;
            let mut j = i * i;
            while j < 4096 {
                crible[j] = false;
                j += i;
            }
        }
    }
    black_box(total + premiers)
}

/// Unités par seconde sur `fils` cœurs pendant `duree`.
fn mesurer_cpu(fils: usize, duree: Duration) -> f64 {
    let stop = Arc::new(AtomicBool::new(false));
    let compte = Arc::new(AtomicU64::new(0));
    let debut = Instant::now();
    let poignees: Vec<_> = (0..fils)
        .map(|f| {
            let (stop, compte) = (stop.clone(), compte.clone());
            std::thread::spawn(move || {
                let mut n = 0u64;
                while !stop.load(Ordering::Relaxed) {
                    black_box(unite(n + f as u64));
                    n += 1;
                }
                compte.fetch_add(n, Ordering::Relaxed);
            })
        })
        .collect();
    std::thread::sleep(duree);
    stop.store(true, Ordering::Relaxed);
    poignees.into_iter().for_each(|p| {
        let _ = p.join();
    });
    compte.load(Ordering::Relaxed) as f64 / debut.elapsed().as_secs_f64()
}

/// Unités par seconde du PC de référence : mesurées sur GitHub Actions en compilation de test
/// (5 110 et 14 530), multipliées par 2,7 = écart mesuré avec la compilation finale de l'app
const REF_CPU_1: f64 = 13_800.0;
const REF_CPU_N: f64 = 39_000.0;

#[derive(Serialize)]
pub struct ResultatCpu {
    pub un_coeur: f64,
    pub tous: f64,
    pub coeurs: usize,
    pub points_un: u32,
    pub points_tous: u32,
}

pub fn cpu(duree_s: f64) -> ResultatCpu {
    let d = Duration::from_secs_f64(duree_s);
    let coeurs = std::thread::available_parallelism().map_or(1, |n| n.get());
    // Petit échauffement : le processeur monte en fréquence
    mesurer_cpu(1, Duration::from_millis(300));
    let un = mesurer_cpu(1, d);
    let tous = mesurer_cpu(coeurs, d);
    ResultatCpu {
        un_coeur: un,
        tous,
        coeurs,
        points_un: (un / REF_CPU_1 * 1000.0).round() as u32,
        points_tous: (tous / REF_CPU_N * 1000.0).round() as u32,
    }
}

/* ---------- Mémoire ---------- */

// Référence mesurée sur GitHub Actions : 15,7 Go/s et 138 ns
const REF_RAM_GO_S: f64 = 15.0;
const REF_RAM_NS: f64 = 130.0;

#[derive(Serialize)]
pub struct ResultatRam {
    /// Copie (Go/s)
    pub debit: f64,
    /// Accès au hasard (nanosecondes)
    pub latence: f64,
    pub points: u32,
}

pub fn ram(duree_s: f64) -> ResultatRam {
    // Débit : copies répétées de 256 Mo, bien plus que les caches du processeur
    let taille = 32 * 1024 * 1024;
    let source: Vec<u64> = (0..taille as u64).collect();
    let mut cible = vec![0u64; taille];
    let debut = Instant::now();
    let mut copies = 0u64;
    while debut.elapsed().as_secs_f64() < duree_s * 0.5 || copies == 0 {
        cible.copy_from_slice(black_box(&source));
        copies += 1;
    }
    black_box(&cible);
    let debit = (copies * taille as u64 * 8) as f64 / debut.elapsed().as_secs_f64() / 1e9;
    drop((source, cible));

    // Latence : on suit une chaîne d'indices au hasard dans 128 Mo (permutation de Sattolo)
    let n = 32 * 1024 * 1024usize;
    let mut chaine: Vec<u32> = (0..n as u32).collect();
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    for i in (1..n).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        chaine.swap(i, (x % i as u64) as usize);
    }
    let pas = 5_000_000u64;
    let mut p = 0u32;
    let debut = Instant::now();
    for _ in 0..pas {
        p = chaine[p as usize];
    }
    black_box(p);
    let latence = debut.elapsed().as_nanos() as f64 / pas as f64;
    let points = (debit / REF_RAM_GO_S * 500.0 + REF_RAM_NS / latence.max(1.0) * 500.0).round() as u32;
    ResultatRam { debit, latence, points }
}

/* ---------- Disque ---------- */

// Référence mesurée sur GitHub Actions : 518 Mo/s en lecture, 113 en écriture, 9,6 en 4 Ko
const REF_LECTURE: f64 = 500.0;
const REF_ECRITURE: f64 = 110.0;
const REF_4K: f64 = 10.0;

#[derive(Serialize)]
pub struct ResultatDisque {
    pub lecteur: String,
    /// Mo/s
    pub lecture: f64,
    pub ecriture: f64,
    /// Lecture de blocs de 4 Ko au hasard, une à la fois (Mo/s)
    pub aleatoire_4k: f64,
    pub points: u32,
}

/// Tampon aligné sur 4 Ko : obligatoire pour lire et écrire sans le cache de Windows.
struct Tampon {
    ptr: *mut u8,
    taille: usize,
}

impl Tampon {
    fn nouveau(taille: usize) -> Self {
        let disposition = std::alloc::Layout::from_size_align(taille, 4096).unwrap();
        let ptr = unsafe { std::alloc::alloc(disposition) };
        assert!(!ptr.is_null(), "mémoire insuffisante");
        // Contenu pseudo-aléatoire : certains SSD compressent les zéros
        let mut x = 0x2545_F491_4F6C_DD1Du64;
        for i in 0..taille {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            unsafe { *ptr.add(i) = x as u8 };
        }
        Self { ptr, taille }
    }
    fn tranche(&mut self, n: usize) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, n.min(self.taille)) }
    }
}

impl Drop for Tampon {
    fn drop(&mut self) {
        unsafe { std::alloc::dealloc(self.ptr, std::alloc::Layout::from_size_align(self.taille, 4096).unwrap()) }
    }
}

pub fn disque(taille_mo: u64, duree_aleatoire_s: f64) -> Result<ResultatDisque, String> {
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::os::windows::fs::OpenOptionsExt;
    const SANS_CACHE: u32 = 0x2000_0000; // FILE_FLAG_NO_BUFFERING
    const ECRITURE_DIRECTE: u32 = 0x8000_0000; // FILE_FLAG_WRITE_THROUGH

    let chemin = std::env::temp_dir().join("fps-cagoule-benchmark.tmp");
    let lecteur = chemin.to_string_lossy().chars().take(2).collect::<String>();
    let bloc = 4 * 1024 * 1024usize;
    let blocs = (taille_mo * 1024 * 1024) as usize / bloc;
    let mut tampon = Tampon::nouveau(bloc);
    let resultat = (|| -> Result<(f64, f64, f64), String> {
        // Écriture en continu
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .truncate(true)
            .custom_flags(SANS_CACHE | ECRITURE_DIRECTE)
            .open(&chemin)
            .map_err(|e| e.to_string())?;
        let debut = Instant::now();
        for _ in 0..blocs {
            f.write_all(tampon.tranche(bloc)).map_err(|e| e.to_string())?;
        }
        f.sync_all().map_err(|e| e.to_string())?;
        let ecriture = (blocs * bloc) as f64 / debut.elapsed().as_secs_f64() / 1e6;

        // Lecture en continu
        f.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        let debut = Instant::now();
        for _ in 0..blocs {
            f.read_exact(tampon.tranche(bloc)).map_err(|e| e.to_string())?;
        }
        let lecture = (blocs * bloc) as f64 / debut.elapsed().as_secs_f64() / 1e6;

        // 4 Ko au hasard, un à la fois (ce que font les jeux en chargeant)
        let blocs_4k = (blocs * bloc / 4096) as u64;
        let mut x = 0x1234_5678_9ABC_DEF1u64;
        let mut lus = 0u64;
        let debut = Instant::now();
        while debut.elapsed().as_secs_f64() < duree_aleatoire_s {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            f.seek(SeekFrom::Start((x % blocs_4k) * 4096)).map_err(|e| e.to_string())?;
            f.read_exact(tampon.tranche(4096)).map_err(|e| e.to_string())?;
            lus += 1;
        }
        let aleatoire_4k = (lus * 4096) as f64 / debut.elapsed().as_secs_f64() / 1e6;
        Ok((lecture, ecriture, aleatoire_4k))
    })();
    let _ = std::fs::remove_file(&chemin);
    let (lecture, ecriture, aleatoire_4k) = resultat?;
    // Racine carrée : un SSD NVMe écrit 20 fois plus vite que la référence, le score ne doit pas exploser
    let points = (1000.0 * (0.35 * (lecture / REF_LECTURE).sqrt() + 0.3 * (ecriture / REF_ECRITURE).sqrt() + 0.35 * (aleatoire_4k / REF_4K).sqrt())).round() as u32;
    Ok(ResultatDisque { lecteur, lecture, ecriture, aleatoire_4k, points })
}

/* ---------- Historique ---------- */

#[derive(Clone, Serialize, serde::Deserialize)]
pub struct Resultat {
    /// Horodatage Unix (secondes)
    pub date: u64,
    pub optimisations_actives: u32,
    pub cpu_un: u32,
    pub cpu_tous: u32,
    pub ram: u32,
    pub disque: u32,
    pub gpu: u32,
    pub total: u32,
    /// Détails bruts (Go/s, Mo/s, nom du GPU…) pour l'affichage
    pub details: serde_json::Value,
}

#[cfg(test)]
mod tests {
    #[test]
    fn unite_deterministe() {
        assert_eq!(super::unite(3), super::unite(3));
    }
}

fn fichier(stock: &crate::config::Stockage) -> std::path::PathBuf {
    stock.dossier().join("benchmarks.json")
}

pub fn historique(stock: &crate::config::Stockage) -> Vec<Resultat> {
    std::fs::read_to_string(fichier(stock)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn enregistrer(stock: &crate::config::Stockage, mut r: Resultat) -> Vec<Resultat> {
    r.date = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let mut liste = historique(stock);
    liste.push(r);
    if liste.len() > 100 {
        liste.drain(..liste.len() - 100);
    }
    if let Ok(t) = serde_json::to_string_pretty(&liste) {
        let _ = std::fs::write(fichier(stock), t);
    }
    liste
}
