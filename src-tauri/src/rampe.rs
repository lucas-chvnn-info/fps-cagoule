// Rampe gamma d'un écran (256 niveaux par couleur) à partir des réglages de l'utilisateur.
// Module sans dépendance : testé avec `rustc --test src/rampe.rs`.

/// luminosite : -0,3 à +0,3 ; contraste : 0,7 à 1,3 ; gamma : 0,7 à 1,5 (1 = neutre)
pub fn calculer(luminosite: f64, contraste: f64, gamma: f64) -> [u16; 768] {
    let luminosite = luminosite.clamp(-0.3, 0.3);
    let contraste = contraste.clamp(0.7, 1.3);
    let gamma = gamma.clamp(0.7, 1.5);
    let mut rampe = [0u16; 768];
    for i in 0..256 {
        let x = i as f64 / 255.0;
        let y = ((x.powf(1.0 / gamma) - 0.5) * contraste + 0.5 + luminosite).clamp(0.0, 1.0);
        let v = (y * 65535.0).round() as u16;
        rampe[i] = v;
        rampe[256 + i] = v;
        rampe[512 + i] = v;
    }
    rampe
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutre_est_lineaire() {
        let r = calculer(0.0, 1.0, 1.0);
        assert_eq!(r[0], 0);
        assert_eq!(r[255], 65535);
        assert_eq!(r[128], (128.0 / 255.0 * 65535.0f64).round() as u16);
        assert_eq!(r[256 + 128], r[128]);
    }

    #[test]
    fn plus_lumineux_et_croissant() {
        let neutre = calculer(0.0, 1.0, 1.0);
        let r = calculer(0.1, 1.1, 1.2);
        assert!(r[64] > neutre[64]);
        assert!(r.windows(2).take(255).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn valeurs_extremes_bornees() {
        let r = calculer(5.0, 9.0, 0.0);
        assert_eq!(r, calculer(0.3, 1.3, 0.7));
    }
}
