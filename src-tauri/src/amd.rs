// Cartes AMD Radeon : réglages du pilote, préréglages de performance et température via ADLX,
// la bibliothèque officielle d'AMD (amdadlx64.dll, installée avec le pilote Adrenalin).
// Interfaces et numéros de méthodes relevés dans les en-têtes C du SDK (github.com/GPUOpen-LibrariesAndSDKs/ADLX).
// Chaque appel ouvre ADLX, fait son travail et le referme : rien ne reste chargé en arrière-plan,
// sauf pendant une partie pour la température (refermé à la fin de la partie).

use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

type P = *mut c_void;
type Res = i32;

fn ok(r: Res) -> bool {
    (0..=2).contains(&r)
}

/// Méthode n° `n` de la table virtuelle de l'interface `p`
unsafe fn m<F: Copy>(p: P, n: usize) -> F {
    let table = *(p as *const *const usize);
    std::mem::transmute_copy(&*table.add(n))
}

unsafe fn liberer(p: P) {
    if !p.is_null() {
        m::<extern "system" fn(P) -> i32>(p, 1)(p);
    }
}

struct Dll {
    initialiser: extern "C" fn(u64, *mut P) -> Res,
    version: extern "C" fn(*mut u64) -> Res,
    terminer: extern "C" fn() -> Res,
}

fn dll() -> Option<&'static Dll> {
    static DLL: OnceLock<Option<Dll>> = OnceLock::new();
    DLL.get_or_init(|| unsafe {
        let nom: Vec<u16> = "amdadlx64.dll\0".encode_utf16().collect();
        let h = LoadLibraryW(nom.as_ptr());
        if h.is_null() {
            return None;
        }
        Some(Dll {
            initialiser: std::mem::transmute(GetProcAddress(h, b"ADLXInitialize\0".as_ptr())?),
            version: std::mem::transmute(GetProcAddress(h, b"ADLXQueryFullVersion\0".as_ptr())?),
            terminer: std::mem::transmute(GetProcAddress(h, b"ADLXTerminate\0".as_ptr())?),
        })
    })
    .as_ref()
}

/// ADLX ouvert, avec la carte AMD dédiée (ou la première trouvée)
pub struct Session {
    systeme: P,
    gpu: P,
}

unsafe impl Send for Session {}

impl Session {
    pub fn ouvrir() -> Result<Self, String> {
        let d = dll().ok_or("Pilote AMD Adrenalin introuvable")?;
        let mut version = 0u64;
        if !ok((d.version)(&mut version)) {
            return Err("Version du pilote AMD illisible".into());
        }
        let mut systeme: P = std::ptr::null_mut();
        if !ok((d.initialiser)(version, &mut systeme)) || systeme.is_null() {
            return Err("Le pilote AMD refuse la connexion (mets à jour Adrenalin)".into());
        }
        let mut s = Session { systeme, gpu: std::ptr::null_mut() };
        unsafe {
            let mut liste: P = std::ptr::null_mut();
            if ok(m::<extern "system" fn(P, *mut P) -> Res>(systeme, 1)(systeme, &mut liste)) && !liste.is_null() {
                let n = m::<extern "system" fn(P) -> u32>(liste, 3)(liste);
                for i in 0..n {
                    let mut g: P = std::ptr::null_mut();
                    if !ok(m::<extern "system" fn(P, u32, *mut P) -> Res>(liste, 11)(liste, i, &mut g)) || g.is_null() {
                        continue;
                    }
                    let mut type_ = 0i32;
                    m::<extern "system" fn(P, *mut i32) -> Res>(g, 5)(g, &mut type_);
                    // 2 = carte dédiée : on la préfère à la puce intégrée au processeur
                    if s.gpu.is_null() || type_ == 2 {
                        liberer(s.gpu);
                        s.gpu = g;
                        if type_ == 2 {
                            break;
                        }
                    } else {
                        liberer(g);
                    }
                }
                liberer(liste);
            }
        }
        if s.gpu.is_null() {
            return Err("Aucune carte AMD trouvée".into());
        }
        Ok(s)
    }

    pub fn nom(&self) -> String {
        unsafe {
            let mut nom: *const i8 = std::ptr::null();
            m::<extern "system" fn(P, *mut *const i8) -> Res>(self.gpu, 7)(self.gpu, &mut nom);
            if nom.is_null() {
                return "Carte AMD".into();
            }
            std::ffi::CStr::from_ptr(nom).to_string_lossy().into_owned()
        }
    }

    /// Service du système (n° de méthode d'IADLXSystem), à libérer après usage
    fn service(&self, n: usize) -> Result<P, String> {
        let mut s: P = std::ptr::null_mut();
        let r = unsafe { m::<extern "system" fn(P, *mut P) -> Res>(self.systeme, n)(self.systeme, &mut s) };
        if ok(r) && !s.is_null() { Ok(s) } else { Err(format!("Service AMD indisponible (code {r})")) }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe { liberer(self.gpu) };
        if let Some(d) = dll() {
            (d.terminer)();
        }
    }
}

/// Réglages 3D d'Adrenalin pris en charge : (méthode d'IADLX3DSettingsServices, lecture, écriture)
/// lecture/écriture : n° des méthodes IsEnabled/SetEnabled, ou GetMode/SetMode pour la synchro verticale
const ANTI_LAG: (usize, usize, usize) = (3, 4, 5);
const CHILL: (usize, usize, usize) = (4, 4, 8);
const BOOST: (usize, usize, usize) = (5, 4, 7);
const ENHANCED_SYNC: (usize, usize, usize) = (7, 4, 5);
const VSYNC: (usize, usize, usize) = (8, 5, 6);

fn reglage(nom: &str) -> Option<(usize, usize, usize)> {
    Some(match nom {
        "antilag" => ANTI_LAG,
        "chill" => CHILL,
        "boost" => BOOST,
        "enhancedsync" => ENHANCED_SYNC,
        "vsync" => VSYNC,
        _ => return None,
    })
}

impl Session {
    /// Lit un réglage 3D : 0/1 pour un interrupteur, le mode (0 à 3) pour la synchro verticale. None = non pris en charge.
    pub fn lire(&self, nom: &str) -> Result<Option<i32>, String> {
        let (obtenir, lire, _) = reglage(nom).ok_or("Réglage AMD inconnu")?;
        let services = self.service(7)?;
        let r = unsafe {
            let mut i: P = std::ptr::null_mut();
            let res = if ok(m::<extern "system" fn(P, P, *mut P) -> Res>(services, obtenir)(services, self.gpu, &mut i)) && !i.is_null() {
                let mut pris = 0u8;
                m::<extern "system" fn(P, *mut u8) -> Res>(i, 3)(i, &mut pris);
                if pris == 0 {
                    None
                } else if nom == "vsync" {
                    let mut mode = 0i32;
                    ok(m::<extern "system" fn(P, *mut i32) -> Res>(i, lire)(i, &mut mode)).then_some(mode)
                } else {
                    let mut actif = 0u8;
                    ok(m::<extern "system" fn(P, *mut u8) -> Res>(i, lire)(i, &mut actif)).then_some(actif as i32)
                }
            } else {
                None
            };
            liberer(i);
            res
        };
        unsafe { liberer(services) };
        Ok(r)
    }

    pub fn ecrire(&self, nom: &str, valeur: i32) -> Result<(), String> {
        let (obtenir, _, ecrire) = reglage(nom).ok_or("Réglage AMD inconnu")?;
        let services = self.service(7)?;
        let r = unsafe {
            let mut i: P = std::ptr::null_mut();
            let r = if ok(m::<extern "system" fn(P, P, *mut P) -> Res>(services, obtenir)(services, self.gpu, &mut i)) && !i.is_null() {
                if nom == "vsync" {
                    m::<extern "system" fn(P, i32) -> Res>(i, ecrire)(i, valeur)
                } else {
                    m::<extern "system" fn(P, u8) -> Res>(i, ecrire)(i, (valeur != 0) as u8)
                }
            } else {
                12
            };
            liberer(i);
            r
        };
        unsafe { liberer(services) };
        if ok(r) { Ok(()) } else { Err(format!("Le pilote AMD refuse ce réglage (code {r})")) }
    }

    pub fn temperature(&self) -> Option<i32> {
        let suivi = self.service(9).ok()?;
        let t = unsafe {
            let mut mesures: P = std::ptr::null_mut();
            let mut t = 0f64;
            let lu = ok(m::<extern "system" fn(P, P, *mut P) -> Res>(suivi, 18)(suivi, self.gpu, &mut mesures))
                && !mesures.is_null()
                && ok(m::<extern "system" fn(P, *mut f64) -> Res>(mesures, 7)(mesures, &mut t));
            liberer(mesures);
            lu.then_some(t.round() as i32)
        };
        unsafe { liberer(suivi) };
        t.filter(|c| (1..130).contains(c))
    }

    /// Préréglage de performance d'Adrenalin (Économie, Silencieux, Équilibré, Turbo, Rage)
    fn avec_prereglage<T>(&self, f: impl FnOnce(P) -> T) -> Result<T, String> {
        let tuning = self.service(8)?;
        let r = unsafe {
            let mut pris = 0u8;
            m::<extern "system" fn(P, P, *mut u8) -> Res>(tuning, 7)(tuning, self.gpu, &mut pris);
            let mut i: P = std::ptr::null_mut();
            if pris == 0 || !ok(m::<extern "system" fn(P, P, *mut P) -> Res>(tuning, 13)(tuning, self.gpu, &mut i)) || i.is_null() {
                Err("Ta carte AMD ne propose pas de préréglages de performance".to_string())
            } else {
                let iid: Vec<u16> = "IADLXGPUPresetTuning\0".encode_utf16().collect();
                let mut p: P = std::ptr::null_mut();
                let q = m::<extern "system" fn(P, *const u16, *mut P) -> Res>(i, 2)(i, iid.as_ptr(), &mut p);
                liberer(i);
                if !ok(q) || p.is_null() {
                    Err("Préréglages AMD indisponibles".to_string())
                } else {
                    let r = f(p);
                    liberer(p);
                    Ok(r)
                }
            }
        };
        unsafe { liberer(tuning) };
        r
    }

    /// (préréglages proposés, préréglage actuel) ; indices 0 à 4 = Économie, Silencieux, Équilibré, Turbo, Rage
    pub fn prereglages(&self) -> Result<(Vec<usize>, Option<usize>), String> {
        self.avec_prereglage(|p| unsafe {
            let mut proposes = Vec::new();
            let mut actuel = None;
            for i in 0..5 {
                let mut b = 0u8;
                m::<extern "system" fn(P, *mut u8) -> Res>(p, 3 + i)(p, &mut b);
                if b != 0 {
                    proposes.push(i);
                    let mut c = 0u8;
                    m::<extern "system" fn(P, *mut u8) -> Res>(p, 8 + i)(p, &mut c);
                    if c != 0 {
                        actuel = Some(i);
                    }
                }
            }
            (proposes, actuel)
        })
    }

    pub fn choisir_prereglage(&self, i: usize) -> Result<(), String> {
        if i > 4 {
            return Err("Préréglage inconnu".into());
        }
        let r = self.avec_prereglage(|p| unsafe { m::<extern "system" fn(P) -> Res>(p, 13 + i)(p) })?;
        if ok(r) { Ok(()) } else { Err(format!("Le pilote AMD refuse ce préréglage (code {r})")) }
    }

    /// Fréquences, tension et ventilation d'usine
    pub fn reglages_usine(&self) -> Result<(), String> {
        let tuning = self.service(8)?;
        let r = unsafe { m::<extern "system" fn(P, P) -> Res>(tuning, 5)(tuning, self.gpu) };
        unsafe { liberer(tuning) };
        if ok(r) { Ok(()) } else { Err(format!("Remise d'usine refusée par le pilote AMD (code {r})")) }
    }
}

/// Une seule connexion ADLX à la fois (ADLXTerminate fermerait aussi celle d'un autre appel) :
/// gardée ouverte pendant une partie pour la température toutes les 3 s, sinon ouverte le temps d'un appel.
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

pub fn avec<T>(f: impl FnOnce(&Session) -> Result<T, String>) -> Result<T, String> {
    let s = SESSION.lock().map_err(|_| "ADLX occupé".to_string())?;
    match s.as_ref() {
        Some(ouverte) => f(ouverte),
        None => {
            let temporaire = Session::ouvrir()?;
            f(&temporaire)
        }
    }
}

pub fn temperature_en_jeu() -> Option<i32> {
    let mut s = SESSION.lock().ok()?;
    if s.is_none() {
        *s = Session::ouvrir().ok();
    }
    s.as_ref()?.temperature()
}

pub fn fin_de_partie() {
    if let Ok(mut s) = SESSION.lock() {
        *s = None;
    }
}
