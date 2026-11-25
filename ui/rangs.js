// Logos des rangs de R6, dessinés ici (pas les images d'Ubisoft) : écusson du classement actuel (saison 28 et plus,
// Ranked 2.0 puis 3.0) ou hexagone de l'ancien système, couleur du palier, chiffre de la division (étoile pour I).
const PALIERS = [
  ['Cuivre', '#e0584a', '#8e2a22'], ['Bronze', '#d79a62', '#7a4a26'], ['Argent', '#c9d0d8', '#6b737e'], ['Or', '#f2cf52', '#9a7414'],
  ['Platine', '#5fd8cf', '#1f7d78'], ['Émeraude', '#5fe08a', '#1d8a45'], ['Diamant', '#b48cff', '#5b34a6'], ['Champion', '#ff5c8f', '#9c1f4d'],
];
const DIVISIONS = ['I', 'II', 'III', 'IV', 'V'];

// Nom d'un rang (identifiant 0 à 40)
function nomRang(id) {
  if (!id) return 'Non classé';
  const p = PALIERS[Math.min(7, Math.floor((id - 1) / 5))];
  return `${p[0]} ${5 - ((id - 1) % 5)}`;
}

// Logo SVG d'un rang ; `saison` < 28 : style de l'ancien classement
function logoRang(id, saison = 0, taille = 22) {
  const ancien = saison > 0 && saison < 28;
  if (!id) {
    return `<svg class="logo-rang" width="${taille}" height="${taille * 1.2}" viewBox="0 0 40 48" aria-hidden="true"><path d="M5 4h30v28L20 45 5 32z" fill="none" stroke="#5b6170" stroke-width="2.5"/><path d="M14 24h12" stroke="#5b6170" stroke-width="3" stroke-linecap="round"/></svg>`;
  }
  const t = Math.min(7, Math.floor((id - 1) / 5));
  const div = 4 - ((id - 1) % 5); // 0 = I … 4 = V
  const [, clair, fonce] = PALIERS[t];
  const g = `rg${t}${ancien ? 'a' : ''}`;
  const forme = ancien ? 'M20 2 37 12v24L20 46 3 36V12z' : 'M5 4h11l4 4 4-4h11v28L20 45 5 32z';
  const couronne = t === 7 && !ancien ? '<path d="M11 6 14 1l3 4 3-4 3 4 3-4 3 5" fill="none" stroke="#ffd1e0" stroke-width="1.6" stroke-linejoin="round"/>' : '';
  const marque = div === 0
    ? '<path d="M20 13.5l2.6 5.4 5.9.8-4.3 4.1 1 5.8-5.2-2.8-5.2 2.8 1-5.8-4.3-4.1 5.9-.8z" fill="#fff"/>'
    : `<text x="20" y="${ancien ? 29 : 28}" text-anchor="middle" font-family="Segoe UI, system-ui, sans-serif" font-weight="800" font-size="${div >= 3 ? 12 : 14}" fill="#fff">${DIVISIONS[div]}</text>`;
  return `<svg class="logo-rang" width="${taille}" height="${taille * 1.2}" viewBox="0 0 40 48" aria-hidden="true">
    <defs><linearGradient id="${g}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${clair}"/><stop offset="1" stop-color="${fonce}"/></linearGradient></defs>
    ${couronne}<path d="${forme}" fill="url(#${g})" stroke="${clair}" stroke-width="2" stroke-linejoin="round"/>
    <path d="${ancien ? 'M20 7 32 14v20L20 41 8 34V14z' : 'M9 8h6l5 4 5-4h6v22L20 40 9 30z'}" fill="none" stroke="rgba(255,255,255,.28)" stroke-width="1.2"/>${marque}</svg>`;
}
