import { evaluer } from './evaluer.js';

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (sel) => document.querySelector(sel);
const esc = (t) => String(t ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
const Go = 1024 ** 3;

let etat = { plan_perso: null };
let derniereAnalyse = null;
let tweaks = [];
let profils = [];
let reglages = null;

function toast(texte, duree = 4000) {
  const t = $('#toast');
  t.textContent = texte;
  t.hidden = false;
  clearTimeout(toast.minuteur);
  toast.minuteur = setTimeout(() => (t.hidden = true), duree);
}

async function appel(cmd, args, bouton) {
  bouton?.classList.add('chargement');
  try {
    return await invoke(cmd, args);
  } catch (e) {
    toast(`Erreur : ${e}`, 7000);
    throw e;
  } finally {
    bouton?.classList.remove('chargement');
  }
}

// Navigation
document.querySelectorAll('nav button').forEach((b) =>
  b.addEventListener('click', () => {
    document.querySelectorAll('nav button, .page').forEach((e) => e.classList.remove('actif'));
    b.classList.add('actif');
    $(`#page-${b.dataset.page}`).classList.add('actif');
    if (['optimisations', 'connexion', 'peripheriques'].includes(b.dataset.page)) chargerTweaks();
    if (b.dataset.page === 'connexion') chargerCartes();
    if (b.dataset.page === 'optimisations') chargerDemarrage();
    if (b.dataset.page === 'ecrans') chargerEcrans();
    if (b.dataset.page === 'pilotes') chargerPilotes();
    if (b.dataset.page === 'fps') chargerSessions();
    if (b.dataset.page === 'jeux') chargerChoixFermer();
  })
);

/* ---------- Analyse et score ---------- */

function memoriserAnalyse(info) {
  derniereAnalyse = info;
  try { localStorage.setItem('analyse', JSON.stringify(info)); } catch {}
}

function afficherScore() {
  if (!derniereAnalyse) return;
  const info = demarrage ? { ...derniereAnalyse, demarrage: demarrage.filter((p) => p.actif) } : derniereAnalyse;
  const { score, problemes } = evaluer(info, { planPerso: [etat.plan_perso, etat.plan_max] });
  $('#score').textContent = score;
  const anneau = $('#anneau');
  anneau.style.strokeDashoffset = 326.7 * (1 - score / 100);
  anneau.style.stroke = score >= 80 ? 'var(--accent)' : score >= 55 ? 'var(--orange)' : 'var(--rouge)';
  $('#score-texte').textContent =
    score >= 80 ? 'Ton PC est bien réglé pour jouer.' : score >= 55 ? 'Quelques réglages peuvent te faire gagner des FPS.' : 'Plusieurs points freinent ton PC en jeu.';

  const ids = new Set(tweaks.map((t) => t.id));
  $('#problemes').innerHTML = problemes.length
    ? problemes
        .map((p) => {
          let bouton = '';
          if (p.correctif === 'nettoyage') bouton = '<button class="petit" data-action="nettoyer">Nettoyer</button>';
          else if (p.correctif === 'pilotes') bouton = '<button class="petit principal" data-action="pilotes">Voir</button>';
          else if (p.correctif === 'demarrage') bouton = '<button class="petit" data-action="demarrage">Gérer</button>';
          else if (p.correctif && (ids.has(p.correctif) || !tweaks.length)) bouton = `<button class="petit principal" data-tweak="${esc(p.correctif)}">Corriger</button>`;
          return `<li class="probleme"><span class="point ${p.gravite}"></span><div><strong>${esc(p.titre)}</strong><p>${esc(p.detail)}</p></div>${bouton}</li>`;
        })
        .join('')
    : '<li class="discret">Aucun problème détecté. Bien joué !</li>';
  afficherMateriel();
}

$('#problemes').addEventListener('click', async (e) => {
  const b = e.target.closest('button');
  if (!b) return;
  if (b.dataset.action === 'nettoyer') return nettoyer(b);
  if (b.dataset.action === 'demarrage') {
    document.querySelector('nav [data-page="optimisations"]').click();
    return setTimeout(() => $('#carte-demarrage').scrollIntoView({ behavior: 'smooth' }), 100);
  }
  if (b.dataset.action === 'pilotes') return document.querySelector('nav [data-page="pilotes"]').click();
  if (b.dataset.tweak) {
    await appliquerTweaks([b.dataset.tweak], b);
    await analyser();
  }
});

async function analyser() {
  const b = $('#btn-analyser');
  b.disabled = true;
  b.textContent = 'Analyse en cours…';
  try {
    memoriserAnalyse(await appel('analyser'));
    afficherScore();
  } finally {
    b.disabled = false;
    b.textContent = 'Analyser mon PC';
  }
}
$('#btn-analyser').addEventListener('click', analyser);

const ligne = (k, v) => (v === null || v === undefined || v === '' ? '' : `<div class="ligne-info"><span>${esc(k)}</span><span>${esc(v)}</span></div>`);
const taille = (o) => (o ? `${Math.round(Number(o) / Go)} Go` : '');

function afficherMateriel() {
  const a = derniereAnalyse;
  if (!a) return;
  $('#date-analyse').textContent = `Dernière analyse : ${new Date(a.date).toLocaleString('fr-CH')}`;
  const ram = a.ram || [];
  const totalRam = ram.reduce((s, b) => s + Number(b.capacite || 0), 0);
  const typeRam = { 26: 'DDR4', 34: 'DDR5', 24: 'DDR3' }[ram[0]?.type] || '';
  const cartes = [
    `<div class="carte"><h2>Processeur</h2>${ligne('Modèle', a.cpu?.nom)}${ligne('Cœurs / threads', `${a.cpu?.coeurs} / ${a.cpu?.threads}`)}${ligne('Fréquence de base', a.cpu?.mhz && `${(a.cpu.mhz / 1000).toFixed(2)} GHz`)}${ligne('Charge actuelle', a.cpu?.charge != null && `${a.cpu.charge} %`)}</div>`,
    ...(a.gpus || []).map(
      (g) =>
        `<div class="carte"><h2>Carte graphique</h2>${ligne('Modèle', g.nom)}${ligne('Pilote', `${g.pilote || ''}${g.datePilote ? ` (${g.datePilote})` : ''}`)}${ligne('Écran', g.largeur && `${g.largeur} × ${g.hauteur} à ${g.hz} Hz`)}${ligne('Température', a.tempGpu != null && /nvidia|geforce|rtx|gtx/i.test(g.nom) ? `${a.tempGpu} °C` : '')}</div>`
    ),
    `<div class="carte"><h2>Mémoire vive</h2>${ligne('Total', `${Math.round(totalRam / Go)} Go ${typeRam}`)}${ligne('Barrettes', ram.length)}${ligne('Vitesse', ram[0] && `${ram[0].vitesseReelle} MHz (prévue : ${ram[0].vitesse} MHz)`)}${ligne('Libre maintenant', a.os?.ramLibreKo && `${(a.os.ramLibreKo / 1048576).toFixed(1)} Go`)}</div>`,
    `<div class="carte"><h2>Stockage</h2>${(a.disques || []).map((d) => ligne(d.nom, `${d.type} ${d.bus} · ${taille(d.taille)} · ${d.sante}`)).join('')}${(a.volumes || []).map((v) => ligne(`Lecteur ${v.lettre}`, `${taille(v.libre)} libres sur ${taille(v.taille)}`)).join('')}</div>`,
    `<div class="carte"><h2>Windows</h2>${ligne('Version', `${a.os?.nom} (build ${a.os?.build})`)}${ligne("Plan d'alimentation", a.plan?.nom)}${ligne('Allumé depuis', a.os?.joursAllume != null && `${a.os.joursAllume} jours`)}${ligne('Isolation du noyau', Number(a.vbs) === 2 ? 'Active' : 'Inactive')}</div>`,
    `<div class="carte"><h2>Programmes au démarrage (${(a.demarrage || []).length})</h2>${(a.demarrage || []).map((d) => ligne(d.nom, d.emplacement?.split('\\').pop())).join('') || '<p class="discret">Aucun.</p>'}</div>`,
  ];
  $('#materiel').innerHTML = cartes.join('');
}

/* ---------- Optimisations ---------- */

const gainBadge = { fort: '<span class="badge accent">Gain fort</span>', moyen: '<span class="badge bleu">Gain moyen</span>', faible: '<span class="badge">Gain faible</span>' };

function ligneTweak(t) {
  let droite;
  if (!t.disponible) droite = '<span class="badge">Non disponible sur ton PC</span>';
  else if (t.actif) droite = `<span class="badge accent">Actif</span>${t.annulable ? `<button class="petit" data-annuler="${esc(t.id)}">Annuler</button>` : ''}`;
  else droite = `${t.annulable ? `<button class="petit" data-annuler="${esc(t.id)}">Annuler</button>` : ''}<button class="petit principal" data-activer="${esc(t.id)}">Activer</button>`;
  return `<li class="tweak">
      <div>
        <div class="titre">${esc(t.titre)} ${gainBadge[t.gain] || ''} ${t.niveau === 'max' ? '<span class="badge violet">Boost max</span>' : ''} ${t.redemarrage ? '<span class="badge orange">Après redémarrage</span>' : ''}</div>
        <p>${esc(t.description)}</p>
      </div>
      <div class="droite">${droite}</div>
    </li>`;
}

async function chargerTweaks() {
  tweaks = await appel('tweaks_liste');
  document.querySelectorAll('.tweaks[data-categorie]').forEach((ul) => {
    ul.innerHTML = tweaks.filter((t) => t.categorie === ul.dataset.categorie).map(ligneTweak).join('');
  });
  const dispo = tweaks.filter((t) => t.disponible);
  const actifs = dispo.filter((t) => t.actif).length;
  $('#resume-tweaks').textContent = `${actifs} optimisation${actifs > 1 ? 's' : ''} active${actifs > 1 ? 's' : ''} sur ${dispo.length} disponibles sur ton PC.`;
  $('#btn-tout-annuler').disabled = !tweaks.some((t) => t.annulable);
  afficherScore();
}

document.querySelectorAll('.tweaks[data-categorie]').forEach((ul) =>
  ul.addEventListener('click', async (e) => {
    const b = e.target.closest('button');
    if (b?.dataset.activer) return appliquerTweaks([b.dataset.activer], b);
    if (!b?.dataset.annuler) return;
    const [r] = await appel('tweaks_annuler', { ids: [b.dataset.annuler] }, b);
    toast(r.ok ? 'Réglage remis comme avant' : `Impossible d'annuler : ${r.erreur}`);
    chargerTweaks();
  })
);

async function appliquerTweaks(ids, bouton) {
  const [point, resultats] = await appel('tweaks_appliquer', { ids, restauration: $('#restauration').checked }, bouton);
  const echecs = resultats.filter((r) => !r.ok);
  const redemarrage = tweaks.some((t) => ids.includes(t.id) && t.redemarrage);
  const reussis = resultats.length - echecs.length;
  let msg = echecs.length ? `${reussis} appliquée(s), ${echecs.length} échec(s) : ${echecs.map((r) => r.erreur).join(' ; ')}` : ids.length > 1 ? `${reussis} optimisations appliquées.` : 'Optimisation appliquée.';
  if (redemarrage && !echecs.length) msg += ids.length > 1 ? ' Redémarre ton PC pour en profiter pleinement.' : ' Elle prend effet au prochain redémarrage.';
  if (point) msg = `${point}. ${msg}`;
  toast(msg, 7000);
  await chargerTweaks();
}

function confirmer(titre, html, libelle = 'Confirmer') {
  return new Promise((resoudre) => {
    $('#fenetre-titre').textContent = titre;
    $('#fenetre-texte').innerHTML = html;
    $('#fenetre-oui').textContent = libelle;
    $('#fenetre').hidden = false;
    const fin = (r) => {
      $('#fenetre').hidden = true;
      resoudre(r);
    };
    $('#fenetre-oui').onclick = () => fin(true);
    $('#fenetre-non').onclick = () => fin(false);
  });
}

async function optimiserTout(max, bouton) {
  if (!tweaks.length) tweaks = await appel('tweaks_liste');
  const a = tweaks.filter((t) => t.disponible && !t.actif && (max || t.niveau !== 'max'));
  if (!a.length) return toast('Tout est déjà optimisé.');
  const reseau = a.some((t) => t.special === 'adaptateur');
  const redemarrage = a.some((t) => t.redemarrage);
  const ok = await confirmer(
    max ? 'Boost maximum' : 'Optimisation recommandée',
    `<p>${a.length} réglage${a.length > 1 ? 's' : ''} vont être appliqués :</p><ul>${a.map((t) => `<li>${esc(t.titre)}</li>`).join('')}</ul>
     ${max ? '<p>Le Boost maximum inclut des réglages plus agressifs : le PC chauffe et consomme davantage, et les applis du Store ne t\'enverront plus de notifications.</p>' : ''}
     ${reseau ? '<p class="alerte">La connexion internet va se couper quelques secondes.</p>' : ''}
     ${redemarrage ? '<p>Certains réglages prennent effet au prochain redémarrage.</p>' : ''}`,
    'Appliquer'
  );
  if (!ok) return;
  await appliquerTweaks(a.map((t) => t.id), bouton);
}

$('#btn-recommande').addEventListener('click', (e) => optimiserTout(false, e.target));
$('#btn-max').addEventListener('click', (e) => optimiserTout(true, e.target));
$('#btn-tout-annuler').addEventListener('click', async (e) => {
  const a = tweaks.filter((t) => t.annulable);
  if (!a.length) return;
  const ok = await confirmer('Tout annuler', `<p>Les ${a.length} réglages changés par FPS Cagoule vont reprendre leur valeur d'origine.</p>`, 'Tout annuler');
  if (!ok) return;
  const r = await appel('tweaks_annuler', { ids: a.map((t) => t.id) }, e.target);
  const echecs = r.filter((x) => !x.ok);
  toast(echecs.length ? `${echecs.length} réglage(s) n'ont pas pu être annulés : ${echecs.map((x) => x.erreur).join(' ; ')}` : 'Ton PC est revenu à ses réglages d\'origine.', 7000);
  chargerTweaks();
});

async function nettoyer(bouton) {
  const r = await appel('nettoyer', {}, bouton);
  const texte = `${r.fichiers} fichiers supprimés, ${(r.octets / 1048576).toFixed(0)} Mo libérés.`;
  $('#nettoyage-resultat').textContent = texte;
  toast(texte);
}
$('#btn-nettoyer').addEventListener('click', (e) => nettoyer(e.target));
/* ---------- Programmes au démarrage ---------- */

let demarrage = null;

async function chargerDemarrage() {
  demarrage = await appel('demarrage_liste');
  const actifs = demarrage.filter((p) => p.actif).length;
  $('#nb-demarrage').textContent = `(${actifs} actif${actifs > 1 ? 's' : ''} sur ${demarrage.length})`;
  $('#demarrage-liste').innerHTML = demarrage.length
    ? demarrage
        .map(
          (p, i) => `<li class="${p.actif ? '' : 'inactif'}">
            <div class="info"><strong>${esc(p.nom.replace(/\.lnk$/i, ''))}</strong><small title="${esc(p.commande)}">${p.tous ? 'Tous les utilisateurs · ' : ''}${esc(p.commande)}</small></div>
            <label class="bascule" title="${p.actif ? 'Désactiver' : 'Activer'}"><input type="checkbox" data-i="${i}" ${p.actif ? 'checked' : ''}><span></span></label>
          </li>`
        )
        .join('')
    : '<li class="discret">Aucun programme ne se lance avec Windows.</li>';
  afficherScore();
}

$('#demarrage-liste').addEventListener('change', async (e) => {
  const p = demarrage[e.target.dataset.i];
  try {
    await appel('demarrage_changer', { source: p.source, nom: p.nom, actif: e.target.checked });
    toast(e.target.checked ? `${p.nom} se lancera avec Windows.` : `${p.nom} ne se lancera plus avec Windows.`);
  } catch {}
  chargerDemarrage();
});

/* ---------- Jeux ---------- */

function afficherProfils() {
  const caseP = (i, champ, titre) => `<td><input type="checkbox" data-i="${i}" data-champ="${champ}" ${profils[i][champ] ? 'checked' : ''} aria-label="${titre}"></td>`;
  $('#profils tbody').innerHTML = profils
    .map(
      (p, i) => `<tr>
        <td>${esc(p.nom)}<small>${esc(p.exes.join(', '))}</small></td>
        ${caseP(i, 'actif', 'Détection')}${caseP(i, 'boost', 'Boost')}${caseP(i, 'priorite_haute', 'Priorité haute')}${caseP(i, 'gpu_perf', 'GPU puissant')}
        <td><button class="petit discret-btn" data-suppr="${i}" title="Retirer">✕</button></td>
      </tr>`
    )
    .join('');
}

async function sauverProfils() {
  await appel('profils_set', { profils });
  afficherProfils();
}

$('#profils').addEventListener('change', (e) => {
  const c = e.target;
  if (c.dataset.champ) {
    profils[c.dataset.i][c.dataset.champ] = c.checked;
    sauverProfils();
  }
});
$('#profils').addEventListener('click', (e) => {
  const i = e.target.closest('button')?.dataset.suppr;
  if (i !== undefined) {
    profils.splice(Number(i), 1);
    sauverProfils();
  }
});
$('#ajout-jeu').addEventListener('submit', (e) => {
  e.preventDefault();
  let exe = $('#ajout-exe').value.trim();
  if (!exe.toLowerCase().endsWith('.exe')) exe += '.exe';
  profils.push({ nom: $('#ajout-nom').value.trim(), exes: [exe], actif: true, boost: true, priorite_haute: true, gpu_perf: true });
  e.target.reset();
  sauverProfils();
});
$('#btn-profils-defaut').addEventListener('click', async () => {
  profils = await appel('profils_defaut');
  sauverProfils();
});

async function sauverReglages() {
  await appel('reglages_set', { reglages });
}

function afficherReglages() {
  $('#boost-auto').checked = reglages.boost_auto;
  $('#boost-max').checked = reglages.boost_max;
  $('#mesure-fps').checked = reglages.mesure_fps;
  $('#overlay').checked = reglages.overlay;
  $('#maj-auto').checked = reglages.maj_auto;
  $('#couleurs-en-jeu').checked = reglages.couleurs_en_jeu;
  $('#fermer-liste').innerHTML = reglages.fermer_applis.length
    ? reglages.fermer_applis.map((a, i) => `<span class="puce">${esc(a)}<button data-retirer="${i}" title="Retirer">✕</button></span>`).join('')
    : '<span class="discret">Aucun programme.</span>';
}

for (const [id, champ] of [['#boost-auto', 'boost_auto'], ['#boost-max', 'boost_max'], ['#mesure-fps', 'mesure_fps'], ['#overlay', 'overlay'], ['#maj-auto', 'maj_auto'], ['#couleurs-en-jeu', 'couleurs_en_jeu']]) {
  $(id).addEventListener('change', (e) => {
    reglages[champ] = e.target.checked;
    sauverReglages();
  });
}

$('#fermer-liste').addEventListener('click', (e) => {
  const i = e.target.dataset.retirer;
  if (i === undefined) return;
  reglages.fermer_applis.splice(Number(i), 1);
  sauverReglages();
  afficherReglages();
});

async function chargerChoixFermer() {
  const noms = await appel('processus_liste');
  // On évite de proposer Windows lui-même : le fermer planterait la session
  const systeme = /^(system|registry|smss|csrss|wininit|winlogon|services|lsass|svchost|dwm|explorer|fontdrvhost|sihost|ctfmon|conhost|runtimebroker|searchhost|startmenuexperiencehost|textinputhost|shellexperiencehost|securityhealth.*|msmpeng|nissrv|spoolsv|audiodg|wudfhost|dllhost|taskhostw|lsaiso|memcompression|fps-cagoule|msedgewebview2|presentmon)(\.exe)?$/i;
  $('#fermer-choix').innerHTML =
    '<option value="">Choisir un programme ouvert…</option>' +
    noms.filter((n) => !systeme.test(n)).map((n) => `<option>${esc(n)}</option>`).join('');
}

$('#btn-fermer-ajout').addEventListener('click', () => {
  const n = $('#fermer-choix').value;
  if (!n || reglages.fermer_applis.some((a) => a.toLowerCase() === n.toLowerCase())) return;
  reglages.fermer_applis.push(n);
  sauverReglages();
  afficherReglages();
});

/* ---------- Partie en cours ---------- */

function afficherJeu(jeu) {
  $('#jeu-vide').hidden = !!jeu;
  $('#jeu-actif').hidden = !jeu;
  if (!jeu) return;
  $('#jeu-nom').textContent = jeu.nom;
  $('#jeu-badge').hidden = !jeu.boost;
  $('#jeu-fps').textContent = jeu.fps ? Math.round(jeu.fps) : '–';
  $('#jeu-actions').innerHTML = jeu.actions.map((a) => `<li>${esc(a)}</li>`).join('');
}

listen('jeu', (e) => afficherJeu(e.payload));
listen('fps', (e) => ($('#jeu-fps').textContent = Math.round(e.payload)));

/* ---------- Connexion ---------- */

async function chargerCartes() {
  const cartes = await appel('reseau_cartes').catch(() => []);
  $('#cartes-reseau').innerHTML = cartes.length
    ? cartes
        .map(
          (c) =>
            `${ligne(c.nom, `${c.wifi ? 'Wi-Fi' : 'Câble'} · ${c.vitesse}`)}<p class="discret">${esc(c.description)}</p>${
              c.wifi ? '<p class="alerte">Tu joues en Wi-Fi : un câble Ethernet supprime la plupart des pics de ping, plus que n\'importe quel réglage.</p>' : ''
            }`
        )
        .join('')
    : '<p class="discret">Aucune carte réseau connectée.</p>';
}

$('#btn-ping').addEventListener('click', async (e) => {
  const b = e.target;
  b.disabled = true;
  b.textContent = 'Test en cours…';
  try {
    const res = await appel('reseau_ping');
    $('#ping-resultats').innerHTML = res
      .map((r) => {
        if (r.moyenne == null) return `<div class="ping"><strong>${esc(r.cible)}</strong><p class="verdict alerte">Aucune réponse.</p></div>`;
        const box = r.cible.startsWith('Ta box');
        const mauvais = r.pertes > 0 || r.gigue > (box ? 2 : 5) || (box && r.moyenne > 5);
        const verdict = mauvais
          ? box
            ? 'Instable entre ton PC et ta box : c\'est souvent le Wi-Fi. Essaie un câble.'
            : r.pertes > 0
              ? 'Des paquets se perdent : relance le test ; si ça persiste, redémarre ta box.'
              : 'Ping irrégulier : ferme les téléchargements et les vidéos sur les autres appareils.'
          : 'Stable, rien à signaler.';
        return `<div class="ping">
          <strong>${esc(r.cible)}<small class="discret"> ${esc(r.adresse)}</small></strong>
          <div class="valeur">${r.moyenne.toFixed(0)} ms<small>ping moyen</small></div>
          <div class="valeur">${r.gigue.toFixed(1)} ms<small>gigue</small></div>
          <div class="valeur">${r.pertes.toFixed(0)} %<small>pertes</small></div>
          <p class="verdict ${mauvais ? 'alerte' : 'ok'}">${verdict}</p>
        </div>`;
      })
      .join('');
  } finally {
    b.disabled = false;
    b.textContent = 'Relancer le test';
  }
});

/* ---------- Périphériques : fréquence de la souris ---------- */

{
  const zone = $('#zone-souris');
  // pointerrawupdate donne chaque position envoyée par la souris ; pointermove est limité à la fréquence de l'écran
  const evenement = 'onpointerrawupdate' in window ? 'pointerrawupdate' : 'pointermove';
  let compte = 0;
  let debut = 0;
  let meilleur = 0;
  zone.addEventListener(evenement, () => {
    const t = performance.now();
    if (!debut) debut = t;
    compte++;
    if (t - debut >= 250) {
      const hz = (compte * 1000) / (t - debut);
      meilleur = Math.max(meilleur, hz);
      $('#souris-hz').textContent = Math.round(meilleur);
      $('#souris-ms').textContent = (1000 / meilleur).toFixed(meilleur > 900 ? 2 : 1);
      compte = 0;
      debut = t;
    }
  });
  zone.addEventListener('pointerleave', () => {
    compte = 0;
    debut = 0;
  });
  zone.addEventListener('dblclick', () => {
    meilleur = 0;
    $('#souris-hz').textContent = $('#souris-ms').textContent = '–';
  });
}

/* ---------- Écrans ---------- */

let couleurs = {};
const NEUTRE = { luminosite: 0, contraste: 1, gamma: 1, vibrance: null };
const CURSEURS = [
  { champ: 'luminosite', titre: 'Luminosité', min: -0.3, max: 0.3, pas: 0.01, aff: (v) => `${v > 0 ? '+' : ''}${Math.round(v * 100)}` },
  { champ: 'contraste', titre: 'Contraste', min: 0.7, max: 1.3, pas: 0.01, aff: (v) => `${Math.round(v * 100)} %` },
  { champ: 'gamma', titre: 'Gamma', min: 0.7, max: 1.5, pas: 0.01, aff: (v) => v.toFixed(2) },
];

async function chargerEcrans() {
  const [ecrans, c] = await Promise.all([appel('ecrans_liste'), appel('couleurs_get')]);
  couleurs = c;
  $('#ecrans').innerHTML = ecrans.length
    ? ecrans
        .map((e) => {
          const r = { ...NEUTRE, ...couleurs[e.id] };
          if (e.vibrance != null && r.vibrance == null) r.vibrance = e.vibrance;
          const max = Math.max(...e.frequences, e.hz);
          const curseurs = CURSEURS.map(
            (k) => `<label class="curseur"><span>${k.titre}</span><input type="range" data-champ="${k.champ}" min="${k.min}" max="${k.max}" step="${k.pas}" value="${r[k.champ]}"><output>${k.aff(r[k.champ])}</output></label>`
          ).join('');
          const vibrance =
            e.vibrance != null
              ? `<label class="curseur"><span>Vibrance</span><input type="range" data-champ="vibrance" min="50" max="100" step="1" value="${r.vibrance}"><output>${r.vibrance} %</output></label>`
              : '<p class="discret">Vibrance : disponible seulement sur les écrans branchés à une carte NVIDIA.</p>';
          return `<div class="carte ecran" data-id="${esc(e.id)}">
            <div class="entete"><h2>${esc(e.nom)}</h2>${e.principal ? '<span class="badge">Principal</span>' : ''}${e.hdr_actif ? '<span class="badge bleu">HDR actif</span>' : e.hdr_supporte ? '<span class="badge">HDR possible</span>' : ''}</div>
            ${ligne('Résolution', `${e.largeur} × ${e.hauteur}`)}
            <div class="ligne-info"><span>Fréquence</span><span>
              <select data-hz>${e.frequences.map((f) => `<option ${f === e.hz ? 'selected' : ''}>${f}</option>`).join('')}</select> Hz
              ${e.hz < max ? `<span class="alerte"> (max ${max} Hz)</span>` : ''}
            </span></div>
            <h2 style="margin-top:14px">Couleurs en jeu</h2>
            ${curseurs}${vibrance}
            <div class="boutons">
              <button class="principal" data-action="enregistrer">Enregistrer</button>
              <button data-action="essayer">Essayer maintenant</button>
              <button data-action="retablir">Rétablir</button>
              <button class="discret-btn" data-action="neutre">Valeurs neutres</button>
            </div>
          </div>`;
        })
        .join('')
    : '<p class="discret">Aucun écran trouvé.</p>';
}

function lireCarteEcran(carte) {
  const r = { ...NEUTRE };
  carte.querySelectorAll('input[type="range"]').forEach((i) => (r[i.dataset.champ] = Number(i.value)));
  return r;
}

$('#ecrans').addEventListener('input', (e) => {
  const i = e.target;
  if (i.type !== 'range') return;
  const k = CURSEURS.find((c) => c.champ === i.dataset.champ);
  i.nextElementSibling.textContent = k ? k.aff(Number(i.value)) : `${i.value} %`;
});

$('#ecrans').addEventListener('change', async (e) => {
  if (!e.target.matches('[data-hz]')) return;
  const carte = e.target.closest('.ecran');
  const hz = Number(e.target.value);
  await appel('ecran_frequence', { id: carte.dataset.id, hz }).then(() => toast(`Écran réglé à ${hz} Hz.`)).catch(() => {});
  chargerEcrans();
});

$('#ecrans').addEventListener('click', async (e) => {
  const b = e.target.closest('button');
  const carte = b?.closest('.ecran');
  if (!carte) return;
  const id = carte.dataset.id;
  const r = lireCarteEcran(carte);
  if (b.dataset.action === 'enregistrer') {
    couleurs[id] = r;
    await appel('couleurs_set', { couleurs });
    toast(reglages.couleurs_en_jeu ? 'Couleurs enregistrées : elles seront appliquées pendant tes parties.' : 'Couleurs enregistrées. Active « Appliquer mes couleurs de jeu pendant les parties » en haut de la page.', 6000);
  } else if (b.dataset.action === 'essayer') {
    const erreurs = await appel('couleurs_essayer', { couleurs: { [id]: r } }, b);
    toast(erreurs.length ? erreurs.join(' ; ') : 'Aperçu appliqué. « Rétablir » remet les couleurs d\'avant.', 6000);
  } else if (b.dataset.action === 'retablir') {
    await appel('couleurs_retablir', {}, b);
    toast('Couleurs remises comme avant.');
  } else if (b.dataset.action === 'neutre') {
    carte.querySelectorAll('input[type="range"]').forEach((i) => {
      i.value = i.dataset.champ === 'vibrance' ? 50 : NEUTRE[i.dataset.champ];
      i.dispatchEvent(new Event('input', { bubbles: true }));
    });
  }
});

/* ---------- Pilotes ---------- */

const LIENS = {
  amd: 'https://www.amd.com/fr/support/download/drivers.html',
  intel: 'https://www.intel.com/content/www/us/en/download-center/home.html',
  nvidia: 'https://www.nvidia.com/fr-fr/drivers/',
};
const Mo = (o) => `${(o / 1048576).toFixed(0)} Mo`;
let suiviNvidia = null;

// Catégories Windows (PNPClass) → libellés ; les composants internes sont masqués par défaut
const CATEGORIES = {
  Display: 'Cartes graphiques', Monitor: 'Écrans', Net: 'Réseau', Bluetooth: 'Bluetooth',
  MEDIA: 'Son', AudioEndpoint: 'Entrées et sorties audio', Mouse: 'Souris', Keyboard: 'Claviers',
  HIDClass: 'Manettes et périphériques USB', USB: 'Contrôleurs USB', Camera: 'Caméras', Image: 'Caméras',
  DiskDrive: 'Disques', SCSIAdapter: 'Contrôleurs de stockage', HDC: 'Contrôleurs de stockage',
  Printer: 'Imprimantes', PrintQueue: 'Imprimantes', Biometric: 'Biométrie', Battery: 'Batteries',
  Ports: 'Ports', SecurityDevices: 'Sécurité', Firmware: 'Micrologiciels', System: 'Système (chipset)',
  Processor: 'Processeur', Sensor: 'Capteurs', WPD: 'Appareils portables', USBDevice: 'Périphériques USB',
};
const INTERNES = new Set(['SoftwareDevice', 'SoftwareComponent', 'Volume', 'VolumeSnapshot', 'LegacyDriver', 'Computer', 'Extension', 'SWD', 'Processor', '']);
let peripheriques = [];
let majWU = null;
const categorie = (p) => CATEGORIES[p.classe] || p.classe || 'Autres';
const majPour = (p) => majWU?.find((m) => m.materiel && p.materiel.some((h) => h.toLowerCase() === m.materiel.toLowerCase()));

async function chargerPilotes() {
  chargerGpu();
  const liste = appel('peripheriques').catch(() => []);
  if (!majWU) rechercherWU();
  peripheriques = await liste;
  const cats = [...new Set(peripheriques.filter((p) => !INTERNES.has(p.classe)).map(categorie))].sort((a, b) => a.localeCompare(b, 'fr'));
  $('#classe-peripheriques').innerHTML = '<option value="">Toutes les catégories</option>' + cats.map((c) => `<option>${esc(c)}</option>`).join('');
  afficherPeripheriques();
}

function etatPeripherique(p) {
  if (p.erreur === 28) return '<span class="badge rouge">Pilote manquant</span>';
  if (p.erreur === 22) return '<span class="badge">Désactivé</span>';
  if (p.erreur) return `<span class="badge rouge">Problème (code ${p.erreur})</span>`;
  if (majPour(p)) return '<span class="badge orange">Mise à jour disponible</span>';
  return '<span class="badge accent">OK</span>';
}

function afficherPeripheriques() {
  const filtre = $('#filtre-peripheriques').value.trim().toLowerCase();
  const cat = $('#classe-peripheriques').value;
  const problemes = $('#problemes-peripheriques').checked;
  const internes = $('#systeme-peripheriques').checked;
  const liste = peripheriques
    .filter((p) => internes || !INTERNES.has(p.classe))
    .filter((p) => !cat || categorie(p) === cat)
    .filter((p) => !problemes || p.erreur || majPour(p))
    .filter((p) => !filtre || `${p.nom} ${p.fabricant} ${p.fournisseur}`.toLowerCase().includes(filtre))
    .sort((a, b) => categorie(a).localeCompare(categorie(b), 'fr') || a.nom.localeCompare(b.nom, 'fr'));
  $('#nb-peripheriques').textContent = `(${liste.length})`;
  let groupe = '';
  $('#peripheriques tbody').innerHTML = liste.length
    ? liste
        .map((p) => {
          const i = peripheriques.indexOf(p);
          const maj = majPour(p);
          let ligneGroupe = '';
          if (categorie(p) !== groupe) {
            groupe = categorie(p);
            ligneGroupe = `<tr class="groupe"><td colspan="4">${esc(groupe)}</td></tr>`;
          }
          return `${ligneGroupe}<tr>
            <td>${esc(p.nom)}<small>${esc(p.fabricant || '')}</small></td>
            <td>${p.version ? `${esc(p.version)}<small>${p.date ? new Date(p.date).toLocaleDateString('fr-CH') : ''}${p.fournisseur ? ` · ${esc(p.fournisseur)}` : ''}</small>` : '<span class="discret">–</span>'}</td>
            <td>${etatPeripherique(p)}</td>
            <td class="actions">${maj ? `<button class="petit principal" data-maj="${esc(maj.id)}">Mettre à jour</button>` : ''}${p.materiel.length ? `<button class="petit" data-web="${i}">Chercher sur internet</button>` : ''}</td>
          </tr>`;
        })
        .join('')
    : '<tr><td colspan="4" class="discret">Aucun appareil ne correspond.</td></tr>';
}

['#filtre-peripheriques', '#classe-peripheriques', '#problemes-peripheriques', '#systeme-peripheriques'].forEach((id) =>
  $(id).addEventListener('input', afficherPeripheriques)
);

$('#peripheriques').addEventListener('click', async (e) => {
  const b = e.target.closest('button');
  if (!b) return;
  if (b.dataset.web) {
    const p = peripheriques[b.dataset.web];
    // L'identifiant le plus précis (VEN_…&DEV_…) est le premier de la liste
    return appel('pilote_chercher_web', { nom: p.nom, materiel: p.materiel[0] });
  }
  if (b.dataset.maj) {
    b.disabled = true;
    b.textContent = 'Installation…';
    try {
      const r = await appel('pilotes_wu_installer', { ids: [b.dataset.maj] });
      const ok = r.details.every((d) => d.ok);
      toast(ok ? `Pilote installé.${r.redemarrage ? ' Redémarre ton PC pour terminer.' : ''}` : 'Windows Update n\'a pas pu installer ce pilote.', 7000);
      if (ok) majWU = majWU.filter((m) => m.id !== b.dataset.maj);
      chargerPilotes();
    } catch {
      b.disabled = false;
      b.textContent = 'Mettre à jour';
    }
  }
});

async function chargerGpu() {
  const carte = $('#carte-gpu');
  let nv = null;
  try {
    nv = await invoke('pilote_nvidia');
  } catch (e) {
    carte.innerHTML = `<h2>Carte graphique</h2><p class="alerte">Recherche impossible (${esc(e)}). Vérifie ta connexion.</p>`;
    return;
  }
  if (!nv) {
    // AMD ou Intel : pas d'API publique pour connaître le dernier pilote
    const gpus = (derniereAnalyse?.gpus || []).filter((g) => !/microsoft|virtual|hyper-v/i.test(g.nom));
    const amd = gpus.some((g) => /amd|radeon/i.test(g.nom));
    carte.innerHTML = `<h2>Carte graphique</h2>
      ${gpus.map((g) => ligne(g.nom, `${g.pilote || ''}${g.datePilote ? ` (${new Date(g.datePilote).toLocaleDateString('fr-CH')})` : ''}`)).join('')}
      <p class="discret">AMD et Intel ne publient pas de moyen de connaître leur dernier pilote. Leur page officielle détecte ta carte ; tu peux y prendre le pilote seul, sans garder leur application.</p>
      <button data-lien="${amd ? 'amd' : 'intel'}">Ouvrir la page des pilotes ${amd ? 'AMD' : 'Intel'}</button>`;
    return;
  }
  const aJour = !nv.derniere || nv.installe === nv.derniere;
  carte.innerHTML = `<h2>Carte graphique</h2>
    ${ligne(nv.gpu, `Pilote ${nv.installe}`)}
    ${nv.derniere ? ligne('Dernier pilote NVIDIA', `${nv.derniere} (${nv.date})`) : '<p class="discret">Modèle introuvable dans le catalogue NVIDIA.</p>'}
    ${aJour && nv.derniere ? '<p class="ok">Ton pilote est à jour.</p>' : ''}
    ${!aJour ? `<p class="discret">Téléchargement direct depuis NVIDIA (${esc(nv.taille)}), sans NVIDIA App. À l'installation, choisis « Personnalisée » pour décocher NVIDIA App si tu n'en veux pas, et coche « Installation propre » en cas de souci.</p>
      <button class="principal" id="btn-nvidia">Télécharger le pilote ${esc(nv.derniere)}</button>
      <div id="nvidia-progression" hidden><div class="barre"><div></div></div><span class="discret"></span></div>` : ''}
    ${!nv.derniere ? `<button data-lien="nvidia">Ouvrir la page des pilotes NVIDIA</button>` : ''}`;
  $('#btn-nvidia')?.addEventListener('click', async (e) => {
    await appel('pilote_nvidia_telecharger', { url: nv.url, version: nv.derniere }, e.target);
    e.target.hidden = true;
    suivreNvidia();
  });
  // Un téléchargement lancé avant de quitter la page continue : on le retrouve
  const t = await invoke('pilote_nvidia_progression');
  if (t && t.version === nv.derniere && $('#btn-nvidia')) {
    $('#btn-nvidia').hidden = true;
    suivreNvidia();
  }
}

function suivreNvidia() {
  clearInterval(suiviNvidia);
  const bloc = $('#nvidia-progression');
  bloc.hidden = false;
  const maj = async () => {
    const t = await invoke('pilote_nvidia_progression');
    if (!t || !document.body.contains(bloc)) return clearInterval(suiviNvidia);
    const pct = t.total ? Math.min(100, (t.recu / t.total) * 100) : 0;
    bloc.querySelector('.barre div').style.width = `${t.fini && !t.erreur ? 100 : pct}%`;
    const texte = bloc.querySelector('span');
    if (t.erreur) {
      clearInterval(suiviNvidia);
      texte.textContent = t.erreur;
      texte.className = 'alerte';
    } else if (t.fini) {
      clearInterval(suiviNvidia);
      texte.innerHTML = '';
      const b = document.createElement('button');
      b.className = 'principal';
      b.textContent = "Lancer l'installation";
      b.onclick = () => appel('pilote_nvidia_installer', {}, b).then(() => toast("Installateur NVIDIA ouvert. L'écran peut clignoter pendant l'installation."));
      texte.append(b);
    } else {
      texte.textContent = `${Mo(t.recu)}${t.total ? ` sur ${Mo(t.total)}` : ''}`;
    }
  };
  maj();
  suiviNvidia = setInterval(maj, 1000);
}

$('#page-pilotes').addEventListener('click', (e) => {
  const lien = e.target.closest('[data-lien]')?.dataset.lien;
  if (lien) appel('ouvrir_lien', { lien: LIENS[lien] });
});

async function rechercherWU() {
  const b = $('#btn-wu');
  b.disabled = true;
  b.textContent = 'Recherche en cours…';
  $('#wu-resultats').innerHTML = '';
  try {
    majWU = await appel('pilotes_wu_rechercher');
    $('#wu-resultats').innerHTML = majWU.length
      ? `<ul class="liste-wu">${majWU
          .map((m) => `<li><label><input type="checkbox" value="${esc(m.id)}" checked><span>${esc(m.titre)}<small>${esc(m.fabricant || '')}${m.date ? ` · ${new Date(m.date).toLocaleDateString('fr-CH')}` : ''}${m.taille ? ` · ${Mo(m.taille)}` : ''}</small></span></label></li>`)
          .join('')}</ul><div class="boutons"><button class="principal" id="btn-wu-installer">Installer la sélection</button></div>`
      : '<p class="ok">Aucune mise à jour de pilote proposée : tout est à jour.</p>';
    afficherPeripheriques();
  } finally {
    b.disabled = false;
    b.textContent = 'Relancer la recherche';
  }
}
$('#btn-wu').addEventListener('click', rechercherWU);

$('#wu-resultats').addEventListener('click', async (e) => {
  if (e.target.id !== 'btn-wu-installer') return;
  const ids = [...document.querySelectorAll('#wu-resultats input:checked')].map((c) => c.value);
  if (!ids.length) return;
  const b = e.target;
  b.disabled = true;
  b.textContent = 'Téléchargement et installation…';
  try {
    const r = await appel('pilotes_wu_installer', { ids });
    const ok = r.details.filter((d) => d.ok).length;
    $('#wu-resultats').innerHTML = `<ul class="liste-wu">${r.details.map((d) => `<li class="${d.ok ? 'ok' : 'alerte'}">${d.ok ? '✓' : '✗'} ${esc(d.titre)}</li>`).join('')}</ul>${
      r.redemarrage ? '<p class="alerte">Redémarre ton PC pour terminer l\'installation.</p>' : ''
    }`;
    toast(`${ok} pilote(s) installé(s) sur ${r.details.length}.`, 6000);
    majWU = null;
    chargerPilotes();
  } catch {
    b.disabled = false;
    b.textContent = 'Installer la sélection';
  }
});

/* ---------- Statistiques ---------- */

const moyenne = (l, f) => l.reduce((s, x) => s + f(x), 0) / l.length;
const duree = (s) => (s >= 3600 ? `${Math.floor(s / 3600)} h ${String(Math.floor((s % 3600) / 60)).padStart(2, '0')}` : `${Math.max(1, Math.round(s / 60))} min`);

async function chargerSessions() {
  const sessions = (await appel('sessions')).reverse();
  $('#sessions tbody').innerHTML = sessions.length
    ? sessions
        .map(
          (s) => `<tr><td>${new Date(s.debut * 1000).toLocaleString('fr-CH', { dateStyle: 'short', timeStyle: 'short' })}</td><td>${esc(s.jeu)}</td><td>${duree(s.duree_s)}</td><td>${s.boost ? '<span class="badge accent">Oui</span>' : '<span class="badge">Non</span>'}</td><td>${Math.round(s.moyenne)}</td><td>${Math.round(s.low1)}</td><td>${Math.round(s.low01)}</td></tr>`
        )
        .join('')
    : '<tr><td colspan="7" class="discret">Aucune partie mesurée pour l\'instant.</td></tr>';

  // Comparaison avec / sans boost, par jeu (parties de plus de 2 minutes)
  const parJeu = {};
  for (const s of sessions.filter((s) => s.duree_s >= 120)) (parJeu[s.jeu] ??= []).push(s);
  $('#comparaisons').innerHTML = Object.entries(parJeu)
    .map(([jeu, l]) => {
      const avec = l.filter((s) => s.boost);
      const sans = l.filter((s) => !s.boost);
      const bloc = (titre, liste) =>
        liste.length
          ? `<div><div class="discret">${titre} (${liste.length})</div><div class="chiffres"><div class="chiffre">${Math.round(moyenne(liste, (s) => s.moyenne))}<small>FPS moyen</small></div><div class="chiffre">${Math.round(moyenne(liste, (s) => s.low1))}<small>1 % low</small></div></div></div>`
          : `<div><div class="discret">${titre}</div><p class="discret">Pas encore de partie.</p></div>`;
      let ecart = '';
      if (avec.length && sans.length) {
        const g = (moyenne(avec, (s) => s.moyenne) / moyenne(sans, (s) => s.moyenne) - 1) * 100;
        ecart = `<p class="${g >= 0 ? 'gain' : 'perte'}">${g >= 0 ? '+' : ''}${g.toFixed(1)} % de FPS moyen avec le boost</p>`;
      }
      return `<div class="carte compar"><h2>${esc(jeu)}</h2>${ecart}<div class="grille-2">${bloc('Avec boost', avec)}${bloc('Sans boost', sans)}</div></div>`;
    })
    .join('');
}

/* ---------- Réglages et mises à jour ---------- */

$('#demarrage-auto').addEventListener('change', async (e) => {
  try {
    await appel('demarrage_auto_set', { actif: e.target.checked });
    toast(e.target.checked ? 'FPS Cagoule se lancera avec Windows.' : 'FPS Cagoule ne se lancera plus avec Windows.');
  } catch {
    e.target.checked = !e.target.checked;
  }
});

$('#btn-maj').addEventListener('click', async (e) => {
  const r = await appel('maj_verifier', {}, e.target);
  $('#maj-texte').textContent = r.disponible ? `Version ${r.disponible} disponible (tu as la ${r.actuelle}).` : `Tu as la dernière version (${r.actuelle}).`;
  $('#btn-maj-installer').hidden = !r.disponible;
});
$('#btn-maj-installer').addEventListener('click', async (e) => {
  toast("Téléchargement de la mise à jour… L'app va redémarrer.", 10000);
  await appel('maj_installer', {}, e.target);
});

/* ---------- Consommation de l'app ---------- */

async function majConso() {
  if (document.hidden) return;
  try {
    const c = await invoke('conso');
    const total = c.app_mo + c.interface_mo + c.presentmon_mo;
    $('#conso').textContent = `${total.toFixed(0)} Mo · ${c.cpu.toFixed(1)} % CPU`;
    $('#conso').title = `Moteur : ${c.app_mo.toFixed(1)} Mo · Interface : ${c.interface_mo.toFixed(0)} Mo${c.presentmon_mo ? ` · Compteur FPS : ${c.presentmon_mo.toFixed(1)} Mo` : ''}`;
  } catch {}
}

/* ---------- Démarrage ---------- */

async function init() {
  etat = await appel('etat_app');
  $('#version').textContent = `Version ${etat.version}`;
  $('#maj-texte').textContent = `Version installée : ${etat.version}.`;
  afficherJeu(etat.jeu);
  [profils, reglages] = await Promise.all([appel('profils_get'), appel('reglages_get')]);
  afficherProfils();
  afficherReglages();
  invoke('demarrage_auto_get').then((v) => ($('#demarrage-auto').checked = v));
  try {
    derniereAnalyse = JSON.parse(localStorage.getItem('analyse'));
  } catch {}
  if (derniereAnalyse) afficherScore();
  else analyser();
  invoke('demarrage_liste').then((d) => {
    demarrage = d;
    afficherScore();
  });
  invoke('tweaks_liste').then((t) => {
    tweaks = t;
    afficherScore();
  });
  majConso();
  setInterval(majConso, 3000);
}

init();
