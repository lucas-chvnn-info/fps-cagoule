// Optimisations Windows réversibles : la valeur d'origine est sauvegardée avant chaque changement.

use crate::config::Stockage;
use crate::{ps, systeme};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};
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
    /// Réglages qui ne sont pas de simples valeurs de registre : plan, adaptateur, nagle, powercfg
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
    /// recommande : sans contrepartie notable ; max : plus agressif (consommation, confort), inclus dans « Boost maximum »
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
    let (cle, _) = ruche(v).create_subkey(&v.chemin).map_err(|e| format!("{} : {e}", v.chemin))?;
    let r = match v.type_.as_str() {
        "dword" => cle.set_value(&v.nom, &(valeur.as_u64().unwrap_or(0) as u32)),
        _ => cle.set_value(&v.nom, &valeur.as_str().map(str::to_string).unwrap_or_else(|| valeur.to_string())),
    };
    r.map_err(|e| format!("{} : {e}", v.nom))
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
    let mut plans: Vec<String> = systeme::plan_actif().into_iter().collect();
    let fixes = [Some(systeme::PLAN_PERF.to_string()), Some(systeme::PLAN_OPTIMAL.to_string())];
    for p in fixes.iter().chain(perso).flatten() {
        if !plans.contains(p) {
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

/* ---------- État, application et annulation ---------- */

pub fn liste(stock: &Stockage) -> Vec<TweakEtat> {
    let (plan_perso, plan_max, sauvegardes) = {
        let d = stock.donnees.lock().unwrap();
        (d.plan_perso.clone(), d.plan_max.clone(), d.sauvegardes.clone())
    };
    let persos = [plan_perso.clone(), plan_max.clone()];
    let plans = plans_alimentation(&persos);
    // Une seule lecture PowerShell pour toutes les optimisations de carte réseau
    let mots: Vec<String> = definitions().iter().flat_map(|t| t.mots_cles.clone()).collect();
    let cartes = lire_cartes(&mots);
    definitions()
        .iter()
        .map(|t| {
            let (actif, disponible) = match t.special.as_deref() {
                Some("plan") => (systeme::plan_actif().is_some_and(|g| systeme::est_plan_perf(&g, &persos)), true),
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
            TweakEtat { tweak: t.clone(), actif, annulable: sauvegardes.contains_key(&t.id), disponible }
        })
        .collect()
}

fn appliquer_un(stock: &Stockage, t: &Tweak) -> Result<(), String> {
    let mut d = stock.donnees.lock().unwrap();
    // On ne sauvegarde qu'à la première application : c'est la vraie valeur d'origine
    let deja = d.sauvegardes.contains_key(&t.id);
    match t.special.as_deref() {
        Some("plan") => {
            let avant = systeme::plan_actif();
            if let Some(cree) = systeme::activer_plan(systeme::PLAN_PERF, d.plan_perso.as_deref())? {
                d.plan_perso = Some(cree);
            }
            if let (false, Some(avant)) = (deja, avant) {
                d.sauvegardes.insert(t.id.clone(), json!(avant));
            }
            Ok(())
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
            if !systeme::activer_plan_guid(origine.as_str().unwrap_or(systeme::PLAN_PERF)) {
                return Err("Impossible de remettre l'ancien plan d'alimentation".into());
            }
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

fn pour_chaque(stock: &Stockage, ids: &[String], f: fn(&Stockage, &Tweak) -> Result<(), String>) -> Vec<Resultat> {
    let res = ids
        .iter()
        .map(|id| {
            let r = definitions().iter().find(|t| &t.id == id).ok_or_else(|| "Optimisation inconnue".to_string()).and_then(|t| f(stock, t));
            Resultat { id: id.clone(), ok: r.is_ok(), erreur: r.err() }
        })
        .collect();
    stock.sauver();
    res
}

pub fn appliquer(stock: &Stockage, ids: &[String]) -> Vec<Resultat> {
    pour_chaque(stock, ids, appliquer_un)
}

pub fn annuler(stock: &Stockage, ids: &[String]) -> Vec<Resultat> {
    pour_chaque(stock, ids, annuler_un)
}

/// Point de restauration Windows avant une série de changements.
/// Windows n'en crée qu'un par 24 h : dans ce cas, le précédent reste valable.
pub fn point_restauration() -> Result<String, String> {
    ps::executer(
        "Checkpoint-Computer -Description 'FPS Cagoule' -RestorePointType MODIFY_SETTINGS -WarningVariable av 3>$null; if ($av) { 'recent' } else { 'cree' }",
    )
    .map_err(|e| format!("Point de restauration impossible (protection du système désactivée ?) : {e}"))
}

#[derive(Serialize, Default)]
pub struct Nettoyage {
    pub fichiers: u64,
    pub octets: u64,
}

fn nettoyer_dossier(dossier: &std::path::Path, limite: SystemTime, bilan: &mut Nettoyage) {
    let Ok(entrees) = std::fs::read_dir(dossier) else { return };
    for e in entrees.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        if meta.is_dir() {
            nettoyer_dossier(&e.path(), limite, bilan);
            let _ = std::fs::remove_dir(e.path()); // seulement s'il est vide
        } else if meta.modified().is_ok_and(|m| m < limite) && std::fs::remove_file(e.path()).is_ok() {
            bilan.fichiers += 1;
            bilan.octets += meta.len();
        }
    }
}

/// Supprime les fichiers temporaires de plus de 24 h (ceux en cours d'utilisation sont ignorés).
pub fn nettoyer() -> Nettoyage {
    let limite = SystemTime::now() - Duration::from_secs(24 * 3600);
    let mut bilan = Nettoyage::default();
    let mut dossiers = vec![std::env::temp_dir()];
    if let Ok(windir) = std::env::var("SystemRoot") {
        dossiers.push(std::path::Path::new(&windir).join("Temp"));
    }
    for d in dossiers {
        nettoyer_dossier(&d, limite, &mut bilan);
    }
    bilan
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
