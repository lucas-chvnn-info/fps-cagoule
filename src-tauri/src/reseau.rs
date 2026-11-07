// Connexion internet : test de ping (API ICMP de Windows, sans console) et carte réseau utilisée.

use crate::{ps, stats};
use serde::Serialize;
use std::ffi::c_void;
use std::net::Ipv4Addr;
use std::thread;
use std::time::Duration;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetBestRoute, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY, IP_SUCCESS, MIB_IPFORWARDROW,
};

const INTERNET: Ipv4Addr = Ipv4Addr::new(1, 1, 1, 1);
const NB_PINGS: usize = 25;

#[derive(Serialize)]
pub struct ResultatPing {
    pub cible: String,
    pub adresse: String,
    pub moyenne: Option<f64>,
    pub min: Option<u32>,
    pub max: Option<u32>,
    pub gigue: Option<f64>,
    pub pertes: f64,
}

fn adresse(ip: Ipv4Addr) -> u32 {
    u32::from_ne_bytes(ip.octets())
}

/// Adresse de la box (passerelle utilisée pour aller sur Internet).
fn passerelle() -> Option<Ipv4Addr> {
    let mut route: MIB_IPFORWARDROW = unsafe { std::mem::zeroed() };
    if unsafe { GetBestRoute(adresse(INTERNET), 0, &mut route) } != 0 || route.dwForwardNextHop == 0 {
        return None;
    }
    Some(Ipv4Addr::from(route.dwForwardNextHop.to_ne_bytes()))
}

fn mesurer(ip: Ipv4Addr) -> Vec<Option<u32>> {
    unsafe {
        let h = IcmpCreateFile();
        if h == INVALID_HANDLE_VALUE {
            return vec![None; NB_PINGS];
        }
        let donnees = *b"fps-cagoule-ping";
        let mut reponse = [0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + 16 + 8];
        let mut mesures = Vec::with_capacity(NB_PINGS);
        for i in 0..NB_PINGS {
            let n = IcmpSendEcho(
                h,
                adresse(ip),
                donnees.as_ptr() as *const c_void,
                donnees.len() as u16,
                std::ptr::null(),
                reponse.as_mut_ptr() as *mut c_void,
                reponse.len() as u32,
                1000,
            );
            let r = &*(reponse.as_ptr() as *const ICMP_ECHO_REPLY);
            mesures.push((n > 0 && r.Status == IP_SUCCESS).then_some(r.RoundTripTime));
            if i + 1 < NB_PINGS {
                thread::sleep(Duration::from_millis(100));
            }
        }
        IcmpCloseHandle(h);
        mesures
    }
}

/// Ping de la box et d'Internet en parallèle (environ 3 secondes).
pub fn tester() -> Vec<ResultatPing> {
    let mut cibles = Vec::new();
    if let Some(gw) = passerelle() {
        cibles.push(("Ta box".to_string(), gw));
    }
    cibles.push(("Internet (Cloudflare)".to_string(), INTERNET));
    let fils: Vec<_> = cibles.into_iter().map(|(nom, ip)| (nom, ip, thread::spawn(move || mesurer(ip)))).collect();
    fils.into_iter()
        .map(|(cible, ip, fil)| {
            let mesures = fil.join().unwrap_or_default();
            let r = stats::resumer_ping(&mesures);
            ResultatPing {
                cible,
                adresse: ip.to_string(),
                moyenne: r.as_ref().map(|r| r.moyenne),
                min: r.as_ref().map(|r| r.min),
                max: r.as_ref().map(|r| r.max),
                gigue: r.as_ref().map(|r| r.gigue),
                pertes: r.map_or(100.0, |r| r.pertes),
            }
        })
        .collect()
}

/// Cartes réseau connectées (nom, Wi-Fi ou câble, vitesse).
pub fn cartes() -> Result<serde_json::Value, String> {
    ps::json(
        "ConvertTo-Json -Compress -Depth 3 -InputObject @(Get-NetAdapter -Physical | Where-Object Status -eq 'Up' | ForEach-Object {
  [ordered]@{ nom = $_.Name; description = $_.InterfaceDescription; vitesse = $_.LinkSpeed
    wifi = ($_.PhysicalMediaType -match '802\\.11|Wireless') -or ($_.InterfaceDescription -match 'Wi-?Fi|Wireless') }
})",
    )
    .map(|v| if v.is_array() { v } else { serde_json::Value::Array(vec![v]) })
}
