// Logos des rangs de R6 selon la saison :
//   rangs/r3     Ranked 3.0 (Y11S2 = saison 42, juin 2026, et après) : 40 rangs, Champion en 5 divisions
//   rangs/r2     Ranked 2.0 (saisons 28 à 41) : 36 = Champion          } logos officiels d'Ubisoft,
//   rangs/ancien ancien classement (avant la saison 28) : 25 = Champion } via dragon6-assets (CC BY 4.0)
const PALIERS = ['Cuivre', 'Bronze', 'Argent', 'Or', 'Platine', 'Émeraude', 'Diamant', 'Champion'];
const SAISON_RANKED_3 = 42; // Y9S3 = 35 → Y11S2 = 42

function serieRang(id, saison) {
  if (id > 36 || !saison || saison >= SAISON_RANKED_3) return 'r3';
  return saison >= 28 ? 'r2' : 'ancien';
}

// Nom d'un rang (identifiant 0 à 40)
function nomRang(id, saison = 0) {
  if (!id) return 'Non classé';
  if (id === 36 && serieRang(id, saison) === 'r2') return 'Champion';
  return `${PALIERS[Math.min(7, Math.floor((id - 1) / 5))]} ${5 - ((id - 1) % 5)}`;
}

// Logo d'un rang ; `taille` = largeur visée en pixels (la hauteur suit)
function logoRang(id, saison = 0, taille = 22) {
  const serie = id ? serieRang(id, saison) : 'r2';
  const fichier = serie === 'ancien' ? Math.min(id, 25) : serie === 'r2' ? Math.min(id, 36) : id;
  return `<img class="logo-rang" src="rangs/${serie}/${fichier}.webp" alt="${nomRang(id, saison)}" title="${nomRang(id, saison)}" style="height:${Math.round(taille * 1.2)}px;width:auto">`;
}
