#!/usr/bin/env bash
# Installe (ou met à jour) r6-api dans le CT 201 : stats Ubisoft pour FPS Cagoule sur https://r6.lucas-chvnn.ch
# À lancer sur le Proxmox, en root :  bash installer.sh
# Avant : CNAME « r6 » → home.lucas-chvnn.ch chez Infomaniak, et un compte Ubisoft SANS double authentification.
# Le mot de passe est demandé sans s'afficher, passe au CT par l'entrée standard (jamais en argument) et reste
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

if ! pct exec $CT -- test -s $DOSSIER/.env.server || [ "${1:-}" = "--compte" ]; then
  echo "== Compte Ubisoft (sans double authentification)"
  read -rp "E-mail : " EMAIL
  read -rsp "Mot de passe (ne s'affiche pas) : " MDP; echo
  echappe() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }
  printf 'UBI_EMAIL="%s"\nUBI_MOT_DE_PASSE="%s"\nPORT=%s\n' "$(echappe "$EMAIL")" "$(echappe "$MDP")" $PORT \
    | pct exec $CT -- bash -c "umask 077; cat > $DOSSIER/.env.server"
  unset MDP
fi
pct exec $CT -- chown -R apps:apps $DOSSIER
pct exec $CT -- chmod 600 $DOSSIER/.env.server

echo "== Service app-r6"
pct exec $CT -- bash -c "cat > /etc/systemd/system/app-r6.service" <<UNIT
[Unit]
Description=Stats Ubisoft pour FPS Cagoule ($DOMAINE)
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
echo "Connexion à Ubisoft (recherche de « la_cagouuuuule ») :"
pct exec $CT -- curl -sS "http://127.0.0.1:$PORT/v1/chercher?pseudo=la_cagouuuuule"; echo
echo "Depuis Internet (il faut le CNAME et le certificat, quelques minutes la première fois) :"
curl -fsS "https://$DOMAINE/sante" && echo || echo "pas encore joignable : vérifie le CNAME r6 → home.lucas-chvnn.ch"
