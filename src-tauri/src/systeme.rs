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

pub fn plan_actif() -> Option<String> {
    let sortie = executer("powercfg", &["/getactivescheme"]).ok()?;
    extraire_guid(&String::from_utf8_lossy(&sortie.stdout))
}

pub fn activer_plan(guid: &str) -> bool {
    executer("powercfg", &["/setactive", guid]).map_or(false, |s| s.status.success())
}

/// Active Performances élevées. Certains PC (portables récents) ne l'ont plus :
/// on en crée alors une copie, dont le GUID est renvoyé pour être mémorisé.
pub fn activer_plan_perf(plan_perso: Option<&str>) -> Result<Option<String>, String> {
    if let Some(p) = plan_perso {
        if activer_plan(p) {
            return Ok(None);
        }
    }
    if activer_plan(PLAN_PERF) {
        return Ok(None);
    }
    let sortie = executer("powercfg", &["-duplicatescheme", PLAN_PERF]).map_err(|e| e.to_string())?;
    let guid = extraire_guid(&String::from_utf8_lossy(&sortie.stdout)).ok_or("Windows refuse de créer le plan Performances élevées")?;
    if activer_plan(&guid) {
        Ok(Some(guid))
    } else {
        Err("Impossible d'activer le plan Performances élevées".into())
    }
}

pub fn est_plan_perf(guid: &str, plan_perso: Option<&str>) -> bool {
    guid == PLAN_PERF || guid == PLAN_OPTIMAL || Some(guid) == plan_perso
}
