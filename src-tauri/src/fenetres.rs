// Fenêtres et icône de la zone de notification.
// L'interface n'existe que tant qu'elle est ouverte : la fermer libère toute sa mémoire (WebView2).

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::PageLoadEvent;
use tauri::window::Color;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const ICONE: &str = "principale";

pub fn ouvrir_principale(app: &AppHandle) {
    if let Some(f) = app.get_webview_window("main") {
        let _ = f.unminimize();
        let _ = f.show();
        let _ = f.set_focus();
        return;
    }
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        // Cachée et sur fond sombre jusqu'au chargement de la page : pas d'éclair blanc avant l'écran de démarrage
        let _ = WebviewWindowBuilder::new(&app2, "main", WebviewUrl::App("index.html".into()))
            .title("FPS Cagoule")
            .inner_size(1120.0, 740.0)
            .min_inner_size(920.0, 620.0)
            .center()
            .visible(false)
            .background_color(Color(11, 12, 16, 255))
            .on_page_load(|f, p| {
                if p.event() == PageLoadEvent::Finished {
                    let _ = f.show();
                    let _ = f.set_focus();
                }
            })
            .build();
    });
}

pub fn ouvrir_overlay(app: &AppHandle) {
    if app.get_webview_window("overlay").is_some() {
        return;
    }
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        let f = WebviewWindowBuilder::new(&app2, "overlay", WebviewUrl::App("overlay.html".into()))
            .title("FPS Cagoule – compteur")
            .inner_size(132.0, 44.0)
            .position(12.0, 12.0)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .focused(false)
            .build();
        if let Ok(f) = f {
            let _ = f.set_ignore_cursor_events(true);
        }
    });
}

/// Change à chaque nouvelle manche : seule la dernière fenêtre ouverte se referme à son heure
static GENERATION_DEGATS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const DEGATS_TAILLE: (f64, f64) = (342.0, 470.0);
const DEGATS_DUREE: std::time::Duration = std::time::Duration::from_secs(15);

/// Dégâts de la manche, à gauche au milieu de l'écran pendant 15 s. Jamais activée (le jeu garde le clavier
/// et ne se réduit pas), les clics la traversent.
pub fn ouvrir_degats(app: &AppHandle) {
    use std::sync::atomic::Ordering;
    let generation = GENERATION_DEGATS.fetch_add(1, Ordering::SeqCst) + 1;
    // Déjà ouverte : elle se met à jour seule (signal « degats » envoyé avec le nouveau bilan)
    if app.get_webview_window("degats").is_none() {
        let app2 = app.clone();
        let _ = app.run_on_main_thread(move || {
            let (mut x, mut y) = (24.0, 160.0);
            if let Ok(Some(ecran)) = app2.primary_monitor() {
                let echelle = ecran.scale_factor();
                let p = ecran.position().to_logical::<f64>(echelle);
                let t = ecran.size().to_logical::<f64>(echelle);
                x = p.x + 24.0;
                y = p.y + ((t.height - DEGATS_TAILLE.1) / 2.0).max(0.0);
            }
            let f = WebviewWindowBuilder::new(&app2, "degats", WebviewUrl::App("degats.html".into()))
                .title("FPS Cagoule – dégâts")
                .inner_size(DEGATS_TAILLE.0, DEGATS_TAILLE.1)
                .position(x, y)
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .focused(false)
                .visible(false)
                .build();
            if let Ok(f) = f {
                let _ = f.set_ignore_cursor_events(true);
                afficher_sans_activer(&f);
            }
        });
    }
    let app3 = app.clone();
    let _ = std::thread::Builder::new().name("degats-fermeture".into()).spawn(move || {
        std::thread::sleep(DEGATS_DUREE);
        if GENERATION_DEGATS.load(Ordering::SeqCst) == generation {
            if let Some(f) = app3.get_webview_window("degats") {
                let _ = f.destroy();
            }
        }
    });
}

/// Affiche une fenêtre sans l'activer (WS_EX_NOACTIVATE + SW_SHOWNOACTIVATE)
fn afficher_sans_activer(f: &tauri::WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, SetWindowLongPtrW, ShowWindow, GWL_EXSTYLE, SW_SHOWNOACTIVATE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW};
    let Ok(h) = f.hwnd() else { return };
    let h = h.0 as windows_sys::Win32::Foundation::HWND;
    unsafe {
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        SetWindowLongPtrW(h, GWL_EXSTYLE, ex | (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW) as isize);
        ShowWindow(h, SW_SHOWNOACTIVATE);
    }
}

pub fn fermer_overlay(app: &AppHandle) {
    if let Some(f) = app.get_webview_window("overlay") {
        let _ = f.destroy();
    }
}

pub fn infobulle(app: &AppHandle, texte: &str) {
    if let Some(t) = app.tray_by_id(ICONE) {
        let _ = t.set_tooltip(Some(texte));
    }
}

pub fn creer_icone(app: &AppHandle) -> tauri::Result<()> {
    let ouvrir = MenuItem::with_id(app, "ouvrir", "Ouvrir FPS Cagoule", true, None::<&str>)?;
    let quitter = MenuItem::with_id(app, "quitter", "Quitter", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&ouvrir, &quitter])?;
    let mut icone = TrayIconBuilder::with_id(ICONE).tooltip("FPS Cagoule").menu(&menu).show_menu_on_left_click(false);
    if let Some(i) = app.default_window_icon() {
        icone = icone.icon(i.clone());
    }
    icone
        .on_menu_event(|app, e| match e.id.as_ref() {
            "ouvrir" => ouvrir_principale(app),
            "quitter" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|t, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                ouvrir_principale(t.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}
