// PowerShell, utilisé seulement pour les tâches ponctuelles (analyse, point de restauration, démarrage auto).
// Le script passe en -EncodedCommand : aucun souci de guillemets ni d'accents.

use crate::systeme::CREATE_NO_WINDOW;
use std::os::windows::process::CommandExt;
use std::process::Command;

const PRELUDE: &str = r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
function Lire($chemin, $nom) {
  try { (Get-ItemProperty -LiteralPath $chemin -Name $nom -ErrorAction Stop).$nom } catch { $null }
}
"#;

pub fn base64(octets: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(octets.len().div_ceil(3) * 4);
    for bloc in octets.chunks(3) {
        let n = (bloc[0] as u32) << 16 | (*bloc.get(1).unwrap_or(&0) as u32) << 8 | *bloc.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= bloc.len() {
                s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

pub fn executer(script: &str) -> Result<String, String> {
    let complet = format!("{PRELUDE}{script}");
    let utf16: Vec<u8> = complet.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let sortie = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &base64(&utf16)])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("PowerShell introuvable : {e}"))?;
    if sortie.status.success() {
        Ok(String::from_utf8_lossy(&sortie.stdout).trim_start_matches('\u{feff}').trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&sortie.stderr);
        Err(message_erreur(&err))
    }
}

/// Première vraie ligne d'erreur. PowerShell écrit parfois ses erreurs en CLIXML (« #< CLIXML ») :
/// on en extrait le texte des balises <S S="Error">.
fn message_erreur(err: &str) -> String {
    if err.trim_start().starts_with("#< CLIXML") {
        let texte: String = err
            .split("<S S=\"Error\">")
            .skip(1)
            .filter_map(|m| m.split("</S>").next())
            .collect::<Vec<_>>()
            .join("")
            .replace("_x000D__x000A_", "\n")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&")
            .replace("&apos;", "'")
            .replace("&quot;", "\"");
        if let Some(l) = texte.lines().map(str::trim).find(|l| !l.is_empty()) {
            return l.to_string();
        }
    }
    err.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("#< CLIXML")).unwrap_or("Erreur PowerShell").to_string()
}

pub fn json(script: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(&executer(script)?).map_err(|e| format!("Réponse PowerShell illisible : {e}"))
}

/// Chaîne PowerShell entre apostrophes.
pub fn chaine(texte: &str) -> String {
    format!("'{}'", texte.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn erreurs_clixml() {
        let e = "#< CLIXML\r\n<Objs Version=\"1.1.0.1\"><Obj S=\"progress\" RefId=\"0\"></Obj><S S=\"Error\">Fichier FFmpeg corrompu_x000D__x000A_</S><S S=\"Error\">Au caractère Ligne:3_x000D__x000A_</S></Objs>";
        assert_eq!(super::message_erreur(e), "Fichier FFmpeg corrompu");
        assert_eq!(super::message_erreur("\r\nAccès refusé\r\n"), "Accès refusé");
    }

    #[test]
    fn base64_standard() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foo"), "Zm9v");
        assert_eq!(super::base64(b"foobar"), "Zm9vYmFy");
    }
}
