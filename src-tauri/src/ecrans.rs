// Écrans connectés : fréquence, HDR, couleurs en jeu (rampe gamma) et vibrance NVIDIA.

use crate::rampe;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Mutex;

use windows_sys::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig, DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME,
    QDC_ONLY_ACTIVE_PATHS,
};
use windows_sys::Win32::Graphics::Gdi::{
    ChangeDisplaySettingsExW, CreateDCW, DeleteDC, EnumDisplayDevicesW, EnumDisplaySettingsW, CDS_UPDATEREGISTRY, DEVMODEW,
    DISPLAY_DEVICEW, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISPLAY_DEVICE_PRIMARY_DEVICE, DISP_CHANGE_BADMODE, DISP_CHANGE_RESTART,
    DISP_CHANGE_SUCCESSFUL, DMDFO_CENTER, DMDFO_STRETCH, DM_DISPLAYFIXEDOUTPUT, DM_DISPLAYFREQUENCY, DM_PELSHEIGHT, DM_PELSWIDTH,
    ENUM_CURRENT_SETTINGS,
};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::UI::ColorSystem::{GetDeviceGammaRamp, SetDeviceGammaRamp};

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Couleurs {
    pub luminosite: f64,
    pub contraste: f64,
    pub gamma: f64,
    /// Vibrance NVIDIA en % (50 = neutre, 100 = maximum), comme dans le panneau NVIDIA
    pub vibrance: Option<u32>,
}

impl Default for Couleurs {
    fn default() -> Self {
        Self { luminosite: 0.0, contraste: 1.0, gamma: 1.0, vibrance: None }
    }
}

#[derive(Serialize)]
pub struct Ecran {
    /// \\.\DISPLAY1…
    pub id: String,
    pub nom: String,
    pub principal: bool,
    pub largeur: u32,
    pub hauteur: u32,
    pub hz: u32,
    pub frequences: Vec<u32>,
    /// Toutes les résolutions proposées, de la plus grande à la plus petite
    pub modes: Vec<Mode>,
    /// Mise à l'échelle des résolutions non natives : 0 par défaut, 1 étirée, 2 bandes noires
    pub mise_echelle: u32,
    pub hdr_supporte: bool,
    pub hdr_actif: bool,
    /// Vibrance actuelle en %, si l'écran est branché sur une carte NVIDIA
    pub vibrance: Option<u32>,
}

#[derive(Serialize, Clone)]
pub struct Mode {
    pub largeur: u32,
    pub hauteur: u32,
    pub frequences: Vec<u32>,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn texte(buf: &[u16]) -> String {
    String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())])
}

fn mode_actuel(id: &str) -> Option<DEVMODEW> {
    let mut dm: DEVMODEW = unsafe { std::mem::zeroed() };
    dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
    (unsafe { EnumDisplaySettingsW(wide(id).as_ptr(), ENUM_CURRENT_SETTINGS, &mut dm) } != 0).then_some(dm)
}

/// Résolutions et fréquences proposées par l'écran.
fn modes(id: &str) -> Vec<Mode> {
    let nom = wide(id);
    let mut liste: Vec<Mode> = Vec::new();
    let mut i = 0;
    loop {
        let mut dm: DEVMODEW = unsafe { std::mem::zeroed() };
        dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        if unsafe { EnumDisplaySettingsW(nom.as_ptr(), i, &mut dm) } == 0 {
            break;
        }
        i += 1;
        if dm.dmDisplayFrequency <= 1 {
            continue;
        }
        match liste.iter_mut().find(|m| m.largeur == dm.dmPelsWidth && m.hauteur == dm.dmPelsHeight) {
            Some(m) if !m.frequences.contains(&dm.dmDisplayFrequency) => m.frequences.push(dm.dmDisplayFrequency),
            Some(_) => {}
            None => liste.push(Mode { largeur: dm.dmPelsWidth, hauteur: dm.dmPelsHeight, frequences: vec![dm.dmDisplayFrequency] }),
        }
    }
    for m in &mut liste {
        m.frequences.sort_unstable();
    }
    liste.sort_by_key(|m| std::cmp::Reverse((m.largeur * m.hauteur, m.largeur)));
    liste
}

struct InfosConfig {
    nom: String,
    hdr_supporte: bool,
    hdr_actif: bool,
}

/// Noms commerciaux des écrans et état HDR, via l'API DisplayConfig.
fn infos_config() -> HashMap<String, InfosConfig> {
    let mut res = HashMap::new();
    unsafe {
        let (mut np, mut nm) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm) != 0 {
            return res;
        }
        let mut chemins: Vec<DISPLAYCONFIG_PATH_INFO> = vec![std::mem::zeroed(); np as usize];
        let mut modes: Vec<DISPLAYCONFIG_MODE_INFO> = vec![std::mem::zeroed(); nm as usize];
        if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, chemins.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), std::ptr::null_mut()) != 0 {
            return res;
        }
        for c in &chemins[..np as usize] {
            let mut source: DISPLAYCONFIG_SOURCE_DEVICE_NAME = std::mem::zeroed();
            source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            source.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            source.header.adapterId = c.sourceInfo.adapterId;
            source.header.id = c.sourceInfo.id;
            if DisplayConfigGetDeviceInfo(&mut source.header) != 0 {
                continue;
            }
            let mut cible: DISPLAYCONFIG_TARGET_DEVICE_NAME = std::mem::zeroed();
            cible.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            cible.header.size = std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            cible.header.adapterId = c.targetInfo.adapterId;
            cible.header.id = c.targetInfo.id;
            let nom = if DisplayConfigGetDeviceInfo(&mut cible.header) == 0 { texte(&cible.monitorFriendlyDeviceName) } else { String::new() };
            let mut couleur: DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO = std::mem::zeroed();
            couleur.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO;
            couleur.header.size = std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32;
            couleur.header.adapterId = c.targetInfo.adapterId;
            couleur.header.id = c.targetInfo.id;
            let bits = if DisplayConfigGetDeviceInfo(&mut couleur.header) == 0 { couleur.Anonymous.value } else { 0 };
            res.insert(texte(&source.viewGdiDeviceName), InfosConfig { nom, hdr_supporte: bits & 1 != 0, hdr_actif: bits & 2 != 0 });
        }
    }
    res
}

pub fn liste() -> Vec<Ecran> {
    let infos = infos_config();
    let vibrances = nvapi::vibrances();
    let mut ecrans = Vec::new();
    let mut i = 0;
    loop {
        let mut dd: DISPLAY_DEVICEW = unsafe { std::mem::zeroed() };
        dd.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
        if unsafe { EnumDisplayDevicesW(std::ptr::null(), i, &mut dd, 0) } == 0 {
            break;
        }
        i += 1;
        if dd.StateFlags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP == 0 {
            continue;
        }
        let id = texte(&dd.DeviceName);
        let Some(dm) = mode_actuel(&id) else { continue };
        let info = infos.get(&id);
        let modes = modes(&id);
        let frequences = modes.iter().find(|m| m.largeur == dm.dmPelsWidth && m.hauteur == dm.dmPelsHeight).map(|m| m.frequences.clone()).unwrap_or_default();
        ecrans.push(Ecran {
            nom: info.map(|i| i.nom.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| texte(&dd.DeviceString)),
            principal: dd.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE != 0,
            largeur: dm.dmPelsWidth,
            hauteur: dm.dmPelsHeight,
            hz: dm.dmDisplayFrequency,
            frequences,
            modes,
            mise_echelle: unsafe { dm.Anonymous1.Anonymous2.dmDisplayFixedOutput },
            hdr_supporte: info.is_some_and(|i| i.hdr_supporte),
            hdr_actif: info.is_some_and(|i| i.hdr_actif),
            vibrance: vibrances.get(&id).map(|v| v.pourcent()),
            id,
        });
    }
    ecrans.sort_by_key(|e| !e.principal);
    ecrans
}

/// Change la résolution, la fréquence et la mise à l'échelle. `flags` = 0 : changement temporaire
/// (oublié au redémarrage) ; CDS_UPDATEREGISTRY : enregistré. Renvoie un avertissement éventuel.
fn appliquer_mode(id: &str, largeur: u32, hauteur: u32, hz: Option<u32>, echelle: Option<u32>, flags: u32) -> Result<Option<String>, String> {
    let mut dm = mode_actuel(id).ok_or("Écran introuvable")?;
    let freqs = modes(id)
        .into_iter()
        .find(|m| m.largeur == largeur && m.hauteur == hauteur)
        .ok_or_else(|| format!("{largeur} × {hauteur} n'est pas proposé par cet écran"))?
        .frequences;
    // Fréquence demandée si possible, sinon la plus haute de cette résolution
    dm.dmDisplayFrequency = hz.filter(|h| freqs.contains(h)).or_else(|| freqs.iter().max().copied()).unwrap_or(dm.dmDisplayFrequency);
    dm.dmPelsWidth = largeur;
    dm.dmPelsHeight = hauteur;
    dm.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;
    let changer = |dm: &DEVMODEW| unsafe { ChangeDisplaySettingsExW(wide(id).as_ptr(), dm, std::ptr::null_mut(), flags, std::ptr::null()) };
    let mut avertissement = None;
    let mut r = if let Some(e) = echelle {
        let mut avec = dm;
        avec.dmFields |= DM_DISPLAYFIXEDOUTPUT;
        avec.Anonymous1.Anonymous2.dmDisplayFixedOutput = e; // écrire un champ d'union est sûr
        changer(&avec)
    } else {
        changer(&dm)
    };
    if r != DISP_CHANGE_SUCCESSFUL && echelle.is_some() {
        // Certains pilotes (souvent NVIDIA) gèrent eux-mêmes la mise à l'échelle
        r = changer(&dm);
        avertissement = Some("Ton pilote refuse de régler la mise à l'échelle depuis Windows : choisis « Plein écran » (NVIDIA : Ajuster la taille et la position du bureau) ou « Plein panneau » (AMD) dans son panneau.".to_string());
    }
    match r {
        DISP_CHANGE_SUCCESSFUL => Ok(avertissement),
        DISP_CHANGE_RESTART => Err("Windows demande un redémarrage pour ce mode".into()),
        DISP_CHANGE_BADMODE => Err("Mode refusé par l'écran ou le pilote".into()),
        code => Err(format!("Windows a refusé ce mode (code {code})")),
    }
}

/// Remet le mode enregistré dans Windows (annule un changement temporaire).
pub fn retablir_mode(id: &str) -> bool {
    unsafe { ChangeDisplaySettingsExW(wide(id).as_ptr(), std::ptr::null(), std::ptr::null_mut(), 0, std::ptr::null()) == DISP_CHANGE_SUCCESSFUL }
}

/// Essai en cours (écran, numéro) : sans confirmation sous 15 s, l'ancien mode revient tout seul,
/// même si l'interface ne s'affiche plus (écran noir, mode non géré).
static ESSAI: Mutex<Option<(String, u64)>> = Mutex::new(None);
static NUMERO: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn essayer_mode(id: &str, largeur: u32, hauteur: u32, hz: Option<u32>, echelle: Option<u32>) -> Result<Option<String>, String> {
    let avertissement = appliquer_mode(id, largeur, hauteur, hz, echelle, 0)?;
    let n = NUMERO.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    *ESSAI.lock().unwrap() = Some((id.to_string(), n));
    let id = id.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(15));
        let mut essai = ESSAI.lock().unwrap();
        if essai.as_ref().is_some_and(|(i, k)| *i == id && *k == n) {
            *essai = None;
            drop(essai);
            retablir_mode(&id);
        }
    });
    Ok(avertissement)
}

/// Garde le mode essayé : il est enregistré et survit au redémarrage.
pub fn confirmer_mode(id: &str) -> Result<(), String> {
    let mut essai = ESSAI.lock().unwrap();
    if !essai.as_ref().is_some_and(|(i, _)| i == id) {
        return Err("Délai dépassé : l'ancienne résolution a été remise".into());
    }
    *essai = None;
    drop(essai);
    let dm = mode_actuel(id).ok_or("Écran introuvable")?;
    let echelle = unsafe { dm.Anonymous1.Anonymous2.dmDisplayFixedOutput };
    appliquer_mode(id, dm.dmPelsWidth, dm.dmPelsHeight, Some(dm.dmDisplayFrequency), Some(echelle), CDS_UPDATEREGISTRY).map(|_| ())
}

pub fn annuler_essai(id: &str) {
    let mut essai = ESSAI.lock().unwrap();
    if essai.as_ref().is_some_and(|(i, _)| i == id) {
        *essai = None;
        drop(essai);
        retablir_mode(id);
    }
}

fn ecran_principal() -> Option<String> {
    (0..16).find_map(|i| {
        let mut dd: DISPLAY_DEVICEW = unsafe { std::mem::zeroed() };
        dd.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
        (unsafe { EnumDisplayDevicesW(std::ptr::null(), i, &mut dd, 0) } != 0 && dd.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE != 0).then(|| texte(&dd.DeviceName))
    })
}

/// Résolution d'un jeu sur l'écran principal, le temps de la partie (non enregistrée).
pub fn mode_pour_jeu(largeur: u32, hauteur: u32, etirer: bool) -> Result<(String, Option<String>), String> {
    let id = ecran_principal().ok_or("Écran principal introuvable")?;
    let echelle = if etirer { DMDFO_STRETCH } else { DMDFO_CENTER };
    appliquer_mode(&id, largeur, hauteur, None, Some(echelle), 0).map(|a| (id, a))
}

/// Rampes et vibrances d'origine, remises à la fin de la partie.
static ORIGINES: Mutex<Option<(HashMap<String, [u16; 768]>, HashMap<String, i32>)>> = Mutex::new(None);

fn rampe_ecran(id: &str, lire: Option<&mut [u16; 768]>, ecrire: Option<&[u16; 768]>) -> bool {
    unsafe {
        let dc = CreateDCW(std::ptr::null(), wide(id).as_ptr(), std::ptr::null(), std::ptr::null());
        if dc.is_null() {
            return false;
        }
        let ok = match (lire, ecrire) {
            (Some(r), _) => GetDeviceGammaRamp(dc, r.as_mut_ptr() as *mut c_void) != 0,
            (_, Some(r)) => SetDeviceGammaRamp(dc, r.as_ptr() as *const c_void) != 0,
            _ => false,
        };
        DeleteDC(dc);
        ok
    }
}

/// Applique les couleurs choisies (écran par écran) en mémorisant l'état d'origine.
pub fn appliquer(reglages: &HashMap<String, Couleurs>) -> Vec<String> {
    let mut erreurs = Vec::new();
    let mut origines = ORIGINES.lock().unwrap();
    let (rampes, vibrances) = origines.get_or_insert_with(Default::default);
    let nv = nvapi::vibrances();
    for (id, c) in reglages {
        if !rampes.contains_key(id) {
            let mut r = [0u16; 768];
            if rampe_ecran(id, Some(&mut r), None) {
                rampes.insert(id.clone(), r);
            }
        }
        if !rampe_ecran(id, None, Some(&rampe::calculer(c.luminosite, c.contraste, c.gamma))) {
            erreurs.push(format!("{id} : Windows refuse ce réglage de couleurs"));
        }
        if let (Some(pct), Some(v)) = (c.vibrance, nv.get(id)) {
            vibrances.entry(id.clone()).or_insert(v.actuel);
            if !v.regler_pourcent(pct) {
                erreurs.push(format!("{id} : vibrance refusée par le pilote NVIDIA"));
            }
        }
    }
    erreurs
}

pub fn restaurer() {
    let Some((rampes, vibrances)) = ORIGINES.lock().unwrap().take() else { return };
    for (id, r) in &rampes {
        rampe_ecran(id, None, Some(r));
    }
    restaurer_vibrances(&vibrances);
}

/// Vibrance numérique NVIDIA (le réglage « Vibrance numérique » du panneau NVIDIA), via nvapi64.dll.
/// Fonctions non documentées publiquement : en cas d'échec, l'option n'est simplement pas proposée.
mod nvapi {
    use super::*;

    type QueryInterface = unsafe extern "C" fn(u32) -> *const c_void;
    type Initialize = unsafe extern "C" fn() -> i32;
    type EnumDisplay = unsafe extern "C" fn(u32, *mut *mut c_void) -> i32;
    type NomDisplay = unsafe extern "C" fn(*mut c_void, *mut [u8; 64]) -> i32;
    type GetDvc = unsafe extern "C" fn(*mut c_void, u32, *mut InfoDvc) -> i32;
    type SetDvc = unsafe extern "C" fn(*mut c_void, u32, i32) -> i32;

    #[repr(C)]
    #[derive(Default)]
    struct InfoDvc {
        version: u32,
        actuel: i32,
        min: i32,
        max: i32,
    }

    pub struct Vibrance {
        poignee: *mut c_void,
        set: SetDvc,
        pub actuel: i32,
        min: i32,
        max: i32,
    }

    impl Vibrance {
        /// Comme le panneau NVIDIA : 50 % = neutre (minimum), 100 % = maximum
        pub fn pourcent(&self) -> u32 {
            let plage = (self.max - self.min).max(1) as f64;
            (50.0 + (self.actuel - self.min) as f64 / plage * 50.0).round() as u32
        }

        pub fn regler_pourcent(&self, pct: u32) -> bool {
            let pct = pct.clamp(50, 100) as f64;
            self.regler(self.min + ((pct - 50.0) / 50.0 * (self.max - self.min) as f64).round() as i32)
        }

        pub fn regler(&self, niveau: i32) -> bool {
            unsafe { (self.set)(self.poignee, 0, niveau.clamp(self.min, self.max)) == 0 }
        }
    }

    pub fn vibrances() -> HashMap<String, Vibrance> {
        let mut res = HashMap::new();
        unsafe {
            let dll = LoadLibraryW(wide("nvapi64.dll").as_ptr());
            if dll.is_null() {
                return res;
            }
            let Some(qi) = GetProcAddress(dll, b"nvapi_QueryInterface\0".as_ptr()) else { return res };
            let qi: QueryInterface = std::mem::transmute(qi);
            let f = |id: u32| qi(id);
            let (init, enumerer, nom, get, set) = (f(0x0150_E828), f(0x9ABD_D40D), f(0x22A7_8B05), f(0x4085_DE45), f(0x1724_09B4));
            if [init, enumerer, nom, get, set].iter().any(|p| p.is_null()) {
                return res;
            }
            let init: Initialize = std::mem::transmute(init);
            let enumerer: EnumDisplay = std::mem::transmute(enumerer);
            let nom: NomDisplay = std::mem::transmute(nom);
            let get: GetDvc = std::mem::transmute(get);
            let set: SetDvc = std::mem::transmute(set);
            if init() != 0 {
                return res;
            }
            for i in 0..8 {
                let mut poignee = std::ptr::null_mut();
                if enumerer(i, &mut poignee) != 0 {
                    break;
                }
                let mut buf = [0u8; 64];
                if nom(poignee, &mut buf) != 0 {
                    continue;
                }
                let id = String::from_utf8_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(64)]).to_string();
                let mut info = InfoDvc { version: std::mem::size_of::<InfoDvc>() as u32 | (1 << 16), ..Default::default() };
                if get(poignee, 0, &mut info) == 0 && info.max > info.min {
                    res.insert(id, Vibrance { poignee, set, actuel: info.actuel, min: info.min, max: info.max });
                }
            }
        }
        res
    }
}

/// Vibrances d'origine en cours de modification, à mémoriser sur disque :
/// le pilote NVIDIA les garde même après un redémarrage si l'app est coupée en pleine partie.
pub fn vibrances_modifiees() -> HashMap<String, i32> {
    ORIGINES.lock().unwrap().as_ref().map(|(_, v)| v.clone()).unwrap_or_default()
}

pub fn restaurer_vibrances(origines: &HashMap<String, i32>) {
    let nv = nvapi::vibrances();
    for (id, niveau) in origines {
        if let Some(v) = nv.get(id) {
            v.regler(*niveau);
        }
    }
}
