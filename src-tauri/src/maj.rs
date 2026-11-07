// Mises à jour automatiques depuis les versions publiées sur GitHub.
// Vérification 2 min après le démarrage puis toutes les 6 h ; jamais d'installation en pleine partie.

use crate::jeux::Etat;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;

#[derive(Serialize)]
pub struct InfoMaj {
    pub actuelle: String,
    pub disponible: Option<String>,
    pub notes: Option<String>,
}

/// Erreurs du module de mise à jour, en clair.
fn message(e: tauri_plugin_updater::Error) -> String {
    match e {
        tauri_plugin_updater::Error::ReleaseNotFound => {
            "Impossible de lire les versions en ligne : le dépôt GitHub de FPS Cagoule est privé, ou ton PC n'est pas connecté à Internet.".into()
        }
        tauri_plugin_updater::Error::Reqwest(_) => "Pas de connexion à GitHub : vérifie ta connexion Internet.".into(),
        autre => format!("Recherche de mise à jour impossible : {autre}"),
    }
}

pub async fn verifier(app: &AppHandle) -> Result<InfoMaj, String> {
    let maj = app.updater().map_err(|e| e.to_string())?.check().await.map_err(message)?;
    Ok(InfoMaj {
        actuelle: app.package_info().version.to_string(),
        disponible: maj.as_ref().map(|m| m.version.clone()),
        notes: maj.and_then(|m| m.body),
    })
}

pub async fn installer(app: &AppHandle) -> Result<bool, String> {
    let Some(maj) = app.updater().map_err(|e| e.to_string())?.check().await.map_err(message)? else {
        return Ok(false);
    };
    // Remet le plan d'alimentation d'origine avant que l'installateur ferme l'app
    crate::jeux::terminer(app, &app.state::<Arc<Etat>>());
    maj.download_and_install(|_, _| {}, || {}).await.map_err(message)?;
    app.restart();
}

pub fn lancer(app: AppHandle) {
    std::thread::Builder::new()
        .name("mises-a-jour".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(120));
            loop {
                let (auto, en_jeu) = {
                    let etat = app.state::<Arc<Etat>>();
                    let auto = etat.stock.donnees.lock().unwrap().reglages.maj_auto;
                    let en_jeu = etat.session.lock().unwrap().is_some();
                    (auto, en_jeu)
                };
                if auto && !en_jeu {
                    let _ = tauri::async_runtime::block_on(installer(&app));
                }
                std::thread::sleep(Duration::from_secs(6 * 3600));
            }
        })
        .expect("fil des mises à jour");
}
