// Tableau de la partie de R6 (Ctrl+X), d'après les replays des manches déjà jouées : score, K/D/A, HS %, dégâts,
// opérateurs, joueurs déjà croisés (contre / avec, sur chacun de tes comptes) et groupes (barre rose).
const { invoke } = window.__TAURI__.core;
const esc = escOp;

// Groupes : ton squad (écrit dans le replay) et les joueurs déjà vus au moins 2 fois dans la même équipe.
// force = nombre de parties ensemble (plus c'est grand, plus la barre est rose)
function groupesDe(b, rencontres) {
  const parent = {};
  const racine = (x) => (parent[x] === undefined || parent[x] === x ? x : (parent[x] = racine(parent[x])));
  const unir = (x, y) => { parent[x] ??= x; parent[y] ??= y; parent[racine(x)] = racine(y); };
  const force = {};
  const lien = (x, y, n) => { unir(x, y); force[x] = Math.min(force[x] ?? n, n); force[y] = Math.min(force[y] ?? n, n); };
  const moi = b.joueurs.find((j) => j.pseudo === b.moi);
  const squad = (j) => j.lien === 3 || j.lien === 5;
  for (const j of b.joueurs) {
    if (moi && j.lien === 3) lien(moi.pseudo, j.pseudo, (rencontres[j.pseudo]?.avec ?? 0) + 1);
    for (const k of b.joueurs) {
      if (k === j || k.equipe !== j.equipe || (squad(j) && squad(k))) continue;
      const n = rencontres[j.pseudo]?.ensemble?.[k.pseudo] ?? 0;
      if (n >= 2) lien(j.pseudo, k.pseudo, n);
    }
  }
  const membres = {};
  for (const x of Object.keys(parent)) (membres[racine(x)] ||= []).push(x);
  const out = {};
  Object.values(membres).filter((m) => m.length > 1).forEach((m, id) => {
    const f = Math.min(...m.map((x) => force[x] ?? 1));
    for (const x of m) out[x] = { id, taille: m.length, force: f };
  });
  return out;
}

// Rang : son logo seul (rangs.js), le nom au survol
function badge(id, saison) {
  return `<span class="rang">${logoRang(id, saison, 30)}</span>`;
}

function rendre({ manches, rencontres, ubisoft = {}, ubisoft_connecte: ubiOk = false }) {
  const cadre = document.getElementById('cadre');
  const haut = (lieu) => `<div class="barre-haut"><b><span>FPS Cagoule</span> · Tableau</b>
    <div class="touche"><kbd>Ctrl</kbd> + <kbd>X</kbd> pour fermer</div><div class="lieu">${lieu}</div></div>`;
  if (!manches.length) {
    cadre.innerHTML = `${haut('')}<div class="vide">Le tableau se remplit à la fin de la première manche.</div>`;
    return;
  }
  const b = manches[manches.length - 1];
  const moi = b.joueurs.find((j) => j.pseudo === b.moi);
  const monEquipe = moi ? moi.equipe : 0;

  // Cumul de la partie : compteurs du jeu (score, kills, assists) quand ils existent, sinon la somme des manches
  const cumul = {};
  for (const m of manches) {
    for (const j of m.joueurs) {
      const c = (cumul[j.pseudo] ||= { k: 0, d: 0, a: 0, hs: 0, donnes: 0, recus: 0, ops: {} });
      c.k += j.kills; c.d += j.mort ? 1 : 0; c.a += j.assists; c.hs += j.kills_tete; c.donnes += j.inflige; c.recus += j.recu;
      if (j.operateur) c.ops[j.operateur] = (c.ops[j.operateur] || 0) + 1;
    }
  }
  for (const j of b.joueurs) {
    if (j.partie && cumul[j.pseudo]) Object.assign(cumul[j.pseudo], { k: j.partie.kills, a: j.partie.assists, score: j.partie.score });
  }

  const groupes = groupesDe(b, rencontres);
  const sousLigne = (j) => {
    const r = rencontres[j.pseudo];
    if (!r || j.pseudo === b.moi) return j.pseudo === b.moi ? '<small>Toi</small>' : '';
    const morceaux = [];
    if (r.contre) morceaux.push(`Contre ${r.contre}×`);
    if (r.avec) morceaux.push(`Avec ${r.avec}×`);
    for (const a of r.autres_comptes) {
      const t = [a.contre && `contre ${a.contre}×`, a.avec && `avec ${a.avec}×`].filter(Boolean).join(', ');
      morceaux.push(`${t} sur ${esc(a.pseudo)}`);
    }
    if (j.lien === 3) morceaux.unshift('Ton groupe');
    return `<small>${morceaux.join(' · ') || 'Jamais croisé'}</small>`;
  };
  const ligne = (j) => {
    const c = cumul[j.pseudo];
    const g = groupes[j.pseudo];
    const barre = g ? `<i class="groupe" style="background:rgba(255,92,170,${(0.3 + 0.7 * Math.min(1, g.force / 8)).toFixed(2)})"></i>` : '<i></i>';
    const ops = Object.entries(c.ops).sort((x, y) => y[1] - x[1]).slice(0, 2).map(([o]) => icone(o)).join('');
    const v = (x, cl = '') => `<div class="v ${cl}${x ? '' : ' zero'}">${x ?? '–'}</div>`;
    const cliquable = ubisoft[j.pseudo] !== null && j.pseudo !== b.moi;
    return `<div class="joueur${j.pseudo === b.moi ? ' toi' : ''}${cliquable ? ' cliquable' : ''}" data-pseudo="${esc(j.pseudo)}">${barre}${icone(j.operateur)}
      <div class="nom"><b>${esc(j.pseudo)}</b>${sousLigne(j)}</div>
      ${v(c.score)}
      <div class="v kda">${c.k}<span>/</span>${c.d}<span>/</span>${c.a}</div>
      <div class="v${c.k ? '' : ' zero'}">${c.k ? Math.round((100 * c.hs) / c.k) + ' %' : '–'}</div>
      ${v(c.donnes)}${ubiOk ? colonnesUbi(j) : v(c.recus, 'recu')}
      <div class="ops">${ops}</div></div>`;
  };
  // Stats Ubisoft : « … » en attente, « ? » sans réponse (nom caché…)
  const colonnesUbi = (j) => {
    const s = ubisoft[j.pseudo];
    if (s === undefined || s === null) {
      const t = s === undefined ? '…' : '?';
      return `<div class="v zero">${t}</div><div class="v zero">${t}</div><div class="v zero">${t}</div><div class="v zero">${t}</div><div class="v zero">${t}</div><div class="v zero">${t}</div>`;
    }
    const parties = s.victoires + s.defaites + s.abandons;
    const kd = s.morts ? (s.kills / s.morts).toFixed(2) : s.kills ? s.kills.toFixed(2) : '–';
    const wr = parties ? Math.round((100 * s.victoires) / parties) + ' %' : '–';
    return `${badge(s.rang, s.saison)}${badge(s.pic_saison, s.saison)}${badge(Math.max(s.pic_general, s.pic_saison), s.saison)}
      <div class="v${s.niveau ? '' : ' zero'}">${s.niveau ?? '–'}</div><div class="v">${kd}</div><div class="v">${wr}</div>`;
  };
  // Membres d'un groupe côte à côte, puis par score
  const ordre = (x, y) => (groupes[y.pseudo]?.id ?? -1) - (groupes[x.pseudo]?.id ?? -1) || (cumul[y.pseudo].score ?? cumul[y.pseudo].k) - (cumul[x.pseudo].score ?? cumul[x.pseudo].k);
  const equipe = (e, classe, chiffre) => `<div class="equipe ${classe}"><div class="chiffre">${chiffre}</div><div>
    <div class="colonnes"><i></i><i></i><span class="g">Joueur</span><span>Score</span><span>K / D / A</span><span>HS %</span><span>Dégâts</span>${ubiOk
      ? '<span class="c">Rang</span><span class="c">Pic saison</span><span class="c">Pic</span><span>Niveau</span><span>KD</span><span>Vict.</span>'
      : '<span>Reçus</span>'}<span>${ubiOk ? 'Ops' : 'Opérateurs'}</span></div>
    ${b.joueurs.filter((j) => j.equipe === e).sort(ordre).map(ligne).join('')}</div></div>`;

  // Bande du milieu : chaque manche (ton côté, gagnée ou perdue) et les totaux des deux équipes
  const chips = manches.map((m) => `<div class="m ${m.gagnee === true ? 'g' : m.gagnee === false ? 'p' : ''}">${m.attaque === true ? 'ATK' : m.attaque === false ? 'DEF' : '·'}<b>${m.gagnee === true ? 'G' : m.gagnee === false ? 'P' : '–'}</b>${m.manche}</div>`).join('');
  const total = (e, cle) => b.joueurs.filter((j) => j.equipe === e).reduce((s, j) => s + (cumul[j.pseudo][cle] || 0), 0);
  const totaux = `<div class="totaux">
    <div class="a">Kills<b>${total(monEquipe, 'k')}</b></div><div class="b">Kills<b>${total(1 - monEquipe, 'k')}</b></div>
    <div class="a">Dégâts<b>${total(monEquipe, 'donnes')}</b></div><div class="b">Dégâts<b>${total(1 - monEquipe, 'donnes')}</b></div></div>`;

  cadre.innerHTML = `${haut(`${esc(b.map || '')} · Manche ${b.manche}`)}
    ${equipe(monEquipe, 'nous', b.nous)}
    <div class="milieu"><div class="manches">${chips}</div>${totaux}</div>
    ${equipe(1 - monEquipe, 'eux', b.eux)}
    <div class="pied">D'après les replays de la partie : mis à jour à chaque fin de manche.${ubiOk
      ? ' Rang, KD et victoires : saison en cours (Ubisoft). Pic : meilleur rang vu par FPS Cagoule.'
      : ' Connecte un compte Ubisoft dans les Réglages pour les rangs.'}</div>`;
  cadre.classList.toggle('sans-ubi', !ubiOk);
  cadre.insertAdjacentHTML('beforeend', '<aside id="fiche" class="fiche" hidden></aside>');
  etat = { b, rencontres, ubisoft, ubiOk, cumul };
  marquer();
}

// Fiche joueur : ↑ ↓ pour choisir, Entrée pour ouvrir / fermer (touches prises par l'app tant que le tableau est ouvert),
// ou clic quand un curseur est visible. Pas de fiche pour un nom caché ni pour toi.
let etat = null, choisi = null, ficheOuverte = false;

function lignes() {
  return [...document.querySelectorAll('.joueur.cliquable')];
}

function marquer() {
  document.querySelectorAll('.joueur').forEach((l) => l.classList.toggle('choisi', l.dataset.pseudo === choisi));
  if (ficheOuverte && choisi) ouvrirFiche(choisi);
}

function deplacer(pas) {
  const l = lignes();
  if (!l.length) return;
  const i = l.findIndex((x) => x.dataset.pseudo === choisi);
  const n = i < 0 ? (pas > 0 ? 0 : l.length - 1) : (i + pas + l.length) % l.length;
  choisi = l[n].dataset.pseudo;
  marquer();
}

async function ouvrirFiche(pseudo) {
  const fiche = document.getElementById('fiche');
  if (!fiche || !etat) return;
  const j = etat.b.joueurs.find((x) => x.pseudo === pseudo);
  if (!j) return;
  ficheOuverte = true;
  const s = etat.ubisoft[pseudo];
  const r = etat.rencontres[pseudo];
  const c = etat.cumul[pseudo];
  const parts = s ? s.victoires + s.defaites + s.abandons : 0;
  const tuile = (t, v, d = '') => `<div class="tuile"><small>${t}</small><b>${v}</b>${d ? `<small>${d}</small>` : ''}</div>`;
  const rang = (id) => `<span class="fiche-rang">${logoRang(id, s?.saison ?? 0, 20)}${id ? nomRang(id) : '–'}</span>`;
  fiche.hidden = false;
  fiche.innerHTML = `
    <div class="fiche-tete">${s ? logoRang(s.rang, s.saison, 46) : icone(j.operateur)}<div><h3>${esc(pseudo)}</h3>
      <small>${s?.niveau ? `Niveau ${s.niveau}` : ''}${r && (r.contre || r.avec) ? `${s?.niveau ? ' · ' : ''}Contre ${r.contre}× · Avec ${r.avec}×` : `${s?.niveau ? ' · ' : ''}Jamais croisé`}</small></div></div>
    ${s ? `<div class="tuiles">${tuile('Rang', rang(s.rang), s.rang ? `${s.points} points` : '')}${tuile('Pic saison', rang(s.pic_saison))}
      ${tuile('Pic', rang(Math.max(s.pic_general, s.pic_saison)))}${tuile('KD saison', s.morts ? (s.kills / s.morts).toFixed(2) : '–')}
      ${tuile('Victoires', parts ? Math.round((100 * s.victoires) / parts) + ' %' : '–', `${parts} parties`)}${tuile('Cette partie', `${c.k} / ${c.d} / ${c.a}`, `${c.donnes} dégâts`)}</div>`
      : `<p class="discret">${etat.ubiOk ? 'Stats Ubisoft en chemin…' : 'Connecte un compte Ubisoft dans FPS Cagoule pour ses rangs.'}</p>`}
    <h4>Vos parties</h4><div id="fiche-parties" class="discret">…</div>
    <small class="fiche-aide">↑ ↓ pour changer de joueur · Entrée pour fermer</small>`;
  const parties = j.profil ? await invoke('r6_parties', { profil: j.profil }).catch(() => []) : [];
  const zone = document.getElementById('fiche-parties');
  if (!zone || choisi !== pseudo) return;
  const passees = parties.filter((p) => p.partie !== etat.b.partie).slice(0, 6);
  zone.innerHTML = passees.length
    ? passees.map((p) => {
      const lui = p.joueurs.find((x) => x.profil === j.profil.toLowerCase());
      const res = p.resultat ? (p.resultat.nous > p.resultat.eux ? 'g' : p.resultat.nous < p.resultat.eux ? 'p' : '') : '';
      return `<div class="fiche-partie ${res}"><span>${new Date(p.date_ms).toLocaleDateString('fr-CH', { day: 'numeric', month: 'short' })}</span><span>${esc(p.map || '—')}</span><b>${lui?.avec ? 'Avec' : 'Contre'}</b>${p.resultat ? `<span>${p.resultat.nous} – ${p.resultat.eux}</span>` : '<span></span>'}</div>`;
    }).join('')
    : 'Première fois que tu le croises.';
}

function fermerFiche() {
  ficheOuverte = false;
  const f = document.getElementById('fiche');
  if (f) f.hidden = true;
}

window.__TAURI__.event.listen('tableau_touche', ({ payload }) => {
  if (payload === 'haut') deplacer(-1);
  else if (payload === 'bas') deplacer(1);
  else if (payload === 'entree') {
    if (ficheOuverte) fermerFiche();
    else {
      if (!choisi) deplacer(1);
      if (choisi) ouvrirFiche(choisi);
    }
  }
});

document.addEventListener('click', (e) => {
  const l = e.target.closest('.joueur.cliquable');
  if (l) {
    choisi = l.dataset.pseudo;
    marquer();
    ouvrirFiche(choisi);
  } else if (!e.target.closest('#fiche')) fermerFiche();
});

async function afficher() {
  const r = await invoke('degats_manches');
  rendre({ manches: r.manches || [], rencontres: r.rencontres || {}, ubisoft: r.ubisoft || {}, ubisoft_connecte: !!r.ubisoft_connecte });
  requestAnimationFrame(() => document.getElementById('cadre').classList.add('visible'));
}

window.__TAURI__.event.listen('degats', afficher);
afficher();
