# FPS Cagoule

Analyse, optimisation et boost par jeu pour Windows 11. Gratuit, pour moi et mes potes.

- **Analyse du PC** : matériel, réglages Windows, problèmes détectés et score sur 100.
- **Optimisations** : réglages Windows réversibles (valeur d'origine sauvegardée, point de restauration), programmes au démarrage activables et désactivables (comme le Gestionnaire des tâches).
- **Optimiser en un clic** : « Optimisation recommandée », « Boost maximum » (36 réglages : Windows, alimentation et processeur, arrière-plan, réseau, périphériques) et « Tout annuler ». Une « Zone risquée » à part (jamais appliquée en lot, confirmation pour chaque réglage) : protections Spectre/Meltdown, isolation du noyau (VBS), hyperviseur et dynamic tick (bcdedit, avec BitLocker suspendu pour un redémarrage), exclusion des dossiers de jeux de Defender.
- **Boost par jeu** : quand un jeu se lance, plan Performances élevées, priorité haute, GPU le plus puissant, fermeture des programmes choisis. Tout revient comme avant à la fin. Mode Boost max : plan Performances optimales, minuteur 0,5 ms, mémoire en veille vidée.
- **Analyse et tests** : matériel, test de ping (box et Internet, gigue, pertes), test de fréquence de la souris.
- **Écrans** : fréquence, HDR, couleurs en jeu (luminosité, contraste, gamma, vibrance NVIDIA).
- **Nettoyage** (façon CCleaner) : fichiers temporaires, corbeille, caches des navigateurs, de Discord et de Steam, rapports d'erreurs, anciens téléchargements de Windows Update, journaux, caches de shaders (au choix), et désinstallation des programmes. Pas de nettoyage du registre.
- **Pilotes** : tous les périphériques du PC avec leur pilote et leur état, mises à jour Windows Update associées à chaque appareil, recherche web par identifiant matériel, dernier pilote NVIDIA téléchargé directement (sans NVIDIA App).
- **Compteur FPS** : FPS moyen, 1 % low et 0,1 % low de chaque partie (avec [PresentMon](https://github.com/GameTechDev/PresentMon), Intel, licence MIT), comparaison avec et sans boost.

## Léger en arrière-plan

L'app est écrite avec [Tauri](https://tauri.app) (Rust) : en arrière-plan, il ne reste qu'un petit programme natif qui regarde la liste des processus toutes les 4 secondes. L'interface (WebView2) n'existe que quand la fenêtre est ouverte. PresentMon ne tourne que pendant les parties. L'app affiche sa propre consommation en bas du menu.

## Installer

Télécharge `FPS Cagoule_x.y.z_x64-setup.exe` dans la [dernière version](https://github.com/lucas-chvnn-info/fps-cagoule/releases/latest). Windows SmartScreen prévient que l'éditeur est inconnu (l'app n'est pas signée) : « Informations complémentaires » → « Exécuter quand même ». Les mises à jour s'installent ensuite toutes seules.

## Développement

- `npm test` : tests de l'évaluation (Node).
- `cargo test --manifest-path src-tauri/Cargo.toml` : tests Rust (sous Windows).
- Publier une version : changer la version dans `package.json`, `src-tauri/Cargo.toml` et `src-tauri/tauri.conf.json`, puis pousser un tag `vX.Y.Z`. GitHub Actions compile l'installateur sur Windows et publie la version, que les apps installées récupèrent toutes seules.
