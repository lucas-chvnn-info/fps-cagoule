// Modification des fichiers de configuration des jeux, sans rien inventer :
// seules les clés déjà présentes sont changées, l'encodage et les fins de ligne sont conservés.
// Module sans dépendance : testé avec `rustc --test src/edition.rs`.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Format {
    /// cle=valeur, avec des [sections] (Unreal Engine, R6, Rocket League)
    Ini,
    /// "cle"		"valeur" (moteur Source : CS2, Apex)
    Valve,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Encodage {
    Utf8,
    Utf8Bom,
    Utf16,
    Latin1,
}

pub fn decoder(octets: &[u8]) -> (String, Encodage) {
    if let Some(r) = octets.strip_prefix(&[0xFF, 0xFE]) {
        let u: Vec<u16> = r.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return (String::from_utf16_lossy(&u), Encodage::Utf16);
    }
    if let Some(r) = octets.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return (String::from_utf8_lossy(r).into_owned(), Encodage::Utf8Bom);
    }
    match std::str::from_utf8(octets) {
        Ok(t) => (t.to_string(), Encodage::Utf8),
        // Fichier « ANSI » : chaque octet est un caractère
        Err(_) => (octets.iter().map(|&o| o as char).collect(), Encodage::Latin1),
    }
}

pub fn encoder(texte: &str, encodage: Encodage) -> Vec<u8> {
    match encodage {
        Encodage::Utf8 => texte.as_bytes().to_vec(),
        Encodage::Utf8Bom => [&[0xEF, 0xBB, 0xBF][..], texte.as_bytes()].concat(),
        Encodage::Utf16 => [0xFF, 0xFE].into_iter().chain(texte.encode_utf16().flat_map(u16::to_le_bytes)).collect(),
        Encodage::Latin1 => texte.chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }).collect(),
    }
}

/// Un changement : section facultative (Ini), clé et valeur voulue.
#[derive(Debug, Clone)]
pub struct Changement<'a> {
    pub section: Option<&'a str>,
    pub cle: &'a str,
    pub valeur: &'a str,
}

/// Valeur actuelle de chaque changement (None = clé absente du fichier).
pub fn lire(texte: &str, format: Format, changements: &[Changement]) -> Vec<Option<String>> {
    changements.iter().map(|c| parcourir(texte, format, c).map(|(_, v)| v)).collect()
}

/// Applique les changements dont la clé existe. Renvoie le nouveau texte.
pub fn appliquer(texte: &str, format: Format, changements: &[Changement]) -> String {
    let mut lignes: Vec<String> = texte.split('\n').map(str::to_string).collect();
    for c in changements {
        if let Some((i, _)) = parcourir(&lignes.join("\n"), format, c) {
            lignes[i] = remplacer(&lignes[i], format, c.valeur);
        }
    }
    lignes.join("\n")
}

/// (numéro de ligne, valeur) de la première occurrence de la clé dans la bonne section.
fn parcourir(texte: &str, format: Format, c: &Changement) -> Option<(usize, String)> {
    let mut section = String::new();
    for (i, ligne) in texte.split('\n').enumerate() {
        let l = ligne.trim();
        match format {
            Format::Ini => {
                if l.starts_with('[') && l.ends_with(']') {
                    section = l[1..l.len() - 1].to_string();
                    continue;
                }
                if c.section.is_some_and(|s| !s.eq_ignore_ascii_case(&section)) {
                    continue;
                }
                if let Some((cle, valeur)) = l.split_once('=') {
                    if cle.trim().eq_ignore_ascii_case(c.cle) {
                        return Some((i, valeur.trim().to_string()));
                    }
                }
            }
            Format::Valve => {
                let morceaux: Vec<&str> = l.split('"').collect();
                // "cle"  "valeur" → ["", cle, espaces, valeur, ""]
                if morceaux.len() >= 5 && morceaux[1].eq_ignore_ascii_case(c.cle) && morceaux[2].trim().is_empty() {
                    return Some((i, morceaux[3].to_string()));
                }
            }
        }
    }
    None
}

fn remplacer(ligne: &str, format: Format, valeur: &str) -> String {
    let (corps, fin) = match ligne.strip_suffix('\r') {
        Some(c) => (c, "\r"),
        None => (ligne, ""),
    };
    match format {
        Format::Ini => {
            let (cle, _) = corps.split_once('=').unwrap_or((corps, ""));
            format!("{cle}={valeur}{fin}")
        }
        Format::Valve => {
            // On garde l'indentation, la clé et l'espacement : seule la valeur change
            let mut morceaux: Vec<String> = corps.split('"').map(str::to_string).collect();
            if morceaux.len() >= 5 {
                morceaux[3] = valeur.to_string();
            }
            format!("{}{fin}", morceaux.join("\""))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UE: &str = "[/Script/FortniteGame.FortGameUserSettings]\r\nbMotionBlur=True\r\nbUseVSync=True\r\nFrameRateLimit=0.000000\r\n\r\n[ScalabilityGroups]\r\nsg.ShadowQuality=3\r\nsg.ViewDistanceQuality=3\r\n";

    #[test]
    fn ini_seules_les_cles_presentes_changent() {
        let ch = [
            Changement { section: Some("ScalabilityGroups"), cle: "sg.ShadowQuality", valeur: "0" },
            Changement { section: None, cle: "bMotionBlur", valeur: "False" },
            Changement { section: None, cle: "sg.Inexistante", valeur: "0" },
        ];
        assert_eq!(lire(UE, Format::Ini, &ch), vec![Some("3".into()), Some("True".into()), None]);
        let nouveau = appliquer(UE, Format::Ini, &ch);
        assert_eq!(
            nouveau,
            "[/Script/FortniteGame.FortGameUserSettings]\r\nbMotionBlur=False\r\nbUseVSync=True\r\nFrameRateLimit=0.000000\r\n\r\n[ScalabilityGroups]\r\nsg.ShadowQuality=0\r\nsg.ViewDistanceQuality=3\r\n"
        );
        assert!(!nouveau.contains("Inexistante"));
    }

    #[test]
    fn ini_respecte_la_section() {
        let t = "[A]\nVSync=1\n[B]\nVSync=1\n";
        let ch = [Changement { section: Some("B"), cle: "VSync", valeur: "0" }];
        assert_eq!(appliquer(t, Format::Ini, &ch), "[A]\nVSync=1\n[B]\nVSync=0\n");
    }

    #[test]
    fn format_valve() {
        let t = "\"VideoConfig\"\r\n{\r\n\t\"setting.mat_vsync\"\t\t\"1\"\r\n\t\"setting.shaderquality\"\t\t\"2\"\r\n}\r\n";
        let ch = [Changement { section: None, cle: "setting.mat_vsync", valeur: "0" }];
        assert_eq!(lire(t, Format::Valve, &ch), vec![Some("1".into())]);
        assert_eq!(appliquer(t, Format::Valve, &ch), t.replace("\"setting.mat_vsync\"\t\t\"1\"", "\"setting.mat_vsync\"\t\t\"0\""));
    }

    #[test]
    fn encodages_conserves() {
        for enc in [Encodage::Utf8, Encodage::Utf8Bom, Encodage::Utf16, Encodage::Latin1] {
            let texte = "[S]\r\nNom=Équipe\r\n";
            let (relu, e) = decoder(&encoder(texte, enc));
            assert_eq!((relu.as_str(), e), (texte, if enc == Encodage::Latin1 { Encodage::Latin1 } else { enc }));
        }
    }
}
