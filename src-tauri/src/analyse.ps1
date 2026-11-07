function Essai([scriptblock]$bloc) { try { & $bloc } catch { $null } }
function Jour($d) { if ($d) { ([datetime]$d).ToString('yyyy-MM-dd') } else { $null } }

$os = Get-CimInstance Win32_OperatingSystem
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$gpus = @(Get-CimInstance Win32_VideoController | ForEach-Object {
  [ordered]@{
    nom = $_.Name; pilote = $_.DriverVersion; datePilote = Jour $_.DriverDate
    vram = $_.AdapterRAM; hz = $_.CurrentRefreshRate; hzMax = $_.MaxRefreshRate
    largeur = $_.CurrentHorizontalResolution; hauteur = $_.CurrentVerticalResolution
  }
})
$ram = @(Get-CimInstance Win32_PhysicalMemory | ForEach-Object {
  [ordered]@{
    capacite = $_.Capacity; vitesse = $_.Speed; vitesseReelle = $_.ConfiguredClockSpeed
    type = $_.SMBIOSMemoryType; fabricant = "$($_.Manufacturer)".Trim(); reference = "$($_.PartNumber)".Trim()
  }
})
$disques = @(Essai { Get-PhysicalDisk | ForEach-Object {
  [ordered]@{ nom = $_.FriendlyName; type = "$($_.MediaType)"; bus = "$($_.BusType)"; taille = $_.Size; sante = "$($_.HealthStatus)" }
} })
$volumes = @(Get-CimInstance Win32_LogicalDisk -Filter 'DriveType=3' | ForEach-Object {
  [ordered]@{ lettre = $_.DeviceID; taille = $_.Size; libre = $_.FreeSpace }
})
$systeme = $env:SystemDrive
$disqueSysteme = Essai { (Get-Partition -DriveLetter $systeme[0] | Get-Disk | Get-PhysicalDisk | Select-Object -First 1).MediaType }

$planActif = Essai { ((powercfg /getactivescheme) -join ' ') }
$planGuid = if ($planActif -match '([0-9a-fA-F-]{36})') { $Matches[1].ToLower() } else { $null }
$planNom = if ($planActif -match '\((.+)\)') { $Matches[1] } else { $null }

$demarrage = @(Essai { Get-CimInstance Win32_StartupCommand | ForEach-Object {
  [ordered]@{ nom = $_.Name; commande = $_.Command; emplacement = $_.Location }
} })

$vbs = Essai { (Get-CimInstance -Namespace root\Microsoft\Windows\DeviceGuard -ClassName Win32_DeviceGuard).VirtualizationBasedSecurityStatus }

$tempGpu = $null; $chargeGpu = $null
$smi = Get-Command nvidia-smi.exe -ErrorAction SilentlyContinue
if ($smi) {
  $l = Essai { (& $smi.Source --query-gpu=temperature.gpu,utilization.gpu --format=csv,noheader,nounits | Select-Object -First 1) }
  if ($l) { $p = $l -split ','; $tempGpu = [int]$p[0].Trim(); $chargeGpu = [int]$p[1].Trim() }
}

[ordered]@{
  os = [ordered]@{
    nom = $os.Caption; version = $os.Version; build = $os.BuildNumber
    ramTotaleKo = $os.TotalVisibleMemorySize; ramLibreKo = $os.FreePhysicalMemory
    demarre = Jour $os.LastBootUpTime; joursAllume = [math]::Round(((Get-Date) - $os.LastBootUpTime).TotalDays, 1)
  }
  cpu = [ordered]@{
    nom = "$($cpu.Name)".Trim(); coeurs = $cpu.NumberOfCores; threads = $cpu.NumberOfLogicalProcessors
    mhz = $cpu.MaxClockSpeed; charge = $cpu.LoadPercentage
  }
  gpus = $gpus; ram = $ram; disques = $disques; volumes = $volumes
  lecteurSysteme = $systeme; typeDisqueSysteme = "$disqueSysteme"
  plan = [ordered]@{ guid = $planGuid; nom = $planNom }
  demarrage = $demarrage
  reglages = [ordered]@{
    modeJeu = Lire 'HKCU:\Software\Microsoft\GameBar' 'AutoGameModeEnabled'
    gameDvr = Lire 'HKCU:\System\GameConfigStore' 'GameDVR_Enabled'
    hags = Lire 'HKLM:\SYSTEM\CurrentControlSet\Control\GraphicsDrivers' 'HwSchMode'
    accelerationSouris = Lire 'HKCU:\Control Panel\Mouse' 'MouseSpeed'
  }
  vbs = $vbs
  tempGpu = $tempGpu; chargeGpu = $chargeGpu
  date = (Get-Date).ToString('s')
} | ConvertTo-Json -Depth 6 -Compress
