#!/usr/bin/env bash
# Installe (ou met à jour) r6-api dans le CT 201 : stats R6 pour FPS Cagoule sur https://r6.lucas-chvnn.ch,
# via l'API d'Arenyze. À lancer sur le Proxmox, en root :  bash installer.sh   (--cle pour changer de clé)
# Avant : CNAME « r6 » → home.lucas-chvnn.ch chez Infomaniak, et une clé d'API créée sur r6.arenyze.com.
# La clé est demandée sans s'afficher, passe au CT par l'entrée standard (jamais en argument) et reste
# dans /srv/apps/r6-api/.env.server, lisible seulement par le service.
set -euo pipefail
CT=201
DOSSIER=/srv/apps/r6-api
DOMAINE=r6.lucas-chvnn.ch
PORT=3103
ICI=$(cd "$(dirname "$0")" && pwd)

pct status $CT | grep -q running || { echo "Le CT $CT ne tourne pas"; exit 1; }

echo "== Fichiers"
pct exec $CT -- mkdir -p $DOSSIER/.data
pct push $CT "$ICI/serveur.js" $DOSSIER/serveur.js

if ! pct exec $CT -- grep -q '^ARENYZE_CLE=' $DOSSIER/.env.server 2>/dev/null || [ "${1:-}" = "--cle" ]; then
  echo "== Clé d'API Arenyze (r6.arenyze.com → API)"
  read -rsp "Clé (ne s'affiche pas) : " CLE; echo
  # Remplace tout le fichier : les anciens identifiants Ubisoft, inutiles maintenant, disparaissent
  printf 'ARENYZE_CLE="%s"\nPORT=%s\n' "$(printf '%s' "$CLE" | sed 's/\\/\\\\/g; s/"/\\"/g')" $PORT \
    | pct exec $CT -- bash -c "umask 077; cat > $DOSSIER/.env.server"
  unset CLE
fi
pct exec $CT -- chown -R apps:apps $DOSSIER
pct exec $CT -- chmod 600 $DOSSIER/.env.server

echo "== Service app-r6"
pct exec $CT -- bash -c "cat > /etc/systemd/system/app-r6.service" <<UNIT
[Unit]
Description=Stats R6 pour FPS Cagoule ($DOMAINE)
After=network-online.target
Wants=network-online.target

[Service]
User=apps
WorkingDirectory=$DOSSIER
EnvironmentFile=$DOSSIER/.env.server
ExecStart=/usr/bin/node serveur.js
Restart=on-failure
NoNewPrivileges=yes
ProtectSystem=strict
ReadWritePaths=$DOSSIER/.data
PrivateTmp=yes

[Install]
WantedBy=multi-user.target
UNIT
pct exec $CT -- systemctl daemon-reload
pct exec $CT -- systemctl enable --now app-r6
pct exec $CT -- systemctl restart app-r6

echo "== Caddy"
if ! pct exec $CT -- grep -q "^$DOMAINE {" /etc/caddy/sites-apps.caddy; then
  pct exec $CT -- cp /etc/caddy/sites-apps.caddy /etc/caddy/sites-apps.caddy.avant-r6
  pct exec $CT -- bash -c "cat >> /etc/caddy/sites-apps.caddy" <<CADDY

$DOMAINE {
	import appli
	reverse_proxy localhost:$PORT
}
CADDY
fi
pct exec $CT -- caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile >/dev/null
# « caddy validate » lancé en root crée des journaux à root : on les rend à Caddy avant de recharger
pct exec $CT -- bash -c "chown -R caddy:caddy /var/log/caddy"
pct exec $CT -- systemctl reload caddy

echo "== Vérifications"
sleep 2
pct exec $CT -- curl -fsS http://127.0.0.1:$PORT/sante && echo
echo "Arenyze (recherche de « la_cagouuuuule ») :"
pct exec $CT -- curl -sS "http://127.0.0.1:$PORT/v1/chercher?pseudo=la_cagouuuuule"; echo
echo "Depuis Internet (il faut le CNAME et le certificat, quelques minutes la première fois) :"
curl -fsS "https://$DOMAINE/sante" && echo || echo "pas encore joignable : vérifie le CNAME r6 → home.lucas-chvnn.ch"
