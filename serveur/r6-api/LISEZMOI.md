# r6-api

Stats Ubisoft (rang, pic, niveau, KD, victoires, saisons) pour FPS Cagoule, avec un seul compte Ubisoft gardé
sur le serveur : les utilisateurs de l'app n'ont rien à configurer.

- `serveur.js` : service Node 20 sans dépendance (127.0.0.1:3103), cache 20 min, 60 demandes/min par adresse,
  saisons de tous les joueurs vus gardées dans `.data/saisons.json` (Ubisoft ne donne que la saison en cours).
- `installer.sh` : installation ou mise à jour dans le CT 201 (à lancer sur le Proxmox, en root).
  `bash installer.sh --compte` pour changer de compte Ubisoft.

Avant : un compte Ubisoft **sans double authentification**, et un CNAME `r6` → `home.lucas-chvnn.ch` chez Infomaniak.
