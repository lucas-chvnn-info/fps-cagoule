// Fenêtre de fin de manche (R6) : dégâts infligés par chaque joueur, lus dans le replay
const { invoke } = window.__TAURI__.core;
const esc = (t) => String(t ?? '').replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
let fermeture;

async function afficher() {
  const b = await invoke('degats_derniere');
  const carte = document.getElementById('carte');
  if (!b) return;
  const moi = b.joueurs.find((j) => j.pseudo === b.moi);
  const monEquipe = moi ? moi.equipe : 0;
  const max = Math.max(1, ...b.joueurs.map((j) => j.inflige));
  // Mes dégâts par ennemi
  const cibles = {};
  for (const c of b.coups) if (moi && c.tireur === moi.pseudo) cibles[c.victime] = (cibles[c.victime] || 0) + c.degats;
  const ligne = (j) => `
    <div class="ligne${j.pseudo === b.moi ? ' toi' : ''}">
      <div class="nom">${esc(j.pseudo)}${j.operateur ? `<small>${esc(j.operateur)}</small>` : ''}</div>
      <div class="barre"><div data-l="${(100 * j.inflige) / max}"></div></div>
      <div class="val">${j.inflige}</div>
    </div>`;
  const equipe = (e) => b.joueurs.filter((j) => j.equipe === e).sort((x, y) => y.inflige - x.inflige).map(ligne).join('');
  carte.innerHTML = `
    <div class="tete">Manche ${b.manche}${b.map ? ' · ' + esc(b.map) : ''}</div>
    ${moi ? `<div class="moi"><b>${moi.inflige}</b><span>dégâts · ${moi.kills} kill${moi.kills > 1 ? 's' : ''}</span></div>` : ''}
    <div class="cibles">${Object.entries(cibles).sort((x, y) => y[1] - x[1]).map(([n, v]) => `<span>${esc(n)}<i>${v}</i></span>`).join('')}</div>
    <h2>Ton équipe</h2>${equipe(monEquipe)}
    <div class="adverse"><h2>Adversaires</h2>${equipe(1 - monEquipe)}</div>`;
  carte.classList.remove('sortie');
  requestAnimationFrame(() => {
    carte.classList.add('visible');
    setTimeout(() => carte.querySelectorAll('.barre div').forEach((d) => (d.style.width = d.dataset.l + '%')), 120);
  });
  // Fondu de sortie juste avant que l'app ferme la fenêtre (15 s)
  clearTimeout(fermeture);
  fermeture = setTimeout(() => carte.classList.add('sortie'), 14000);
}

window.__TAURI__.event.listen('degats', afficher);
afficher();
