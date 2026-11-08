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
