// Statut Discord pendant les parties (Rich Presence) : « FPS Cagoule · Rainbow Six Siege · Boosté · 238 FPS ».
// Connexion locale à Discord (tube nommé discord-ipc-N) ouverte au début de la partie et fermée à la fin :
// rien ne tourne en dehors des parties. Si Discord n'est pas ouvert, rien ne se passe.

use serde_json::json;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

/// Identifiant de l'application Discord « FPS Cagoule » (portail développeur Discord). Vide = statut désactivé.
pub const ID_APPLI: &str = "";
const LOGO: &str = "https://raw.githubusercontent.com/lucas-chvnn-info/fps-cagoule/main/ui/logo.png";
const SITE: &str = "https://github.com/lucas-chvnn-info/fps-cagoule/releases/latest";
/// Discord accepte 5 changements de statut toutes les 20 s : on reste bien en dessous
const INTERVALLE: Duration = Duration::from_secs(15);

pub fn disponible() -> bool {
    !ID_APPLI.is_empty()
}

fn trame(op: u32, contenu: &serde_json::Value) -> Vec<u8> {
    let texte = contenu.to_string();
    let mut t = Vec::with_capacity(texte.len() + 8);
    t.extend_from_slice(&op.to_le_bytes());
    t.extend_from_slice(&(texte.len() as u32).to_le_bytes());
    t.extend_from_slice(texte.as_bytes());
    t
}

fn lire_trame(tube: &mut File) -> std::io::Result<()> {
    let mut entete = [0u8; 8];
    tube.read_exact(&mut entete)?;
    let longueur = u32::from_le_bytes(entete[4..8].try_into().unwrap()) as usize;
    let mut corps = vec![0u8; longueur.min(1 << 20)];
    tube.read_exact(&mut corps)
}

fn connecter() -> Option<File> {
    for n in 0..10 {
        if let Ok(mut tube) = OpenOptions::new().read(true).write(true).open(format!(r"\\.\pipe\discord-ipc-{n}")) {
            if tube.write_all(&trame(0, &json!({ "v": 1, "client_id": ID_APPLI }))).is_ok() && lire_trame(&mut tube).is_ok() {
                return Some(tube);
            }
        }
    }
    None
}

pub struct Presence {
    envoi: Sender<String>,
}

impl Presence {
    /// Nouvel état affiché sous le nom du jeu (ex. « Boosté · 238 FPS »), envoyé au plus toutes les 15 s
    pub fn etat(&self, texte: String) {
        let _ = self.envoi.send(texte);
    }
}

/// Démarre le statut pour la partie. `image` : jaquette du jeu (URL https), sinon le logo de l'app.
/// Le statut disparaît quand la `Presence` est lâchée (fin de partie).
pub fn demarrer(jeu: &str, etat: String, debut_unix: u64, image: Option<String>) -> Option<Presence> {
    if !disponible() {
        return None;
    }
    let (envoi, reception) = mpsc::channel::<String>();
    let jeu = jeu.to_string();
    thread::Builder::new()
        .name("discord".into())
        .spawn(move || {
            let Some(mut tube) = connecter() else { return };
            let mut actuel = etat;
            let mut envoye: Option<(Instant, String)> = None;
            let mut numero = 0u64;
            loop {
                let a_jour = envoye.as_ref().is_some_and(|(_, t)| *t == actuel);
                let attendre = envoye.as_ref().is_some_and(|(quand, _)| quand.elapsed() < INTERVALLE);
                if !a_jour && !attendre {
                    let activite = json!({
                        "details": jeu,
                        "state": actuel,
                        "timestamps": { "start": debut_unix },
                        "assets": {
                            "large_image": image.clone().unwrap_or_else(|| LOGO.into()),
                            "large_text": jeu,
                            "small_image": LOGO,
                            "small_text": "Boosté par FPS Cagoule",
                        },
                        "buttons": [{ "label": "FPS Cagoule", "url": SITE }],
                    });
                    let commande = json!({
                        "cmd": "SET_ACTIVITY",
                        "args": { "pid": std::process::id(), "activity": activite },
                        "nonce": format!("fps-cagoule-{debut_unix}-{numero}"),
                    });
                    if tube.write_all(&trame(1, &commande)).is_err() || lire_trame(&mut tube).is_err() {
                        return; // Discord fermé
                    }
                    envoye = Some((Instant::now(), actuel.clone()));
                    numero += 1;
                }
                match reception.recv_timeout(INTERVALLE) {
                    Ok(texte) => actuel = texte,
                    Err(RecvTimeoutError::Timeout) => {}
                    // Fin de partie : fermer le tube efface le statut
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        })
        .ok()?;
    Some(Presence { envoi })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_des_trames() {
        let t = trame(1, &json!({ "a": 1 }));
        assert_eq!(&t[..4], &1u32.to_le_bytes());
        assert_eq!(u32::from_le_bytes(t[4..8].try_into().unwrap()) as usize, t.len() - 8);
        assert_eq!(&t[8..], br#"{"a":1}"#);
    }
}
