// Programmes installés et désinstallation (comme « Applications installées » de Windows).

use serde::Serialize;
use std::os::windows::process::CommandExt;
use std::process::Command;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
use winreg::RegKey;

const SOURCES: [(bool, &str); 3] = [
    (true, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
    (true, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
    (false, r"Software\Microsoft\Windows\CurrentVersion\Uninstall"),
];

#[derive(Serialize)]
pub struct Programme {
    /// Identifiant interne (source + clé) renvoyé par l'interface pour désinstaller
    pub id: String,
    pub nom: String,
    pub editeur: String,
    pub version: String,
    pub taille_ko: u64,
    /// AAAAMMJJ
    pub date: String,
    #[serde(skip)]
    commande: String,
}

pub fn liste() -> Vec<Programme> {
    let mut res: Vec<Programme> = Vec::new();
    for (i, (machine, chemin)) in SOURCES.iter().enumerate() {
        let ruche = RegKey::predef(if *machine { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER });
        let Ok(base) = ruche.open_subkey_with_flags(chemin, KEY_READ) else { continue };
        for cle in base.enum_keys().flatten() {
            let Ok(k) = base.open_subkey_with_flags(&cle, KEY_READ) else { continue };
            let texte = |n: &str| k.get_value::<String, _>(n).unwrap_or_default();
            let nom = texte("DisplayName");
            let commande = texte("UninstallString");
            // Composants système et mises à jour : Windows les cache aussi
            let cache = k.get_value::<u32, _>("SystemComponent").unwrap_or(0) == 1 || !texte("ParentKeyName").is_empty();
            if nom.is_empty() || commande.is_empty() || cache || res.iter().any(|p| p.nom == nom) {
                continue;
            }
            res.push(Programme {
                id: format!("{i}|{cle}"),
                nom,
                editeur: texte("Publisher"),
                version: texte("DisplayVersion"),
                taille_ko: k.get_value::<u32, _>("EstimatedSize").unwrap_or(0) as u64,
                date: texte("InstallDate"),
                commande,
            });
        }
    }
    res.sort_by_key(|p| p.nom.to_lowercase());
    res
}

/// Sépare « "C:\…\uninst.exe" /S » ou « MsiExec.exe /X{…} » en programme et arguments.
fn decouper(commande: &str) -> (String, String) {
    let c = commande.trim();
    if let Some(reste) = c.strip_prefix('"') {
        if let Some(fin) = reste.find('"') {
            return (reste[..fin].to_string(), reste[fin + 1..].trim().to_string());
        }
    }
    match c.to_lowercase().find(".exe") {
        Some(i) => (c[..i + 4].to_string(), c[i + 4..].trim().to_string()),
        None => (c.to_string(), String::new()),
    }
}

/// Lance le désinstalleur du programme (sa propre fenêtre s'ouvre).
pub fn desinstaller(id: &str) -> Result<(), String> {
    let p = liste().into_iter().find(|p| p.id == id).ok_or("Programme introuvable : il est peut-être déjà désinstallé")?;
    let (programme, arguments) = decouper(&p.commande);
    Command::new(&programme)
        .raw_arg(&arguments)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Impossible de lancer le désinstalleur de {} : {e}", p.nom))
}

#[cfg(test)]
mod tests {
    use super::decouper;

    #[test]
    fn commandes_de_desinstallation() {
        assert_eq!(decouper(r#""C:\Program Files\Jeu\unins000.exe" /SILENT"#), (r"C:\Program Files\Jeu\unins000.exe".into(), "/SILENT".into()));
        assert_eq!(decouper("MsiExec.exe /X{1234}"), ("MsiExec.exe".into(), "/X{1234}".into()));
        assert_eq!(decouper(r"C:\Program Files\Outil\uninstall.exe"), (r"C:\Program Files\Outil\uninstall.exe".into(), String::new()));
    }
}

/* ---------- Applis préinstallées par Windows ---------- */

/// (début du nom du paquet, nom affiché, description, cochée par défaut)
/// Jamais listés : Microsoft Store, Edge, composants Xbox de connexion (nécessaires aux jeux Xbox/Game Pass).
const APPLIS_CONNUES: &[(&str, &str, &str, bool)] = &[
    ("king.com.", "Jeux King (Candy Crush…)", "Installés d'office par Windows.", true),
    ("BytedancePte.Ltd.TikTok", "TikTok", "Installé d'office par Windows.", true),
    ("Facebook.", "Facebook et Instagram", "Installés d'office par Windows.", true),
    ("Disney.", "Disney+", "Installé d'office par Windows.", true),
    ("7EE7776C.LinkedInforWindows", "LinkedIn", "Installé d'office par Windows.", true),
    ("SpotifyAB.SpotifyMusic", "Spotify", "Garde-le si tu t'en sers.", false),
    ("Clipchamp.Clipchamp", "Clipchamp", "Montage vidéo de Microsoft.", true),
    ("Microsoft.BingNews", "Actualités", "Fil d'actualités de Microsoft.", true),
    ("Microsoft.BingWeather", "Météo", "", true),
    ("Microsoft.BingSearch", "Recherche Bing", "Recherche web intégrée à Windows.", true),
    ("Microsoft.MicrosoftSolitaireCollection", "Solitaire", "Avec publicités.", true),
    ("Microsoft.GetHelp", "Obtenir de l'aide", "", true),
    ("Microsoft.Getstarted", "Astuces", "", true),
    ("Microsoft.WindowsFeedbackHub", "Hub de commentaires", "", true),
    ("Microsoft.WindowsMaps", "Cartes", "", true),
    ("Microsoft.People", "Contacts", "", true),
    ("Microsoft.MicrosoftOfficeHub", "Microsoft 365", "Raccourci publicitaire vers Office (pas Office lui-même).", true),
    ("Microsoft.PowerAutomateDesktop", "Power Automate", "Automatisation pour les entreprises.", true),
    ("MicrosoftTeams", "Teams (version perso)", "Se lance avec Windows et tourne en arrière-plan.", true),
    ("MSTeams", "Teams", "Garde-le si ton école ou ton travail l'utilise.", false),
    ("Microsoft.SkypeApp", "Skype", "Service arrêté par Microsoft.", true),
    ("Microsoft.ZuneVideo", "Films et TV", "", true),
    ("Microsoft.549981C3F5F10", "Cortana", "Assistant arrêté par Microsoft.", true),
    ("Microsoft.Copilot", "Copilot (application)", "Assistant IA de Microsoft.", true),
    ("Microsoft.Windows.DevHome", "Dev Home", "Outil pour développeurs.", true),
    ("MicrosoftCorporationII.MicrosoftFamily", "Microsoft Family", "Contrôle parental.", true),
    ("Microsoft.MixedReality.Portal", "Portail de réalité mixte", "Pour les casques Windows Mixed Reality (abandonnés).", true),
    ("Microsoft.Microsoft3DViewer", "Visionneuse 3D", "", true),
    ("Microsoft.MSPaint", "Paint 3D", "L'ancien Paint 3D (le Paint classique n'est pas touché).", true),
    ("Microsoft.Office.OneNote", "OneNote (version Store)", "", false),
    ("Microsoft.Todos", "Microsoft To Do", "Garde-le si tu t'en sers.", false),
    ("Microsoft.OutlookForWindows", "Outlook (nouveau)", "Garde-le si tu lis tes mails avec.", false),
    ("microsoft.windowscommunicationsapps", "Courrier et Calendrier", "Ancienne appli remplacée par Outlook.", false),
    ("Microsoft.YourPhone", "Mobile connecté", "Relie ton téléphone ; tourne souvent en arrière-plan.", false),
    ("MicrosoftCorporationII.QuickAssist", "Assistance rapide", "Prise en main à distance.", false),
    ("Microsoft.ZuneMusic", "Lecteur multimédia", "Lecteur de musique et vidéos par défaut.", false),
    ("Microsoft.GamingApp", "Xbox", "Nécessaire pour le PC Game Pass.", false),
    ("Microsoft.XboxGamingOverlay", "Xbox Game Bar", "Nécessaire pour Win+G et les captures Xbox.", false),
];

#[derive(Serialize)]
pub struct AppliWindows {
    pub paquet: String,
    pub nom: String,
    pub description: String,
    pub defaut: bool,
}

pub fn applis_windows() -> Result<Vec<AppliWindows>, String> {
    let installees = crate::ps::json("ConvertTo-Json -Compress -InputObject @(Get-AppxPackage | Where-Object { -not $_.NonRemovable } | ForEach-Object { $_.Name } | Sort-Object -Unique)")?;
    let noms: Vec<String> = serde_json::from_value(installees).unwrap_or_default();
    Ok(APPLIS_CONNUES
        .iter()
        .flat_map(|&(motif, nom, description, defaut)| {
            noms.iter().filter(move |n| n.starts_with(motif)).map(move |n| AppliWindows {
                paquet: n.clone(),
                nom: nom.into(),
                description: description.into(),
                defaut,
            })
        })
        .collect())
}

/// Désinstalle pour l'utilisateur et retire du modèle de Windows (pour les nouveaux comptes et
/// certaines mises à jour). Seuls les paquets de la liste connue sont acceptés.
pub fn supprimer_applis(paquets: &[String]) -> Result<serde_json::Value, String> {
    let autorises: Vec<&String> = paquets.iter().filter(|p| APPLIS_CONNUES.iter().any(|(m, ..)| p.starts_with(m))).collect();
    if autorises.is_empty() {
        return Ok(serde_json::json!([]));
    }
    let liste = autorises.iter().map(|p| crate::ps::chaine(p)).collect::<Vec<_>>().join(",");
    crate::ps::json(&format!(
        r#"$res = @()
foreach ($n in @({liste})) {{
  try {{
    Get-AppxPackage -Name $n | Remove-AppxPackage -ErrorAction Stop
    Get-AppxProvisionedPackage -Online -ErrorAction SilentlyContinue | Where-Object DisplayName -eq $n | Remove-AppxProvisionedPackage -Online -ErrorAction SilentlyContinue | Out-Null
    $res += [ordered]@{{ paquet = $n; ok = $true; erreur = $null }}
  }} catch {{
    $res += [ordered]@{{ paquet = $n; ok = $false; erreur = $_.Exception.Message }}
  }}
}}
ConvertTo-Json -Compress -Depth 3 -InputObject @($res)"#
    ))
}
