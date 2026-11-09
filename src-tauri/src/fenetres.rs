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
