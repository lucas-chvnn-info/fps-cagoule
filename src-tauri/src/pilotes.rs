// Pilotes : tout au même endroit, sans les applications des fabricants.
// - liste des pilotes installés (cartes graphique, réseau, son, Bluetooth)
// - dernier pilote NVIDIA via l'API publique du site de NVIDIA, téléchargé et lancé par l'app
// - autres pilotes via le catalogue Windows Update (API COM de Windows)

use crate::ps;
use crate::systeme::CREATE_NO_WINDOW;
use serde::Serialize;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

pub fn installes() -> Result<serde_json::Value, String> {
    ps::json(
        r#"ConvertTo-Json -Compress -Depth 3 -InputObject @(Get-CimInstance Win32_PnPSignedDriver |
  Where-Object { $_.DeviceClass -in 'DISPLAY','NET','MEDIA','BLUETOOTH' -and $_.DeviceName -and $_.Manufacturer -notmatch '^(Microsoft|Microsoft Corporation)$' -and $_.DeviceName -notmatch 'Virtual|Hyper-V|WAN Miniport|Kernel Debug|Remote' } |
  Sort-Object DeviceClass, DeviceName -Unique |
  ForEach-Object { [ordered]@{
    nom = $_.DeviceName; classe = $_.DeviceClass; fabricant = $_.Manufacturer; version = $_.DriverVersion
    date = if ($_.DriverDate) { ([datetime]$_.DriverDate).ToString('yyyy-MM-dd') } else { $null }
  } })"#,
    )
}

/// Carte graphique NVIDIA : version installée et dernier pilote Game Ready publié.
pub fn nvidia() -> Result<serde_json::Value, String> {
    ps::json(
        r#"[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$gpu = Get-CimInstance Win32_VideoController | Where-Object Name -match 'NVIDIA' | Select-Object -First 1
if (-not $gpu) { 'null'; return }
# 32.0.15.6211 -> 562.11 : les 5 derniers chiffres donnent la version NVIDIA
$chiffres = ($gpu.DriverVersion -split '\.')[-2..-1] -join ''
$installe = $chiffres.Substring($chiffres.Length - 5, 3) + '.' + $chiffres.Substring($chiffres.Length - 2)
$nom = ($gpu.Name -replace '^NVIDIA\s+', '').Trim()
$xml = [xml](Invoke-WebRequest -UseBasicParsing -TimeoutSec 20 'https://www.nvidia.com/Download/API/lookupValueSearch.aspx?TypeID=3').Content
$produit = $xml.LookupValueSearch.LookupValues.LookupValue | Where-Object { ($_.Name -replace '^NVIDIA\s+', '').Trim() -eq $nom } | Select-Object -First 1
$res = [ordered]@{ gpu = $gpu.Name; installe = $installe; derniere = $null; date = $null; url = $null; taille = $null }
if ($produit) {
  $os = if ([Environment]::OSVersion.Version.Build -ge 22000) { 135 } else { 57 }
  $u = "https://gfwsl.geforce.com/services_toolkit/services/com/nvidia/services/AjaxDriverService.php?func=DriverManualLookup&psid=$($produit.ParentID)&pfid=$($produit.Value)&osID=$os&languageCode=1033&beta=0&isWHQL=1&dltype=-1&dch=1&upCRD=0&qnf=0&sort1=0&numberOfResults=1"
  $info = (Invoke-RestMethod -UseBasicParsing -TimeoutSec 20 $u).IDS[0].downloadInfo
  $res.derniere = $info.Version; $res.date = $info.ReleaseDateTime; $res.url = $info.DownloadURL; $res.taille = $info.DownloadURLFileSize
}
$res | ConvertTo-Json -Compress"#,
    )
}

/// Pilotes proposés par Windows Update (ceux de « Mises à jour facultatives »).
pub fn wu_rechercher() -> Result<serde_json::Value, String> {
    ps::json(
        r#"$s = New-Object -ComObject Microsoft.Update.Session
$r = $s.CreateUpdateSearcher().Search("IsInstalled=0 and Type='Driver' and IsHidden=0")
ConvertTo-Json -Compress -Depth 3 -InputObject @($r.Updates | ForEach-Object { [ordered]@{
  id = $_.Identity.UpdateID; titre = $_.Title; fabricant = $_.DriverManufacturer; classe = $_.DriverClass
  date = if ($_.DriverVerDate) { ([datetime]$_.DriverVerDate).ToString('yyyy-MM-dd') } else { $null }
  taille = $_.MaxDownloadSize
} })"#,
    )
}

pub fn wu_installer(ids: &[String]) -> Result<serde_json::Value, String> {
    let liste = ids.iter().map(|i| ps::chaine(i)).collect::<Vec<_>>().join(",");
    ps::json(&format!(
        r#"$ids = @({liste})
$s = New-Object -ComObject Microsoft.Update.Session
$r = $s.CreateUpdateSearcher().Search("IsInstalled=0 and Type='Driver' and IsHidden=0")
$col = New-Object -ComObject Microsoft.Update.UpdateColl
foreach ($u in $r.Updates) {{ if ($ids -contains $u.Identity.UpdateID) {{ if (-not $u.EulaAccepted) {{ $u.AcceptEula() }}; [void]$col.Add($u) }} }}
if ($col.Count -eq 0) {{ throw 'Ces pilotes ne sont plus proposés : relance la recherche' }}
$d = $s.CreateUpdateDownloader(); $d.Updates = $col; [void]$d.Download()
$i = $s.CreateUpdateInstaller(); $i.Updates = $col; $res = $i.Install()
[ordered]@{{
  redemarrage = $res.RebootRequired
  details = @(0..($col.Count - 1) | ForEach-Object {{ [ordered]@{{ titre = $col.Item($_).Title; ok = $res.GetUpdateResult($_).ResultCode -in 2, 3 }} }})
}} | ConvertTo-Json -Compress -Depth 4"#
    ))
}

/* ---------- Téléchargement du pilote NVIDIA ---------- */

#[derive(Serialize, Clone, Default)]
pub struct Telechargement {
    pub version: String,
    pub recu: u64,
    pub total: u64,
    pub fini: bool,
    pub erreur: Option<String>,
    #[serde(skip)]
    fichier: PathBuf,
}

static EN_COURS: Mutex<Option<Telechargement>> = Mutex::new(None);

fn curl(args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new("curl.exe").args(args).creation_flags(CREATE_NO_WINDOW).output()
}

/// Lance le téléchargement en arrière-plan (curl.exe est fourni avec Windows 10 et 11).
pub fn telecharger_nvidia(url: &str, version: &str) -> Result<(), String> {
    if !url.starts_with("https://us.download.nvidia.com/") && !url.starts_with("https://international.download.nvidia.com/") {
        return Err("Adresse de téléchargement inattendue".into());
    }
    if EN_COURS.lock().unwrap().as_ref().is_some_and(|t| !t.fini) {
        return Err("Un téléchargement est déjà en cours".into());
    }
    let dossier = std::env::temp_dir().join("fps-cagoule");
    std::fs::create_dir_all(&dossier).map_err(|e| e.to_string())?;
    let fichier = dossier.join(format!("nvidia-{version}.exe"));
    *EN_COURS.lock().unwrap() = Some(Telechargement { version: version.into(), fichier: fichier.clone(), ..Default::default() });
    let url = url.to_string();
    std::thread::spawn(move || {
        // Taille totale pour la barre de progression
        let total = curl(&["-sIL", &url])
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .and_then(|t| t.lines().filter_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<u64>().ok())).flatten().last())
            .unwrap_or(0);
        if let Some(t) = EN_COURS.lock().unwrap().as_mut() {
            t.total = total;
        }
        let res = curl(&["-sSL", "--fail", "--retry", "3", "-o", &fichier.to_string_lossy(), &url]);
        let mut etat = EN_COURS.lock().unwrap();
        if let Some(t) = etat.as_mut() {
            t.fini = true;
            t.recu = std::fs::metadata(&fichier).map_or(0, |m| m.len());
            match res {
                Ok(o) if o.status.success() => {}
                Ok(o) => t.erreur = Some(format!("Téléchargement échoué : {}", String::from_utf8_lossy(&o.stderr).trim())),
                Err(e) => t.erreur = Some(format!("curl.exe introuvable : {e}")),
            }
        }
    });
    Ok(())
}

pub fn progression() -> Option<Telechargement> {
    let mut etat = EN_COURS.lock().unwrap();
    let t = etat.as_mut()?;
    if !t.fini {
        t.recu = std::fs::metadata(&t.fichier).map_or(0, |m| m.len());
    }
    Some(t.clone())
}

/// Ouvre l'installateur NVIDIA téléchargé (l'utilisateur garde la main sur les options).
pub fn installer_nvidia() -> Result<(), String> {
    let t = progression().ok_or("Aucun pilote téléchargé")?;
    if !t.fini || t.erreur.is_some() {
        return Err("Le téléchargement n'est pas terminé".into());
    }
    Command::new(&t.fichier).spawn().map(|_| ()).map_err(|e| format!("Impossible de lancer l'installateur : {e}"))
}
