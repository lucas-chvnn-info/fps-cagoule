// Mesure des FPS avec PresentMon (Intel, licence MIT), lancé uniquement pendant les parties.

use crate::stats::{Accumulateur, LecteurCsv};
use crate::systeme::CREATE_NO_WINDOW;
use std::io::{BufRead, BufReader};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub struct Capture {
    enfant: Child,
    fil: Option<JoinHandle<Accumulateur>>,
}

/// `rappel` reçoit le FPS instantané une fois par seconde.
pub fn demarrer(presentmon: &Path, exe: &str, rappel: impl Fn(f64) + Send + 'static) -> std::io::Result<Capture> {
    let mut enfant = Command::new(presentmon)
        .args([
            "--process_name", exe,
            "--output_stdout",
            "--no_console_stats",
            "--v1_metrics",
            // On ne veut que le temps entre deux images : le reste du suivi coûte des ressources pour rien
            "--no_track_display", "--no_track_gpu", "--no_track_input",
            "--session_name", "FPSCagoule",
            "--stop_existing_session",
            "--terminate_on_proc_exit",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    let sortie = enfant.stdout.take().expect("stdout de PresentMon");
    let fil = thread::Builder::new().name("presentmon".into()).spawn(move || {
        let mut acc = Accumulateur::default();
        let mut lecteur: Option<LecteurCsv> = None;
        let mut dernier = Instant::now();
        for ligne in BufReader::new(sortie).lines() {
            let Ok(ligne) = ligne else { break };
            match &lecteur {
                None => lecteur = LecteurCsv::depuis_entete(&ligne),
                Some(l) => {
                    if let Some((_, swap, temps)) = l.lire(&ligne) {
                        acc.ajouter(swap, temps);
                    }
                }
            }
            if dernier.elapsed() >= Duration::from_secs(1) {
                dernier = Instant::now();
                if let Some(fps) = acc.fps_instantane() {
                    rappel(fps);
                }
            }
        }
        acc
    })?;
    Ok(Capture { enfant, fil: Some(fil) })
}

impl Capture {
    /// Attend que PresentMon se termine (il le fait seul à la fermeture du jeu), sinon l'arrête.
    pub fn terminer(mut self) -> Accumulateur {
        let limite = Instant::now() + Duration::from_secs(5);
        while Instant::now() < limite {
            if matches!(self.enfant.try_wait(), Ok(Some(_))) {
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
        let _ = self.enfant.kill();
        let _ = self.enfant.wait();
        self.fil.take().and_then(|f| f.join().ok()).unwrap_or_default()
    }
}

