// Serveurs DNS : mesure du plus rapide depuis chez l'utilisateur, changement réversible.
// Le DNS ne change pas le ping en jeu : il accélère la connexion aux serveurs (lancement, matchmaking, sites).

use crate::config::{DnsAvant, Stockage};
use crate::ps;
use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, UdpSocket};
use std::thread;
use std::time::{Duration, Instant};

pub const FOURNISSEURS: [(&str, &str, [&str; 2]); 3] = [
    ("cloudflare", "Cloudflare", ["1.1.1.1", "1.0.0.1"]),
    ("google", "Google", ["8.8.8.8", "8.8.4.4"]),
    ("quad9", "Quad9", ["9.9.9.9", "149.112.112.112"]),
];

const DOMAINES: [&str; 5] = ["www.google.com", "discord.com", "www.ubisoft.com", "www.epicgames.com", "store.steampowered.com"];

#[derive(Serialize)]
pub struct Mesure {
    pub id: String,
    pub nom: String,
    pub adresse: String,
    /// Temps de réponse médian (ms), None si aucune réponse
    pub ms: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Carte {
    pub index: u32,
    pub nom: String,
    #[serde(default)]
    pub serveurs: Vec<String>,
    /// DNS fixés à la main (vide = donnés par la box)
    #[serde(default)]
    pub statiques: String,
}

#[derive(Serialize)]
pub struct EtatDns {
    pub mesures: Vec<Mesure>,
    pub cartes: Vec<Carte>,
    /// L'app a changé les DNS (on peut revenir à ceux d'origine)
    pub modifie: bool,
}

/// Une requête DNS « A » en UDP, chronométrée.
fn requete(serveur: Ipv4Addr, domaine: &str, id: u16) -> Option<f64> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.set_read_timeout(Some(Duration::from_millis(1500))).ok()?;
    let mut q = vec![(id >> 8) as u8, id as u8, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    for l in domaine.split('.') {
        q.push(l.len() as u8);
        q.extend(l.as_bytes());
    }
    q.extend([0, 0, 1, 0, 1]);
    let debut = Instant::now();
    sock.send_to(&q, (serveur, 53)).ok()?;
    let mut buf = [0u8; 512];
    loop {
        let (n, _) = sock.recv_from(&mut buf).ok()?;
        if n >= 2 && buf[..2] == q[..2] {
            return Some(debut.elapsed().as_secs_f64() * 1000.0);
        }
    }
}

/// Médiane de 10 requêtes (5 domaines, 2 fois : la 2e montre le serveur avec son cache).
pub fn mesurer(adresse: Ipv4Addr) -> Option<f64> {
    let mut temps: Vec<f64> = Vec::new();
    for tour in 0..2u16 {
        for (i, d) in DOMAINES.iter().enumerate() {
            if let Some(t) = requete(adresse, d, 0x4000 + tour * 16 + i as u16) {
                temps.push(t);
            }
        }
    }
    if temps.len() < 5 {
        return None;
    }
    temps.sort_by(|a, b| a.total_cmp(b));
    Some(temps[temps.len() / 2])
}

const CARTES: &str = r#"
$routes = Get-NetRoute -DestinationPrefix '0.0.0.0/0' -ErrorAction SilentlyContinue | Sort-Object RouteMetric
$vus = @{}
$res = foreach ($r in $routes) {
  if ($vus[$r.InterfaceIndex]) { continue }
  $vus[$r.InterfaceIndex] = 1
  $a = Get-NetAdapter -InterfaceIndex $r.InterfaceIndex -ErrorAction SilentlyContinue
  if (-not $a -or $a.Status -ne 'Up') { continue }
  $dns = (Get-DnsClientServerAddress -InterfaceIndex $r.InterfaceIndex -AddressFamily IPv4 -ErrorAction SilentlyContinue).ServerAddresses
  $statiques = Lire "HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces\$($a.InterfaceGuid)" 'NameServer'
  [pscustomobject]@{ index = [int]$r.InterfaceIndex; nom = $a.Name; serveurs = @($dns); statiques = "$statiques" }
}
ConvertTo-Json -InputObject @($res) -Depth 3 -Compress
"#;

pub fn cartes() -> Result<Vec<Carte>, String> {
    serde_json::from_value(ps::json(CARTES)?).map_err(|e| e.to_string())
}

pub fn etat(stock: &Stockage) -> Result<EtatDns, String> {
    let cartes = cartes()?;
    let mut cibles: Vec<(String, String, Ipv4Addr)> = Vec::new();
    // DNS actuel (souvent la box) en premier, s'il n'est pas déjà un des fournisseurs
    if let Some(a) = cartes.iter().flat_map(|c| &c.serveurs).find_map(|s| s.parse::<Ipv4Addr>().ok()) {
        if !FOURNISSEURS.iter().any(|f| f.2.contains(&a.to_string().as_str())) {
            cibles.push(("actuel".into(), "Ton DNS actuel".into(), a));
        }
    }
    for (id, nom, adresses) in FOURNISSEURS {
        cibles.push((id.into(), nom.into(), adresses[0].parse().unwrap()));
    }
    let fils: Vec<_> = cibles.into_iter().map(|(id, nom, a)| thread::spawn(move || Mesure { id, nom, adresse: a.to_string(), ms: mesurer(a) })).collect();
    let mesures = fils.into_iter().filter_map(|f| f.join().ok()).collect();
    let modifie = !stock.donnees.lock().unwrap().dns_avant.is_empty();
    Ok(EtatDns { mesures, cartes, modifie })
}

fn liste_ps(adresses: &[String]) -> String {
    adresses.iter().filter(|a| a.parse::<Ipv4Addr>().is_ok()).map(|a| format!("'{a}'")).collect::<Vec<_>>().join(",")
}

/// Met les DNS d'un fournisseur sur les cartes réseau utilisées. Les DNS d'origine sont gardés la première fois.
pub fn appliquer(stock: &Stockage, id: &str) -> Result<(), String> {
    let (_, _, adresses) = FOURNISSEURS.iter().find(|f| f.0 == id).ok_or("Fournisseur DNS inconnu")?;
    let cartes = cartes()?;
    if cartes.is_empty() {
        return Err("Aucune carte réseau connectée à Internet".into());
    }
    {
        let mut d = stock.donnees.lock().unwrap();
        if d.dns_avant.is_empty() {
            d.dns_avant = cartes
                .iter()
                .map(|c| DnsAvant {
                    index: c.index,
                    nom: c.nom.clone(),
                    serveurs: c.statiques.split([',', ' ']).filter(|s| !s.is_empty()).map(String::from).collect(),
                })
                .collect();
        }
    }
    stock.sauver();
    let liste = liste_ps(&adresses.iter().map(|a| a.to_string()).collect::<Vec<_>>());
    let script: String = cartes.iter().map(|c| format!("Set-DnsClientServerAddress -InterfaceIndex {} -ServerAddresses @({liste})\n", c.index)).collect();
    ps::executer(&format!("{script}Clear-DnsClientCache")).map(|_| ())
}

/// Remet les DNS d'origine (ceux de la box, ou ceux fixés à la main avant).
pub fn restaurer(stock: &Stockage) -> Result<(), String> {
    let avant = stock.donnees.lock().unwrap().dns_avant.clone();
    if avant.is_empty() {
        return Ok(());
    }
    let script: String = avant
        .iter()
        .map(|c| {
            if c.serveurs.is_empty() {
                format!("try {{ Set-DnsClientServerAddress -InterfaceIndex {} -ResetServerAddresses }} catch {{}}\n", c.index)
            } else {
                format!("try {{ Set-DnsClientServerAddress -InterfaceIndex {} -ServerAddresses @({}) }} catch {{}}\n", c.index, liste_ps(&c.serveurs))
            }
        })
        .collect();
    ps::executer(&format!("{script}Clear-DnsClientCache"))?;
    stock.donnees.lock().unwrap().dns_avant.clear();
    stock.sauver();
    Ok(())
}
