// Touche de sauvegarde des clips : raccourci global de Windows (RegisterHotKey), reçu même en plein jeu.
// Un seul fil qui dort dans sa boucle de messages.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, MOD_NOREPEAT};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetMessageW, PostThreadMessageW, MSG, WM_APP, WM_HOTKEY};

const ID: i32 = 1;

pub struct Raccourci {
    fil: u32,
    resultats: mpsc::Receiver<bool>,
}

/// Touches qui peuvent servir seules sans gêner la frappe (F1–F24, Pause, Inser…, pavé numérique)
pub fn seule_permise(vk: u32) -> bool {
    matches!(vk, 0x70..=0x87 | 0x13 | 0x91 | 0x2D | 0x24 | 0x23 | 0x21 | 0x22 | 0x60..=0x6F)
}

impl Raccourci {
    pub fn lancer(rappel: impl Fn() + Send + 'static) -> Option<Self> {
        let (envoi_fil, reception_fil) = mpsc::channel();
        let (envoi_res, resultats) = mpsc::channel();
        thread::Builder::new()
            .name("touche-clips".into())
            .spawn(move || unsafe {
                let _ = envoi_fil.send(GetCurrentThreadId());
                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    match msg.message {
                        WM_HOTKEY if msg.wParam == ID as usize => rappel(),
                        // Changement de touche : wParam = modificateurs, lParam = touche (0 = aucune)
                        WM_APP => {
                            UnregisterHotKey(std::ptr::null_mut(), ID);
                            let vk = msg.lParam as u32;
                            let ok = vk == 0 || RegisterHotKey(std::ptr::null_mut(), ID, msg.wParam as u32 | MOD_NOREPEAT, vk) != 0;
                            let _ = envoi_res.send(ok);
                        }
                        _ => {}
                    }
                }
            })
            .ok()?;
        let fil = reception_fil.recv_timeout(Duration::from_secs(2)).ok()?;
        Some(Self { fil, resultats })
    }

    /// Enregistre la touche (vk = 0 pour la retirer). Faux si une autre appli l'utilise déjà.
    pub fn changer(&self, modificateurs: u32, vk: u32) -> bool {
        while self.resultats.try_recv().is_ok() {}
        // Le fil peut ne pas avoir encore sa file de messages : on réessaie un peu
        for _ in 0..20 {
            if unsafe { PostThreadMessageW(self.fil, WM_APP, modificateurs as usize, vk as isize) } != 0 {
                return self.resultats.recv_timeout(Duration::from_secs(2)).unwrap_or(false);
            }
            thread::sleep(Duration::from_millis(50));
        }
        false
    }
}
