import { evaluer } from './evaluer.js';
import { testerGpu, preparerStabilite, references, testerStabilite } from './benchmark-gpu.js';

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
    if (b.dataset.page === 'optimisations') {
      chargerTweaks();
      chargerDemarrage();
    }
    if (b.dataset.page === 'analyse') {
      chargerCartes();
      chargerMemoire();
    }
    if (b.dataset.page === 'benchmark') chargerBenchmarks();
    if (b.dataset.page === 'oc') {
      chargerOc();
      chargerProcesseur();
    }
    if (b.dataset.page === 'nettoyage') {
      if (!nettoyage) analyserNettoyage();
      chargerProgrammes();
      chargerApplis();
    }
    if (b.dataset.page === 'ecrans') chargerEcrans();
    if (b.dataset.page === 'pilotes') chargerPilotes();
    if (b.dataset.page === 'fps') chargerSessions();
    if (b.dataset.page === 'jeux') {
      chargerChoixFermer();
      chargerModesJeu();
      chargerConfigsJeux();
    }
  })
);

function allerA(page) {
  document.querySelector(`nav [data-page="${page}"]`).click();
}

/* ---------- Analyse et score ---------- */

function memoriserAnalyse(info) {
  derniereAnalyse = info;
  try { localStorage.setItem('analyse', JSON.stringify(info)); } catch {}
}

let extraAnalyse = {};
let scoreAffiche = 0;
let analyseEnCours = false;
let nettoyageAnalyse = null;
let jeuxAnalyse = null;

/** Ce que l'analyse complète ajoute au score (voir evaluer.js) */
function calculerExtra() {
  const ex = { ...extraAnalyse };
  if (tweaks.length) {
    ex.recommandeesInactives = tweaks.filter((t) => t.disponible && !t.actif && !t.attente && t.niveau === 'recommande').length;
    ex.attente = tweaks.filter((t) => t.attente).length;
  }
  if (nettoyageAnalyse) ex.nettoyable = nettoyageAnalyse.filter((c) => c.defaut).reduce((t, c) => t + c.octets, 0);
  if (applis.length || nettoyageAnalyse) ex.applis = applis.filter((a) => a.defaut).length;
  if (jeuxAnalyse) ex.jeux = jeuxAnalyse.filter((j) => j.fichiers && !j.optimise && j.reglages.length).map((j) => j.nom);
  extraAnalyse = ex;
  try {
    localStorage.setItem('extra', JSON.stringify(ex));
  } catch {}
  return ex;
}

function animerScore(cible) {
  const depart = scoreAffiche;
  const debut = performance.now();
  const duree = 900;
  const etape = () => {
    const p = Math.max(0, Math.min(1, (performance.now() - debut) / duree));
    const v = Math.round(depart + (cible - depart) * (1 - Math.pow(1 - p, 3)));
    $('#score').textContent = v;
    if (p < 1) requestAnimationFrame(etape);
  };
  requestAnimationFrame(etape);
  scoreAffiche = cible;
}

const BOUTONS_CORRECTIF = {
  nettoyer: ['Nettoyer', 'nettoyer'],
  nettoyage: ['Nettoyer', 'nettoyer'],
  optimiser: ['Optimiser', 'optimiser'],
  msconfig: ['Retirer la limite', 'msconfig'],
  redemarrer: ['Redémarrer', 'redemarrer'],
  pilotes: ['Voir', 'pilotes'],
  demarrage: ['Gérer', 'demarrage'],
  applis: ['Voir', 'applis'],
  jeux: ['Voir', 'jeux'],
  memoire: ['Voir', 'memoire'],
};

function afficherScore() {
  if (!derniereAnalyse) return;
  const info = demarrage ? { ...derniereAnalyse, demarrage: demarrage.filter((p) => p.actif) } : derniereAnalyse;
  const { score, problemes } = evaluer(info, { planPerso: [etat.plan_perso, etat.plan_max], extra: calculerExtra() });
  animerScore(score);
  const anneau = $('#anneau');
  anneau.style.strokeDashoffset = 326.7 * (1 - score / 100);
  anneau.style.stroke = score >= 80 ? 'var(--accent-clair)' : score >= 55 ? 'var(--orange)' : 'var(--rouge)';
  $('#score-texte').textContent =
    score >= 90 ? 'Ton PC est au top pour jouer.' : score >= 70 ? 'Quelques réglages peuvent encore te faire gagner des FPS.' : 'Plusieurs points freinent ton PC en jeu.';
  if (derniereAnalyse.date) {
    const min = Math.round((Date.now() - new Date(derniereAnalyse.date)) / 60000);
    $('#date-derniere').textContent = `Dernière analyse : ${min < 1 ? "à l'instant" : min < 60 ? `il y a ${min} min` : min < 1440 ? `il y a ${Math.round(min / 60)} h` : new Date(derniereAnalyse.date).toLocaleDateString('fr-CH')}`;
  }

  const ids = new Set(tweaks.map((t) => t.id));
  $('#problemes').innerHTML = problemes.length
    ? problemes
        .map((p, i) => {
          let bouton = '';
          const b = BOUTONS_CORRECTIF[p.correctif];
          if (b) bouton = `<button class="petit ${['nettoyer', 'optimiser', 'msconfig', 'redemarrer'].includes(b[1]) ? 'principal' : ''}" data-action="${b[1]}">${b[0]}</button>`;
          else if (p.correctif && (ids.has(p.correctif) || !tweaks.length)) bouton = `<button class="petit principal" data-tweak="${esc(p.correctif)}">Corriger</button>`;
          return `<li class="probleme" style="animation-delay:${i * 40}ms"><span class="point ${p.gravite}"></span><div><strong>${esc(p.titre)}</strong><p>${esc(p.detail)}</p></div>${bouton}</li>`;
        })
        .join('')
    : '<li class="ok">Aucun problème détecté : ton PC est au top. 🎯</li>';
  $('#btn-tout-corriger').hidden = !correctifsAuto(problemes).length;
  afficherMateriel();
}

/** Problèmes que l'app peut corriger seule, sans choix à faire */
function correctifsAuto(problemes) {
  const ids = new Set(tweaks.map((t) => t.id));
  return problemes.filter((p) => ['optimiser', 'nettoyer', 'nettoyage', 'msconfig'].includes(p.correctif) || ids.has(p.correctif));
}

async function nettoyerRapide(bouton) {
  const ids = (nettoyageAnalyse || []).filter((c) => c.defaut && c.octets).map((c) => c.id);
  if (!ids.length) return 0;
  const bilan = await appel('nettoyage_nettoyer', { ids }, bouton);
  return bilan.reduce((t, b) => t + b.octets, 0);
}

async function reparerMsconfig(bouton) {
  const r = await appel('reparer_msconfig', {}, bouton);
  if (r.length) toast('Limite msconfig retirée. Redémarre ton PC pour récupérer tous tes cœurs et ta mémoire.', 8000);
}

$('#problemes').addEventListener('click', async (e) => {
  const b = e.target.closest('button');
  if (!b) return;
  const action = b.dataset.action;
  try {
    if (action === 'nettoyer') {
      const o = await nettoyerRapide(b);
      toast(`${formatOctets(o)} libérés.`);
      nettoyageAnalyse = await invoke('nettoyage_analyser');
    } else if (action === 'optimiser') {
      await optimiserTout(false, b);
    } else if (action === 'msconfig') {
      if (!(await confirmer('Retirer la limite msconfig', "<p>L'option « Nombre de processeurs » ou « Mémoire maximale » de msconfig va être retirée : Windows utilisera de nouveau tout ton PC après un redémarrage. BitLocker est suspendu le temps de ce redémarrage.</p>", 'Retirer'))) return;
      await reparerMsconfig(b);
      memoriserAnalyse(await invoke('analyser'));
    } else if (action === 'redemarrer') {
      return $('#btn-redemarrer').click();
    } else if (action === 'demarrage') {
      allerA('optimisations');
      return choisirCategorie('demarrage');
    } else if (action === 'applis') return allerA('nettoyage');
    else if (action === 'jeux') return allerA('jeux');
    else if (action === 'memoire') {
      allerA('analyse');
      return setTimeout(() => $('#carte-memoire').scrollIntoView({ behavior: 'smooth' }), 150);
    }
    else if (action === 'pilotes') return allerA('pilotes');
    else if (b.dataset.tweak) {
      await appliquerTweaks([b.dataset.tweak], b);
      memoriserAnalyse(await invoke('analyser'));
    }
  } catch {}
  afficherScore();
});

$('#btn-tout-corriger').addEventListener('click', async (e) => {
  const info = demarrage ? { ...derniereAnalyse, demarrage: demarrage.filter((p) => p.actif) } : derniereAnalyse;
  const liste = correctifsAuto(evaluer(info, { planPerso: [etat.plan_perso, etat.plan_max], extra: extraAnalyse }).problemes);
  const ok = await confirmer(
    'Tout corriger',
    `<p>FPS Cagoule va corriger :</p><ul>${liste.map((p) => `<li>${esc(p.titre)}</li>`).join('')}</ul><p>Tout reste annulable depuis la page Optimisations (sauf les fichiers nettoyés).</p>`,
    'Tout corriger'
  );
  if (!ok) return;
  const bouton = e.target;
  bouton.classList.add('chargement');
  try {
    // Toutes les optimisations d'un coup (un seul point de restauration)
    const ids = new Set(liste.filter((p) => tweaks.some((t) => t.id === p.correctif)).map((p) => p.correctif));
    if (liste.some((p) => p.correctif === 'optimiser')) {
      tweaks.filter((t) => t.disponible && !t.actif && !t.attente && t.niveau === 'recommande').forEach((t) => ids.add(t.id));
    }
    if (ids.size) await appliquerTweaks([...ids]);
    if (liste.some((p) => ['nettoyer', 'nettoyage'].includes(p.correctif))) {
      const o = await nettoyerRapide();
      toast(`${formatOctets(o)} libérés.`);
    }
    if (liste.some((p) => p.correctif === 'msconfig')) await reparerMsconfig();
  } finally {
    bouton.classList.remove('chargement');
  }
  analyser();
});

const ETAPES = [
  ['analyser', 'Matériel et Windows'],
  ['tweaks_liste', 'Optimisations'],
  ['nettoyage_analyser', 'Fichiers à nettoyer'],
  ['demarrage_liste', 'Programmes au démarrage'],
  ['applis_windows', 'Applis préinstallées'],
  ['jeux_configs', 'Réglages des jeux'],
];

/** Analyse complète : toutes les vérifications en parallèle, chacune cochée quand elle finit */
async function analyser() {
  if (analyseEnCours) return;
  analyseEnCours = true;
  const b = $('#btn-analyser');
  const carte = $('#carte-score');
  b.classList.add('chargement');
  b.textContent = 'Analyse…';
  carte.classList.add('analyse');
  const etapes = $('#etapes-analyse');
  etapes.hidden = false;
  etapes.innerHTML = ETAPES.map(([, nom]) => `<li><span class="icone"></span>${nom}</li>`).join('');
  const lignes = etapes.querySelectorAll('li');
  try {
    const [info, t, n, d, a, j] = await Promise.all(
      ETAPES.map(([cmd], i) =>
        invoke(cmd)
          .then((r) => {
            lignes[i].classList.add('fait');
            return r;
          })
          .catch(() => {
            lignes[i].classList.add('echec');
            return null;
          })
      )
    );
    if (info) memoriserAnalyse(info);
    if (t) tweaks = t;
    if (d) demarrage = d;
    if (n) nettoyageAnalyse = n;
    if (a) applis = a;
    if (j) jeuxAnalyse = j;
    await new Promise((r) => setTimeout(r, 400));
  } finally {
    analyseEnCours = false;
    carte.classList.remove('analyse');
    etapes.hidden = true;
    b.classList.remove('chargement');
    b.textContent = 'Analyser mon PC';
  }
  afficherScore();
}
$('#btn-analyser').addEventListener('click', analyser);

const ligne = (k, v) => (v === null || v === undefined || v === '' || v === false ? '' : `<div class="ligne-info"><span>${esc(k)}</span><span>${esc(v)}</span></div>`);
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
  const annuler = t.annulable ? `<button class="petit" data-annuler="${esc(t.id)}">Annuler</button>` : '';
  const activer = `<button class="petit principal" data-activer="${esc(t.id)}">Activer</button>`;
  let droite;
  if (!t.disponible) droite = '<span class="badge">Non disponible sur ton PC</span>';
  else if (t.attente === 'activation') droite = `<span class="badge orange">Redémarre pour l'activer</span>${annuler}`;
  else if (t.attente === 'annulation') droite = `<span class="badge orange">Redémarre pour terminer l'annulation</span>${t.actif ? '' : activer}`;
  else if (t.actif) droite = `<span class="badge accent">Actif</span>${annuler}`;
  else droite = `${annuler}${activer}`;
  // « Redémarrage nécessaire » n'est qu'une information tant que le réglage n'est pas appliqué
  const redemarrage = t.redemarrage && !t.actif && !t.attente ? '<span class="badge">Redémarrage nécessaire</span>' : '';
  return `<li class="tweak" data-id="${esc(t.id)}">
      <div>
        <div class="titre">${esc(t.titre)} ${gainBadge[t.gain] || ''} ${t.niveau === 'max' ? '<span class="badge violet">Boost max</span>' : ''}${t.niveau === 'risque' ? '<span class="badge rouge">Risqué</span>' : ''}${t.niveau === 'choix' ? '<span class="badge">À choisir</span>' : ''} ${redemarrage}</div>
        <p>${esc(t.description)}</p>
      </div>
      <div class="droite">${droite}</div>
    </li>`;
}

const CATEGORIES_OPTI = [
  ['windows', 'Windows'],
  ['carte-graphique', 'Carte graphique'],
  ['alimentation', 'Alimentation'],
  ['arriere-plan', 'Arrière-plan'],
  ['reseau', 'Réseau'],
  ['peripheriques', 'Souris et clavier'],
  ['confort', 'Confort et vie privée'],
  ['demarrage', 'Démarrage'],
  ['risque', 'Zone risquée'],
];
let categorieChoisie = '';
try {
  categorieChoisie = localStorage.getItem('categorie') || '';
} catch {}

/** [actifs, total] d'une catégorie, null si pas encore chargée */
function compteCategorie(id) {
  if (id === 'demarrage') return demarrage ? [demarrage.filter((p) => p.actif).length, demarrage.length] : null;
  if (!tweaks.length) return null;
  const l = tweaks.filter((t) => t.categorie === id && t.disponible);
  return [l.filter((t) => t.actif).length, l.length];
}

function afficherCategories() {
  $('#filtres-categories').innerHTML = [['', 'Toutes'], ...CATEGORIES_OPTI]
    .map(([id, nom]) => {
      const c = id ? compteCategorie(id) : null;
      return `<button role="tab" data-cat="${id}" class="${categorieChoisie === id ? 'actif' : ''}" aria-selected="${categorieChoisie === id}">${nom}${c ? `<small>${c[0]}/${c[1]}</small>` : ''}</button>`;
    })
    .join('');
  document.querySelectorAll('.categorie').forEach((sec) => {
    sec.hidden = !!categorieChoisie && sec.dataset.categorie !== categorieChoisie;
    const c = compteCategorie(sec.dataset.categorie);
    const mot = sec.dataset.categorie === 'demarrage' ? 'lancé' : 'active';
    sec.querySelector('.compte').textContent = c ? `${c[0]} ${mot}${c[0] > 1 ? 's' : ''} sur ${c[1]}` : '';
  });
}

function choisirCategorie(id) {
  categorieChoisie = id;
  try {
    localStorage.setItem('categorie', id);
  } catch {}
  afficherCategories();
}

$('#filtres-categories').addEventListener('click', (e) => {
  const b = e.target.closest('button');
  if (b) choisirCategorie(b.dataset.cat);
});

async function chargerTweaks() {
  tweaks = await appel('tweaks_liste');
  document.querySelectorAll('.tweaks[data-categorie]').forEach((ul) => {
    ul.innerHTML = tweaks.filter((t) => t.categorie === ul.dataset.categorie).map(ligneTweak).join('');
  });
  const dispo = tweaks.filter((t) => t.disponible);
  const actifs = dispo.filter((t) => t.actif).length;
  $('#resume-tweaks').textContent = `${actifs} optimisation${actifs > 1 ? 's' : ''} active${actifs > 1 ? 's' : ''} sur ${dispo.length} disponibles sur ton PC.`;
  $('#btn-tout-annuler').disabled = !tweaks.some((t) => t.annulable);
  $('#btn-nvidia-reglages').hidden = !tweaks.some((t) => t.special === 'nvidia' && t.disponible);
  const enAttente = tweaks.filter((t) => t.attente).length;
  $('#banniere-redemarrage').hidden = !enAttente;
  $('#banniere-texte').textContent = `${enAttente} optimisation${enAttente > 1 ? 's attendent' : ' attend'} un redémarrage de ton PC pour prendre effet.`;
  afficherCategories();
  afficherScore();
}

$('#btn-redemarrer').addEventListener('click', async (e) => {
  const ok = await confirmer('Redémarrer maintenant', '<p>Ton PC va redémarrer dans 10 secondes. Enregistre ton travail en cours avant de continuer.</p>', 'Redémarrer');
  if (!ok) return;
  await appel('redemarrer_pc', {}, e.target);
  toast('Redémarrage dans 10 secondes…', 10000);
});

document.querySelectorAll('.tweaks[data-categorie]').forEach((ul) =>
  ul.addEventListener('click', async (e) => {
    const b = e.target.closest('button');
    if (b?.dataset.activer) {
      const t = tweaks.find((x) => x.id === b.dataset.activer);
      if (t?.niveau === 'risque') {
        const ok = await confirmer(
          t.titre,
          `<p>${esc(t.description)}</p><p class="alerte">Ce réglage réduit la sécurité ou modifie le démarrage de Windows. Tu pourras l'annuler ici à tout moment.</p>`,
          'Activer quand même'
        );
        if (!ok) return;
      }
      return appliquerTweaks([b.dataset.activer], b);
    }
    if (!b?.dataset.annuler) return;
    const [r] = await appel('tweaks_annuler', { ids: [b.dataset.annuler] }, b);
    toast(r.ok ? 'Réglage remis comme avant' : `Impossible d'annuler : ${r.erreur}`);
    chargerTweaks();
  })
);

async function appliquerTweaks(ids, bouton) {
  const lignes = () => ids.flatMap((id) => [...document.querySelectorAll(`.tweak[data-id="${CSS.escape(id)}"]`)]);
  lignes().forEach((l) => l.classList.add('en-cours'));
  let point, resultats;
  try {
    [point, resultats] = await appel('tweaks_appliquer', { ids, restauration: $('#restauration').checked }, bouton);
  } finally {
    lignes().forEach((l) => l.classList.remove('en-cours'));
  }
  const echecs = resultats.filter((r) => !r.ok);
  const redemarrage = tweaks.some((t) => ids.includes(t.id) && t.redemarrage);
  const reussis = resultats.length - echecs.length;
  let msg = echecs.length ? `${reussis} appliquée(s), ${echecs.length} échec(s) : ${echecs.map((r) => r.erreur).join(' ; ')}` : ids.length > 1 ? `${reussis} optimisations appliquées.` : 'Optimisation appliquée.';
  if (redemarrage && !echecs.length) msg += ' Redémarre ton PC pour terminer (bouton en haut de la page).';
  if (point) msg = `${point}. ${msg}`;
  toast(msg, 7000);
  await chargerTweaks();
  // Petite onde sur les lignes réussies
  resultats.filter((r) => r.ok).forEach((r) => document.querySelectorAll(`.tweak[data-id="${CSS.escape(r.id)}"]`).forEach((l) => l.classList.add('reussi')));
}

function confirmer(titre, html, libelle = 'Confirmer', { delai = 0, non = 'Annuler' } = {}) {
  return new Promise((resoudre) => {
    $('#fenetre-titre').textContent = titre;
    $('#fenetre-texte').innerHTML = html;
    $('#fenetre-oui').textContent = libelle;
    $('#fenetre-non').textContent = non;
    $('#fenetre').hidden = false;
    let reste = delai;
    const minuteur = delai
      ? setInterval(() => {
          reste--;
          $('#fenetre-non').textContent = `${non} (${reste})`;
          if (reste <= 0) fin(false);
        }, 1000)
      : null;
    if (delai) $('#fenetre-non').textContent = `${non} (${reste})`;
    function fin(r) {
      clearInterval(minuteur);
      $('#fenetre').hidden = true;
      resoudre(r);
    }
    $('#fenetre-oui').onclick = () => fin(true);
    $('#fenetre-non').onclick = () => fin(false);
  });
}

async function optimiserTout(max, bouton) {
  if (!tweaks.length) tweaks = await appel('tweaks_liste');
  // Les réglages risqués ne partent jamais en lot
  const a = tweaks.filter((t) => t.disponible && !t.actif && (max ? !['risque', 'choix'].includes(t.niveau) : t.niveau === 'recommande'));
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
$('#btn-nvidia-reglages').addEventListener('click', async (e) => {
  const a = tweaks.filter((t) => t.special === 'nvidia' && t.disponible && !t.actif && t.niveau !== 'choix');
  if (!a.length) return toast('Les réglages NVIDIA sont déjà appliqués.');
  const ok = await confirmer(
    'Réglages NVIDIA',
    `<ul>${a.map((t) => `<li>${esc(t.titre.replace(/^NVIDIA : /, ''))}</li>`).join('')}</ul><p>Appliqués aux paramètres globaux du panneau NVIDIA. Les jeux qui ont leur propre réglage dans le panneau le gardent. La V-Sync reste à choisir selon ton écran.</p>`,
    'Appliquer'
  );
  if (ok) appliquerTweaks(a.map((t) => t.id), e.target);
});
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

/* ---------- Nettoyage ---------- */

let nettoyage = null;
const formatOctets = (o) => (o >= 1073741824 ? `${(o / 1073741824).toFixed(1).replace('.', ',')} Go` : o >= 1048576 ? `${Math.round(o / 1048576)} Mo` : `${Math.max(0, Math.round(o / 1024))} Ko`);

async function analyserNettoyage() {
  const b = $('#btn-analyser-nettoyage');
  b.disabled = true;
  b.textContent = 'Analyse…';
  $('#btn-nettoyer').disabled = true;
  const coches = new Set([...document.querySelectorAll('#nettoyage-categories input:checked')].map((c) => c.value));
  try {
    nettoyage = await appel('nettoyage_analyser');
    $('#nettoyage-categories').innerHTML = nettoyage
      .map((c) => {
        const coche = coches.size ? coches.has(c.id) : c.defaut;
        return `<li><label>
          <input type="checkbox" value="${c.id}" ${coche ? 'checked' : ''}>
          <span><strong>${esc(c.titre)}</strong><small>${esc(c.description)}</small></span>
          <span class="taille ${c.octets ? '' : 'discret'}">${c.octets ? formatOctets(c.octets) : 'Rien à nettoyer'}</span>
        </label></li>`;
      })
      .join('');
    afficherTotalNettoyage();
  } finally {
    b.disabled = false;
    b.textContent = 'Réanalyser';
  }
}

function afficherTotalNettoyage() {
  const ids = new Set([...document.querySelectorAll('#nettoyage-categories input:checked')].map((c) => c.value));
  const choisies = (nettoyage || []).filter((c) => ids.has(c.id));
  const octets = choisies.reduce((t, c) => t + c.octets, 0);
  const fichiers = choisies.reduce((t, c) => t + c.fichiers, 0);
  $('#nettoyage-total').textContent = formatOctets(octets);
  $('#nettoyage-detail').textContent = `à libérer · ${fichiers.toLocaleString('fr-CH')} fichier${fichiers > 1 ? 's' : ''} dans ${choisies.length} catégorie${choisies.length > 1 ? 's' : ''} sélectionnée${choisies.length > 1 ? 's' : ''}`;
  $('#btn-nettoyer').disabled = !octets;
}

$('#nettoyage-categories').addEventListener('change', afficherTotalNettoyage);
$('#btn-analyser-nettoyage').addEventListener('click', analyserNettoyage);
$('#btn-nettoyer').addEventListener('click', async (e) => {
  const ids = [...document.querySelectorAll('#nettoyage-categories input:checked')].map((c) => c.value);
  e.target.textContent = 'Nettoyage…';
  try {
    const bilan = await appel('nettoyage_nettoyer', { ids }, e.target);
    const octets = bilan.reduce((t, b) => t + b.octets, 0);
    const fichiers = bilan.reduce((t, b) => t + b.fichiers, 0);
    toast(`${formatOctets(octets)} libérés (${fichiers.toLocaleString('fr-CH')} fichiers). Les fichiers en cours d'utilisation ont été laissés.`, 7000);
  } finally {
    e.target.textContent = 'Nettoyer la sélection';
  }
  analyserNettoyage();
});

/* ---------- Disques et applis préinstallées ---------- */

$('#btn-disques').addEventListener('click', async (e) => {
  e.target.textContent = 'Optimisation en cours…';
  try {
    const r = await appel('optimiser_disques', {}, e.target);
    $('#disques-resultat').innerHTML = r
      .map((d) => `<p class="${d.ok ? 'ok' : 'alerte'}">${esc(d.lecteur)} ${d.type === 'HDD' ? 'défragmenté' : 'TRIM fait'}${d.ok ? '' : ` : échec (${esc(d.erreur)})`}</p>`)
      .join('');
  } finally {
    e.target.textContent = 'Optimiser les disques';
  }
});

let applis = [];

async function chargerApplis() {
  applis = await appel('applis_windows').catch(() => []);
  $('#nb-applis').textContent = `(${applis.length})`;
  $('#applis-liste').innerHTML = applis.length
    ? applis
        .map(
          (a) => `<li><label><input type="checkbox" value="${esc(a.paquet)}" ${a.defaut ? 'checked' : ''}>
            <span>${esc(a.nom)}${a.description ? `<small>${esc(a.description)}</small>` : ''}</span></label></li>`
        )
        .join('')
    : '<li class="ok">Aucune appli préinstallée inutile : c\'est propre.</li>';
  majBoutonApplis();
}

function majBoutonApplis() {
  const n = document.querySelectorAll('#applis-liste input:checked').length;
  $('#btn-applis').disabled = !n;
  $('#btn-applis').textContent = n ? `Désinstaller la sélection (${n})` : 'Désinstaller la sélection';
}

$('#applis-liste').addEventListener('change', majBoutonApplis);
$('#btn-applis').addEventListener('click', async (e) => {
  const paquets = [...document.querySelectorAll('#applis-liste input:checked')].map((c) => c.value);
  const noms = applis.filter((a) => paquets.includes(a.paquet)).map((a) => a.nom);
  const ok = await confirmer(
    'Désinstaller les applis préinstallées',
    `<ul>${noms.map((n) => `<li>${esc(n)}</li>`).join('')}</ul><p>Elles se réinstallent depuis le Microsoft Store si tu changes d'avis.</p>`,
    'Désinstaller'
  );
  if (!ok) return;
  e.target.textContent = 'Désinstallation…';
  try {
    const r = await appel('applis_windows_supprimer', { paquets }, e.target);
    const echecs = r.filter((x) => !x.ok);
    toast(echecs.length ? `${r.length - echecs.length} désinstallée(s), ${echecs.length} échec(s) : ${echecs.map((x) => x.erreur).join(' ; ')}` : `${r.length} appli(s) désinstallée(s).`, 7000);
  } finally {
    chargerApplis();
  }
});

/* ---------- Désinstallation ---------- */

let programmes = [];

async function chargerProgrammes() {
  programmes = await appel('programmes_liste').catch(() => []);
  afficherProgrammes();
}

function afficherProgrammes() {
  const filtre = $('#filtre-programmes').value.trim().toLowerCase();
  const liste = programmes.filter((p) => !filtre || `${p.nom} ${p.editeur}`.toLowerCase().includes(filtre));
  $('#nb-programmes').textContent = `(${programmes.length})`;
  const date = (d) => (/^\d{8}$/.test(d) ? `${d.slice(6, 8)}.${d.slice(4, 6)}.${d.slice(0, 4)}` : '');
  $('#programmes-liste').innerHTML = liste.length
    ? liste
        .map((p) => {
          const details = [p.editeur, p.version, p.taille_ko ? formatOctets(p.taille_ko * 1024) : '', date(p.date)].filter(Boolean).map(esc).join(' · ');
          return `<li><div class="info"><strong>${esc(p.nom)}</strong><small>${details}</small></div><button class="petit" data-desinstaller="${esc(p.id)}">Désinstaller</button></li>`;
        })
        .join('')
    : '<li class="discret">Aucun programme ne correspond.</li>';
}

$('#filtre-programmes').addEventListener('input', afficherProgrammes);
$('#programmes-liste').addEventListener('click', async (e) => {
  const id = e.target.closest('button')?.dataset.desinstaller;
  if (!id) return;
  const p = programmes.find((x) => x.id === id);
  const ok = await confirmer(`Désinstaller ${p.nom}`, '<p>Le désinstalleur du programme va s\'ouvrir : suis ses étapes. La liste se mettra à jour ensuite.</p>', 'Désinstaller');
  if (!ok) return;
  await appel('programme_desinstaller', { id }, e.target);
  // Le désinstalleur tourne dans sa propre fenêtre : on rafraîchit la liste un peu plus tard
  setTimeout(chargerProgrammes, 20000);
});

/* ---------- Programmes au démarrage ---------- */

let demarrage = null;

async function chargerDemarrage() {
  demarrage = await appel('demarrage_liste');
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
  afficherCategories();
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

/* ---------- Réglages dans les jeux ---------- */

async function chargerConfigsJeux() {
  const jeux = await appel('jeux_configs').catch(() => []);
  const installes = jeux.filter((j) => j.fichiers);
  const absents = jeux.filter((j) => !j.fichiers);
  $('#jeux-configs').innerHTML =
    installes
      .map((j) => {
        const a_changer = j.reglages.filter((r) => r.actuel.toLowerCase() !== r.cible.toLowerCase());
        let etat;
        if (j.en_cours) etat = '<span class="badge orange">Jeu lancé : ferme-le d\'abord</span>';
        else if (j.optimise) etat = '<span class="badge accent">Optimisé</span>';
        else etat = `<span class="badge">${a_changer.length} réglage${a_changer.length > 1 ? 's' : ''} à changer</span>`;
        const boutons = [
          !j.optimise && !j.en_cours && j.reglages.length ? `<button class="petit principal" data-optimiser="${j.id}">Optimiser</button>` : '',
          j.sauvegarde && !j.en_cours ? `<button class="petit" data-restaurer="${j.id}">Restaurer</button>` : '',
        ].join('');
        const detail = j.reglages.length
          ? `<details><summary>Ce qui change</summary><ul>${j.reglages
              .map((r) => `<li>${esc(r.libelle)}${r.actuel.toLowerCase() === r.cible.toLowerCase() ? ' ✓' : ` <span class="discret">(${esc(r.actuel)} → ${esc(r.cible)})</span>`}</li>`)
              .join('')}</ul></details>`
          : '<p class="note">Aucun réglage connu dans ce fichier (version du jeu différente).</p>';
        return `<li><div class="jeu-config-entete"><strong>${esc(j.nom)}</strong>${etat}${boutons}</div>${detail}<p class="note">${esc(j.note)}${j.fichiers > 1 ? ` (${j.fichiers} comptes)` : ''}</p></li>`;
      })
      .join('') +
    (absents.length ? `<li class="discret">Pas trouvés sur ce PC (ou jamais lancés) : ${absents.map((j) => esc(j.nom)).join(', ')}.</li>` : '');
}

$('#jeux-configs').addEventListener('click', async (e) => {
  const b = e.target.closest('button');
  if (!b) return;
  try {
    if (b.dataset.optimiser) {
      const n = await appel('jeux_config_appliquer', { id: b.dataset.optimiser }, b);
      toast(n ? `${n} réglage${n > 1 ? 's' : ''} changé${n > 1 ? 's' : ''}. Lance le jeu pour en profiter.` : 'Déjà optimisé.');
    } else if (b.dataset.restaurer) {
      await appel('jeux_config_restaurer', { id: b.dataset.restaurer }, b);
      toast('Réglages du jeu remis comme avant.');
    }
  } catch {}
  chargerConfigsJeux();
});

let modesPrincipal = [];

async function chargerModesJeu() {
  const ecrans = await appel('ecrans_liste').catch(() => []);
  modesPrincipal = (ecrans.find((e) => e.principal) || ecrans[0])?.modes || [];
  afficherProfils();
}

/** Liste des résolutions pour un jeu : même format que l'écran = agrandie, autre format = étirée ou bandes noires */
function optionsResolution(p) {
  const r = p.resolution;
  const choisie = r ? `${r.largeur}x${r.hauteur}x${r.etirer ? 1 : 0}` : '';
  const natif = modesPrincipal[0];
  const formatNatif = natif ? formatEcran(natif.largeur, natif.hauteur) : '';
  const valeurs = [];
  const parFormat = {};
  for (const m of modesPrincipal) (parFormat[formatEcran(m.largeur, m.hauteur)] ??= []).push(m);
  let html = `<option value="">Celle du bureau</option>`;
  for (const [format, modes] of Object.entries(parFormat)) {
    html += `<optgroup label="${format}">`;
    for (const m of modes) {
      const base = `${m.largeur}x${m.hauteur}`;
      const variantes = format === formatNatif ? [[1, '']] : [[1, ' étirée'], [0, ' bandes noires']];
      for (const [e, nom] of variantes) {
        const v = `${base}x${e}`;
        valeurs.push(v);
        html += `<option value="${v}" ${v === choisie ? 'selected' : ''}>${m.largeur} × ${m.hauteur}${nom}</option>`;
      }
    }
    html += '</optgroup>';
  }
  // Résolution enregistrée mais absente de la liste (liste pas encore chargée, autre écran)
  if (choisie && !valeurs.includes(choisie)) html += `<option value="${choisie}" selected>${r.largeur} × ${r.hauteur}${r.etirer ? ' étirée' : ''}</option>`;
  return html;
}

function afficherProfils() {
  const caseP = (i, champ, titre) => `<td><input type="checkbox" data-i="${i}" data-champ="${champ}" ${profils[i][champ] ? 'checked' : ''} aria-label="${titre}"></td>`;
  $('#profils tbody').innerHTML = profils
    .map(
      (p, i) => `<tr>
        <td>${esc(p.nom)}<small>${esc(p.exes.join(', '))}</small></td>
        ${caseP(i, 'actif', 'Détection')}${caseP(i, 'boost', 'Boost')}${caseP(i, 'priorite_haute', 'Priorité haute')}${caseP(i, 'gpu_perf', 'GPU puissant')}
        <td><select data-i="${i}" data-resolution aria-label="Résolution pendant ${esc(p.nom)}">${optionsResolution(p)}</select></td>
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
  if (c.dataset.resolution !== undefined) {
    const [largeur, hauteur, etirer] = c.value.split('x').map(Number);
    profils[c.dataset.i].resolution = c.value ? { largeur, hauteur, etirer: etirer === 1 } : null;
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
  profils.push({ nom: $('#ajout-nom').value.trim(), exes: [exe], actif: true, boost: true, priorite_haute: true, gpu_perf: true, resolution: null });
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
let ecransListe = [];
const FORMATS = [['32:9', 32 / 9], ['21:9', 21 / 9], ['16:9', 16 / 9], ['16:10', 1.6], ['3:2', 1.5], ['4:3', 4 / 3], ['5:4', 1.25]];
const pgcd = (a, b) => (b ? pgcd(b, a % b) : a);
/** Format d'une résolution : 16:9, 4:3… (les 21:9 « réels » vont de 2,33 à 2,39) */
function formatEcran(l, h) {
  const r = l / h;
  const f = FORMATS.find(([, v]) => Math.abs(r - v) / v < 0.035);
  if (f) return f[0];
  const g = pgcd(l, h);
  return `${l / g}:${h / g}`;
}
const ECHELLES = [
  [0, 'Par défaut'],
  [1, 'Étirée (plein écran)'],
  [2, 'Bandes noires'],
];

/** Remplit les listes résolution / fréquence d'une carte d'écran pour le format choisi */
function remplirAffichage(carte, choix) {
  const e = ecransListe.find((x) => x.id === carte.dataset.id);
  const format = carte.dataset.format;
  const natif = e.modes[0];
  const modes = e.modes.filter((m) => !format || formatEcran(m.largeur, m.hauteur) === format);
  const actuel = choix || `${e.largeur}x${e.hauteur}`;
  const selRes = carte.querySelector('[data-res]');
  selRes.innerHTML = modes
    .map((m) => {
      const v = `${m.largeur}x${m.hauteur}`;
      return `<option value="${v}" ${v === actuel ? 'selected' : ''}>${m.largeur} × ${m.hauteur}${m === natif ? ' (native)' : ''}</option>`;
    })
    .join('');
  remplirFrequences(carte);
  carte.querySelectorAll('[data-format]').forEach((b) => b.classList.toggle('actif', b.dataset.format === format));
}

function remplirFrequences(carte) {
  const e = ecransListe.find((x) => x.id === carte.dataset.id);
  const [l, h] = carte.querySelector('[data-res]').value.split('x').map(Number);
  const m = e.modes.find((x) => x.largeur === l && x.hauteur === h);
  const freqs = m ? m.frequences : [e.hz];
  const voulue = l === e.largeur && h === e.hauteur ? e.hz : Math.max(...freqs);
  carte.querySelector('[data-freq]').innerHTML = freqs.map((f) => `<option ${f === voulue ? 'selected' : ''}>${f}</option>`).join('');
}
const NEUTRE = { luminosite: 0, contraste: 1, gamma: 1, vibrance: null };
const CURSEURS = [
  { champ: 'luminosite', titre: 'Luminosité', min: -0.3, max: 0.3, pas: 0.01, aff: (v) => `${v > 0 ? '+' : ''}${Math.round(v * 100)}` },
  { champ: 'contraste', titre: 'Contraste', min: 0.7, max: 1.3, pas: 0.01, aff: (v) => `${Math.round(v * 100)} %` },
  { champ: 'gamma', titre: 'Gamma', min: 0.7, max: 1.5, pas: 0.01, aff: (v) => v.toFixed(2) },
];

async function chargerEcrans() {
  const [ecrans, c] = await Promise.all([appel('ecrans_liste'), appel('couleurs_get')]);
  couleurs = c;
  ecransListe = ecrans;
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
            <p class="discret">Actuellement ${e.largeur} × ${e.hauteur} (${formatEcran(e.largeur, e.hauteur)}) à ${e.hz} Hz${e.hz < max ? ` · <span class="alerte">ton écran monte à ${max} Hz</span>` : ''}</p>
            <div class="formats">
              <button class="petit" data-format="">Tous</button>
              ${[...new Set(e.modes.map((m) => formatEcran(m.largeur, m.hauteur)))].map((f) => `<button class="petit" data-format="${f}">${f}</button>`).join('')}
            </div>
            <div class="reglages-affichage">
              <label>Résolution<select data-res></select></label>
              <label>Fréquence<select data-freq></select></label>
              <label>Mise à l'échelle<select data-echelle>${ECHELLES.map(([v, n]) => `<option value="${v}" ${v === e.mise_echelle ? 'selected' : ''}>${n}</option>`).join('')}</select></label>
              <button class="principal" data-action="mode">Appliquer</button>
            </div>
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
  document.querySelectorAll('#ecrans .ecran').forEach((carte) => {
    const e = ecransListe.find((x) => x.id === carte.dataset.id);
    carte.dataset.format = formatEcran(e.largeur, e.hauteur);
    remplirAffichage(carte);
  });
}

/** Essaie le mode choisi ; sans « Garder » sous 15 s, l'ancien revient (côté app ET côté interface) */
async function essayerAffichage(carte, bouton) {
  const e = ecransListe.find((x) => x.id === carte.dataset.id);
  const [largeur, hauteur] = carte.querySelector('[data-res]').value.split('x').map(Number);
  const hz = Number(carte.querySelector('[data-freq]').value);
  const echelle = Number(carte.querySelector('[data-echelle]').value);
  const params = { id: e.id, largeur, hauteur, hz, echelle: echelle === 0 && e.mise_echelle === 0 ? null : echelle };
  let avertissement;
  try {
    avertissement = await appel('ecran_mode_essayer', params, bouton);
  } catch {
    return;
  }
  const garder = await confirmer(
    'Garder cet affichage ?',
    `<p>${largeur} × ${hauteur} (${formatEcran(largeur, hauteur)}) à ${hz} Hz.</p>${avertissement ? `<p class="alerte">${esc(avertissement)}</p>` : ''}<p>Sans réponse, l'ancien affichage revient tout seul.</p>`,
    'Garder',
    { delai: 14, non: 'Revenir' }
  );
  if (garder) {
    await appel('ecran_mode_confirmer', { id: e.id }).then(() => toast('Affichage enregistré.')).catch(() => {});
  } else {
    await invoke('ecran_mode_annuler', { id: e.id });
    toast('Ancien affichage remis.');
  }
  chargerEcrans();
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

$('#ecrans').addEventListener('change', (e) => {
  if (e.target.matches('[data-res]')) remplirFrequences(e.target.closest('.ecran'));
});

$('#ecrans').addEventListener('click', async (e) => {
  const b = e.target.closest('button');
  const carte = b?.closest('.ecran');
  if (!carte) return;
  const id = carte.dataset.id;
  if (b.dataset.format !== undefined) {
    carte.dataset.format = b.dataset.format;
    return remplirAffichage(carte);
  }
  if (b.dataset.action === 'mode') return essayerAffichage(carte, b);
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

/* ---------- Benchmark ---------- */

let benchHistorique = [];
let benchEnCours = false;
const nf = (v, d = 0) => Number(v).toLocaleString('fr-CH', { maximumFractionDigits: d, minimumFractionDigits: d });
const TESTS_BENCH = [
  ['cpu_un', 'Processeur · 1 cœur'],
  ['cpu_tous', 'Processeur · tous les cœurs'],
  ['ram', 'Mémoire'],
  ['disque', 'Disque'],
  ['gpu', 'Carte graphique'],
];

function ecart(actuel, avant) {
  if (!avant) return '';
  const p = ((actuel - avant) / avant) * 100;
  if (Math.abs(p) < 1) return '<span class="ecart">=</span>';
  return `<span class="ecart ${p > 0 ? 'plus' : 'moins'}">${p > 0 ? '+' : ''}${nf(p)} %</span>`;
}

function afficherBenchmark() {
  const dernier = benchHistorique.at(-1);
  const avant = benchHistorique.at(-2);
  $('#btn-bench-copier').hidden = !dernier;
  if (!dernier) {
    $('#bench-resultats').innerHTML = '';
  } else {
    $('#bench-total').innerHTML = `${nf(dernier.total)}${ecart(dernier.total, avant?.total)}`;
    $('#bench-comparaison').textContent = avant
      ? `Comparé au test du ${new Date(avant.date * 1000).toLocaleString('fr-CH', { dateStyle: 'short', timeStyle: 'short' })} (${avant.optimisations_actives} optimisations actives, contre ${dernier.optimisations_actives} maintenant).`
      : 'Premier test : relance-le après tes optimisations pour comparer.';
    const d = dernier.details || {};
    const details = {
      cpu_un: d.cpu ? `${nf(d.cpu.un_coeur)} calculs/s` : '',
      cpu_tous: d.cpu ? `${d.cpu.coeurs} cœurs · ${nf(d.cpu.tous)} calculs/s` : '',
      ram: d.ram ? `${nf(d.ram.debit, 1)} Go/s · ${nf(d.ram.latence, 0)} ns` : '',
      disque: d.disque ? `Lecture ${nf(d.disque.lecture)} Mo/s · écriture ${nf(d.disque.ecriture)} Mo/s · 4 Ko ${nf(d.disque.aleatoire_4k, 1)} Mo/s` : (d.erreur_disque ? esc(d.erreur_disque) : ''),
      gpu: d.gpu ? `${esc(d.gpu.nom || '')} · ${nf(d.gpu.images_s)} images/s en 1080p` : (d.erreur_gpu ? esc(d.erreur_gpu) : ''),
    };
    $('#bench-resultats').innerHTML = TESTS_BENCH.map(
      ([cle, nom], i) => `<div class="carte" style="animation-delay:${i * 60}ms"><div class="discret">${nom}</div><div class="points">${nf(dernier[cle])}${ecart(dernier[cle], avant?.[cle])}</div><div class="detail">${details[cle]}</div></div>`
    ).join('');
  }
  $('#bench-historique tbody').innerHTML = benchHistorique.length
    ? [...benchHistorique]
        .reverse()
        .map(
          (b) => `<tr><td>${new Date(b.date * 1000).toLocaleString('fr-CH', { dateStyle: 'short', timeStyle: 'short' })}</td><td>${b.optimisations_actives}</td><td><strong>${nf(b.total)}</strong></td>${TESTS_BENCH.map(([c]) => `<td>${nf(b[c])}</td>`).join('')}</tr>`
        )
        .join('')
    : '<tr><td colspan="8" class="discret">Aucun test pour l\'instant.</td></tr>';
}

async function chargerBenchmarks() {
  benchHistorique = await appel('benchmarks').catch(() => []);
  afficherBenchmark();
}

async function lancerBenchmark() {
  if (benchEnCours) return;
  if (etat.jeu) return toast('Ferme ton jeu avant de lancer le benchmark.');
  benchEnCours = true;
  const bouton = $('#btn-benchmark');
  bouton.classList.add('chargement');
  bouton.textContent = 'Benchmark en cours…';
  $('#bench-en-cours').hidden = false;
  const barre = $('#bench-barre');
  // Progression estimée pour les tests faits par l'app (sans retour pendant le calcul)
  let minuteur;
  const etape = (n, nom, dureeEstimee) => {
    $('#bench-etape-nom').textContent = nom;
    $('#bench-etape-numero').textContent = `Test ${n} sur 4`;
    clearInterval(minuteur);
    barre.style.width = '0%';
    if (dureeEstimee) {
      const debut = performance.now();
      minuteur = setInterval(() => (barre.style.width = `${Math.min(97, ((performance.now() - debut) / dureeEstimee) * 100)}%`), 100);
    }
  };
  const details = {};
  try {
    etape(1, 'Processeur…', 10500);
    details.cpu = await invoke('benchmark_cpu', { duree: 5 });
    etape(2, 'Mémoire…', 5000);
    details.ram = await invoke('benchmark_ram');
    etape(3, 'Disque…', 8000);
    try {
      details.disque = await invoke('benchmark_disque');
    } catch (e) {
      details.erreur_disque = `Test du disque impossible : ${e}`;
    }
    etape(4, 'Carte graphique…', 0);
    const canvas = $('#bench-gpu');
    canvas.hidden = false;
    try {
      const g = await testerGpu(canvas, 10000, (f) => (barre.style.width = `${f * 100}%`));
      details.gpu = { nom: g.gpu, images_s: g.images_s, points: g.points };
    } catch (e) {
      details.erreur_gpu = `Test de la carte graphique impossible : ${e.message || e}`;
    }
    canvas.hidden = true;
    const t = await invoke('tweaks_liste').catch(() => tweaks);
    const r = {
      date: 0,
      optimisations_actives: t.filter((x) => x.actif).length,
      cpu_un: details.cpu.points_un,
      cpu_tous: details.cpu.points_tous,
      ram: details.ram.points,
      disque: details.disque?.points || 0,
      gpu: details.gpu?.points || 0,
    };
    const valeurs = TESTS_BENCH.map(([c]) => r[c]).filter((v) => v > 0);
    r.total = Math.round(valeurs.reduce((a, b) => a + b, 0) / valeurs.length);
    r.details = details;
    benchHistorique = await appel('benchmark_enregistrer', { resultat: r });
    afficherBenchmark();
    toast(`Benchmark terminé : ${nf(r.total)} points.`);
  } catch (e) {
    toast(`Benchmark interrompu : ${e}`, 7000);
  } finally {
    clearInterval(minuteur);
    $('#bench-en-cours').hidden = true;
    bouton.classList.remove('chargement');
    bouton.textContent = 'Relancer le benchmark';
    benchEnCours = false;
  }
}

$('#btn-benchmark').addEventListener('click', lancerBenchmark);
$('#btn-bench-copier').addEventListener('click', async () => {
  const b = benchHistorique.at(-1);
  if (!b) return;
  const texte = `FPS Cagoule : ${b.total} points (CPU 1 cœur ${b.cpu_un} · CPU multi ${b.cpu_tous} · mémoire ${b.ram} · disque ${b.disque} · GPU ${b.gpu}${b.details?.gpu?.nom ? ` · ${b.details.gpu.nom}` : ''})`;
  try {
    await navigator.clipboard.writeText(texte);
    toast('Résultat copié : colle-le à tes potes.');
  } catch {
    toast(texte, 10000);
  }
});

/* ---------- Mémoire par programme ---------- */

const NOMS_CONNUS = {
  'discord.exe': 'Discord', 'chrome.exe': 'Google Chrome', 'msedge.exe': 'Microsoft Edge', 'firefox.exe': 'Firefox',
  'opera.exe': 'Opera', 'brave.exe': 'Brave', 'medal.exe': 'Medal', 'steam.exe': 'Steam', 'steamwebhelper.exe': 'Steam (pages web)',
  'spotify.exe': 'Spotify', 'rainbowsix.exe': 'Rainbow Six Siege', 'rainbowsix_vulkan.exe': 'Rainbow Six Siege',
  'ubisoftconnect.exe': 'Ubisoft Connect', 'upc.exe': 'Ubisoft Connect', 'epicgameslauncher.exe': 'Epic Games',
  'memory compression': 'Compression mémoire de Windows', 'explorer.exe': 'Explorateur Windows', 'msmpeng.exe': 'Antivirus Defender',
  'nvcontainer.exe': 'NVIDIA', 'obs64.exe': 'OBS', 'teams.exe': 'Teams', 'ms-teams.exe': 'Teams',
};

async function chargerMemoire() {
  const m = await appel('memoire_programmes').catch(() => null);
  if (!m) return;
  const occupe = Math.round((1 - m.libre_mo / m.totale_mo) * 100);
  const barre = $('#ram-barre');
  barre.style.width = `${occupe}%`;
  barre.style.background = occupe >= 90 ? 'var(--rouge)' : occupe >= 80 ? 'var(--orange)' : 'var(--accent)';
  $('#ram-texte').textContent = `${nf((m.totale_mo - m.libre_mo) / 1024, 1)} Go utilisés sur ${nf(m.totale_mo / 1024, 1)} Go (${occupe} %)`;
  const max = m.programmes[0]?.mo || 1;
  $('#ram-liste').innerHTML = m.programmes
    .map((p) => {
      const nom = NOMS_CONNUS[p.nom.toLowerCase()] || p.nom.replace(/\.exe$/i, '');
      return `<li><span>${esc(nom)}${p.processus > 1 ? ` <span class="discret">(${p.processus})</span>` : ''}</span>
        <span class="mini-barre"><div style="width:${(p.mo / max) * 100}%"></div></span>
        <span class="mo">${p.mo >= 1024 ? `${nf(p.mo / 1024, 1)} Go` : `${nf(p.mo)} Mo`}</span>
        ${p.protege ? '<span class="discret petit-texte">Windows</span>' : `<button class="petit discret-btn" data-fermer="${esc(p.nom)}" title="Fermer ${esc(nom)}">Fermer</button>`}</li>`;
    })
    .join('');
}

$('#btn-memoire').addEventListener('click', chargerMemoire);
$('#ram-liste').addEventListener('click', async (e) => {
  const nom = e.target.closest('button')?.dataset.fermer;
  if (!nom) return;
  const lisible = NOMS_CONNUS[nom.toLowerCase()] || nom;
  if (!(await confirmer(`Fermer ${lisible}`, `<p>Toutes les fenêtres de ${esc(lisible)} vont être fermées. Enregistre ce qui doit l'être avant.</p>`, 'Fermer'))) return;
  try {
    const n = await appel('fermer_programme', { nom }, e.target);
    toast(`${lisible} fermé (${n} processus).`);
  } catch {}
  setTimeout(chargerMemoire, 800);
});

/* ---------- Processeur ---------- */

const LIENS_CPU = {
  amd: 'https://www.amd.com/fr/products/software/ryzen-master.html',
  intel: 'https://www.intel.com/content/www/us/en/download/17881/intel-extreme-tuning-utility-intel-xtu.html',
};

async function chargerProcesseur() {
  const p = await appel('processeur').catch(() => null);
  if (!p) return;
  const nom = p.nom || 'Processeur';
  $('#cpu-nom').textContent = nom;
  const amd = /ryzen/i.test(nom);
  const x3d = /x3d/i.test(nom);
  const serie9000x3d = /9\d{3}x3d/i.test(nom);
  const intelK = /intel/i.test(nom) && /\d{4,5}K/i.test(nom);
  const intel1314 = /intel/i.test(nom) && /i[579]-1[34]\d{3}/i.test(nom);
  let badge = '';
  let html = '';
  let boutons = '';
  if (amd) {
    badge = x3d && !serie9000x3d ? 'Undervolt seulement' : 'Overclockable';
    html = `<p>Le réglage qui marche le mieux sur un Ryzen, c'est le <strong>Curve Optimizer</strong> : il baisse la tension à chaque fréquence. Le processeur chauffe moins et monte plus haut tout seul, donc plus de FPS sans risque pour la puce.${
      x3d && !serie9000x3d ? ' Ton processeur X3D ne se surcadence pas autrement (protection du cache 3D) : le Curve Optimizer est la bonne méthode.' : ''
    }</p>
      <ol>
        <li>Ouvre <strong>AMD Ryzen Master</strong> (l'outil officiel d'AMD), mode <strong>Curve Optimizer</strong>, et lance son réglage automatique « Per Core ». Ou, dans le BIOS : PBO → Curve Optimizer → All Cores → Negative → 15.</li>
        <li>Lance le <strong>Benchmark</strong> de FPS Cagoule avant et après pour voir le gain.</li>
        <li>Joue une heure : si le PC plante ou redémarre, baisse la valeur (par exemple de 15 à 10).</li>
      </ol>`;
    boutons = p.ryzen_master ? '<button class="principal" data-cpu="ryzen">Ouvrir AMD Ryzen Master</button>' : '<button class="principal" data-cpu-lien="amd">Télécharger AMD Ryzen Master</button>';
  } else if (intelK) {
    badge = 'Overclockable';
    html = `<p>Ton processeur Intel « K » peut être réglé avec <strong>Intel XTU</strong> (outil officiel) ou dans le BIOS. Le plus utile est souvent un léger <strong>undervolt</strong> : moins de chaleur, des fréquences plus stables en jeu.${
      intel1314 ? ' Sur les 13e et 14e génération, mets d\'abord ton BIOS à jour (correctif Intel contre la dégradation) et n\'augmente jamais la tension.' : ''
    }</p>`;
    boutons = '<button class="principal" data-cpu-lien="intel">Télécharger Intel XTU</button>';
  } else {
    badge = 'Non overclockable';
    html = `<p>Ce processeur n'est pas fait pour être surcadencé. Les optimisations de FPS Cagoule (alimentation, cœurs éveillés, priorité des jeux) sont le meilleur moyen de le garder à pleine vitesse.</p>`;
  }
  $('#cpu-badge').textContent = badge;
  $('#cpu-conseils').innerHTML = html + '<p class="petit-texte">FPS Cagoule ne règle pas le processeur lui-même : il faudrait un pilote système que Windows bloque et que les anti-triches (Vanguard, FACEIT) détectent. Les outils officiels d\'AMD et Intel utilisent leur propre pilote signé.</p>';
  $('#cpu-boutons').innerHTML = boutons + '<button data-cpu="benchmark">Ouvrir le benchmark</button>';
}

$('#cpu-boutons').addEventListener('click', (e) => {
  const b = e.target.closest('button');
  if (!b) return;
  if (b.dataset.cpu === 'ryzen') appel('ouvrir_ryzen_master', {}, b).catch(() => {});
  else if (b.dataset.cpu === 'benchmark') allerA('benchmark');
  else if (b.dataset.cpuLien) appel('ouvrir_lien', { lien: LIENS_CPU[b.dataset.cpuLien] });
});

/* ---------- Overclocking ---------- */

let oc = null;
let ocOccupe = false;

async function chargerOc() {
  try {
    oc = await invoke('oc_etat');
  } catch (e) {
    $('#oc-contenu').hidden = true;
    $('#oc-indisponible').hidden = false;
    $('#oc-indisponible-texte').textContent = `${e}. L'overclocking n'est proposé que pour les cartes NVIDIA.`;
    return;
  }
  $('#oc-indisponible').hidden = true;
  $('#oc-contenu').hidden = false;
  $('#oc-plantage').hidden = !oc.coupe_apres_plantage;
  $('#oc-gpu').textContent = oc.gpu;
  const [cmin, cmax, mmin, mmax] = oc.bornes;
  const regler = (id, plage, min, max, valeur) => {
    const i = $(id);
    i.disabled = !plage?.modifiable;
    i.min = Math.max(min, plage?.min ?? 0);
    i.max = Math.min(max, plage?.max ?? 0);
    i.value = valeur;
  };
  const e = oc.enregistre;
  regler('#oc-coeur', oc.coeur, cmin, cmax, e?.coeur_mhz ?? oc.coeur?.actuel ?? 0);
  regler('#oc-memoire', oc.memoire, mmin, mmax, e?.memoire_mhz ?? oc.memoire?.actuel ?? 0);
  $('#oc-puissance-ligne').hidden = !oc.puissance;
  if (oc.puissance) {
    const p = $('#oc-puissance');
    p.min = Math.round(oc.puissance.min);
    p.max = Math.round(oc.puissance.max);
    p.value = Math.round(e?.puissance_pct ?? oc.puissance.actuel);
  }
  $('#oc-demarrage').checked = !!e?.au_demarrage;
  majCurseursOc();
  const actif = (oc.coeur?.actuel || 0) !== 0 || (oc.memoire?.actuel || 0) !== 0;
  $('#oc-resume').textContent = actif ? `Actif : GPU ${signe(oc.coeur?.actuel)} MHz, mémoire ${signe(oc.memoire?.actuel)} MHz` : 'Fréquences d\'origine';
  $('#oc-resume').className = `badge ${actif ? 'accent' : ''}`;
}

const signe = (v) => (v > 0 ? `+${v}` : `${v ?? 0}`);

function majCurseursOc() {
  $('#oc-coeur-v').textContent = `${signe(Number($('#oc-coeur').value))} MHz`;
  $('#oc-memoire-v').textContent = `${signe(Number($('#oc-memoire').value))} MHz`;
  $('#oc-puissance-v').textContent = `${$('#oc-puissance').value} %`;
}
['#oc-coeur', '#oc-memoire', '#oc-puissance'].forEach((id) => $(id).addEventListener('input', majCurseursOc));

const valeursOc = () => ({
  coeur: Number($('#oc-coeur').value),
  memoire: Number($('#oc-memoire').value),
  puissance: oc?.puissance ? Number($('#oc-puissance').value) : null,
});

function journal(texte, classe = '') {
  const li = document.createElement('li');
  li.textContent = texte;
  li.className = classe;
  $('#oc-journal').append(li);
  li.scrollIntoView({ block: 'nearest' });
}

/** Scène de test : un nouveau canvas à chaque fois (après un plantage du pilote, l'ancien est perdu) */
function nouvelleScene() {
  const ancien = $('#oc-canvas');
  const canvas = ancien.cloneNode();
  ancien.replaceWith(canvas);
  canvas.hidden = false;
  return preparerStabilite(canvas);
}

async function avecOccupation(bouton, f) {
  if (ocOccupe) return;
  ocOccupe = true;
  bouton.classList.add('chargement');
  $('#oc-barre-zone').hidden = false;
  try {
    await f();
  } catch (e) {
    toast(`Overclocking : ${e}`, 8000);
  } finally {
    ocOccupe = false;
    bouton.classList.remove('chargement');
    $('#oc-barre-zone').hidden = true;
    $('#oc-canvas').hidden = true;
    chargerOc();
  }
}

const barreOc = (f) => ($('#oc-barre').style.width = `${f * 100}%`);

/** Référence aux fréquences d'origine, puis test des fréquences voulues */
async function testerReglage(coeur, memoire, puissance, duree) {
  await invoke('oc_essayer', { coeur: 0, memoire: 0, puissance: null });
  let scene = nouvelleScene();
  const ref = references(scene);
  await invoke('oc_essayer', { coeur, memoire, puissance });
  return testerStabilite(scene, ref, duree, barreOc);
}

$('#btn-oc-tester').addEventListener('click', (e) =>
  avecOccupation(e.target, async () => {
    const v = valeursOc();
    const r = await testerReglage(v.coeur, v.memoire, v.puissance, 30000);
    if (r.ok) toast(`Stable pendant 30 s (${Math.round(r.images_s)} images/s). Clique sur « Appliquer » pour le garder.`, 8000);
    else {
      await invoke('oc_essayer', { coeur: 0, memoire: 0, puissance: null });
      toast(`Pas stable : ${r.raison}. Fréquences d'origine remises : baisse les curseurs.`, 9000);
    }
  })
);

$('#btn-oc-appliquer').addEventListener('click', (e) =>
  avecOccupation(e.target, async () => {
    const v = valeursOc();
    await appel('oc_enregistrer', { reglage: { coeur_mhz: v.coeur, memoire_mhz: v.memoire, puissance_pct: v.puissance, au_demarrage: $('#oc-demarrage').checked } });
    toast('Overclocking appliqué. Si un jeu plante, baisse les curseurs ou remets par défaut.', 7000);
  })
);

$('#btn-oc-defaut').addEventListener('click', (e) =>
  avecOccupation(e.target, async () => {
    await appel('oc_defaut');
    toast("Fréquences et puissance d'origine remises.");
  })
);

$('#oc-demarrage').addEventListener('change', async (e) => {
  if (!oc?.enregistre) return;
  await appel('oc_enregistrer', { reglage: { ...oc.enregistre, au_demarrage: e.target.checked } }).catch(() => {});
});

/** Mode automatique : monte par paliers tant que les images restent justes, puis garde une marge */
$('#btn-oc-auto').addEventListener('click', async (e) => {
  const ok = await confirmer(
    'Overclocking automatique',
    "<p>Environ 4 minutes. Ferme tes jeux et ne touche à rien : l'écran peut clignoter si le pilote graphique plante (il redémarre tout seul en quelques secondes).</p>",
    'Lancer'
  );
  if (!ok) return;
  $('#oc-journal').innerHTML = '';
  avecOccupation(e.target, async () => {
    const [, cmax, , mmax] = oc.bornes;
    const maxCoeur = Math.min(cmax, oc.coeur?.max ?? 0);
    const maxMem = Math.min(mmax, oc.memoire?.max ?? 0);
    const puissance = oc.puissance ? Math.round(oc.puissance.max) : null;
    await invoke('oc_essayer', { coeur: 0, memoire: 0, puissance: null });
    let scene = nouvelleScene();
    const ref = references(scene);
    const base = await testerStabilite(scene, ref, 6000, barreOc);
    journal(`Référence : ${Math.round(base.images_s)} images/s aux fréquences d'origine`);
    const essai = async (c, m, duree) => {
      await invoke('oc_essayer', { coeur: c, memoire: m, puissance });
      if (scene.perdu()) scene = nouvelleScene();
      return testerStabilite(scene, ref, duree, barreOc);
    };

    // 1. Fréquence du GPU
    let coeur = 0;
    if (oc.coeur?.modifiable) {
      for (let c = 50; c <= maxCoeur; c += 25) {
        const r = await essai(c, 0, 12000);
        if (!r.ok) {
          journal(`GPU +${c} MHz : ${r.raison}`, 'ko');
          break;
        }
        journal(`GPU +${c} MHz : stable (${Math.round(r.images_s)} images/s)`, 'ok');
        coeur = c;
      }
      coeur = Math.max(0, coeur - 25);
    }

    // 2. Mémoire : les erreurs y sont corrigées en ralentissant, on s'arrête quand les performances baissent
    let memoire = 0;
    if (oc.memoire?.modifiable) {
      let meilleur = 0;
      for (let m = 200; m <= maxMem; m += 200) {
        const r = await essai(coeur, m, 10000);
        if (!r.ok || r.images_s < meilleur * 0.99) {
          journal(`Mémoire +${m} MHz : ${r.ok ? 'plus lent (erreurs corrigées par la carte)' : r.raison}`, 'ko');
          break;
        }
        journal(`Mémoire +${m} MHz : stable (${Math.round(r.images_s)} images/s)`, 'ok');
        meilleur = Math.max(meilleur, r.images_s);
        memoire = m;
      }
      memoire = Math.max(0, memoire - 100);
    }

    // 3. Vérification finale plus longue, avec la marge de sécurité
    let final = await essai(coeur, memoire, 20000);
    if (!final.ok) {
      journal(`Vérification finale ratée (${final.raison}) : je baisse encore`, 'ko');
      coeur = Math.max(0, coeur - 50);
      memoire = Math.max(0, memoire - 200);
      final = await essai(coeur, memoire, 20000);
    }
    if (!final.ok || (coeur === 0 && memoire === 0)) {
      await invoke('oc_defaut');
      journal('Aucun overclocking stable trouvé : fréquences d\'origine remises', 'ko');
      return;
    }
    const gain = ((final.images_s / base.images_s - 1) * 100).toFixed(1).replace('.', ',');
    journal(`Résultat : GPU +${coeur} MHz, mémoire +${memoire} MHz, ${gain} % d'images en plus sur ce test`, 'ok');
    await appel('oc_enregistrer', { reglage: { coeur_mhz: coeur, memoire_mhz: memoire, puissance_pct: puissance, au_demarrage: $('#oc-demarrage').checked } });
    toast(`Overclocking trouvé et appliqué : GPU +${coeur} MHz, mémoire +${memoire} MHz.`, 8000);
  });
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
  const texte = $('#maj-texte');
  e.target.classList.add('chargement');
  try {
    const r = await invoke('maj_verifier');
    texte.className = 'discret';
    texte.textContent = r.disponible ? `Version ${r.disponible} disponible (tu as la ${r.actuelle}).` : `Tu as la dernière version (${r.actuelle}).`;
    $('#btn-maj-installer').hidden = !r.disponible;
  } catch (err) {
    texte.className = 'alerte';
    texte.textContent = String(err);
  } finally {
    e.target.classList.remove('chargement');
  }
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
  const debutSplash = performance.now();
  try {
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
      extraAnalyse = JSON.parse(localStorage.getItem('extra')) || {};
    } catch {}
  } finally {
    // L'écran de démarrage reste au moins 1,2 s, le temps de son animation
    const reste = 1200 - (performance.now() - debutSplash);
    setTimeout(() => {
      $('#splash').classList.add('fini');
      setTimeout(() => $('#splash').remove(), 600);
    }, Math.max(0, reste));
  }
  // Analyse complète au premier lancement, puis une fois par jour
  const vieille = !derniereAnalyse?.date || Date.now() - new Date(derniereAnalyse.date) > 24 * 3600 * 1000;
  if (vieille) {
    setTimeout(analyser, 700);
  } else {
    afficherScore();
    Promise.all([invoke('demarrage_liste'), invoke('tweaks_liste')]).then(([d, t]) => {
      demarrage = d;
      tweaks = t;
      afficherCategories();
      afficherScore();
    });
  }
  majConso();
  setInterval(majConso, 3000);
}

init();
