// Accès direct à Windows (API Win32, sans PowerShell) : processus, priorité, mémoire, plan d'alimentation.

use std::os::windows::process::CommandExt;
use std::process::{Command, Output};
use std::sync::Mutex;
use std::time::Instant;

use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, SetPriorityClass, TerminateProcess,
    HIGH_PRIORITY_CLASS, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION, PROCESS_TERMINATE,
};

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub const PLAN_EQUILIBRE: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";
pub const PLAN_PERF: &str = "8c5e7fd1-ce92-466d-be55-c3230bc7bdb8";
pub const PLAN_OPTIMAL: &str = "e9a42b02-d5df-448d-aa00-03f14749eb61";

pub struct Processus {
    pub pid: u32,
    pub parent: u32,
    pub nom: String,
}

/// Liste des processus : un instantané Toolhelp coûte moins d'une milliseconde.
pub fn processus() -> Vec<Processus> {
    let mut liste = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return liste;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut e);
        while ok != 0 {
            let fin = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            liste.push(Processus {
                pid: e.th32ProcessID,
                parent: e.th32ParentProcessID,
                nom: String::from_utf16_lossy(&e.szExeFile[..fin]),
            });
            ok = Process32NextW(snap, &mut e);
        }
        CloseHandle(snap);
    }
    liste
}

struct Poignee(HANDLE);

impl Poignee {
    fn ouvrir(droits: u32, pid: u32) -> Option<Self> {
        let h = unsafe { OpenProcess(droits, 0, pid) };
        (!h.is_null()).then(|| Self(h))
    }
}

impl Drop for Poignee {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

pub fn priorite_haute(pid: u32) -> bool {
    Poignee::ouvrir(PROCESS_SET_INFORMATION, pid).map_or(false, |h| unsafe { SetPriorityClass(h.0, HIGH_PRIORITY_CLASS) != 0 })
}

pub fn terminer(pid: u32) -> bool {
    Poignee::ouvrir(PROCESS_TERMINATE, pid).map_or(false, |h| unsafe { TerminateProcess(h.0, 0) != 0 })
}

pub fn chemin_exe(pid: u32) -> Option<String> {
    let h = Poignee::ouvrir(PROCESS_QUERY_LIMITED_INFORMATION, pid)?;
    let mut buf = [0u16; 1024];
    let mut taille = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(h.0, 0, buf.as_mut_ptr(), &mut taille) };
    (ok != 0).then(|| String::from_utf16_lossy(&buf[..taille as usize]))
}

/// Mémoire privée d'un processus, en octets.
pub fn memoire(pid: u32) -> u64 {
    let Some(h) = Poignee::ouvrir(PROCESS_QUERY_LIMITED_INFORMATION, pid) else { return 0 };
    unsafe {
        let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        if GetProcessMemoryInfo(h.0, &mut c, c.cb) != 0 {
            c.PagefileUsage as u64
        } else {
            0
        }
    }
}

fn filetime(f: &FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

static DERNIER_CPU: Mutex<Option<(Instant, u64)>> = Mutex::new(None);

/// Part du processeur utilisée par l'app depuis le dernier appel, en % de la machine entière.
pub fn cpu_app() -> f64 {
    let temps = unsafe {
        let (mut a, mut b, mut k, mut u): (FILETIME, FILETIME, FILETIME, FILETIME) = std::mem::zeroed();
        GetProcessTimes(GetCurrentProcess(), &mut a, &mut b, &mut k, &mut u);
        filetime(&k) + filetime(&u)
    };
    let maintenant = Instant::now();
    let coeurs = std::thread::available_parallelism().map_or(1, |n| n.get()) as f64;
    let mut dernier = DERNIER_CPU.lock().unwrap();
    let pct = match *dernier {
        Some((t0, c0)) => {
            let ecoule = maintenant.duration_since(t0).as_secs_f64();
            if ecoule > 0.0 { (temps.saturating_sub(c0) as f64 / 1e7) / ecoule / coeurs * 100.0 } else { 0.0 }
        }
        None => 0.0,
    };
    *dernier = Some((maintenant, temps));
    pct
}

/// Lance un programme sans ouvrir de console.
pub fn executer(programme: &str, args: &[&str]) -> std::io::Result<Output> {
    Command::new(programme).args(args).creation_flags(CREATE_NO_WINDOW).output()
}

fn extraire_guid(texte: &str) -> Option<String> {
    texte
        .split(|c: char| !(c.is_ascii_hexdigit() || c == '-'))
        .find(|m| m.len() == 36 && m.matches('-').count() == 4)
        .map(str::to_lowercase)
}

/// GUID de tous les plans d'alimentation présents sur le PC.
pub fn plans_existants() -> Vec<String> {
    let Ok(sortie) = executer("powercfg", &["/list"]) else { return vec![] };
    String::from_utf8_lossy(&sortie.stdout).lines().filter_map(extraire_guid).collect()
}

pub fn plan_actif() -> Option<String> {
    let sortie = executer("powercfg", &["/getactivescheme"]).ok()?;
    extraire_guid(&String::from_utf8_lossy(&sortie.stdout))
}

/// Première ligne non vide de la réponse d'un programme (pour expliquer un refus).
fn message(sortie: &Output) -> String {
    let texte = format!("{}\n{}", String::from_utf8_lossy(&sortie.stdout), String::from_utf8_lossy(&sortie.stderr));
    texte.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("sans détail").to_string()
}

fn essayer_plan(guid: &str) -> Result<(), String> {
    let sortie = executer("powercfg", &["/setactive", guid]).map_err(|e| e.to_string())?;
    if sortie.status.success() { Ok(()) } else { Err(message(&sortie)) }
}

pub fn activer_plan_guid(guid: &str) -> bool {
    essayer_plan(guid).is_ok()
}

/// Active un plan intégré (Performances élevées ou optimales). Certains PC ne l'ont plus :
/// on en crée alors une copie, dont le GUID est renvoyé pour être mémorisé.
pub fn activer_plan(modele: &str, copie: Option<&str>) -> Result<Option<String>, String> {
    if let Some(p) = copie {
        if activer_plan_guid(p) {
            return Ok(None);
        }
    }
    let refus = match essayer_plan(modele) {
        Ok(()) => return Ok(None),
        Err(e) => e,
    };
    let sortie = executer("powercfg", &["-duplicatescheme", modele]).map_err(|e| e.to_string())?;
    let Some(guid) = extraire_guid(&String::from_utf8_lossy(&sortie.stdout)).filter(|_| sortie.status.success()) else {
        return Err(format!("Windows refuse ce plan d'alimentation ({refus} ; {})", message(&sortie)));
    };
    essayer_plan(&guid).map(|_| Some(guid)).map_err(|e| format!("Windows refuse ce plan d'alimentation ({e})"))
}

/* ---------- Mode d'alimentation de Windows 11 (Paramètres → Alimentation) ---------- */
// Sur les PC en veille moderne (la plupart des portables récents), Windows n'accepte que le plan
// Équilibré : le seul réglage possible est ce mode, fait de surcouches au plan Équilibré.

pub const MODE_PERF: &str = "ded574b5-45a0-4f42-8737-46345c09c238";

fn fonction(dll: &str, nom: &[u8]) -> Option<unsafe extern "system" fn() -> isize> {
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    let dll: Vec<u16> = dll.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let module = LoadLibraryW(dll.as_ptr());
        if module.is_null() {
            return None;
        }
        GetProcAddress(module, nom.as_ptr())
    }
}

fn vers_guid(texte: &str) -> Option<windows_sys::core::GUID> {
    let hex: String = texte.chars().filter(|c| *c != '-').collect();
    (hex.len() == 32).then(|| u128::from_str_radix(&hex, 16).ok()).flatten().map(windows_sys::core::GUID::from_u128)
}

fn depuis_guid(g: &windows_sys::core::GUID) -> String {
    let fin: String = g.data4[2..].iter().map(|o| format!("{o:02x}")).collect();
    format!("{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{fin}", g.data1, g.data2, g.data3, g.data4[0], g.data4[1])
}

/// Mode d'alimentation actuel, None si Windows ne le gère pas.
pub fn mode_alimentation() -> Option<String> {
    type Lire = unsafe extern "system" fn(*mut windows_sys::core::GUID) -> u32;
    let f: Lire = unsafe { std::mem::transmute(fonction("powrprof.dll", b"PowerGetEffectiveOverlayScheme\0")?) };
    let mut g: windows_sys::core::GUID = unsafe { std::mem::zeroed() };
    (unsafe { f(&mut g) } == 0).then(|| depuis_guid(&g))
}

pub fn changer_mode_alimentation(guid: &str) -> bool {
    // Le GUID est passé par valeur ; en x64, une structure de 16 octets passe par adresse
    type Changer = unsafe extern "system" fn(*const windows_sys::core::GUID) -> u32;
    let (Some(f), Some(g)) = (fonction("powrprof.dll", b"PowerSetActiveOverlayScheme\0"), vers_guid(guid)) else { return false };
    let f: Changer = unsafe { std::mem::transmute(f) };
    unsafe { f(&g) == 0 }
}

/// PC en veille moderne (« Modern Standby »).
pub fn veille_moderne() -> bool {
    use windows_sys::Win32::System::Power::{GetPwrCapabilities, SYSTEM_POWER_CAPABILITIES};
    let mut c: SYSTEM_POWER_CAPABILITIES = unsafe { std::mem::zeroed() };
    unsafe { GetPwrCapabilities(&mut c) && c.AoAc }
}

/// Ce qu'il faut remettre après un passage en performances.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Retour {
    Plan(String),
    Mode(String),
}

pub struct Passage {
    /// None : déjà en performances, rien à remettre
    pub retour: Option<Retour>,
    pub plan_cree: Option<String>,
    pub libelle: &'static str,
}

/// Passe en performances : le plan `modele` (copie créée au besoin), ou à défaut le mode
/// « Meilleures performances » de Windows 11.
pub fn passer_en_performances(modele: &str, copie: Option<&str>, nom_plan: &'static str) -> Result<Passage, String> {
    let avant = plan_actif().unwrap_or_else(|| PLAN_EQUILIBRE.to_string());
    match activer_plan(modele, copie) {
        Ok(cree) => Ok(Passage { retour: Some(Retour::Plan(avant)), plan_cree: cree, libelle: nom_plan }),
        Err(refus) => {
            let mode = mode_alimentation().ok_or_else(|| refus.clone())?;
            if mode == MODE_PERF {
                return Ok(Passage { retour: None, plan_cree: None, libelle: "Meilleures performances" });
            }
            if changer_mode_alimentation(MODE_PERF) {
                Ok(Passage { retour: Some(Retour::Mode(mode)), plan_cree: None, libelle: "Meilleures performances" })
            } else {
                Err(format!("{refus}. Le mode « Meilleures performances » est aussi refusé."))
            }
        }
    }
}

pub fn remettre(r: &Retour) -> bool {
    match r {
        Retour::Plan(g) => activer_plan_guid(g),
        Retour::Mode(g) => changer_mode_alimentation(g),
    }
}

/// Heure du dernier démarrage de Windows (secondes Unix).
pub fn demarrage_windows() -> u64 {
    use windows_sys::Win32::System::SystemInformation::GetTickCount64;
    let maintenant = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    maintenant.saturating_sub(unsafe { GetTickCount64() } / 1000)
}

pub fn redemarrer() -> Result<(), String> {
    executer("shutdown", &["/r", "/t", "10", "/c", "FPS Cagoule : redémarrage pour terminer les optimisations"])
        .map_err(|e| e.to_string())
        .and_then(|s| if s.status.success() { Ok(()) } else { Err(message(&s)) })
}

/// Plans considérés comme « performances » : les intégrés et leurs copies créées par l'app.
pub fn est_plan_perf(guid: &str, persos: &[Option<String>]) -> bool {
    guid == PLAN_PERF || guid == PLAN_OPTIMAL || persos.iter().flatten().any(|p| p == guid)
}

/* ---------- Boost max : minuteur et mémoire en veille (fonctions de ntdll) ---------- */

fn ntdll(nom: &[u8]) -> Option<unsafe extern "system" fn() -> isize> {
    fonction("ntdll.dll", nom)
}

/// Minuteur système à 0,5 ms (au lieu de 15,6 ms) : les jeux réveillent leurs fils plus précisément,
/// ce qui régularise le rythme des images. `false` rend la main à Windows.
pub fn minuteur_precis(actif: bool) -> bool {
    type SetTimer = unsafe extern "system" fn(u32, u8, *mut u32) -> i32;
    let Some(f) = ntdll(b"NtSetTimerResolution\0") else { return false };
    let f: SetTimer = unsafe { std::mem::transmute(f) };
    let mut actuel = 0u32;
    unsafe { f(5000, actif as u8, &mut actuel) == 0 }
}

/// Mémoire disponible (octets).
pub fn memoire_libre() -> u64 {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    unsafe {
        let mut m: MEMORYSTATUSEX = std::mem::zeroed();
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut m) != 0 { m.ullAvailPhys } else { 0 }
    }
}

/// Mémoire vive installée (octets).
pub fn memoire_totale() -> u64 {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    unsafe {
        let mut m: MEMORYSTATUSEX = std::mem::zeroed();
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut m) != 0 { m.ullTotalPhys } else { 0 }
    }
}

/// Vide la liste de mémoire en veille (comme l'outil ISLC) : évite les saccades quand Windows
/// tarde à libérer de la mémoire pour le jeu. Renvoie le nombre d'octets rendus disponibles.
pub fn vider_memoire_veille() -> Result<u64, String> {
    use windows_sys::Win32::Foundation::LUID;
    use windows_sys::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES,
        TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::OpenProcessToken;
    type SetInfo = unsafe extern "system" fn(u32, *mut core::ffi::c_void, u32) -> i32;
    const SYSTEM_MEMORY_LIST_INFORMATION: u32 = 80;
    const MEMORY_PURGE_STANDBY_LIST: i32 = 4;

    let avant = memoire_libre();
    unsafe {
        // Le privilège « profiler un seul processus » est nécessaire, même en administrateur
        let mut jeton: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut jeton) == 0 {
            return Err("Droits administrateur nécessaires".into());
        }
        let nom: Vec<u16> = "SeProfileSingleProcessPrivilege\0".encode_utf16().collect();
        let mut luid: LUID = std::mem::zeroed();
        let ok = LookupPrivilegeValueW(std::ptr::null(), nom.as_ptr(), &mut luid) != 0 && {
            let tp = TOKEN_PRIVILEGES { PrivilegeCount: 1, Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }] };
            AdjustTokenPrivileges(jeton, 0, &tp, 0, std::ptr::null_mut(), std::ptr::null_mut()) != 0
        };
        CloseHandle(jeton);
        if !ok {
            return Err("Privilège refusé par Windows".into());
        }
        let f: SetInfo = std::mem::transmute(ntdll(b"NtSetSystemInformation\0").ok_or("Fonction Windows introuvable")?);
        let mut commande = MEMORY_PURGE_STANDBY_LIST;
        let statut = f(SYSTEM_MEMORY_LIST_INFORMATION, &mut commande as *mut i32 as *mut core::ffi::c_void, 4);
        if statut != 0 {
            return Err(format!("Windows a refusé (code {statut:#x})"));
        }
    }
    Ok(memoire_libre().saturating_sub(avant))
}
