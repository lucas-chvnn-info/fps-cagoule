// Son du PC pendant l'enregistrement des clips : capture « loopback » de la sortie audio (ce que tu entends),
// envoyée en continu à FFmpeg. Les silences sont comblés pour que le son reste calé sur l'image.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT,
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED};

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

pub struct Capture {
    arret: Arc<AtomicBool>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.arret.store(true, Ordering::Relaxed);
    }
}

/// Ouvre la sortie audio par défaut. Renvoie son format et une fonction qui démarre l'envoi vers `sortie`.
/// Tout se passe dans un fil dédié (les objets COM y restent).
pub fn ouvrir() -> Result<(Format, impl FnOnce(Box<dyn Write + Send>) -> Capture), String> {
    let (envoi_format, reception_format) = mpsc::channel::<Result<Format, String>>();
    let (envoi_sortie, reception_sortie) = mpsc::channel::<Box<dyn Write + Send>>();
    let arret = Arc::new(AtomicBool::new(false));
    let arret_fil = arret.clone();
    thread::Builder::new()
        .name("audio-clips".into())
        .spawn(move || unsafe {
            let preparation = (|| -> windows::core::Result<(IAudioClient, IAudioCaptureClient, Result<Format, String>)> {
                CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
                let enumerateur: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
                let appareil = enumerateur.GetDefaultAudioEndpoint(eRender, eConsole)?;
                let client: IAudioClient = appareil.Activate(CLSCTX_ALL, None)?;
                let f = client.GetMixFormat()?;
                let format = lire_format(f);
                // Tampon de 200 ms (en unités de 100 ns)
                client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 2_000_000, 0, f, None)?;
                CoTaskMemFree(Some(f as *const _));
                let capture: IAudioCaptureClient = client.GetService()?;
                Ok((client, capture, format))
            })();
            let (client, capture, format) = match preparation {
                Ok((c, cap, Ok(f))) => (c, cap, f),
                Ok((_, _, Err(e))) => return drop(envoi_format.send(Err(e))),
                Err(e) => return drop(envoi_format.send(Err(format!("Son du PC indisponible : {e}")))),
            };
            let _ = envoi_format.send(Ok(format));
            let Ok(mut sortie) = reception_sortie.recv() else { return };
            if client.Start().is_err() {
                return;
            }
            let par_image = format.octets_par_image();
            let debut = Instant::now();
            let mut ecrites: u64 = 0;
            let mut zeros = Vec::new();
            while !arret_fil.load(Ordering::Relaxed) {
                let mut recu = false;
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
                        let _ = client.Stop();
                        return; // FFmpeg arrêté
                    }
                    ecrites += images as u64;
                    recu = true;
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
                }
                thread::sleep(Duration::from_millis(10));
            }
            let _ = client.Stop();
        })
        .map_err(|e| e.to_string())?;
    let format = reception_format.recv_timeout(Duration::from_secs(5)).map_err(|_| "Son du PC : pas de réponse".to_string())??;
    Ok((format, move |sortie: Box<dyn Write + Send>| {
        let _ = envoi_sortie.send(sortie);
        Capture { arret }
    }))
}
