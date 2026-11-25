# r6-api

Stats R6 (rang, pic, niveau, KD, victoires, saisons) pour FPS Cagoule via l'API d'Arenyze, avec une clé gardée
sur le serveur : les utilisateurs de l'app n'ont rien à configurer.

- `serveur.js` : service Node 20 sans dépendance (127.0.0.1:3103), cache 6 h (quota gratuit Arenyze : 40/min, 200/h, 400/jour, 2 000/mois), 60 demandes/min par adresse,
  saisons de tous les joueurs vus gardées dans `.data/saisons.json` (Ubisoft ne donne que la saison en cours).
- `installer.sh` : installation ou mise à jour dans le CT 201 (à lancer sur le Proxmox, en root).
  `bash installer.sh --cle` pour changer de clé.

Avant : une clé d'API créée sur r6.arenyze.com, et un CNAME `r6` → `home.lucas-chvnn.ch` chez Infomaniak.
