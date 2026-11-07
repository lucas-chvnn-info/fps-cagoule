// Calcul des FPS à partir des temps d'image de PresentMon.
// Module sans dépendance : testé avec `rustc --test src/stats.rs`.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    pub images: u64,
    pub moyenne: f64,
    pub low1: f64,
    pub low01: f64,
}

/// FPS moyen, 1 % low et 0,1 % low (moyenne des pires images, convertie en FPS).
pub fn calculer(temps: &[f32]) -> Option<Stats> {
    let temps: Vec<f64> = temps.iter().map(|&t| t as f64).filter(|t| *t > 0.0 && t.is_finite()).collect();
    if temps.len() < 10 {
        return None;
    }
    let total: f64 = temps.iter().sum();
    let mut tries = temps.clone();
    tries.sort_by(|a, b| b.partial_cmp(a).unwrap());
    let pire = |part: usize| {
        let n = (tries.len() / part).max(1);
        1000.0 / (tries[..n].iter().sum::<f64>() / n as f64)
    };
    Some(Stats {
        images: temps.len() as u64,
        moyenne: temps.len() as f64 * 1000.0 / total,
        low1: pire(100),
        low01: pire(1000),
    })
}

/// Lecture du CSV de PresentMon, ligne par ligne.
pub struct LecteurCsv {
    col_app: usize,
    col_swap: Option<usize>,
    col_temps: usize,
}

impl LecteurCsv {
    pub fn depuis_entete(ligne: &str) -> Option<Self> {
        let cols: Vec<&str> = ligne.trim().split(',').map(str::trim).collect();
        let pos = |nom: &str| cols.iter().position(|c| c.eq_ignore_ascii_case(nom));
        let col_temps = ["MsBetweenPresents", "FrameTime", "MsBetweenAppStart"].iter().find_map(|n| pos(n))?;
        Some(Self { col_app: pos("Application")?, col_swap: pos("SwapChainAddress"), col_temps })
    }

    /// (application, swapchain, temps de l'image en ms)
    pub fn lire<'a>(&self, ligne: &'a str) -> Option<(&'a str, &'a str, f32)> {
        let cols: Vec<&str> = ligne.trim().split(',').collect();
        let temps: f32 = cols.get(self.col_temps)?.trim().parse().ok()?;
        let swap = self.col_swap.and_then(|i| cols.get(i).copied()).unwrap_or("");
        Some((cols.get(self.col_app)?.trim(), swap.trim(), temps))
    }
}

/// Images regroupées par swapchain : un jeu peut en avoir plusieurs (menu, overlay Discord…),
/// on garde celle qui affiche le plus d'images.
#[derive(Default)]
pub struct Accumulateur {
    pub par_swap: HashMap<String, Vec<f32>>,
    fenetre: HashMap<String, (u32, f64)>,
}

impl Accumulateur {
    pub fn ajouter(&mut self, swap: &str, temps: f32) {
        self.par_swap.entry(swap.to_string()).or_default().push(temps);
        let f = self.fenetre.entry(swap.to_string()).or_insert((0, 0.0));
        f.0 += 1;
        f.1 += temps as f64;
    }

    /// FPS depuis le dernier appel, pour la swapchain la plus active.
    pub fn fps_instantane(&mut self) -> Option<f64> {
        let fps = self
            .fenetre
            .values()
            .filter(|(n, total)| *n > 0 && *total > 0.0)
            .max_by_key(|(n, _)| *n)
            .map(|(n, total)| *n as f64 * 1000.0 / total);
        self.fenetre.clear();
        fps
    }

    pub fn principale(&self) -> &[f32] {
        self.par_swap.values().max_by_key(|v| v.len()).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fps_constant() {
        let s = calculer(&vec![10.0; 1000]).unwrap();
        assert!((s.moyenne - 100.0).abs() < 1e-9);
        assert!((s.low1 - 100.0).abs() < 1e-9);
    }

    #[test]
    fn low_sur_les_pires_images() {
        // 990 images à 5 ms (200 FPS) et 10 saccades à 50 ms (20 FPS)
        let mut t = vec![5.0; 990];
        t.extend(vec![50.0; 10]);
        let s = calculer(&t).unwrap();
        assert!((s.low1 - 20.0).abs() < 1e-9);
        assert!((s.low01 - 20.0).abs() < 1e-9);
        assert!((s.moyenne - 1000.0 * 1000.0 / (990.0 * 5.0 + 500.0)).abs() < 1e-9);
    }

    #[test]
    fn trop_peu_d_images() {
        assert_eq!(calculer(&[16.6; 5]), None);
    }

    #[test]
    fn csv_presentmon() {
        let l = LecteurCsv::depuis_entete(
            "Application,ProcessID,SwapChainAddress,Runtime,SyncInterval,PresentFlags,Dropped,TimeInSeconds,MsInPresentAPI,MsBetweenPresents\r",
        )
        .unwrap();
        assert_eq!(
            l.lire("RainbowSix.exe,1234,0x000001F2A,DXGI,0,0,0,1.25,0.10,6.94\r"),
            Some(("RainbowSix.exe", "0x000001F2A", 6.94))
        );
        assert_eq!(l.lire("ligne,incomplete"), None);
        assert!(LecteurCsv::depuis_entete("rien,du,tout").is_none());
    }

    #[test]
    fn swapchain_la_plus_active() {
        let mut a = Accumulateur::default();
        for _ in 0..100 {
            a.ajouter("jeu", 10.0);
        }
        for _ in 0..5 {
            a.ajouter("overlay", 100.0);
        }
        assert!((a.fps_instantane().unwrap() - 100.0).abs() < 1e-9);
        assert_eq!(a.fps_instantane(), None);
        assert_eq!(a.principale().len(), 100);
    }
}
