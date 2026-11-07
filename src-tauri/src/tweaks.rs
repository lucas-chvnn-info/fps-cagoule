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

#[derive(Clone, Serialize, Deserialize)]
pub struct Tweak {
    pub id: String,
    pub titre: String,
    pub description: String,
    pub gain: String,
    pub redemarrage: bool,
    #[serde(default)]
    pub special: Option<String>,
    #[serde(default)]
    pub valeurs: Vec<Valeur>,
}

#[derive(Serialize)]
pub struct TweakEtat {
    #[serde(flatten)]
    pub tweak: Tweak,
    pub actif: bool,
    /// Appliqué par FPS Cagoule (donc annulable)
    pub annulable: bool,
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

pub fn liste(stock: &Stockage) -> Vec<TweakEtat> {
    let d = stock.donnees.lock().unwrap();
    definitions()
        .iter()
        .map(|t| {
            let actif = match t.special.as_deref() {
                Some("plan") => systeme::plan_actif().is_some_and(|g| systeme::est_plan_perf(&g, d.plan_perso.as_deref())),
                _ => t.valeurs.iter().all(|v| identique(&lire(v), &v.valeur)),
            };
            TweakEtat { tweak: t.clone(), actif, annulable: d.sauvegardes.contains_key(&t.id) }
        })
        .collect()
}

fn appliquer_un(stock: &Stockage, t: &Tweak) -> Result<(), String> {
    let mut d = stock.donnees.lock().unwrap();
    if t.special.as_deref() == Some("plan") {
        let avant = systeme::plan_actif();
        if let Some(cree) = systeme::activer_plan_perf(d.plan_perso.as_deref())? {
            d.plan_perso = Some(cree);
        }
        if let Some(avant) = avant {
            d.sauvegardes.entry(t.id.clone()).or_insert(json!(avant));
        }
        return Ok(());
    }
    // On ne sauvegarde qu'à la première application : c'est la vraie valeur d'origine
    if !d.sauvegardes.contains_key(&t.id) {
        let origine: Vec<Value> = t.valeurs.iter().map(|v| lire(v).unwrap_or(Value::Null)).collect();
        d.sauvegardes.insert(t.id.clone(), Value::Array(origine));
    }
    t.valeurs.iter().try_for_each(|v| ecrire(v, &v.valeur))
}

fn annuler_un(stock: &Stockage, t: &Tweak) -> Result<(), String> {
    let mut d = stock.donnees.lock().unwrap();
    let Some(origine) = d.sauvegardes.get(&t.id).cloned() else {
        return Err("Aucune sauvegarde : ce réglage n'a pas été changé par FPS Cagoule".into());
    };
    if t.special.as_deref() == Some("plan") {
        let guid = origine.as_str().unwrap_or(systeme::PLAN_PERF);
        if !systeme::activer_plan(guid) {
            return Err("Impossible de remettre l'ancien plan d'alimentation".into());
        }
    } else {
        let valeurs = origine.as_array().cloned().unwrap_or_default();
        for (v, o) in t.valeurs.iter().zip(valeurs) {
            if o.is_null() { effacer(v) } else { ecrire(v, &o)? }
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
