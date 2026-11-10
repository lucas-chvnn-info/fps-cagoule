// Checklist BIOS : ce que Windows peut lire des réglages du BIOS (XMP, Resizable BAR, mode UEFI, âge du BIOS).
// Rien n'est modifié ici : seul le BIOS peut changer ces réglages.

use crate::ps;

const SCRIPT: &str = r#"
$carte = Get-CimInstance Win32_BaseBoard | Select-Object -First 1
$bios = Get-CimInstance Win32_BIOS | Select-Object -First 1
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$ram = @(Get-CimInstance Win32_PhysicalMemory | ForEach-Object { [pscustomobject]@{ vitesse = [int]$_.Speed; reelle = [int]$_.ConfiguredClockSpeed; type = [int]$_.SMBIOSMemoryType } })
$gpu = Get-CimInstance Win32_VideoController | Where-Object { $_.PNPDeviceID -like 'PCI\*' } | Select-Object -First 1

# Resizable BAR : taille de la fenêtre mémoire du processeur vers la carte graphique (256 Mo sans, la VRAM avec)
$bar = $null; $source = $null
$smi = Join-Path $env:SystemRoot 'System32\nvidia-smi.exe'
if (Test-Path $smi) {
  try {
    $txt = (& $smi -q -d MEMORY) -join "`n"
    if ($txt -match 'BAR1 Memory Usage\s*\n\s*Total\s*:\s*(\d+)\s*MiB') { $bar = [int]$Matches[1]; $source = 'nvidia' }
  } catch {}
}
if (-not $bar -and $gpu) {
  try {
    $plages = @{}
    Get-CimInstance Win32_DeviceMemoryAddress | ForEach-Object { $plages[[string]$_.StartingAddress] = [uint64]$_.EndingAddress - [uint64]$_.StartingAddress + 1 }
    $max = [uint64]0
    Get-CimInstance Win32_PNPAllocatedResource | Where-Object { $_.Dependent.DeviceID -eq $gpu.PNPDeviceID } | ForEach-Object {
      $t = $plages[[string]$_.Antecedent.StartingAddress]
      if ($t -and $t -gt $max) { $max = $t }
    }
    if ($max -gt 0) { $bar = [int]($max / 1MB); $source = 'windows' }
  } catch {}
}

[pscustomobject]@{
  fabricant = "$($carte.Manufacturer)".Trim()
  modele = "$($carte.Product)".Trim()
  biosVersion = "$($bios.SMBIOSBIOSVersion)".Trim()
  biosDate = if ($bios.ReleaseDate) { $bios.ReleaseDate.ToString('yyyy-MM-dd') } else { $null }
  uefi = ($env:firmware_type -eq 'UEFI')
  cpu = "$($cpu.Name)".Trim()
  ram = $ram
  gpu = if ($gpu) { $gpu.Name } else { $null }
  barMo = $bar
  barSource = $source
} | ConvertTo-Json -Depth 3 -Compress
"#;

pub fn infos() -> Result<serde_json::Value, String> {
    ps::json(SCRIPT)
}

/// Redémarre directement dans les réglages du BIOS (UEFI uniquement), dans 5 secondes.
pub fn redemarrer_dans_le_bios() -> Result<(), String> {
    let s = crate::systeme::executer("shutdown", &["/r", "/fw", "/t", "5"]).map_err(|e| e.to_string())?;
    if s.status.success() {
        Ok(())
    } else {
        Err("Windows refuse de redémarrer dans le BIOS (PC en mode BIOS ancien, sans UEFI). Redémarre et appuie sur Suppr ou F2 pendant le démarrage.".into())
    }
}
