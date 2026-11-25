// Fenêtre de fin de manche (R6) : 1 résumé, 2 zones touchées sur chaque ennemi, 3 dégâts donnés et reçus.
// Les pages défilent seules ; un clic sur 1, 2 ou 3 arrête le défilement et garde la fenêtre ouverte.
const { invoke } = window.__TAURI__.core;
const esc = escOp;
const DEFILEMENT = 6500;
let page = 1, defile, fermeture, manches = [], indice = 0;


// Petite silhouette : zones touchées en couleur ; à côté, balles et dégâts de chaque zone (tête, corps, bras et jambes)
function silhouette(n, d, couleur) {
  const z = (k) => (n[k] ? couleur : 'var(--vide)');
  return `<div class="zones"><svg viewBox="0 0 16 30" aria-hidden="true">
    <circle cx="8" cy="3.4" r="3" fill="${z(0)}"/>
    <rect x="4.5" y="7.5" width="7" height="10" rx="2" fill="${z(1)}"/>
    <rect x="1.2" y="8" width="2.4" height="9" rx="1.2" fill="${z(2)}"/><rect x="12.4" y="8" width="2.4" height="9" rx="1.2" fill="${z(2)}"/>
    <rect x="4.8" y="18.5" width="2.8" height="11" rx="1.4" fill="${z(2)}"/><rect x="8.4" y="18.5" width="2.8" height="11" rx="1.4" fill="${z(2)}"/>
  </svg><span>${n.map((x, i) => (x || d[i] ? `<i class="oui"><b>${x}×</b> ${d[i]}</i>` : '<i>–</i>')).join('')}</span></div>`;
}

function rendre(b) {
  const moi = b.joueurs.find((j) => j.pseudo === b.moi);
  const monEquipe = moi ? moi.equipe : 0;
  const max = Math.max(1, ...b.joueurs.map((j) => j.inflige));
  const parEquipe = (e, tri) => b.joueurs.filter((j) => j.equipe === e).sort(tri);
  const classe = (j) => (j.pseudo === b.moi ? ' toi' : '');

  // 1 · Résumé
  const cibles = {};
  for (const c of b.coups) if (moi && c.tireur === moi.pseudo) cibles[c.victime] = (cibles[c.victime] || 0) + c.degats;
  const ligne = (j) => `
    <div class="ligne${classe(j)}">${icone(j.operateur)}<div class="nom">${esc(j.pseudo)}</div>
      <div class="barre"><div data-l="${(100 * j.inflige) / max}"></div></div><div class="val">${j.inflige}</div></div>`;
  const parInflige = (x, y) => y.inflige - x.inflige;
  const p1 = `
    ${moi ? `<div class="moi"><b>${moi.inflige}</b><span>dégâts · ${moi.kills} kill${moi.kills > 1 ? 's' : ''}</span></div>` : ''}
    <div class="cibles">${Object.entries(cibles).sort((x, y) => y[1] - x[1]).map(([n, v]) => `<span>${esc(n)}<i>${v}</i></span>`).join('')}</div>
    <h2>Ton équipe</h2>${parEquipe(monEquipe, parInflige).map(ligne).join('')}
    <div class="adverse"><h2>Adversaires</h2>${parEquipe(1 - monEquipe, parInflige).map(ligne).join('')}</div>`;

  // 2 · Tes duels : chaque ennemi touché ou qui t'a touché (comme le Damage Report de stats.cc)
  const duels = {};
  const duel = (n) => (duels[n] ||= { sortant: 0, entrant: 0, nS: [0, 0, 0], nE: [0, 0, 0], dS: [0, 0, 0], dE: [0, 0, 0], tue: false, tueur: false });
  for (const c of b.coups) {
    if (!moi) break;
    if (c.tireur === moi.pseudo) {
      const d = duel(c.victime);
      d.sortant += c.degats; c.balles.forEach((x, i) => (d.nS[i] += x));
      [c.tete, c.corps, c.membres].forEach((x, i) => (d.dS[i] += x));
      if (c.final) d.tue = true;
    } else if (c.victime === moi.pseudo && c.tireur) {
      const d = duel(c.tireur);
      d.entrant += c.degats; c.balles.forEach((x, i) => (d.nE[i] += x));
      [c.tete, c.corps, c.membres].forEach((x, i) => (d.dE[i] += x));
      if (c.final) d.tueur = true;
    }
  }
  const lignesDuels = Object.entries(duels).sort((x, y) => y[1].sortant - x[1].sortant || y[1].entrant - x[1].entrant).map(([n, d]) => {
    const j = b.joueurs.find((x) => x.pseudo === n) || {};
    const chiffre = (v, mot) => `<div class="chiffre${v ? '' : ' zero'}">${v}${mot ? `<small>${mot}</small>` : ''}</div>`;
    return `<div class="duel">
      <div class="sortant">${chiffre(d.sortant, d.tue ? 'TUÉ' : '')}</div>${silhouette(d.nS, d.dS, 'var(--accent)')}
      ${icone(j.operateur)}<div class="nom">${esc(n)}</div>
      ${silhouette(d.nE, d.dE, 'var(--rouge)')}<div class="entrant">${chiffre(d.entrant, d.tueur ? "T'A TUÉ" : '')}</div></div>`;
  });
  const p2 = `<div class="entetes"><span>Tes dégâts</span><span>Sur toi</span></div>${lignesDuels.join('') || '<div class="vide">Aucun duel cette manche</div>'}`;

  // 3 · Dégâts donnés et reçus de chacun (cette manche)
  const duo = (j) => `
    <div class="ligne duo${classe(j)}">${icone(j.operateur)}<div class="nom">${esc(j.pseudo)}</div>
      <div class="val${j.inflige ? '' : ' zero'}">${j.inflige}</div><div class="val recu${j.recu ? '' : ' zero'}">${j.recu}</div></div>`;
  const p3 = `
    <h2>Ton équipe<span>donnés</span><span>reçus</span></h2>${parEquipe(monEquipe, parInflige).map(duo).join('')}
    <h2>Adversaires<span>donnés</span><span>reçus</span></h2>${parEquipe(1 - monEquipe, parInflige).map(duo).join('')}`;

  return `
    <div class="haut"><div class="manche">
        <button data-manche="-1"${indice > 0 ? '' : ' disabled'} title="Manche d'avant">‹</button>
        <span>Manche ${b.manche}${b.map ? ' · ' + esc(b.map) : ''}</span>
        <button data-manche="1"${indice < manches.length - 1 ? '' : ' disabled'} title="Manche suivante">›</button></div>
      <div class="onglets">${[1, 2, 3].map((n) => `<button data-page="${n}">${n}</button>`).join('')}</div></div>
    <div class="page" data-page="1">${p1}</div><div class="page" data-page="2">${p2}</div><div class="page" data-page="3">${p3}</div>`;
}

function montrer(n) {
  page = n;
  document.querySelectorAll('.page').forEach((p) => p.classList.toggle('active', p.dataset.page == n));
  document.querySelectorAll('.onglets button').forEach((x) => x.classList.toggle('actif', x.dataset.page == n));
  if (n === 1) setTimeout(() => document.querySelectorAll('.page.active .barre div').forEach((d) => (d.style.width = d.dataset.l + '%')), 120);
}

function programmerSortie(ms) {
  const carte = document.getElementById('carte');
  carte.classList.remove('sortie');
  clearTimeout(fermeture);
  fermeture = setTimeout(() => carte.classList.add('sortie'), ms);
}

// Affiche la manche `indice` en gardant la page en cours
function dessiner() {
  const carte = document.getElementById('carte');
  carte.innerHTML = rendre(manches[indice]);
  montrer(page);
}

async function afficher() {
  const r = await invoke('degats_manches');
  manches = r.manches || [];
  if (!manches.length) return;
  indice = manches.length - 1;
  page = 1;
  dessiner();
  requestAnimationFrame(() => document.getElementById('carte').classList.add('visible'));
  // Défilement 1 → 2 → 3, puis fondu juste avant la fermeture par l'app (20 s)
  clearInterval(defile);
  defile = setInterval(() => (page < 3 ? montrer(page + 1) : clearInterval(defile)), DEFILEMENT);
  programmerSortie(19000);
}

// Un clic (page ou manche) arrête le défilement et garde la fenêtre ouverte
function interagir() {
  clearInterval(defile);
  invoke('degats_garder');
  programmerSortie(24000);
}

document.addEventListener('click', (e) => {
  const n = e.target.closest('.onglets button')?.dataset.page;
  const pas = e.target.closest('.manche button:not([disabled])')?.dataset.manche;
  if (n) {
    montrer(Number(n));
  } else if (pas) {
    indice = Math.max(0, Math.min(manches.length - 1, indice + Number(pas)));
    dessiner();
  } else {
    return;
  }
  interagir();
});

window.__TAURI__.event.listen('degats', afficher);
afficher();
