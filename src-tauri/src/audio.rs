// Sons des clips, chacun envoyé en continu à FFmpeg sur sa propre piste :
// - le son du PC (ce que tu entends), en laissant de côté Discord si besoin ;
// - Discord seul (voix des potes) : capture d'un programme et de ses enfants (Windows 10 21H2 / 11) ;
// - le micro.
// Les silences sont comblés pour que le son reste calé sur l'image.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use windows::core::{implement, Interface, IUnknown, Ref, HRESULT};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Media::Audio::{
    eCapture, eConsole, eRender, ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
    IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK, AUDIOCLIENT_ACTIVATION_PARAMS,
    AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
    PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX,
    WAVEFORMATEXTENSIBLE,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

/// Format du son envoyé à FFmpeg
#[derive(Clone, Copy, Debug)]
pub struct Format {
    pub frequence: u32,
    pub canaux: u16,
    /// Échantillons flottants 32 bits (sinon entiers 16 bits)
    pub flottant: bool,
}

impl Format {
    /// Nom du format brut pour FFmpeg (-f)
    pub fn ffmpeg(&self) -> &'static str {
        if self.flottant {
            "f32le"
        } else {
            "s16le"
        }
    }

    fn octets_par_image(&self) -> usize {
        self.canaux as usize * if self.flottant { 4 } else { 2 }
    }
}

/// Lit le format de mixage : float 32 bits presque toujours, sinon PCM 16 bits
unsafe fn lire_format(f: *const WAVEFORMATEX) -> Result<Format, String> {
    let w = &*f;
    const FLOTTANT: u16 = 3;
    const PCM: u16 = 1;
    const EXTENSIBLE: u16 = 0xFFFE;
    let mut etiquette = w.wFormatTag;
    if etiquette == EXTENSIBLE {
        let ext = &*(f as *const WAVEFORMATEXTENSIBLE);
        // Les deux sous-formats partagent la fin du GUID : seul le premier mot change (1 = PCM, 3 = float)
        etiquette = ext.SubFormat.data1 as u16;
    }
    let bits = w.wBitsPerSample;
    let flottant = match (etiquette, bits) {
        (FLOTTANT, 32) => true,
        (PCM, 16) => false,
        _ => return Err(format!("Format audio non pris en charge ({etiquette}, {bits} bits)")),
    };
    Ok(Format { frequence: w.nSamplesPerSec, canaux: w.nChannels, flottant })
}

/// D'où vient le son d'une piste
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)] // Programme : capture de Discord à part, suspendue (voir clips::sons_choisis)
pub enum Source {
    /// Tout le son du PC, sauf (si donné) un programme et ses enfants : Discord, enregistré à part ou pas du tout
    Pc { sauf: Option<u32> },
    /// Un seul programme et ses enfants (Discord)
    Programme(u32),
    /// Micro par défaut de Windows
    Micro,
    /// Rien (Discord fermé, micro absent…) : la piste existe quand même, muette, pour garder les mêmes pistes
    Silence,
}

/// Format demandé pour le son d'un seul programme (Windows ne donne pas de format « naturel » dans ce cas)
const FORMAT_PROGRAMME: Format = Format { frequence: 48000, canaux: 2, flottant: false };

pub struct Capture {
    arret: Arc<AtomicBool>,
    /// Tuyau nommé encore en attente de FFmpeg : on s'y branche nous-mêmes pour débloquer le fil
    tuyau: Option<String>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.arret.store(true, Ordering::Relaxed);
        if let Some(nom) = &self.tuyau {
            let _ = std::fs::OpenOptions::new().read(true).open(nom);
        }
    }
}

/// Fin de l'ouverture asynchrone du son d'un programme
#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Ouverture(std::sync::Mutex<mpsc::Sender<()>>);

impl IActivateAudioInterfaceCompletionHandler_Impl for Ouverture_Impl {
    fn ActivateCompleted(&self, _: Ref<'_, IActivateAudioInterfaceAsyncOperation>) -> windows::core::Result<()> {
        let _ = self.0.lock().unwrap().send(());
        Ok(())
    }
}

/// Valeur « blob » (VT_BLOB) attendue par Windows pour les réglages d'ouverture, disposée comme un PROPVARIANT
#[repr(C)]
struct Blob {
    vt: u16,
    reserve: [u16; 3],
    taille: u32,
    _bourrage: u32,
    donnees: *mut u8,
}

/// Son d'un programme et de ses enfants (`inclure`), ou de tout le PC sauf eux
unsafe fn client_programme(pid: u32, inclure: bool) -> windows::core::Result<IAudioClient> {
    let mut reglage = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: pid,
                ProcessLoopbackMode: if inclure { PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE } else { PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE },
            },
        },
    };
    let blob = Blob {
        vt: 65, // VT_BLOB
        reserve: [0; 3],
        taille: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
        _bourrage: 0,
        donnees: &mut reglage as *mut _ as *mut u8,
    };
    let (envoi, reception) = mpsc::channel();
    let fini: IActivateAudioInterfaceCompletionHandler = Ouverture(std::sync::Mutex::new(envoi)).into();
    let operation = ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, &IAudioClient::IID, Some(&blob as *const Blob as *const PROPVARIANT), &fini)?;
    if reception.recv_timeout(Duration::from_secs(5)).is_err() {
        return Err(windows::core::Error::from_hresult(HRESULT(0x8000_4005_u32 as i32))); // E_FAIL
    }
    let mut resultat = HRESULT(0);
    let mut client: Option<IUnknown> = None;
    operation.GetActivateResult(&mut resultat, &mut client)?;
    resultat.ok()?;
    client.ok_or_else(|| windows::core::Error::from_hresult(HRESULT(0x8000_4005_u32 as i32)))?.cast()
}

/// Prépare la capture d'une source : client audio, lecteur, format, et l'événement de Windows à attendre (s'il y en a un)
unsafe fn preparer(source: Source) -> Result<(IAudioClient, IAudioCaptureClient, Format, Option<HANDLE>), String> {
    CoInitializeEx(None, COINIT_MULTITHREADED).ok().map_err(|e| e.to_string())?;
    // Tampon de 200 ms (en unités de 100 ns)
    const TAMPON: i64 = 2_000_000;
    let par_appareil = |flux, drapeaux| -> Result<(IAudioClient, Format), String> {
        let enumerateur: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        let appareil = enumerateur.GetDefaultAudioEndpoint(flux, eConsole).map_err(|e| e.to_string())?;
        let client: IAudioClient = appareil.Activate(CLSCTX_ALL, None).map_err(|e| e.to_string())?;
        let f = client.GetMixFormat().map_err(|e| e.to_string())?;
        let format = lire_format(f);
        let init = client.Initialize(AUDCLNT_SHAREMODE_SHARED, drapeaux, TAMPON, 0, f, None);
        CoTaskMemFree(Some(f as *const _));
        init.map_err(|e| e.to_string())?;
        Ok((client, format?))
    };
    let par_programme = |pid, inclure| -> Result<(IAudioClient, Format, HANDLE), String> {
        let client = client_programme(pid, inclure).map_err(|e| format!("son par programme refusé par Windows ({e})"))?;
        let f = FORMAT_PROGRAMME;
        let wf = WAVEFORMATEX {
            wFormatTag: 1, // PCM
            nChannels: f.canaux,
            nSamplesPerSec: f.frequence,
            nAvgBytesPerSec: f.frequence * f.octets_par_image() as u32,
            nBlockAlign: f.octets_par_image() as u16,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        client
            .Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK, TAMPON, 0, &wf, None)
            .map_err(|e| e.to_string())?;
        let evenement = CreateEventW(None, false, false, None).map_err(|e| e.to_string())?;
        client.SetEventHandle(evenement).map_err(|e| e.to_string())?;
        Ok((client, f, evenement))
    };
    let (client, format, evenement) = match source {
        Source::Pc { sauf: None } => par_appareil(eRender, AUDCLNT_STREAMFLAGS_LOOPBACK).map(|(c, f)| (c, f, None))?,
        Source::Pc { sauf: Some(pid) } => par_programme(pid, false).map(|(c, f, e)| (c, f, Some(e)))?,
        Source::Programme(pid) => par_programme(pid, true).map(|(c, f, e)| (c, f, Some(e)))?,
        Source::Micro => par_appareil(eCapture, 0).map(|(c, f)| (c, f, None))?,
        Source::Silence => return Err("silence".into()),
    };
    let capture: IAudioCaptureClient = client.GetService().map_err(|e| e.to_string())?;
    Ok((client, capture, format, evenement))
}

/// Ouvre une source de son. Renvoie son format et une fonction qui démarre l'envoi vers `sortie`
/// (`tuyau` : nom du tuyau nommé de `sortie`, s'il en est un). Tout se passe dans un fil dédié (les objets COM y restent).
/// Une source qui ne s'ouvre pas renvoie une erreur ; `Source::Silence` s'ouvre toujours.
/// `nom` et `journal` : le débit réel de la source est noté dans le journal des clips (toutes les 10 min et à la fin),
/// pour repérer un son qui arrive plus vite ou moins vite que le temps réel.
pub fn ouvrir(source: Source, nom: &'static str, journal: Option<std::path::PathBuf>) -> Result<(Format, impl FnOnce(Box<dyn Write + Send>, Option<String>) -> Capture), String> {
    let (envoi_format, reception_format) = mpsc::channel::<Result<Format, String>>();
    let (envoi_sortie, reception_sortie) = mpsc::channel::<Box<dyn Write + Send>>();
    let arret = Arc::new(AtomicBool::new(false));
    let arret_fil = arret.clone();
    thread::Builder::new()
        .name("audio-clips".into())
        .spawn(move || unsafe {
            let prepare = if source == Source::Silence { None } else {
                match preparer(source) {
                    Ok(p) => Some(p),
                    Err(e) => return drop(envoi_format.send(Err(e))),
                }
            };
            let format = prepare.as_ref().map_or(FORMAT_PROGRAMME, |p| p.2);
            let _ = envoi_format.send(Ok(format));
            let Ok(mut sortie) = reception_sortie.recv() else { return };
            if let Some((client, _, _, _)) = &prepare {
                if client.Start().is_err() {
                    return;
                }
            }
            let par_image = format.octets_par_image();
            let debut = Instant::now();
            let mut ecrites: u64 = 0;
            let mut comblees: u64 = 0;
            let mut ferme = false;
            let mut zeros = Vec::new();
            let mut prochain_bilan = Duration::from_secs(60);
            let bilan = |ecrites: u64, comblees: u64| {
                if let Some(j) = &journal {
                    let ecoule = debut.elapsed().as_secs_f64();
                    let son = ecrites as f64 / format.frequence as f64;
                    crate::clips::noter_dans(j, &format!(
                        "Son « {nom} » : {son:.0} s envoyées en {ecoule:.0} s ({:.3} × le temps réel, dont {:.0} s de silence ajouté)",
                        son / ecoule.max(0.001),
                        comblees as f64 / format.frequence as f64
                    ));
                }
            };
            while !arret_fil.load(Ordering::Relaxed) {
                if debut.elapsed() >= prochain_bilan {
                    bilan(ecrites, comblees);
                    prochain_bilan += Duration::from_secs(600);
                }
                let mut recu = false;
                if let Some((_, capture, _, _)) = &prepare {
                    while let Ok(n) = capture.GetNextPacketSize() {
                        if n == 0 {
                            break;
                        }
                        let mut donnees = std::ptr::null_mut();
                        let (mut images, mut drapeaux) = (0u32, 0u32);
                        if capture.GetBuffer(&mut donnees, &mut images, &mut drapeaux, None, None).is_err() {
                            break;
                        }
                        let taille = images as usize * par_image;
                        let ok = if drapeaux & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || donnees.is_null() {
                            zeros.resize(taille, 0);
                            sortie.write_all(&zeros).is_ok()
                        } else {
                            sortie.write_all(std::slice::from_raw_parts(donnees, taille)).is_ok()
                        };
                        let _ = capture.ReleaseBuffer(images);
                        if !ok {
                            ferme = true; // FFmpeg arrêté
                            break;
                        }
                        ecrites += images as u64;
                        recu = true;
                    }
                }
                if ferme {
                    break;
                }
                // Rien ne joue : Windows n'envoie rien, on comble avec du silence pour garder le rythme
                let attendues = (debut.elapsed().as_secs_f64() * format.frequence as f64) as u64;
                if !recu && attendues > ecrites + format.frequence as u64 / 20 {
                    let manque = (attendues - ecrites) as usize;
                    zeros.resize(manque * par_image, 0);
                    if sortie.write_all(&zeros).is_err() {
                        break;
                    }
                    ecrites += manque as u64;
                    comblees += manque as u64;
                }
                match prepare.as_ref().and_then(|p| p.3) {
                    Some(evenement) => {
                        WaitForSingleObject(evenement, 10);
                    }
                    None => thread::sleep(Duration::from_millis(10)),
                }
            }
            bilan(ecrites, comblees);
            if let Some((client, _, _, evenement)) = &prepare {
                let _ = client.Stop();
                if let Some(e) = evenement {
                    let _ = CloseHandle(*e);
                }
            }
        })
        .map_err(|e| e.to_string())?;
    let format = reception_format.recv_timeout(Duration::from_secs(8)).map_err(|_| "Son du PC : pas de réponse".to_string())??;
    Ok((format, move |sortie: Box<dyn Write + Send>, tuyau: Option<String>| {
        let _ = envoi_sortie.send(sortie);
        Capture { arret, tuyau }
    }))
}

/// Discord ouvert : son processus de départ (ses fenêtres et sa voix sont dans des processus enfants)
pub fn pid_discord() -> Option<u32> {
    let liste = crate::systeme::processus();
    let est_discord = |nom: &str| {
        let n = nom.to_ascii_lowercase();
        n == "discord.exe" || n == "discordptb.exe" || n == "discordcanary.exe"
    };
    liste
        .iter()
        .filter(|p| est_discord(&p.nom))
        .find(|p| !liste.iter().any(|q| q.pid == p.parent && est_discord(&q.nom)))
        .map(|p| p.pid)
}

/// Tuyau nommé de Windows vers FFmpeg (une entrée de plus que stdin). Il attend que FFmpeg l'ouvre à la première écriture.
pub struct Tuyau {
    poignee: windows_sys::Win32::Foundation::HANDLE,
    branche: bool,
}

// La poignée n'est utilisée que par le fil du son
unsafe impl Send for Tuyau {}

impl Tuyau {
    pub fn creer(nom: &str) -> std::io::Result<Self> {
        use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_OUTBOUND;
        use windows_sys::Win32::System::Pipes::{CreateNamedPipeW, PIPE_TYPE_BYTE, PIPE_WAIT};
        let large: Vec<u16> = nom.encode_utf16().chain(Some(0)).collect();
        let poignee = unsafe { CreateNamedPipeW(large.as_ptr(), PIPE_ACCESS_OUTBOUND, PIPE_TYPE_BYTE | PIPE_WAIT, 1, 1 << 22, 0, 0, std::ptr::null()) };
        if poignee == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self { poignee, branche: false })
    }
}

impl Write for Tuyau {
    fn write(&mut self, octets: &[u8]) -> std::io::Result<usize> {
        use windows_sys::Win32::Foundation::{GetLastError, ERROR_PIPE_CONNECTED};
        use windows_sys::Win32::Storage::FileSystem::WriteFile;
        use windows_sys::Win32::System::Pipes::ConnectNamedPipe;
        unsafe {
            if !self.branche {
                if ConnectNamedPipe(self.poignee, std::ptr::null_mut()) == 0 && GetLastError() != ERROR_PIPE_CONNECTED {
                    return Err(std::io::Error::last_os_error());
                }
                self.branche = true;
            }
            let mut ecrits = 0u32;
            if WriteFile(self.poignee, octets.as_ptr(), octets.len().min(u32::MAX as usize) as u32, &mut ecrits, std::ptr::null_mut()) == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(ecrits as usize)
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for Tuyau {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.poignee);
        }
    }
}
