// Dossiers sur un serveur (partage Windows/Samba) vus depuis FPS Cagoule.
// L'app tourne en administrateur : Windows lui cache les lecteurs réseau (Z:) et la connexion
// ouverte dans l'Explorateur. On remplace donc Z: par \\serveur\partage et on se connecte nous-mêmes.

use std::path::Path;

fn large(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Partie « serveur\partage » d'un chemin réseau (\\192.168.1.204\fichiers\Clips → (192.168.1.204, \\192.168.1.204\fichiers))
pub fn racine(chemin: &str) -> Option<(String, String)> {
    let reste = chemin.strip_prefix(r"\\")?;
    let mut parts = reste.split(['\\', '/']).filter(|p| !p.is_empty());
    let (serveur, partage) = (parts.next()?, parts.next()?);
    if serveur == "?" || serveur == "." {
        return None;
    }
    Some((serveur.to_string(), format!(r"\\{serveur}\{partage}")))
}

/// Chemin du serveur d'un lecteur réseau (Z: → \\serveur\partage), lu dans les lecteurs enregistrés de l'utilisateur
#[cfg(windows)]
fn lecteur_reseau(lettre: char) -> Option<String> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    let cle = large(&format!(r"Network\{}", lettre.to_ascii_uppercase()));
    let valeur = large("RemotePath");
    let mut tampon = [0u16; 1024];
    let mut taille = (tampon.len() * 2) as u32;
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, cle.as_ptr(), valeur.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), tampon.as_mut_ptr().cast(), &mut taille) };
    if r != 0 {
        return None;
    }
    let n = tampon.iter().position(|&c| c == 0).unwrap_or(0);
    let s = String::from_utf16_lossy(&tampon[..n]);
    s.starts_with(r"\\").then(|| s.trim_end_matches('\\').to_string())
}
#[cfg(not(windows))]
fn lecteur_reseau(_: char) -> Option<String> {
    None
}

/// Remplace un lecteur réseau (Z:\Clips) par son chemin serveur (\\serveur\partage\Clips) ; sinon chemin inchangé
pub fn resoudre(chemin: &str) -> String {
    let c: Vec<char> = chemin.chars().collect();
    if c.len() >= 2 && c[0].is_ascii_alphabetic() && c[1] == ':' && (c.len() == 2 || c[2] == '\\' || c[2] == '/') {
        let lecteur = format!("{}:\\", c[0]);
        // Lecteur visible par l'app (disque local, clé USB…) : on n'y touche pas
        if !Path::new(&lecteur).exists() {
            if let Some(serveur) = lecteur_reseau(c[0]) {
                let suite = chemin[2..].trim_start_matches(['\\', '/']);
                return if suite.is_empty() { serveur } else { format!(r"{serveur}\{suite}") };
            }
        }
    }
    chemin.to_string()
}

/// Lettre de lecteur introuvable pour l'app (ni disque, ni lecteur réseau enregistré)
pub fn lecteur_inconnu(chemin: &str) -> bool {
    let c: Vec<char> = chemin.chars().collect();
    c.len() >= 2 && c[0].is_ascii_alphabetic() && c[1] == ':' && !Path::new(&format!("{}:\\", c[0])).exists()
}

/// Se connecte au partage avec un identifiant, et le garde dans le Gestionnaire d'identification de Windows
/// (comme « Mémoriser mes informations » dans l'Explorateur) pour les prochains démarrages.
#[cfg(windows)]
pub fn connecter(chemin: &str, identifiant: &str, mot_de_passe: &str) -> Result<(), String> {
    use windows_sys::Win32::NetworkManagement::WNet::{WNetAddConnection2W, WNetCancelConnection2W, NETRESOURCEW, RESOURCETYPE_DISK};
    use windows_sys::Win32::Security::Credentials::{CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_DOMAIN_PASSWORD};
    let (serveur, partage) = racine(chemin).ok_or("Chemin du serveur invalide (exemple : \\\\192.168.1.204\\fichiers)")?;
    let (partage_w, ident_w, mdp_w, serveur_w) = (large(&partage), large(identifiant), large(mot_de_passe), large(&serveur));

    // Identifiant mémorisé pour ce serveur
    let blob: Vec<u16> = mot_de_passe.encode_utf16().collect();
    let mut cred: CREDENTIALW = unsafe { std::mem::zeroed() };
    cred.Type = CRED_TYPE_DOMAIN_PASSWORD;
    cred.TargetName = serveur_w.as_ptr() as *mut u16;
    cred.UserName = ident_w.as_ptr() as *mut u16;
    cred.CredentialBlob = blob.as_ptr() as *mut u8;
    cred.CredentialBlobSize = (blob.len() * 2) as u32;
    cred.Persist = CRED_PERSIST_LOCAL_MACHINE;
    let memorise = unsafe { CredWriteW(&cred, 0) } != 0;

    // Connexion tout de suite (sans lettre de lecteur)
    let mut res: NETRESOURCEW = unsafe { std::mem::zeroed() };
    res.dwType = RESOURCETYPE_DISK;
    res.lpRemoteName = partage_w.as_ptr() as *mut u16;
    let mut r = unsafe { WNetAddConnection2W(&res, mdp_w.as_ptr(), ident_w.as_ptr(), 0) };
    if r == 1219 {
        // Déjà connecté avec un autre identifiant : on ferme cette connexion puis on réessaie
        unsafe { WNetCancelConnection2W(partage_w.as_ptr(), 0, 1) };
        r = unsafe { WNetAddConnection2W(&res, mdp_w.as_ptr(), ident_w.as_ptr(), 0) };
    }
    match r {
        0 => Ok(()),
        86 | 1326 | 5 => Err("Identifiant ou mot de passe refusé par le serveur.".into()),
        53 | 67 | 1231 | 1222 => Err(format!("Serveur {serveur} injoignable : vérifie qu'il est allumé et le nom du partage.")),
        1219 if memorise => Ok(()),
        e => Err(format!("Connexion au serveur impossible (code {e}).")),
    }
}
#[cfg(not(windows))]
pub fn connecter(_: &str, _: &str, _: &str) -> Result<(), String> {
    Err("Windows uniquement".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn racine_du_partage() {
        assert_eq!(racine(r"\\192.168.1.204\fichiers\Clips\R6"), Some(("192.168.1.204".into(), r"\\192.168.1.204\fichiers".into())));
        assert_eq!(racine(r"\\nas\clips"), Some(("nas".into(), r"\\nas\clips".into())));
        assert_eq!(racine(r"\\nas"), None);
        assert_eq!(racine(r"\\?\C:\x"), None);
        assert_eq!(racine(r"C:\Clips"), None);
    }

    #[test]
    fn chemins_locaux_inchanges() {
        assert_eq!(resoudre(r"\\nas\clips"), r"\\nas\clips");
        assert_eq!(resoudre("dossier"), "dossier");
    }
}
