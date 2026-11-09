// Réglages du pilote NVIDIA (ceux du « Panneau de configuration NVIDIA »), via nvapi64.dll.
// Identifiants, structures et numéros de réglages tirés du SDK officiel : github.com/NVIDIA/nvapi
// On modifie le profil global (« Paramètres globaux ») ; les jeux qui ont leur propre réglage le gardent.

use std::ffi::c_void;
use std::sync::OnceLock;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

// Numéros des réglages utilisés (NvApiDriverSettings.h), repris dans tweaks.json en décimal :
// mode de gestion de l'alimentation PREFERRED_PSTATE 0x1057EB71 (1 = performances maximales),
// images pré-rendues PRERENDERLIMIT 0x007BA09E (1 = mode de faible latence activé).

const NVAPI_OK: i32 = 0;
const NVAPI_SETTING_NOT_FOUND: i32 = -160;
const NVDRS_DWORD_TYPE: u32 = 0;

/// NVDRS_SETTING_V1 (#pragma pack(4)) : 12 320 octets
#[repr(C, packed(4))]
struct Reglage {
    version: u32,
    nom: [u16; 2048],
    id: u32,
    type_: u32,
    emplacement: u32,
    predefini_courant: u32,
    predefini_valide: u32,
    predefini: [u8; 4100],
    valeur: [u8; 4100],
}

const _: () = assert!(std::mem::size_of::<Reglage>() == 12320);

impl Reglage {
    fn nouveau(id: u32) -> Box<Self> {
        // Mémoire à zéro : la structure est trop grosse pour Default sur les tableaux
        let mut r: Box<Self> = unsafe { Box::new(std::mem::zeroed()) };
        r.version = std::mem::size_of::<Self>() as u32 | (1 << 16);
        r.id = id;
        r
    }
}

type Poignee = *mut c_void;

struct Api {
    creer_session: unsafe extern "C" fn(*mut Poignee) -> i32,
    detruire_session: unsafe extern "C" fn(Poignee) -> i32,
    charger: unsafe extern "C" fn(Poignee) -> i32,
    enregistrer: unsafe extern "C" fn(Poignee) -> i32,
    profil_global: unsafe extern "C" fn(Poignee, *mut Poignee) -> i32,
    lire: unsafe extern "C" fn(Poignee, Poignee, u32, *mut Reglage) -> i32,
    ecrire: unsafe extern "C" fn(Poignee, Poignee, *mut Reglage) -> i32,
    effacer: unsafe extern "C" fn(Poignee, Poignee, u32) -> i32,
}

// Les pointeurs de fonctions de la DLL restent valides toute la vie du programme
unsafe impl Send for Api {}
unsafe impl Sync for Api {}

fn api() -> Option<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(|| unsafe {
        let nom: Vec<u16> = "nvapi64.dll\0".encode_utf16().collect();
        let dll = LoadLibraryW(nom.as_ptr());
        if dll.is_null() {
            return None;
        }
        let qi: unsafe extern "C" fn(u32) -> *const c_void = std::mem::transmute(GetProcAddress(dll, b"nvapi_QueryInterface\0".as_ptr())?);
        let f = |id: u32| {
            let p = qi(id);
            (!p.is_null()).then_some(p)
        };
        let initialiser: unsafe extern "C" fn() -> i32 = std::mem::transmute(f(0x0150_E828)?);
        if initialiser() != NVAPI_OK {
            return None;
        }
        Some(Api {
            creer_session: std::mem::transmute(f(0x0694_D52E)?),
            detruire_session: std::mem::transmute(f(0xDAD9_CFF8)?),
            charger: std::mem::transmute(f(0x375D_BD6B)?),
            enregistrer: std::mem::transmute(f(0xFCBC_7E14)?),
            profil_global: std::mem::transmute(f(0xDA84_66A0)?),
            lire: std::mem::transmute(f(0x73BF_8338)?),
            ecrire: std::mem::transmute(f(0x577D_D202)?),
            effacer: std::mem::transmute(f(0xE4A2_6362)?),
        })
    })
    .as_ref()
}

/// Ouvre une session sur les profils du pilote, exécute `f` sur le profil global, puis ferme.
fn avec_profil<T>(f: impl FnOnce(&Api, Poignee, Poignee) -> Result<T, String>) -> Result<T, String> {
    let a = api().ok_or("Pas de carte NVIDIA (ou pilote NVIDIA absent)")?;
    unsafe {
        let mut session: Poignee = std::ptr::null_mut();
        if (a.creer_session)(&mut session) != NVAPI_OK {
            return Err("Le pilote NVIDIA refuse l'accès à ses réglages".into());
        }
        let resultat = (|| {
            if (a.charger)(session) != NVAPI_OK {
                return Err("Lecture des réglages NVIDIA impossible".to_string());
            }
            let mut profil: Poignee = std::ptr::null_mut();
            if (a.profil_global)(session, &mut profil) != NVAPI_OK {
                return Err("Profil global NVIDIA introuvable".to_string());
            }
            f(a, session, profil)
        })();
        (a.detruire_session)(session);
        resultat
    }
}

/// Valeur du réglage dans le profil global, None s'il est laissé par défaut.
pub fn lire(id: u32) -> Result<Option<u32>, String> {
    avec_profil(|a, session, profil| {
        let mut r = Reglage::nouveau(id);
        match unsafe { (a.lire)(session, profil, id, &mut *r) } {
            NVAPI_OK => Ok(Some(u32::from_le_bytes([r.valeur[0], r.valeur[1], r.valeur[2], r.valeur[3]]))),
            NVAPI_SETTING_NOT_FOUND => Ok(None),
            code => Err(format!("Lecture du réglage NVIDIA refusée (code {code})")),
        }
    })
}

/// Écrit le réglage (None : le remet par défaut) et enregistre.
pub fn ecrire(id: u32, valeur: Option<u32>) -> Result<(), String> {
    avec_profil(|a, session, profil| unsafe {
        let code = match valeur {
            Some(v) => {
                let mut r = Reglage::nouveau(id);
                r.type_ = NVDRS_DWORD_TYPE;
                r.valeur[..4].copy_from_slice(&v.to_le_bytes());
                (a.ecrire)(session, profil, &mut *r)
            }
            None => match (a.effacer)(session, profil, id) {
                NVAPI_SETTING_NOT_FOUND => NVAPI_OK,
                c => c,
            },
        };
        if code != NVAPI_OK {
            return Err(format!("Le pilote NVIDIA refuse ce réglage (code {code})"));
        }
        match (a.enregistrer)(session) {
            NVAPI_OK => Ok(()),
            c => Err(format!("Enregistrement des réglages NVIDIA impossible (code {c})")),
        }
    })
}

/* ---------- Overclocking : décalages de fréquence (P-states 2.0) et limite de puissance ----------
   Lecture : NvAPI_GPU_GetPstates20 (public, structures du SDK officiel).
   Écriture : NvAPI_GPU_SetPstates20 et ClientPowerPolicies (non documentés, identifiants et structures
   repris de la crate nvapi-sys et utilisés par NVIDIA Inspector). Structures versionnées : un format
   inattendu est refusé par le pilote. Les décalages disparaissent au redémarrage. */

#[repr(C)]
#[derive(Clone, Copy)]
struct Delta {
    valeur: i32,
    min: i32,
    max: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Horloge {
    domaine: u32,
    type_: u32,
    editable: u32,
    delta: Delta,
    donnees: [u32; 5],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Tension {
    domaine: u32,
    editable: u32,
    uv: u32,
    delta: Delta,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Pstate {
    id: u32,
    editable: u32,
    horloges: [Horloge; 8],
    tensions: [Tension; 4],
}

#[repr(C)]
struct Pstates20V1 {
    version: u32,
    editable: u32,
    nb_pstates: u32,
    nb_horloges: u32,
    nb_tensions: u32,
    pstates: [Pstate; 16],
}

#[repr(C)]
struct Pstates20V2 {
    v1: Pstates20V1,
    nb_ov: u32,
    ov: [Tension; 4],
}

const _: () = assert!(std::mem::size_of::<Horloge>() == 44);
const _: () = assert!(std::mem::size_of::<Pstate>() == 456);
const _: () = assert!(std::mem::size_of::<Pstates20V1>() == 0x1C94);
const _: () = assert!(std::mem::size_of::<Pstates20V2>() == 0x1CF8);

#[repr(C)]
#[derive(Clone, Copy)]
struct InfoPuissanceEntree {
    pstate: u32,
    b: u32,
    c: u32,
    min: u32,
    e: u32,
    f: u32,
    defaut: u32,
    h: u32,
    i: u32,
    max: u32,
    k: u32,
}

#[repr(C)]
struct InfoPuissance {
    version: u32,
    valide: u8,
    nombre: u8,
    _remplissage: [u8; 2],
    entrees: [InfoPuissanceEntree; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EtatPuissanceEntree {
    a: u32,
    b: u32,
    puissance: u32,
    d: u32,
}

#[repr(C)]
struct EtatPuissance {
    version: u32,
    nombre: u32,
    entrees: [EtatPuissanceEntree; 4],
}

const _: () = assert!(std::mem::size_of::<InfoPuissance>() == 184);
const _: () = assert!(std::mem::size_of::<EtatPuissance>() == 72);

const DOMAINE_GPU: u32 = 0;
const DOMAINE_MEMOIRE: u32 = 4;

/// Bornes de sécurité de l'app, plus prudentes que ce que le pilote accepte parfois
pub const COEUR_MIN: i32 = -300;
pub const COEUR_MAX: i32 = 250;
pub const MEMOIRE_MIN: i32 = -500;
pub const MEMOIRE_MAX: i32 = 1200;

struct ApiOc {
    gpus: unsafe extern "C" fn(*mut [Poignee; 64], *mut u32) -> i32,
    nom: unsafe extern "C" fn(Poignee, *mut [u8; 64]) -> i32,
    lire_pstates: unsafe extern "C" fn(Poignee, *mut c_void) -> i32,
    ecrire_pstates: unsafe extern "C" fn(Poignee, *const c_void) -> i32,
    info_puissance: unsafe extern "C" fn(Poignee, *mut InfoPuissance) -> i32,
    lire_puissance: unsafe extern "C" fn(Poignee, *mut EtatPuissance) -> i32,
    ecrire_puissance: unsafe extern "C" fn(Poignee, *const EtatPuissance) -> i32,
}

unsafe impl Send for ApiOc {}
unsafe impl Sync for ApiOc {}

fn api_oc() -> Option<&'static ApiOc> {
    static API: OnceLock<Option<ApiOc>> = OnceLock::new();
    API.get_or_init(|| unsafe {
        api()?; // charge la DLL et initialise NVAPI
        let nom: Vec<u16> = "nvapi64.dll\0".encode_utf16().collect();
        let dll = LoadLibraryW(nom.as_ptr());
        let qi: unsafe extern "C" fn(u32) -> *const c_void = std::mem::transmute(GetProcAddress(dll, b"nvapi_QueryInterface\0".as_ptr())?);
        let f = |id: u32| {
            let p = qi(id);
            (!p.is_null()).then_some(p)
        };
        Some(ApiOc {
            gpus: std::mem::transmute(f(0xE5AC_921F)?),
            nom: std::mem::transmute(f(0xCEEE_8E9F)?),
            lire_pstates: std::mem::transmute(f(0x6FF8_1213)?),
            ecrire_pstates: std::mem::transmute(f(0x0F4D_AE6B)?),
            info_puissance: std::mem::transmute(f(0x3420_6D86)?),
            lire_puissance: std::mem::transmute(f(0x7091_6171)?),
            ecrire_puissance: std::mem::transmute(f(0xAD95_F5ED)?),
        })
    })
    .as_ref()
}

fn premier_gpu(a: &ApiOc) -> Result<Poignee, String> {
    let mut gpus = [std::ptr::null_mut(); 64];
    let mut n = 0u32;
    if unsafe { (a.gpus)(&mut gpus, &mut n) } != NVAPI_OK || n == 0 {
        return Err("Aucune carte NVIDIA trouvée".into());
    }
    Ok(gpus[0])
}

#[derive(serde::Serialize)]
pub struct Plage {
    /// MHz
    pub actuel: i32,
    pub min: i32,
    pub max: i32,
    pub modifiable: bool,
}

#[derive(serde::Serialize)]
pub struct Puissance {
    /// %
    pub actuel: f64,
    pub min: f64,
    pub defaut: f64,
    pub max: f64,
}

#[derive(serde::Serialize)]
pub struct EtatOc {
    pub gpu: String,
    pub coeur: Option<Plage>,
    pub memoire: Option<Plage>,
    pub puissance: Option<Puissance>,
}

pub fn lire_oc() -> Result<EtatOc, String> {
    let a = api_oc().ok_or("Overclocking indisponible : pas de carte NVIDIA ou pilote trop ancien")?;
    let gpu = premier_gpu(a)?;
    let mut nom = [0u8; 64];
    unsafe { (a.nom)(gpu, &mut nom) };
    let nom = String::from_utf8_lossy(&nom[..nom.iter().position(|&c| c == 0).unwrap_or(64)]).to_string();

    let mut ps: Box<Pstates20V2> = unsafe { Box::new(std::mem::zeroed()) };
    ps.v1.version = std::mem::size_of::<Pstates20V2>() as u32 | (2 << 16);
    let code = unsafe { (a.lire_pstates)(gpu, &mut *ps as *mut _ as *mut c_void) };
    if code != NVAPI_OK {
        return Err(format!("Lecture des fréquences refusée par le pilote (code {code})"));
    }
    let p0 = ps.v1.pstates[..ps.v1.nb_pstates.min(16) as usize].iter().find(|p| p.id == 0);
    let plage = |domaine: u32| {
        p0.and_then(|p| p.horloges[..ps.v1.nb_horloges.min(8) as usize].iter().find(|h| h.domaine == domaine)).map(|h| Plage {
            actuel: h.delta.valeur / 1000,
            min: h.delta.min / 1000,
            max: h.delta.max / 1000,
            modifiable: h.editable & 1 == 1,
        })
    };

    let puissance = unsafe {
        let mut info: InfoPuissance = std::mem::zeroed();
        info.version = std::mem::size_of::<InfoPuissance>() as u32 | (1 << 16);
        let mut etat: EtatPuissance = std::mem::zeroed();
        etat.version = std::mem::size_of::<EtatPuissance>() as u32 | (1 << 16);
        ((a.info_puissance)(gpu, &mut info) == NVAPI_OK && (a.lire_puissance)(gpu, &mut etat) == NVAPI_OK && info.nombre > 0 && etat.nombre > 0).then(|| {
            let e = info.entrees[0];
            // Valeurs en millièmes de pour cent
            Puissance {
                actuel: etat.entrees[0].puissance as f64 / 1000.0,
                min: e.min as f64 / 1000.0,
                defaut: e.defaut as f64 / 1000.0,
                max: e.max as f64 / 1000.0,
            }
        })
    };
    Ok(EtatOc { gpu: nom, coeur: plage(DOMAINE_GPU), memoire: plage(DOMAINE_MEMOIRE), puissance })
}

/// Applique les décalages (MHz) au P-state 0, bornés aux limites de l'app et du pilote.
pub fn appliquer_oc(coeur_mhz: i32, memoire_mhz: i32) -> Result<(), String> {
    let a = api_oc().ok_or("Overclocking indisponible")?;
    let gpu = premier_gpu(a)?;
    let etat = lire_oc()?;
    let borne = |v: i32, p: &Option<Plage>, min: i32, max: i32| p.as_ref().map(|p| v.clamp(min.max(p.min), max.min(p.max)));
    let mut horloges = Vec::new();
    if let Some(v) = borne(coeur_mhz, &etat.coeur, COEUR_MIN, COEUR_MAX) {
        horloges.push((DOMAINE_GPU, v));
    }
    if let Some(v) = borne(memoire_mhz, &etat.memoire, MEMOIRE_MIN, MEMOIRE_MAX) {
        horloges.push((DOMAINE_MEMOIRE, v));
    }
    if horloges.is_empty() {
        return Err("Ta carte ne permet pas de changer ses fréquences".into());
    }
    let mut ps: Box<Pstates20V1> = unsafe { Box::new(std::mem::zeroed()) };
    ps.version = std::mem::size_of::<Pstates20V1>() as u32 | (1 << 16);
    ps.nb_pstates = 1;
    ps.nb_horloges = horloges.len() as u32;
    ps.pstates[0].id = 0;
    for (i, (domaine, mhz)) in horloges.into_iter().enumerate() {
        ps.pstates[0].horloges[i].domaine = domaine;
        ps.pstates[0].horloges[i].delta.valeur = mhz * 1000;
    }
    match unsafe { (a.ecrire_pstates)(gpu, &*ps as *const _ as *const c_void) } {
        NVAPI_OK => Ok(()),
        code => Err(format!("Le pilote NVIDIA refuse ces fréquences (code {code})")),
    }
}

/// Limite de puissance en % (bornée à la plage de la carte).
pub fn appliquer_puissance(pourcent: f64) -> Result<(), String> {
    let a = api_oc().ok_or("Overclocking indisponible")?;
    let gpu = premier_gpu(a)?;
    let p = lire_oc()?.puissance.ok_or("Ta carte ne permet pas de changer sa limite de puissance")?;
    unsafe {
        let mut etat: EtatPuissance = std::mem::zeroed();
        etat.version = std::mem::size_of::<EtatPuissance>() as u32 | (1 << 16);
        if (a.lire_puissance)(gpu, &mut etat) != NVAPI_OK {
            return Err("Lecture de la limite de puissance impossible".into());
        }
        etat.nombre = 1;
        etat.entrees[0].puissance = (pourcent.clamp(p.min, p.max) * 1000.0).round() as u32;
        match (a.ecrire_puissance)(gpu, &etat) {
            NVAPI_OK => Ok(()),
            code => Err(format!("Le pilote NVIDIA refuse cette limite de puissance (code {code})")),
        }
    }
}

/// Fréquences et puissance d'origine.
pub fn remettre_par_defaut() -> Result<(), String> {
    let etat = lire_oc()?;
    appliquer_oc(0, 0)?;
    if let Some(p) = etat.puissance {
        appliquer_puissance(p.defaut)?;
    }
    Ok(())
}
