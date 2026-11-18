// Optimisations Windows réversibles : la valeur d'origine est sauvegardée avant chaque changement.

use crate::config::{EnAttente, Stockage};
use crate::{ps, systeme};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE};
use winreg::RegKey;

#[derive(Clone, Serialize, Deserialize)]
pub struct Valeur {
    pub ruche: String,
    pub chemin: String,
    pub nom: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub valeur: Value,
}

impl Valeur {
    fn cle(&self) -> String {
        format!(r"{}\{}\{}", self.ruche, self.chemin, self.nom)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Tweak {
    pub id: String,
    #[serde(default = "categorie_windows")]
    pub categorie: String,
    pub titre: String,
    pub description: String,
    pub gain: String,
    pub redemarrage: bool,
    /// Réglages qui ne sont pas de simples valeurs de registre : plan, adaptateur, nagle, powercfg, bcd, defender, nvidia, veille-prolongee, service, amd
    #[serde(default)]
    pub special: Option<String>,
    #[serde(default)]
    pub valeurs: Vec<Valeur>,
    /// Propriétés avancées de la carte réseau (special = adaptateur) et valeur voulue
    #[serde(default)]
    pub mots_cles: Vec<String>,
    #[serde(default)]
    pub cible: String,
    /// Réglage du plan d'alimentation (special = powercfg) : sous-groupe, paramètre et valeur voulue
    #[serde(default)]
    pub sous_groupe: String,
    #[serde(default)]
    pub parametre: String,
    #[serde(default)]
    pub index: u32,
    /// Réglage du pilote NVIDIA (special = nvidia), valeur voulue dans `index`
    #[serde(default)]
    pub id_nvidia: u32,
    /// Plusieurs réglages NVIDIA d'un coup : [numéro, valeur voulue]
    #[serde(default)]
    pub reglages_nvidia: Vec<(u32, u32)>,
    /// Tâches planifiées à désactiver (special = taches), chemin complet : \Dossier\Nom
    #[serde(default)]
    pub taches: Vec<String>,
    /// recommande : sans contrepartie notable ; max : plus agressif (consommation, confort), inclus dans « Boost maximum » ;
    /// risque : baisse la sécurité ou touche au démarrage, jamais appliqué en lot
    /// choix : dépend du matériel (ex. écran G-Sync), jamais appliqué en lot non plus
    #[serde(default = "niveau_recommande")]
    pub niveau: String,
}

fn niveau_recommande() -> String {
    "recommande".into()
}

fn categorie_windows() -> String {
    "windows".into()
}

#[derive(Serialize)]
pub struct TweakEtat {
    #[serde(flatten)]
    pub tweak: Tweak,
    pub actif: bool,
    /// Appliqué par FPS Cagoule (donc annulable)
    pub annulable: bool,
    /// Faux si le matériel ne propose pas ce réglage (ex. carte réseau sans économie d'énergie)
    pub disponible: bool,
    /// Changé depuis le dernier démarrage de Windows : « activation » ou « annulation » en attente de redémarrage
    pub attente: Option<&'static str>,
}

#[derive(Serialize)]
pub struct Resultat {
    pub id: String,
    pub ok: bool,
    pub erreur: Option<String>,
}

fn definitions() -> &'static [Tweak] {
    static DEFS: OnceLock<Vec<Tweak>> = OnceLock::new();
    DEFS.get_or_init(|| serde_json::from_str(include_str!("tweaks.json")).expect("tweaks.json invalide"))
}

fn ruche(v: &Valeur) -> RegKey {
    RegKey::predef(if v.ruche == "HKLM" { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER })
}

fn lire(v: &Valeur) -> Option<Value> {
    let cle = ruche(v).open_subkey_with_flags(&v.chemin, KEY_READ).ok()?;
    if let Ok(n) = cle.get_value::<u32, _>(&v.nom) {
        return Some(json!(n));
    }
    cle.get_value::<String, _>(&v.nom).ok().map(Value::String)
}

fn ecrire(v: &Valeur, valeur: &Value) -> Result<(), String> {
    let r = (|| {
        let (cle, _) = ruche(v).create_subkey(&v.chemin).map_err(|e| format!("{} : {e}", v.chemin))?;
        match v.type_.as_str() {
            "dword" => cle.set_value(&v.nom, &(valeur.as_u64().unwrap_or(0) as u32)),
            _ => cle.set_value(&v.nom, &valeur.as_str().map(str::to_string).unwrap_or_else(|| valeur.to_string())),
        }
        .map_err(|e| format!("{} : {e}", v.nom))
    })();
    // Démarrage d'un service dont Windows protège la clé (accès refusé même en administrateur) :
    // on passe par le gestionnaire des services, qui a le droit de la modifier
    match (r, service_de(v)) {
        (Err(e), Some(service)) => demarrage_service(service, valeur.as_u64().unwrap_or(3) as u32).map_err(|e2| format!("{e} ; {e2}")),
        (r, _) => r,
    }
}

/// Nom du service quand la valeur est son mode de démarrage (Services\<nom>\Start)
fn service_de(v: &Valeur) -> Option<&str> {
    let nom = v.chemin.strip_prefix(r"SYSTEM\CurrentControlSet\Services\")?;
    (v.ruche == "HKLM" && v.nom.eq_ignore_ascii_case("Start") && !nom.contains('\\')).then_some(nom)
}

/// Change le démarrage d'un service (2 automatique, 3 manuel, 4 désactivé) par le gestionnaire des services
fn demarrage_service(nom: &str, demarrage: u32) -> Result<(), String> {
    use windows_sys::Win32::System::Services::{
        ChangeServiceConfigW, CloseServiceHandle, OpenSCManagerW, OpenServiceW, SC_MANAGER_CONNECT, SERVICE_CHANGE_CONFIG, SERVICE_NO_CHANGE,
    };
    let large: Vec<u16> = nom.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let scm = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
        if scm.is_null() {
            return Err(format!("Gestionnaire des services inaccessible ({})", std::io::Error::last_os_error()));
        }
        let service = OpenServiceW(scm, large.as_ptr(), SERVICE_CHANGE_CONFIG);
        let r = if service.is_null() {
            Err(format!("Service {nom} : {}", std::io::Error::last_os_error()))
        } else {
            let ok = ChangeServiceConfigW(
                service,
                SERVICE_NO_CHANGE,
                demarrage,
                SERVICE_NO_CHANGE,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            ) != 0;
            let erreur = std::io::Error::last_os_error();
            CloseServiceHandle(service);
            if ok {
                Ok(())
            } else {
                Err(format!("Service {nom} : {erreur}"))
            }
        };
        CloseServiceHandle(scm);
        r
    }
}

fn effacer(v: &Valeur) {
    if let Ok(cle) = ruche(v).open_subkey_with_flags(&v.chemin, KEY_SET_VALUE) {
        let _ = cle.delete_value(&v.nom);
    }
}

fn identique(lu: &Option<Value>, cible: &Value) -> bool {
    match (lu, cible) {
        (Some(Value::Number(a)), Value::Number(b)) => a.as_u64() == b.as_u64(),
        (Some(Value::String(a)), Value::String(b)) => a == b,
        _ => false,
    }
}

/// Nagle : TcpAckFrequency et TCPNoDelay sur chaque interface réseau qui a une adresse IP.
fn valeurs_nagle() -> Vec<Valeur> {
    const BASE: &str = r"SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces";
    let Ok(interfaces) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(BASE, KEY_READ) else { return vec![] };
    let mut v = Vec::new();
    for nom in interfaces.enum_keys().flatten() {
        let Ok(i) = interfaces.open_subkey_with_flags(&nom, KEY_READ) else { continue };
        let ip = i.get_value::<String, _>("DhcpIPAddress").ok().filter(|a| !a.is_empty() && a != "0.0.0.0");
        let fixe = i.get_value::<Vec<String>, _>("IPAddress").ok().filter(|l| l.iter().any(|a| !a.is_empty() && a != "0.0.0.0"));
        if ip.is_none() && fixe.is_none() {
            continue;
        }
        for cle in ["TcpAckFrequency", "TCPNoDelay"] {
            v.push(Valeur { ruche: "HKLM".into(), chemin: format!(r"{BASE}\{nom}"), nom: cle.into(), type_: "dword".into(), valeur: json!(1) });
        }
    }
    v
}

/* ---------- Tâches planifiées et stockage réservé (une seule lecture PowerShell pour les deux) ---------- */

/// État des tâches (chemin → activée ?) et du stockage réservé (Some(true) = activé, None = inconnu)
fn lire_taches_reserve(taches: &[String]) -> (std::collections::HashMap<String, bool>, Option<bool>) {
    let liste = taches.iter().map(|t| ps::chaine(t)).collect::<Vec<_>>().join(",");
    let script = format!(
        r#"$t = @{{}}
foreach ($p in @({liste})) {{
  $i = $p.LastIndexOf('\')
  $x = Get-ScheduledTask -TaskPath $p.Substring(0, $i + 1) -TaskName $p.Substring($i + 1) -ErrorAction SilentlyContinue
  if ($x) {{ $t[$p] = ([string]$x.State -ne 'Disabled') }}
}}
$r = $null
try {{ $r = ([string](Get-WindowsReservedStorageState -ErrorAction Stop).ReservedStorageState) -eq 'Enabled' }} catch {{}}
@{{ taches = $t; reserve = $r }} | ConvertTo-Json -Compress"#
    );
    let Ok(sortie) = ps::executer(&script) else { return (Default::default(), None) };
    let v: Value = serde_json::from_str(&sortie).unwrap_or_default();
    let taches = v["taches"].as_object().map(|m| m.iter().filter_map(|(k, b)| Some((k.clone(), b.as_bool()?))).collect()).unwrap_or_default();
    (taches, v["reserve"].as_bool())
}

/// Active ou désactive des tâches planifiées (celles absentes sont ignorées)
fn changer_taches(taches: &[String], activer: bool) -> Result<(), String> {
    if taches.is_empty() {
        return Ok(());
    }
    let liste = taches.iter().map(|t| ps::chaine(t)).collect::<Vec<_>>().join(",");
    let action = if activer { "Enable-ScheduledTask" } else { "Disable-ScheduledTask" };
    let script = format!(
        r#"$ko = @()
foreach ($p in @({liste})) {{
  $i = $p.LastIndexOf('\')
  $x = Get-ScheduledTask -TaskPath $p.Substring(0, $i + 1) -TaskName $p.Substring($i + 1) -ErrorAction SilentlyContinue
  if ($x) {{ try {{ $x | {action} -ErrorAction Stop | Out-Null }} catch {{ $ko += $p.Substring($i + 1) }} }}
}}
if ($ko.Count) {{ throw ('Tâches non modifiées : ' + ($ko -join ', ')) }}"#
    );
    ps::executer(&script).map(|_| ())
}

fn reserve(activer: bool) -> Result<(), String> {
    let etat = if activer { "Enabled" } else { "Disabled" };
    ps::executer(&format!("Set-WindowsReservedStorageState -State {etat} -ErrorAction Stop | Out-Null"))
        .map(|_| ())
        .map_err(|e| if e.to_lowercase().contains("use") || e.contains("utilis") { "Windows utilise ce stockage en ce moment (mise à jour en cours) : réessaie après".into() } else { e })
}

fn valeurs_de(t: &Tweak) -> Vec<Valeur> {
    match t.special.as_deref() {
        Some("nagle") => valeurs_nagle(),
        _ => t.valeurs.clone(),
    }
}

/* ---------- Carte réseau (propriétés avancées du pilote) ---------- */

#[derive(Clone, Serialize, Deserialize)]
struct ProprieteCarte {
    a: String,
    k: String,
    v: String,
}

fn lire_cartes(mots_cles: &[String]) -> Vec<ProprieteCarte> {
    if mots_cles.is_empty() {
        return vec![];
    }
    let liste = mots_cles.iter().map(|m| ps::chaine(m)).collect::<Vec<_>>().join(",");
    let script = format!(
        "$mots = @({liste})
ConvertTo-Json -Compress -InputObject @(Get-NetAdapter -Physical | Where-Object Status -eq 'Up' | ForEach-Object {{
  $n = $_.Name
  Get-NetAdapterAdvancedProperty -Name $n -AllProperties -ErrorAction SilentlyContinue |
    Where-Object {{ $mots -contains $_.RegistryKeyword }} |
    ForEach-Object {{ [ordered]@{{ a = $n; k = $_.RegistryKeyword; v = \"$($_.RegistryValue)\" }} }}
}})"
    );
    ps::json(&script).ok().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

/// Change des propriétés de cartes réseau, puis redémarre chaque carte une seule fois.
fn ecrire_cartes(proprietes: &[ProprieteCarte]) -> Result<(), String> {
    if proprietes.is_empty() {
        return Ok(());
    }
    let mut script = String::new();
    let mut cartes: Vec<&str> = Vec::new();
    for p in proprietes {
        script += &format!(
            "Set-NetAdapterAdvancedProperty -Name {} -RegistryKeyword {} -RegistryValue {} -NoRestart\n",
            ps::chaine(&p.a),
            ps::chaine(&p.k),
            ps::chaine(&p.v)
        );
        if !cartes.contains(&p.a.as_str()) {
            cartes.push(&p.a);
        }
    }
    for c in cartes {
        script += &format!("Restart-NetAdapter -Name {}\n", ps::chaine(c));
    }
    ps::executer(&script).map(|_| ())
}

/* ---------- Réglages du plan d'alimentation (veille USB, cœurs, PCIe…) ---------- */

/// Plans concernés : l'actuel et ceux que FPS Cagoule active pendant les parties.
fn plans_alimentation(perso: &[Option<String>]) -> Vec<String> {
    let existants = systeme::plans_existants();
    let mut plans: Vec<String> = systeme::plan_actif().into_iter().collect();
    let fixes = [Some(systeme::PLAN_PERF.to_string()), Some(systeme::PLAN_OPTIMAL.to_string())];
    for p in fixes.iter().chain(perso).flatten() {
        if !plans.contains(p) && existants.contains(p) {
            plans.push(p.clone());
        }
    }
    plans
}

fn interroger(plan: &str, t: &Tweak) -> Option<u32> {
    let sortie = systeme::executer("powercfg", &["/query", plan, &t.sous_groupe, &t.parametre]).ok()?;
    if !sortie.status.success() {
        return None;
    }
    // Les deux dernières valeurs 0x… sont les index secteur puis batterie (quelle que soit la langue)
    let texte = String::from_utf8_lossy(&sortie.stdout);
    let hex: Vec<u32> = texte
        .split_whitespace()
        .filter_map(|m| m.strip_prefix("0x"))
        .filter_map(|h| u32::from_str_radix(h, 16).ok())
        .collect();
    (hex.len() >= 2).then(|| hex[hex.len() - 2])
}

/// Valeur « sur secteur » du réglage, None si le plan n'existe pas.
/// Certains réglages (cœurs, turbo) sont masqués par Windows : on les rend visibles.
fn lire_alimentation(plan: &str, t: &Tweak) -> Option<u32> {
    interroger(plan, t).or_else(|| {
        let _ = systeme::executer("powercfg", &["-attributes", &t.sous_groupe, &t.parametre, "-ATTRIB_HIDE"]);
        interroger(plan, t)
    })
}

fn ecrire_alimentation(plan: &str, t: &Tweak, valeur: u32) -> bool {
    systeme::executer("powercfg", &["/setacvalueindex", plan, &t.sous_groupe, &t.parametre, &valeur.to_string()]).is_ok_and(|s| s.status.success())
}

/* ---------- Démarrage de Windows (bcdedit) ---------- */

/// Valeur actuelle d'un paramètre de démarrage, None s'il n'est pas défini.
fn lire_bcd(cle: &str) -> Option<String> {
    let sortie = systeme::executer("bcdedit", &["/enum", "{current}"]).ok()?;
    String::from_utf8_lossy(&sortie.stdout).lines().find_map(|l| {
        let mut mots = l.split_whitespace();
        mots.next().filter(|m| m.eq_ignore_ascii_case(cle)).map(|_| mots.collect::<Vec<_>>().join(" "))
    })
}

/// BitLocker vérifie la configuration de démarrage : sans suspension, un changement peut faire
/// demander la clé de récupération. On le suspend pour un seul redémarrage, comme le recommande Microsoft.
fn suspendre_bitlocker() -> Result<(), String> {
    ps::executer(
        "$lecteur = $env:SystemDrive
$etat = (New-Object -ComObject Shell.Application).NameSpace($lecteur + '\').Self.ExtendedProperty('System.Volume.BitLockerProtection')
if ($etat -eq 1 -or $etat -eq 3) {
  manage-bde -protectors -disable $lecteur -RebootCount 1 | Out-Null
  if ($LASTEXITCODE -ne 0) { throw 'Impossible de suspendre BitLocker : changement annulé' }
}",
    )
    .map(|_| ())
}

fn ecrire_bcd(cle: &str, valeur: Option<&str>) -> Result<(), String> {
    suspendre_bitlocker()?;
    let sortie = match valeur {
        Some(v) => systeme::executer("bcdedit", &["/set", "{current}", cle, v]),
        None => systeme::executer("bcdedit", &["/deletevalue", "{current}", cle]),
    }
    .map_err(|e| e.to_string())?;
    if sortie.status.success() {
        Ok(())
    } else {
        Err(format!("bcdedit : {}", String::from_utf8_lossy(&sortie.stdout).trim()))
    }
}

/// Réglages NVIDIA d'une optimisation : la liste, ou le réglage unique (id_nvidia, index).
fn reglages_nvidia(t: &Tweak) -> Vec<(u32, u32)> {
    if t.reglages_nvidia.is_empty() { vec![(t.id_nvidia, t.index)] } else { t.reglages_nvidia.clone() }
}

/* ---------- Veille prolongée (fichier hiberfil.sys) ---------- */

fn veille_prolongee_active() -> bool {
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(r"SYSTEM\CurrentControlSet\Control\Power", KEY_READ)
        .and_then(|k| k.get_value::<u32, _>("HibernateEnabled"))
        .map_or(true, |v| v != 0)
}

fn veille_prolongee(activer: bool) -> Result<(), String> {
    let sortie = systeme::executer("powercfg", &["/hibernate", if activer { "on" } else { "off" }]).map_err(|e| e.to_string())?;
    if sortie.status.success() {
        Ok(())
    } else {
        Err(format!("powercfg : {}", String::from_utf8_lossy(&sortie.stdout).trim()))
    }
}

/// Retire les limites posées dans msconfig (« Nombre de processeurs », « Mémoire maximale ») :
/// ce sont des options de débogage qui brident Windows, pas des « boosts ». Renvoie ce qui a été retiré.
pub fn reparer_limites_demarrage() -> Result<Vec<String>, String> {
    let mut retirees = Vec::new();
    for cle in ["numproc", "truncatememory", "removememory"] {
        if lire_bcd(cle).is_some() {
            ecrire_bcd(cle, None)?;
            retirees.push(cle.to_string());
        }
    }
    Ok(retirees)
}

/* ---------- Exclusions Defender pour les dossiers de jeux ---------- */

#[derive(Default, Deserialize)]
#[serde(default)]
struct EtatDefender {
    dossiers: Vec<String>,
    exclus: Vec<String>,
}

/// Dossiers de jeux présents (Steam et ses autres bibliothèques, Epic, Riot, Ubisoft, EA, Battle.net)
/// et ceux déjà exclus de l'analyse. Erreur si Defender n'est pas l'antivirus actif.
fn lire_defender() -> Result<EtatDefender, String> {
    let v = ps::json(
        r#"$x86 = ${env:ProgramFiles(x86)}
$d = @("$x86\Steam\steamapps\common", "$env:ProgramFiles\Epic Games", "$env:SystemDrive\Riot Games",
  "$x86\Ubisoft\Ubisoft Game Launcher\games", "$env:ProgramFiles\EA Games", "$env:ProgramFiles\Electronic Arts", "$x86\Battle.net")
$vdf = "$x86\Steam\steamapps\libraryfolders.vdf"
if (Test-Path $vdf) {
  Select-String -Path $vdf -Pattern '"path"\s+"(.+)"' | ForEach-Object { $d += (($_.Matches[0].Groups[1].Value -replace '\\\\', '\') + '\steamapps\common') }
}
$d = @($d | Where-Object { Test-Path $_ } | Select-Object -Unique)
$ex = @((Get-MpPreference).ExclusionPath)
ConvertTo-Json -Compress @{ dossiers = $d; exclus = @($ex | Where-Object { $d -contains $_ }) }"#,
    )?;
    serde_json::from_value(v).map_err(|e| e.to_string())
}

fn exclusions(ajouter: bool, dossiers: &[String]) -> Result<(), String> {
    if dossiers.is_empty() {
        return Ok(());
    }
    let liste = dossiers.iter().map(|d| ps::chaine(d)).collect::<Vec<_>>().join(",");
    let cmd = if ajouter { "Add-MpPreference" } else { "Remove-MpPreference" };
    ps::executer(&format!("{cmd} -ExclusionPath @({liste})")).map(|_| ())
}

/* ---------- État, application et annulation ---------- */

pub fn liste(stock: &Stockage) -> Vec<TweakEtat> {
    let (plan_perso, plan_max, sauvegardes, attentes) = {
        let mut d = stock.donnees.lock().unwrap();
        // Les changements faits avant le dernier démarrage sont appliqués : on les oublie
        let demarrage = systeme::demarrage_windows();
        let avant = d.redemarrage.len();
        d.redemarrage.retain(|_, e| e.quand > demarrage);
        let change = d.redemarrage.len() != avant;
        let r = (d.plan_perso.clone(), d.plan_max.clone(), d.sauvegardes.clone(), d.redemarrage.clone());
        drop(d);
        if change {
            stock.sauver();
        }
        r
    };
    let persos = [plan_perso.clone(), plan_max.clone()];
    let plans = plans_alimentation(&persos);
    // Une seule lecture PowerShell pour toutes les optimisations de carte réseau
    let mots: Vec<String> = definitions().iter().flat_map(|t| t.mots_cles.clone()).collect();
    let cartes = lire_cartes(&mots);
    // Réglages AMD : lus en une seule connexion au pilote, seulement si la carte principale est une AMD
    let amd: Option<std::collections::HashMap<String, Option<i32>>> = crate::gpu::principale().filter(|c| c.fabricant == "amd").and_then(|_| {
        crate::amd::avec(|s| {
            Ok(definitions()
                .iter()
                .filter(|t| t.special.as_deref() == Some("amd"))
                .map(|t| (t.parametre.clone(), s.lire(&t.parametre).ok().flatten()))
                .collect())
        })
        .ok()
    });
    let toutes_taches: Vec<String> = definitions().iter().flat_map(|t| t.taches.clone()).collect();
    let (etat_taches, etat_reserve) = lire_taches_reserve(&toutes_taches);
    definitions()
        .iter()
        .map(|t| {
            let (actif, disponible) = match t.special.as_deref() {
                // Tâches planifiées : proposé si au moins une existe, actif quand toutes celles présentes sont coupées
                Some("taches") => {
                    let presentes: Vec<bool> = t.taches.iter().filter_map(|p| etat_taches.get(p).copied()).collect();
                    (!presentes.is_empty() && presentes.iter().all(|active| !active), !presentes.is_empty())
                }
                Some("reserve") => match etat_reserve {
                    Some(active) => (!active, true),
                    None => (false, false),
                },
                Some("plan") => (
                    systeme::plan_actif().is_some_and(|g| systeme::est_plan_perf(&g, &persos))
                        || systeme::mode_alimentation().as_deref() == Some(systeme::MODE_PERF),
                    true,
                ),
                // Veille moderne : proposé seulement sur les PC concernés (ou s'il a déjà été appliqué)
                Some("veille-moderne") => {
                    let actif = valeurs_de(t).iter().all(|v| identique(&lire(v), &v.valeur));
                    (actif, actif || systeme::veille_moderne())
                }
                Some("bcd") => (lire_bcd(&t.parametre).is_some_and(|v| v.eq_ignore_ascii_case(&t.cible)), true),
                Some("nvidia") => {
                    let lus: Result<Vec<bool>, String> = reglages_nvidia(t).iter().map(|(id, v)| crate::nvidia::lire(*id).map(|l| l == Some(*v))).collect();
                    match lus {
                        Ok(l) => (l.iter().all(|b| *b), true),
                        Err(_) => (false, false),
                    }
                }
                Some("veille-prolongee") => (!veille_prolongee_active(), true),
                Some("amd") => match amd.as_ref().and_then(|m| m.get(&t.parametre).copied().flatten()) {
                    Some(v) => (v == t.index as i32, true),
                    None => (false, false),
                },
                // Service Windows : proposé seulement s'il existe sur ce PC (le fax n'existe pas partout)
                Some("service") => {
                    let valeurs = valeurs_de(t);
                    let existe = service_existe(t);
                    (existe && valeurs.iter().all(|v| identique(&lire(v), &v.valeur)), existe)
                }
                Some("defender") => match lire_defender() {
                    Ok(d) => (!d.dossiers.is_empty() && d.dossiers.len() == d.exclus.len(), !d.dossiers.is_empty()),
                    Err(_) => (false, false),
                },
                Some("adaptateur") => {
                    let miennes: Vec<_> = cartes.iter().filter(|p| t.mots_cles.iter().any(|m| m.eq_ignore_ascii_case(&p.k))).collect();
                    (!miennes.is_empty() && miennes.iter().all(|p| p.v == t.cible), !miennes.is_empty())
                }
                Some("powercfg") => {
                    let valeurs: Vec<u32> = plans.iter().filter_map(|p| lire_alimentation(p, t)).collect();
                    (!valeurs.is_empty() && valeurs.iter().all(|v| *v == t.index), !valeurs.is_empty())
                }
                _ => {
                    let valeurs = valeurs_de(t);
                    (!valeurs.is_empty() && valeurs.iter().all(|v| identique(&lire(v), &v.valeur)), !valeurs.is_empty())
                }
            };
            let attente = attentes.get(&t.id).map(|e| if e.annulation { "annulation" } else { "activation" });
            let annulable = sauvegardes.contains_key(&t.id);
            // Réglages d'alimentation retirés de l'app (Windows ne les applique pas sur beaucoup de PC) :
            // jamais proposés ni appliqués ; ceux appliqués avant restent annulables par « Tout annuler »
            let disponible = disponible && t.categorie != "alimentation";
            TweakEtat { tweak: t.clone(), actif, annulable, disponible, attente }
        })
        .collect()
}

fn service_existe(t: &Tweak) -> bool {
    !t.valeurs.is_empty() && t.valeurs.iter().all(|v| ruche(v).open_subkey_with_flags(&v.chemin, KEY_READ).is_ok())
}

fn appliquer_un(stock: &Stockage, t: &Tweak) -> Result<(), String> {
    if t.categorie == "alimentation" {
        return Err("Réglage d'alimentation retiré de FPS Cagoule : à régler dans Windows si besoin".into());
    }
    // Ne jamais créer la clé d'un service absent : on change seulement son démarrage
    if t.special.as_deref() == Some("service") && !service_existe(t) {
        return Err("Ce service n'existe pas sur ton PC".into());
    }
    let mut d = stock.donnees.lock().unwrap();
    // On ne sauvegarde qu'à la première application : c'est la vraie valeur d'origine
    let deja = d.sauvegardes.contains_key(&t.id);
    match t.special.as_deref() {
        Some("plan") => {
            let p = systeme::passer_en_performances(systeme::PLAN_PERF, d.plan_perso.as_deref(), "Performances élevées")?;
            if let Some(cree) = p.plan_cree {
                d.plan_perso = Some(cree);
            }
            // Plan : texte du GUID (comme en 0.1) ; mode d'alimentation : {"mode": GUID}
            if let (false, Some(r)) = (deja, p.retour) {
                d.sauvegardes.insert(
                    t.id.clone(),
                    match r {
                        systeme::Retour::Plan(g) => json!(g),
                        systeme::Retour::Mode(g) => json!({ "mode": g }),
                    },
                );
            }
            Ok(())
        }
        Some("nvidia") => {
            // Sauvegarde : { "numéro": valeur ou null (= réglage par défaut) }
            let mut avant = serde_json::Map::new();
            for (id, _) in reglages_nvidia(t) {
                avant.insert(id.to_string(), crate::nvidia::lire(id)?.map_or(Value::Null, |v| json!(v)));
            }
            if !deja {
                d.sauvegardes.insert(t.id.clone(), Value::Object(avant));
            }
            drop(d);
            reglages_nvidia(t).into_iter().try_for_each(|(id, v)| crate::nvidia::ecrire(id, Some(v)))
        }
        Some("amd") => {
            drop(d);
            let avant = crate::amd::avec(|s| s.lire(&t.parametre))?.ok_or("Ta carte AMD ne propose pas ce réglage")?;
            crate::amd::avec(|s| s.ecrire(&t.parametre, t.index as i32))?;
            let mut d = stock.donnees.lock().unwrap();
            if !deja {
                d.sauvegardes.insert(t.id.clone(), json!(avant));
            }
            Ok(())
        }
        Some("veille-prolongee") => {
            if !deja {
                d.sauvegardes.insert(t.id.clone(), json!(veille_prolongee_active()));
            }
            drop(d);
            veille_prolongee(false)
        }
        Some("taches") => {
            drop(d);
            // Sauvegarde : les tâches qui étaient actives (seules elles seront réactivées)
            let (etat, _) = lire_taches_reserve(&t.taches);
            let actives: Vec<String> = t.taches.iter().filter(|p| etat.get(*p) == Some(&true)).cloned().collect();
            if etat.is_empty() {
                return Err("Aucune de ces tâches n'existe sur ton PC".into());
            }
            let mut d = stock.donnees.lock().unwrap();
            let mut avant: Vec<String> = d.sauvegardes.get(&t.id).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
            for a in &actives {
                if !avant.contains(a) {
                    avant.push(a.clone());
                }
            }
            d.sauvegardes.insert(t.id.clone(), json!(avant));
            drop(d);
            changer_taches(&actives, false)
        }
        Some("reserve") => {
            drop(d);
            let (_, avant) = lire_taches_reserve(&[]);
            let avant = avant.ok_or("Ce réglage n'existe pas sur ton Windows")?;
            if !deja {
                stock.donnees.lock().unwrap().sauvegardes.insert(t.id.clone(), json!(avant));
            }
            reserve(false)
        }
        Some("bcd") => {
            if !deja {
                d.sauvegardes.insert(t.id.clone(), lire_bcd(&t.parametre).map_or(Value::Null, Value::String));
            }
            drop(d);
            ecrire_bcd(&t.parametre, Some(&t.cible))
        }
        Some("defender") => {
            let etat = lire_defender().map_err(|e| format!("Defender n'est pas ton antivirus actif ({e})"))?;
            let a_ajouter: Vec<String> = etat.dossiers.iter().filter(|x| !etat.exclus.contains(x)).cloned().collect();
            if etat.dossiers.is_empty() {
                return Err("Aucun dossier de jeux trouvé".into());
            }
            // On ne retire ensuite que ce que l'app a ajouté
            let mut ajoutes: Vec<String> = d.sauvegardes.get(&t.id).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
            ajoutes.extend(a_ajouter.iter().cloned());
            d.sauvegardes.insert(t.id.clone(), json!(ajoutes));
            drop(d);
            exclusions(true, &a_ajouter)
        }
        Some("adaptateur") => {
            let actuelles = lire_cartes(&t.mots_cles);
            if actuelles.is_empty() {
                return Err("Ta carte réseau ne propose pas ce réglage".into());
            }
            if !deja {
                d.sauvegardes.insert(t.id.clone(), serde_json::to_value(&actuelles).unwrap_or_default());
            }
            drop(d);
            let voulues: Vec<_> = actuelles.into_iter().filter(|p| p.v != t.cible).map(|p| ProprieteCarte { v: t.cible.clone(), ..p }).collect();
            ecrire_cartes(&voulues)
        }
        Some("powercfg") => {
            let plans = plans_alimentation(&[d.plan_perso.clone(), d.plan_max.clone()]);
            let origine: serde_json::Map<String, Value> = plans.iter().filter_map(|p| lire_alimentation(p, t).map(|v| (p.clone(), json!(v)))).collect();
            if origine.is_empty() {
                return Err("Ce réglage n'existe pas sur ton PC".into());
            }
            if !deja {
                d.sauvegardes.insert(t.id.clone(), Value::Object(origine.clone()));
            }
            for p in origine.keys() {
                ecrire_alimentation(p, t, t.index);
            }
            systeme::executer("powercfg", &["/setactive", "SCHEME_CURRENT"]).map_err(|e| e.to_string())?;
            Ok(())
        }
        _ => {
            let valeurs = valeurs_de(t);
            if valeurs.is_empty() {
                return Err("Aucune interface réseau active".into());
            }
            if !deja {
                let origine: serde_json::Map<String, Value> = valeurs.iter().map(|v| (v.cle(), lire(v).unwrap_or(Value::Null))).collect();
                d.sauvegardes.insert(t.id.clone(), Value::Object(origine));
            }
            valeurs.iter().try_for_each(|v| ecrire(v, &v.valeur))
        }
    }
}

fn annuler_un(stock: &Stockage, t: &Tweak) -> Result<(), String> {
    let mut d = stock.donnees.lock().unwrap();
    let Some(origine) = d.sauvegardes.get(&t.id).cloned() else {
        return Err("Aucune sauvegarde : ce réglage n'a pas été changé par FPS Cagoule".into());
    };
    match t.special.as_deref() {
        Some("plan") => {
            let retour = match origine.get("mode").and_then(Value::as_str) {
                Some(mode) => systeme::Retour::Mode(mode.to_string()),
                None => systeme::Retour::Plan(origine.as_str().unwrap_or(systeme::PLAN_EQUILIBRE).to_string()),
            };
            if !systeme::remettre(&retour) {
                return Err("Impossible de remettre l'ancien réglage d'alimentation".into());
            }
        }
        Some("bcd") => {
            drop(d);
            ecrire_bcd(&t.parametre, origine.as_str())?;
            d = stock.donnees.lock().unwrap();
        }
        Some("nvidia") => {
            drop(d);
            match &origine {
                Value::Object(m) => {
                    for (id, v) in m {
                        if let Ok(id) = id.parse::<u32>() {
                            crate::nvidia::ecrire(id, v.as_u64().map(|v| v as u32))?;
                        }
                    }
                }
                // Version 0.10 : un seul réglage, valeur ou null
                autre => crate::nvidia::ecrire(t.id_nvidia, autre.as_u64().map(|v| v as u32))?,
            }
            d = stock.donnees.lock().unwrap();
        }
        Some("amd") => {
            drop(d);
            crate::amd::avec(|s| s.ecrire(&t.parametre, origine.as_i64().unwrap_or(0) as i32))?;
            d = stock.donnees.lock().unwrap();
        }
        Some("veille-prolongee") => {
            drop(d);
            if origine.as_bool().unwrap_or(true) {
                veille_prolongee(true)?;
            }
            d = stock.donnees.lock().unwrap();
        }
        Some("taches") => {
            let actives: Vec<String> = serde_json::from_value(origine).unwrap_or_default();
            drop(d);
            changer_taches(&actives, true)?;
            d = stock.donnees.lock().unwrap();
        }
        Some("reserve") => {
            drop(d);
            if origine.as_bool().unwrap_or(true) {
                reserve(true)?;
            }
            d = stock.donnees.lock().unwrap();
        }
        Some("defender") => {
            let ajoutes: Vec<String> = serde_json::from_value(origine).unwrap_or_default();
            drop(d);
            exclusions(false, &ajoutes)?;
            d = stock.donnees.lock().unwrap();
        }
        Some("adaptateur") => {
            let proprietes: Vec<ProprieteCarte> = serde_json::from_value(origine).unwrap_or_default();
            drop(d);
            ecrire_cartes(&proprietes)?;
            d = stock.donnees.lock().unwrap();
        }
        Some("powercfg") => {
            for (plan, v) in origine.as_object().cloned().unwrap_or_default() {
                ecrire_alimentation(&plan, t, v.as_u64().unwrap_or(1) as u32);
            }
            let _ = systeme::executer("powercfg", &["/setactive", "SCHEME_CURRENT"]);
        }
        _ => {
            // Version 0.1 : tableau dans l'ordre de tweaks.json ; ensuite : objet clé → valeur
            let paires: Vec<(Valeur, Value)> = match origine {
                Value::Array(l) => t.valeurs.iter().cloned().zip(l).collect(),
                Value::Object(m) => m
                    .into_iter()
                    .filter_map(|(cle, o)| valeurs_de(t).into_iter().chain(t.valeurs.iter().cloned()).find(|v| v.cle() == cle).map(|v| (v, o)))
                    .collect(),
                _ => vec![],
            };
            for (v, o) in paires {
                if o.is_null() { effacer(&v) } else { ecrire(&v, &o)? }
            }
        }
    }
    d.sauvegardes.remove(&t.id);
    Ok(())
}

fn pour_chaque(stock: &Stockage, ids: &[String], annulation: bool) -> Vec<Resultat> {
    let f = if annulation { annuler_un } else { appliquer_un };
    let res = ids
        .iter()
        .map(|id| {
            let def = definitions().iter().find(|t| &t.id == id);
            let r = def.ok_or_else(|| "Optimisation inconnue".to_string()).and_then(|t| f(stock, t));
            if let (Ok(()), Some(t)) = (&r, def) {
                let mut d = stock.donnees.lock().unwrap();
                if t.redemarrage {
                    let quand = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
                    // Annuler avant d'avoir redémarré : il n'y a plus rien en attente
                    match d.redemarrage.get(&t.id) {
                        Some(e) if e.annulation != annulation => {
                            d.redemarrage.remove(&t.id);
                        }
                        _ => {
                            d.redemarrage.insert(t.id.clone(), EnAttente { quand, annulation });
                        }
                    }
                }
            }
            Resultat { id: id.clone(), ok: r.is_ok(), erreur: r.err() }
        })
        .collect();
    stock.sauver();
    res
}

pub fn appliquer(stock: &Stockage, ids: &[String]) -> Vec<Resultat> {
    pour_chaque(stock, ids, false)
}

pub fn annuler(stock: &Stockage, ids: &[String]) -> Vec<Resultat> {
    pour_chaque(stock, ids, true)
}

/// Point de restauration Windows avant une série de changements.
/// Windows n'en crée qu'un par 24 h : dans ce cas, le précédent reste valable.
pub fn point_restauration() -> Result<String, String> {
    ps::executer(
        "Checkpoint-Computer -Description 'FPS Cagoule' -RestorePointType MODIFY_SETTINGS -WarningVariable av 3>$null; if ($av) { 'recent' } else { 'cree' }",
    )
    .map_err(|e| format!("Point de restauration impossible (protection du système désactivée ?) : {e}"))
}

/// Préférence « hautes performances » pour un jeu (utile sur les PC avec deux cartes graphiques).
pub fn gpu_performance(exe: &str) -> Result<(), String> {
    let (cle, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(r"Software\Microsoft\DirectX\UserGpuPreferences")
        .map_err(|e| e.to_string())?;
    cle.set_value(exe, &"GpuPreference=2;").map_err(|e| e.to_string())
}

const TACHE: &str = "FPS Cagoule";

/// Lancement à l'ouverture de session via le Planificateur de tâches :
/// c'est le seul moyen de démarrer une app administrateur sans fenêtre UAC.
pub fn demarrage_auto(actif: bool) -> Result<(), String> {
    let script = if actif {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        format!(
            "$a = New-ScheduledTaskAction -Execute {} -Argument '--cache'
$t = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME
$s = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -Priority 7
$p = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel Highest
Register-ScheduledTask -TaskName {} -Action $a -Trigger $t -Settings $s -Principal $p -Force | Out-Null",
            ps::chaine(&exe.to_string_lossy()),
            ps::chaine(TACHE)
        )
    } else {
        format!("Unregister-ScheduledTask -TaskName {} -Confirm:$false -ErrorAction SilentlyContinue", ps::chaine(TACHE))
    };
    ps::executer(&script).map(|_| ())
}

pub fn demarrage_auto_actif() -> bool {
    systeme::executer("schtasks", &["/Query", "/TN", TACHE]).is_ok_and(|s| s.status.success())
}

#[cfg(test)]
mod tests_services {
    use super::*;

    fn valeur(chemin: &str, nom: &str) -> Valeur {
        serde_json::from_value(json!({ "ruche": "HKLM", "chemin": chemin, "nom": nom, "type": "dword", "valeur": 4 })).unwrap()
    }

    #[test]
    fn taches_et_stockage_reserve_lus() {
        // Lecture seule : les tâches de télémétrie existent sur un Windows normal
        let toutes: Vec<String> = definitions().iter().flat_map(|t| t.taches.clone()).collect();
        assert!(toutes.len() >= 5);
        let (taches, reserve) = lire_taches_reserve(&toutes);
        println!("tâches trouvées : {taches:?}, stockage réservé : {reserve:?}");
        assert!(taches.keys().all(|k| toutes.contains(k)));
        assert!(definitions().iter().any(|t| t.id == "recherche-webview" && t.valeurs.len() == 5));
    }

    #[test]
    fn reconnait_les_services() {
        assert_eq!(service_de(&valeur(r"SYSTEM\CurrentControlSet\Services\TrkWks", "Start")), Some("TrkWks"));
        assert_eq!(service_de(&valeur(r"SYSTEM\CurrentControlSet\Services\Tcpip\Parameters", "Start")), None);
        assert_eq!(service_de(&valeur(r"SOFTWARE\Microsoft", "Start")), None);
    }

    /// Vrai gestionnaire des services (PC Windows de GitHub, administrateur) : on redonne à TrkWks son démarrage actuel
    #[test]
    fn gestionnaire_des_services() {
        let v = valeur(r"SYSTEM\CurrentControlSet\Services\TrkWks", "Start");
        let Some(actuel) = lire(&v).and_then(|x| x.as_u64()) else { return };
        demarrage_service("TrkWks", actuel as u32).expect("gestionnaire des services");
        assert_eq!(lire(&v).and_then(|x| x.as_u64()), Some(actuel));
    }
}
