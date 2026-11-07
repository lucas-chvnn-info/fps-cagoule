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

fn base64(octets: &[u8]) -> String {
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
        Err(err.lines().find(|l| !l.trim().is_empty()).unwrap_or("Erreur PowerShell").trim().to_string())
    }
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
    fn base64_standard() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foo"), "Zm9v");
        assert_eq!(super::base64(b"foobar"), "Zm9vYmFy");
    }
}
