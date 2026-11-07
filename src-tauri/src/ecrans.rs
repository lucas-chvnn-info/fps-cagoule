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
    DISPLAY_DEVICEW, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISPLAY_DEVICE_PRIMARY_DEVICE, DISP_CHANGE_SUCCESSFUL, DM_DISPLAYFREQUENCY,
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
    pub hdr_supporte: bool,
    pub hdr_actif: bool,
    /// Vibrance actuelle en %, si l'écran est branché sur une carte NVIDIA
    pub vibrance: Option<u32>,
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

/// Fréquences possibles dans la résolution actuelle.
fn frequences(id: &str, largeur: u32, hauteur: u32) -> Vec<u32> {
    let nom = wide(id);
    let mut liste = Vec::new();
    let mut i = 0;
    loop {
        let mut dm: DEVMODEW = unsafe { std::mem::zeroed() };
        dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        if unsafe { EnumDisplaySettingsW(nom.as_ptr(), i, &mut dm) } == 0 {
            break;
        }
        if dm.dmPelsWidth == largeur && dm.dmPelsHeight == hauteur && dm.dmDisplayFrequency > 1 && !liste.contains(&dm.dmDisplayFrequency) {
            liste.push(dm.dmDisplayFrequency);
        }
        i += 1;
    }
    liste.sort_unstable();
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
        ecrans.push(Ecran {
            nom: info.map(|i| i.nom.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| texte(&dd.DeviceString)),
            principal: dd.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE != 0,
            largeur: dm.dmPelsWidth,
            hauteur: dm.dmPelsHeight,
            hz: dm.dmDisplayFrequency,
            frequences: frequences(&id, dm.dmPelsWidth, dm.dmPelsHeight),
            hdr_supporte: info.is_some_and(|i| i.hdr_supporte),
            hdr_actif: info.is_some_and(|i| i.hdr_actif),
            vibrance: vibrances.get(&id).map(|v| v.pourcent()),
            id,
        });
    }
    ecrans.sort_by_key(|e| !e.principal);
    ecrans
}

pub fn changer_frequence(id: &str, hz: u32) -> Result<(), String> {
    let mut dm = mode_actuel(id).ok_or("Écran introuvable")?;
    dm.dmDisplayFrequency = hz;
    dm.dmFields = DM_DISPLAYFREQUENCY;
    let r = unsafe { ChangeDisplaySettingsExW(wide(id).as_ptr(), &dm, std::ptr::null_mut(), CDS_UPDATEREGISTRY, std::ptr::null()) };
    if r == DISP_CHANGE_SUCCESSFUL {
        Ok(())
    } else {
        Err(format!("Windows a refusé {hz} Hz (code {r})"))
    }
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
