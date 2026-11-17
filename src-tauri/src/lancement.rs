// Bouton « Jouer » : lance un jeu par son launcher (Steam, Epic, Ubisoft Connect, Riot) ou, à défaut, par son exécutable.

use crate::bibliotheques::{self, Lancement};
use crate::config::Profil;
use std::path::{Path, PathBuf};

fn large(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Ouvre un lien ou un programme comme un double-clic (ShellExecute)
pub(crate) fn ouvrir(cible: &str, arguments: &str, dossier: Option<&Path>) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    let (verbe, cible_w, args_w) = (large("open"), large(cible), large(arguments));
    let dossier_w = dossier.map(|d| large(&d.to_string_lossy()));
    let r = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verbe.as_ptr(),
            cible_w.as_ptr(),
            if arguments.is_empty() { std::ptr::null() } else { args_w.as_ptr() },
            dossier_w.as_ref().map_or(std::ptr::null(), |d| d.as_ptr()),
            1, // SW_SHOWNORMAL
        )
    };
    // ShellExecute renvoie une valeur > 32 en cas de succès
    if r as isize > 32 {
        Ok(())
    } else {
        Err(format!("Windows n'a pas pu lancer {cible}"))
    }
}

/// Premier exécutable du profil trouvé dans le dossier du jeu (4 niveaux au plus)
fn chercher_exe(dossier: &Path, noms: &[String]) -> Option<PathBuf> {
    let mut pile = vec![(dossier.to_path_buf(), 0u8)];
    let mut vus = 0usize;
    while let Some((d, profondeur)) = pile.pop() {
        for e in std::fs::read_dir(&d).ok()?.flatten() {
            vus += 1;
            if vus > 20_000 {
                return None;
            }
            let p = e.path();
            if p.is_dir() {
                if profondeur < 4 {
                    pile.push((p, profondeur + 1));
                }
            } else if noms.iter().any(|n| e.file_name().to_string_lossy().eq_ignore_ascii_case(n)) {
                return Some(p);
            }
        }
    }
    None
}

/// Le jeu installé correspondant au profil et la façon de le lancer
pub fn jouable(profil: &Profil, jeux: &[bibliotheques::JeuTrouve]) -> Option<(Option<u32>, bool)> {
    let j = crate::images::meilleur(profil, jeux)?;
    Some((j.steam, j.lancement.is_some() || !j.dossier.is_empty()))
}

/// Lance le jeu. `compte` : SteamID64 du compte Steam à utiliser (jeux Steam). Renvoie un message pour l'interface.
pub fn lancer(profil: &Profil, compte: Option<&str>) -> Result<String, String> {
    let jeux = bibliotheques::sources(false);
    let j = crate::images::meilleur(profil, &jeux).ok_or("Jeu introuvable dans tes bibliothèques (Steam, Epic, Ubisoft, Riot, GOG…). Lance-le depuis son launcher.")?;
    let mut message = format!("Lancement de {}…", profil.nom);
    if let (Some(id), Some(_)) = (compte, j.steam) {
        if crate::steam_comptes::basculer(id)? {
            message = format!("Steam redémarre sur ton autre compte, puis {} se lance…", profil.nom);
        }
    }
    match &j.lancement {
        Some(Lancement::Lien(lien)) => ouvrir(lien, "", None)?,
        Some(Lancement::Programme(programme, args)) => {
            let dossier = Path::new(programme).parent();
            ouvrir(programme, &args.join(" "), dossier)?
        }
        None => {
            let exe = chercher_exe(Path::new(&j.dossier), &profil.exes).ok_or("Exécutable du jeu introuvable : lance-le depuis son launcher.")?;
            ouvrir(&exe.to_string_lossy(), "", exe.parent())?
        }
    }
    Ok(message)
}
