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
/** Carte graphique principale : { fabricant: nvidia | amd | intel, nom, dediee } */
let carte = null;

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
/** Menu en catégories : une seule ouverte à la fois, celle de la page affichée */
function ouvrirGroupe(groupe) {
  document.querySelectorAll('.groupe-nav').forEach((g) => {
    g.classList.toggle('ouvert', g === groupe);
    g.querySelector('.nav-groupe').setAttribute('aria-expanded', String(g === groupe));
  });
}
document.querySelectorAll('.nav-groupe').forEach((b) =>
  b.addEventListener('click', () => {
    const groupe = b.closest('.groupe-nav');
    if (groupe.classList.contains('ouvert') && !groupe.querySelector('[data-page].actif')) return ouvrirGroupe(null);
    ouvrirGroupe(groupe);
    // Ouvrir une catégorie affiche sa première page
    if (!groupe.querySelector('[data-page].actif')) [...groupe.querySelectorAll('[data-page]')].find((x) => getComputedStyle(x).display !== 'none')?.click();
  })
);

document.querySelectorAll('button[data-page]').forEach((b) =>
  b.addEventListener('click', () => {
    document.querySelectorAll('button[data-page], .page').forEach((e) => e.classList.remove('actif'));
    b.classList.add('actif');
    ouvrirGroupe(b.closest('.groupe-nav'));
    document.querySelectorAll('.groupe-nav').forEach((g) => g.classList.toggle('contient-actif', g.contains(b)));
    $(`#page-${b.dataset.page}`).classList.add('actif');
    if (b.dataset.page === 'optimisations') {
      chargerTweaks();
      chargerDemarrage();
    }
    if (b.dataset.page === 'analyse') {
      chargerCartes();
      chargerMemoire();
      chargerBios();
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
    if (b.dataset.page === 'clips') chargerClips();
    if (b.dataset.page === 'jeux') {
      chargerChoixFermer();
      chargerModesJeu();
      chargerJeux();
    }
  })
);

function allerA(page) {
  document.querySelector(`button[data-page="${page}"]`).click();
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
  ex.surBatterie = !!etat.alimentation?.sur_batterie;
  ex.economiseur = !!etat.alimentation?.economiseur;
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
  bios: ['Voir le guide', 'bios'],
};

function afficherScore() {
  if (!derniereAnalyse) return;
  const info = demarrage ? { ...derniereAnalyse, demarrage: demarrage.filter((p) => p.actif) } : derniereAnalyse;
  const { score, problemes } = evaluer(info, { planPerso: [etat.plan_perso, etat.plan_max], extra: calculerExtra() });
  animerScore(score);
  const anneau = $('#anneau');
  anneau.style.strokeDashoffset = 326.7 * (1 - score / 100);
  anneau.style.stroke = score >= 80 ? 'var(--accent-clair)' : score >= 55 ? 'var(--orange)' : 'var(--rouge)';
  let depart = null;
  try {
    depart = JSON.parse(localStorage.getItem('scoreDepart'));
    if (!depart) localStorage.setItem('scoreDepart', JSON.stringify({ score, date: new Date().toISOString() }));
  } catch {}
  $('#score-depart').hidden = !(depart && score > depart.score);
  if (depart && score > depart.score) $('#score-depart').innerHTML = `Avant FPS Cagoule : ${depart.score} → <span class="gain">${score} aujourd'hui (+${score - depart.score})</span>`;
  $('#score-texte').textContent =
    score >= 90 ? 'Au top' : score >= 70 ? 'Encore un peu de FPS à gagner' : 'Ton PC peut faire mieux';
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
          return `<li class="probleme" style="animation-delay:${i * 40}ms" title="${esc(p.detail)}"><span class="point ${p.gravite}"></span><div><strong>${esc(p.titre)}</strong></div>${bouton}</li>`;
        })
        .join('')
    : '<li class="ok">Rien à corriger</li>';
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
    else if (action === 'bios') {
      allerA('analyse');
      return setTimeout(() => $('#carte-bios').scrollIntoView({ behavior: 'smooth' }), 150);
    }
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

let promesseAnalyse = null;
/** Analyse complète, partagée si elle tourne déjà */
function analyser() {
  promesseAnalyse ??= analyserTout().finally(() => (promesseAnalyse = null));
  return promesseAnalyse;
}

/** Analyse complète : toutes les vérifications en parallèle, chacune cochée quand elle finit */
async function analyserTout() {
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
$('#btn-analyser').addEventListener('click', () => analyser());

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
  if (!t.disponible) return '';
  const coche = t.attente === 'annulation' ? false : t.attente === 'activation' || t.actif;
  const badges = [
    t.gain === 'fort' ? '<span class="badge accent">Gros gain</span>' : '',
    t.niveau === 'max' ? '<span class="badge violet">Max</span>' : '',
    t.niveau === 'risque' ? '<span class="badge rouge">Risqué</span>' : '',
    t.attente ? '<span class="badge orange" title="Redémarre ton PC pour terminer">↻ Redémarrage</span>' : '',
  ].join('');
  return `<li class="tweak${coche ? ' actif' : ''}" data-id="${esc(t.id)}" title="${esc(t.description)}">
      <div class="titre">${esc(t.titre)}</div>
      <div class="badges">${badges}</div>
      <label class="bascule"><input type="checkbox" data-tweak-bascule="${esc(t.id)}" ${coche ? 'checked' : ''} ${coche && !t.annulable ? 'disabled' : ''} aria-label="${esc(t.titre)}"><span></span></label>
    </li>`;
}

const CATEGORIES_OPTI = [
  ['windows', 'Windows'],
  ['carte-graphique', 'Carte graphique'],
  ['arriere-plan', 'Arrière-plan'],
  ['reseau', 'Réseau'],
  ['peripheriques', 'Souris et clavier'],
  ['confort', 'Confort et vie privée'],
  ['services', 'Services'],
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
  const pilote = ['nvidia', 'amd'].find((f) => tweaks.some((t) => t.special === f && t.disponible));
  $('#btn-nvidia-reglages').hidden = !pilote;
  $('#btn-nvidia-reglages').dataset.fabricant = pilote || '';
  $('#btn-nvidia-reglages').textContent = `Appliquer les réglages ${pilote === 'amd' ? 'AMD' : 'NVIDIA'}`;
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
  ul.addEventListener('change', async (e) => {
    const c = e.target;
    const id = c.dataset.tweakBascule;
    if (!id) return;
    const b = { dataset: c.checked ? { activer: id } : { annuler: id }, classList: c.closest('.tweak').classList };
    c.disabled = true;
    if (b?.dataset.activer) {
      const t = tweaks.find((x) => x.id === b.dataset.activer);
      if (t?.niveau === 'risque') {
        const ok = await confirmer(
          t.titre,
          `<p>${esc(t.description)}</p><p class="alerte">Ce réglage réduit la sécurité ou modifie le démarrage de Windows. Tu pourras l'annuler ici à tout moment.</p>`,
          'Activer quand même'
        );
        if (!ok) return chargerTweaks();
      }
      return appliquerTweaks([b.dataset.activer], b).catch(() => chargerTweaks());
    }
    try {
      const [r] = await appel('tweaks_annuler', { ids: [b.dataset.annuler] }, b);
      toast(r.ok ? 'Remis comme avant' : `Impossible d'annuler : ${r.erreur}`);
    } catch {}
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
    `<p>${a.length > 1 ? `${a.length} réglages vont être appliqués` : '1 réglage va être appliqué'} :</p><ul>${a.map((t) => `<li>${esc(t.titre)}</li>`).join('')}</ul>
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
  const fab = e.target.dataset.fabricant || 'nvidia';
  const nom = fab === 'amd' ? 'AMD' : 'NVIDIA';
  const a = tweaks.filter((t) => t.special === fab && t.disponible && !t.actif && t.niveau !== 'choix');
  if (!a.length) return toast(`Les réglages ${nom} sont déjà appliqués.`);
  const ok = await confirmer(
    `Réglages ${nom}`,
    `<ul>${a.map((t) => `<li>${esc(t.titre.replace(/^(NVIDIA|AMD) : /, ''))}</li>`).join('')}</ul><p>${
      fab === 'amd'
        ? "Appliqués aux réglages globaux d'AMD Adrenalin. Les profils de jeux que tu as réglés toi-même dans Adrenalin gardent leurs valeurs."
        : 'Appliqués aux paramètres globaux du panneau NVIDIA. Les jeux qui ont leur propre réglage dans le panneau le gardent. La V-Sync reste à choisir selon ton écran.'
    }</p>`,
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

/* ---------- Mes jeux ---------- */

const ECLAIR = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M13.5 2 4 13.5h6.5L9.5 22 20 9.5h-6.6z"/></svg>';
const ENGRENAGE =
  '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/></svg>';
const JOUER = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 5.5v13a1 1 0 0 0 1.5.86l10.4-6.5a1 1 0 0 0 0-1.72L9.5 4.64A1 1 0 0 0 8 5.5z"/></svg>';
const calme = () => matchMedia('(prefers-reduced-motion: reduce)').matches;

/** Image et état de chaque jeu (nom → { image, genre, installe }) */
let vignettes = new Map();
/** Réglages connus dans les fichiers des jeux (Fortnite, Valorant…) */
let configsJeux = [];
let nomsDefaut = null;
let imagesCherchees = false;

const pareil = (a, b) => a.toLowerCase() === b.toLowerCase();
function configPour(p) {
  return configsJeux.find((c) => c.exes.some((e) => p.exes.some((x) => pareil(x, e))) || pareil(c.nom, p.nom));
}
/** Le fichier de réglages du jeu a encore des réglages à changer */
const configAFaire = (c) => c && c.fichiers && c.reglages.length && !c.optimise;
function estOptimise(p) {
  return p.actif && p.boost && p.priorite_haute && p.gpu_perf && !!reglages?.boost_auto && !configAFaire(configPour(p));
}
/** Jeu présent sur le PC, ou ajouté par l'utilisateur (pas dans la liste d'origine) */
function jeuVisible(p) {
  const c = configPour(p);
  return vignettes.get(p.nom)?.installe || (c && c.fichiers > 0) || (nomsDefaut && !nomsDefaut.has(p.nom));
}
function teinte(nom) {
  let h = 0;
  for (const c of nom) h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
}
const initiales = (nom) =>
  nom
    .split(/[\s:-]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((m) => m[0].toUpperCase())
    .join('');

function visuelJeu(p) {
  const v = vignettes.get(p.nom);
  if (v?.image && v.genre !== 'icone') return `<img class="jaquette" src="${v.image}" alt="" draggable="false">`;
  return `<div class="sans-image">${v?.image ? `<img class="icone-jeu" src="${v.image}" alt="" draggable="false">` : `<span class="initiales">${esc(initiales(p.nom))}</span>`}</div>`;
}

function tuileJeu(p, i) {
  const c = configPour(p);
  const opti = estOptimise(p);
  let etat = 'Pas encore optimisé';
  if (!p.actif) etat = 'Pas surveillé';
  else if (opti) etat = 'Optimisé';
  else if (c?.en_cours) etat = 'Partie en cours';
  return `<div class="tuile-jeu${opti ? ' optimise' : ''}" data-i="${i}" style="--teinte:${teinte(p.nom)}">
    <div class="visuel">${visuelJeu(p)}</div>
    <span class="etiquette-opti">${ECLAIR}Optimisé</span>
    <button class="btn-reglages-jeu" data-reglages="${i}" title="Réglages de ${esc(p.nom)}" aria-label="Réglages de ${esc(p.nom)}">${ENGRENAGE}</button>
    <button class="eclair" data-eclair="${i}" title="${opti ? 'Déjà optimisé : relancer' : 'Optimiser'} ${esc(p.nom)}" aria-label="Optimiser ${esc(p.nom)}">${ECLAIR}</button>
    <div class="pied"><div><strong>${esc(p.nom)}</strong><small>${etat}</small></div>${
      vignettes.get(p.nom)?.jouable ? `<button class="btn-jouer" data-jouer="${i}" title="Jouer à ${esc(p.nom)}" aria-label="Jouer à ${esc(p.nom)}">${JOUER}</button>` : ''
    }</div>
  </div>`;
}

function afficherProfils() {
  const visibles = [];
  const absents = [];
  profils.forEach((p, i) => (jeuVisible(p) ? visibles : absents).push([p, i]));
  $('#grille-jeux').innerHTML =
    visibles.map(([p, i]) => tuileJeu(p, i)).join('') +
    `<button class="tuile-ajout" data-ajout><span>+</span>Détecter mes jeux</button>`;
  const zone = $('#jeux-absents');
  zone.hidden = !absents.length;
  zone.querySelector('summary').textContent = `${absents.length} autre${absents.length > 1 ? 's' : ''} jeu${absents.length > 1 ? 'x' : ''} de la liste, pas trouvé${absents.length > 1 ? 's' : ''} sur ce PC`;
  zone.querySelector('.grille-jeux').innerHTML = absents.map(([p, i]) => tuileJeu(p, i)).join('');
  const reste = visibles.filter(([p]) => !estOptimise(p)).length;
  $('#btn-jeux-tout').disabled = !reste;
  if (!$('#fenetre-jeu').hidden) afficherFenetreJeu();
}

async function chargerVignettes(chercher) {
  const liste = await invoke(chercher ? 'jeux_images_chercher' : 'jeux_cartes').catch(() => null);
  if (!liste) return;
  vignettes = new Map(liste.map((c) => [c.nom, c]));
  afficherProfils();
}

async function chargerConfigsJeux() {
  configsJeux = await invoke('jeux_configs').catch(() => []);
  // Jeu installé dont on connaît les réglages mais absent de la liste : ajouté
  const manquants = configsJeux.filter((c) => c.fichiers && !profils.some((p) => configPour(p) === c));
  for (const c of manquants) profils.push({ nom: c.nom, exes: [...c.exes], actif: true, boost: true, priorite_haute: true, gpu_perf: true, resolution: null, oc: true });
  if (manquants.length) await sauverProfils();
  else afficherProfils();
}

async function chargerJeux() {
  if (!nomsDefaut) nomsDefaut = new Set((await invoke('profils_defaut').catch(() => [])).map((p) => p.nom));
  await Promise.all([chargerVignettes(false), chargerConfigsJeux()]);
  // Images manquantes : une seule recherche par ouverture de l'app (le reste vient du cache)
  if (!imagesCherchees) {
    imagesCherchees = true;
    chargerVignettes(true);
  }
}

/** Arcs électriques autour de l'éclair (SVG éphémère) */
function arcs(tuile, nombre = 5, longueur = [30, 58]) {
  if (calme()) return;
  const ns = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(ns, 'svg');
  svg.setAttribute('class', 'arcs');
  svg.setAttribute('viewBox', '0 0 100 150');
  svg.setAttribute('preserveAspectRatio', 'none');
  for (let k = 0; k < nombre; k++) {
    const angle = Math.random() * Math.PI * 2;
    const l = longueur[0] + Math.random() * (longueur[1] - longueur[0]);
    const points = [[50, 66]];
    for (let s = 1; s <= 6; s++) {
      const d = (l * s) / 6;
      points.push([50 + Math.cos(angle) * d + (Math.random() - 0.5) * 9, 66 + Math.sin(angle) * d * 1.4 + (Math.random() - 0.5) * 9]);
    }
    const ligne = document.createElementNS(ns, 'polyline');
    ligne.setAttribute('points', points.map((p) => p.join(',')).join(' '));
    ligne.setAttribute('pathLength', '1');
    ligne.style.animationDelay = `${Math.random() * 0.12}s`;
    svg.append(ligne);
  }
  tuile.append(svg);
  setTimeout(() => svg.remove(), 700);
}

// Survol : petites étincelles autour de l'éclair
let etincelles = null;
document.addEventListener('mouseover', (e) => {
  const t = e.target.closest?.('.tuile-jeu');
  if (!t || t === etincelles?.tuile) return;
  clearInterval(etincelles?.minuteur);
  etincelles = { tuile: t, minuteur: setInterval(() => (t.matches(':hover') ? arcs(t, 2, [12, 24]) : clearInterval(etincelles.minuteur)), 650) };
});

/** Optimise un jeu : boost, priorité haute, GPU puissant et réglages dans le jeu */
async function optimiserJeu(i, { silencieux = false } = {}) {
  const p = profils[i];
  const tuile = document.querySelector(`.tuile-jeu[data-i="${i}"]`);
  tuile?.classList.add('charge');
  const crepite = tuile && setInterval(() => arcs(tuile), 260);
  if (tuile) arcs(tuile, 7);
  const fait = [];
  const avertir = [];
  try {
    const debut = performance.now();
    Object.assign(p, { actif: true, boost: true, priorite_haute: true, gpu_perf: true });
    await invoke('profils_set', { profils });
    fait.push('boost, priorité haute et carte graphique puissante');
    if (!reglages.boost_auto) {
      reglages.boost_auto = true;
      await sauverReglages();
      afficherReglages();
      fait.push('boost automatique allumé');
    }
    const c = configPour(p);
    if (configAFaire(c)) {
      if (c.en_cours) avertir.push(`ferme ${p.nom} pour optimiser ses réglages en jeu`);
      else {
        const n = await invoke('jeux_config_appliquer', { id: c.id });
        if (n) fait.push(`${n} réglage${n > 1 ? 's' : ''} changé${n > 1 ? 's' : ''} dans le jeu`);
      }
    }
    // L'effet a le temps de se voir
    await new Promise((r) => setTimeout(r, Math.max(0, 900 - (performance.now() - debut))));
  } catch (err) {
    avertir.push(String(err));
  } finally {
    clearInterval(crepite);
    tuile?.classList.remove('charge');
  }
  configsJeux = await invoke('jeux_configs').catch(() => configsJeux);
  afficherProfils();
  const nouvelle = document.querySelector(`.tuile-jeu[data-i="${i}"]`);
  if (nouvelle) {
    nouvelle.classList.add('eclate');
    arcs(nouvelle, 9, [40, 75]);
    setTimeout(() => nouvelle.classList.remove('eclate'), 900);
  }
  if (!silencieux) toast(avertir.length ? `${p.nom} : ${avertir.join(', ')}` : `⚡ ${p.nom} optimisé`, avertir.length ? 7000 : 3000);
  return !avertir.length;
}

document.addEventListener('click', (e) => {
  const b = e.target.closest('#page-jeux button');
  if (!b) return;
  if (b.dataset.eclair !== undefined) optimiserJeu(Number(b.dataset.eclair));
  else if (b.dataset.jouer !== undefined) jouer(Number(b.dataset.jouer));
  else if (b.dataset.reglages !== undefined) ouvrirFenetreJeu(Number(b.dataset.reglages));
  else if (b.hasAttribute('data-ajout')) $('#btn-jeux-detecter').click();
});

/** Choix du compte Steam : résout avec le SteamID64 choisi, ou undefined si annulé */
function choisirCompte(p, comptes) {
  return new Promise((resoudre) => {
    const prefere = p.compte_steam || comptes.find((c) => c.actif)?.id;
    $('#comptes-titre').textContent = `Jouer à ${p.nom} avec…`;
    const liste = $('#comptes-liste');
    liste.innerHTML = '';
    comptes.forEach((c, i) => {
      const b = document.createElement('button');
      b.className = `compte-steam${c.id === prefere ? ' prefere' : ''}`;
      b.disabled = !c.memorise;
      b.style.animationDelay = `${i * 50}ms`;
      const photo = document.createElement('span');
      photo.className = 'avatar';
      if (c.avatar) photo.style.backgroundImage = `url("${c.avatar}")`;
      else photo.textContent = (c.pseudo[0] || '?').toUpperCase();
      const texte = document.createElement('span');
      texte.className = 'texte';
      const nom = document.createElement('strong');
      nom.textContent = c.pseudo;
      const detail = document.createElement('small');
      detail.textContent = !c.memorise ? 'Reconnecte-toi une fois dans Steam' : c.actif ? 'Connecté dans Steam' : c.id === p.compte_steam ? 'Dernier choix' : c.identifiant;
      texte.append(nom, detail);
      b.append(photo, texte);
      b.addEventListener('click', () => fin(c.id));
      liste.append(b);
    });
    const fenetre = $('#fenetre-comptes');
    function fin(r) {
      fenetre.hidden = true;
      fenetre.onclick = null;
      resoudre(r);
    }
    $('#comptes-annuler').onclick = () => fin(undefined);
    fenetre.onclick = (e) => e.target === fenetre && fin(undefined);
    fenetre.hidden = false;
    liste.querySelector('.prefere:not(:disabled), button:not(:disabled)')?.focus();
  });
}

async function jouer(i) {
  const p = profils[i];
  const v = vignettes.get(p.nom);
  let compte = null;
  if (v?.steam) {
    const comptes = await invoke('comptes_steam').catch(() => []);
    if (comptes.length > 1) {
      compte = await choisirCompte(p, comptes);
      if (compte === undefined) return;
    }
  }
  const tuile = document.querySelector(`.tuile-jeu[data-i="${i}"]`);
  tuile?.classList.add('lance');
  const actif = compte && !(await invoke('comptes_steam').catch(() => [])).find((c) => c.id === compte)?.actif;
  toast(actif ? `Changement de compte Steam… ${p.nom} se lance juste après.` : `Lancement de ${p.nom}…`, 6000);
  try {
    toast(await invoke('jeu_lancer', { nom: p.nom, compte }), 6000);
    if (compte) p.compte_steam = compte;
  } catch (err) {
    toast(String(err), 8000);
  } finally {
    setTimeout(() => tuile?.classList.remove('lance'), 1200);
  }
}

$('#btn-jeux-tout').addEventListener('click', async (e) => {
  const bouton = e.currentTarget;
  const a = profils.map((p, i) => [p, i]).filter(([p]) => jeuVisible(p) && !estOptimise(p));
  if (!a.length) return toast('Tous tes jeux sont déjà optimisés.');
  bouton.classList.add('chargement');
  let ok = 0;
  for (const [, i] of a) if (await optimiserJeu(i, { silencieux: true })) ok++;
  bouton.classList.remove('chargement');
  toast(ok === a.length ? `⚡ ${ok} jeu${ok > 1 ? 'x' : ''} optimisé${ok > 1 ? 's' : ''}` : `${ok} sur ${a.length} optimisés : ferme les jeux lancés`, 7000);
});

/* Fenêtre de réglages d'un jeu */
let jeuOuvert = null;

function ouvrirFenetreJeu(i) {
  jeuOuvert = profils[i];
  afficherFenetreJeu();
  $('#fenetre-jeu').hidden = false;
  $('#fenetre-jeu [data-fermer]')?.focus();
}
function fermerFenetreJeu() {
  $('#fenetre-jeu').hidden = true;
  jeuOuvert = null;
}

function afficherFenetreJeu() {
  const i = profils.indexOf(jeuOuvert);
  if (i < 0) return fermerFenetreJeu();
  const p = jeuOuvert;
  const v = vignettes.get(p.nom);
  const c = configPour(p);
  const bascule = (champ, titre, texte) =>
    `<label class="bascule"><input type="checkbox" data-champ="${champ}" ${p[champ] ? 'checked' : ''}><span></span><div><strong>${titre}</strong><small>${texte}</small></div></label>`;
  let config = '';
  if (c && c.fichiers) {
    const aChanger = c.reglages.filter((r) => (r.actuel || '').toLowerCase() !== r.cible.toLowerCase());
    const etat = c.en_cours
      ? '<span class="badge orange">Jeu lancé : ferme-le d\'abord</span>'
      : c.optimise
        ? '<span class="badge accent">Optimisés</span>'
        : `<span class="badge">${aChanger.length} réglage${aChanger.length > 1 ? 's' : ''} à changer</span>`;
    config = `<div class="fj-section"><div class="jeu-config-entete"><strong>Réglages dans le jeu</strong>${etat}
        ${!c.optimise && !c.en_cours && c.reglages.length ? `<button class="petit principal" data-config-optimiser>Optimiser</button>` : ''}
        ${c.sauvegarde && !c.en_cours ? `<button class="petit" data-config-restaurer>Restaurer</button>` : ''}</div>
      ${
        c.reglages.length
          ? `<details><summary>Ce qui change</summary><ul>${c.reglages
              .map((r) => `<li>${esc(r.libelle)}${(r.actuel || '').toLowerCase() === r.cible.toLowerCase() ? ' ✓' : ` <span class="discret">(${esc(r.actuel ?? '?')} → ${esc(r.cible)})</span>`}</li>`)
              .join('')}</ul></details>`
          : '<p class="note">Aucun réglage connu dans ce fichier (version du jeu différente).</p>'
      }
      <p class="note">${esc(c.note)}${c.fichiers > 1 ? ` (${c.fichiers} comptes)` : ''}</p></div>`;
  }
  $('#fenetre-jeu .boite').innerHTML = `
    <div class="fj-entete">
      <div class="tuile-jeu mini" style="--teinte:${teinte(p.nom)}"><div class="visuel">${visuelJeu(p)}</div></div>
      <div class="fj-titre">
        <h2 id="fj-titre">${esc(p.nom)}</h2>
        <p class="discret">${esc(p.exes.join(', '))}</p>
        <div class="boutons">
          <label class="bouton petit">Changer l'image<input type="file" accept="image/png,image/jpeg,image/webp" data-image hidden></label>
          ${v?.genre === 'perso' ? '<button class="petit" data-image-origine>Image d\'origine</button>' : ''}
          ${!v?.image || v.genre === 'icone' ? '<button class="petit" data-image-chercher>Chercher l\'image</button>' : ''}
        </div>
      </div>
    </div>
    <div class="interrupteurs fj-bascules">
      ${bascule('actif', 'Repérer ce jeu', 'FPS Cagoule voit quand il se lance et mesure tes FPS.')}
      ${bascule('boost', 'Boost pendant les parties', 'Plan d\'alimentation performant, programmes de ta liste fermés.')}
      ${bascule('priorite_haute', 'Priorité haute', 'Le processeur sert d\'abord le jeu.')}
      ${bascule('gpu_perf', 'Carte graphique la plus puissante', 'Utile sur les PC portables avec deux cartes graphiques.')}
      ${bascule('oc', 'Garder l\'overclocking', 'Décoche si ce jeu plante avec l\'overclocking de la carte graphique.')}
    </div>
    <label class="fj-resolution"><span>Résolution pendant la partie</span><select data-resolution aria-label="Résolution pendant ${esc(p.nom)}">${optionsResolution(p)}</select></label>
    ${config}
    <div class="boutons fj-pied">
      <button class="discret-btn danger" data-retirer>Retirer de la liste</button>
      <button class="principal" data-fermer>Fermer</button>
    </div>`;
}

/** Réduit l'image choisie (400 × 600, recadrée au centre) avant de l'envoyer */
async function reduireImage(fichier) {
  const img = await createImageBitmap(fichier);
  const canvas = document.createElement('canvas');
  canvas.width = 400;
  canvas.height = 600;
  const echelle = Math.max(400 / img.width, 600 / img.height);
  const l = img.width * echelle;
  const h = img.height * echelle;
  canvas.getContext('2d').drawImage(img, (400 - l) / 2, (600 - h) / 2, l, h);
  return canvas.toDataURL('image/jpeg', 0.86);
}

$('#fenetre-jeu').addEventListener('click', async (e) => {
  if (e.target === e.currentTarget) return fermerFenetreJeu();
  const b = e.target.closest('button');
  if (!b || !jeuOuvert) return;
  const p = jeuOuvert;
  const c = configPour(p);
  try {
    if (b.hasAttribute('data-fermer')) return fermerFenetreJeu();
    if (b.hasAttribute('data-retirer')) {
      profils.splice(profils.indexOf(p), 1);
      fermerFenetreJeu();
      await sauverProfils();
      return toast(`${p.nom} retiré de la liste.`);
    }
    if (b.hasAttribute('data-config-optimiser')) {
      const n = await appel('jeux_config_appliquer', { id: c.id }, b);
      toast(n ? `${n} réglage${n > 1 ? 's' : ''} changé${n > 1 ? 's' : ''}. Lance le jeu pour en profiter.` : 'Déjà optimisé.');
    } else if (b.hasAttribute('data-config-restaurer')) {
      await appel('jeux_config_restaurer', { id: c.id }, b);
      toast('Réglages du jeu remis comme avant.');
    } else if (b.hasAttribute('data-image-origine')) {
      await appel('jeu_image_effacer', { nom: p.nom }, b);
      return chargerVignettes(true);
    } else if (b.hasAttribute('data-image-chercher')) {
      b.classList.add('chargement');
      const liste = await invoke('jeux_images_chercher', { forcer: p.nom });
      vignettes = new Map(liste.map((x) => [x.nom, x]));
      const v = vignettes.get(p.nom);
      if (!v?.image || v.genre === 'icone') toast('Pas d\'image trouvée pour ce jeu. Tu peux mettre la tienne avec « Changer l\'image ».', 6000);
      return afficherProfils();
    }
  } catch {}
  configsJeux = await invoke('jeux_configs').catch(() => configsJeux);
  afficherProfils();
});

$('#fenetre-jeu').addEventListener('change', async (e) => {
  const c = e.target;
  const p = jeuOuvert;
  if (!p) return;
  if (c.dataset.champ) {
    p[c.dataset.champ] = c.checked;
    sauverProfils();
  } else if (c.dataset.resolution !== undefined) {
    const [largeur, hauteur, etirer] = c.value.split('x').map(Number);
    p.resolution = c.value ? { largeur, hauteur, etirer: etirer === 1 } : null;
    sauverProfils();
  } else if (c.dataset.image !== undefined && c.files[0]) {
    try {
      await appel('jeu_image_choisir', { nom: p.nom, image: await reduireImage(c.files[0]) });
      await chargerVignettes(false);
      toast('Image changée.');
    } catch (err) {
      if (!String(err).includes('Erreur')) toast(`Image illisible : ${err}`, 6000);
    }
  }
});

document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && !$('#fenetre-jeu').hidden) fermerFenetreJeu();
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

async function sauverProfils() {
  await appel('profils_set', { profils });
  afficherProfils();
}

/** Jeux installés qui ne sont pas encore dans la liste */
async function nouveauxJeux() {
  const trouves = await invoke('jeux_installes');
  const connus = new Set(profils.flatMap((p) => p.exes.map((e) => e.toLowerCase())));
  const noms = new Set(profils.map((p) => p.nom.toLowerCase()));
  return trouves.filter((j) => !j.exes.some((e) => connus.has(e.toLowerCase())) && !noms.has(j.nom.toLowerCase()));
}

function ajouterJeux(jeux) {
  for (const j of jeux) profils.push({ nom: j.nom, exes: j.exes, actif: true, boost: true, priorite_haute: true, gpu_perf: true, resolution: null, oc: true });
  try {
    localStorage.setItem('jeuxDetectes', String(Date.now()));
  } catch {}
  return sauverProfils();
}

$('#btn-jeux-detecter').addEventListener('click', async (e) => {
  const bouton = e.currentTarget;
  let jeux;
  try {
    bouton.classList.add('chargement');
    jeux = await nouveauxJeux();
  } catch (err) {
    return toast(`Recherche impossible : ${err}`, 7000);
  } finally {
    bouton.classList.remove('chargement');
  }
  if (!jeux.length) return toast('Aucun nouveau jeu trouvé : tous tes jeux sont déjà dans la liste.');
  const ok = await confirmer(
    `${jeux.length} jeu${jeux.length > 1 ? 'x' : ''} trouvé${jeux.length > 1 ? 's' : ''}`,
    `<p>Coche ceux que FPS Cagoule doit booster :</p><ul class="liste-jeux-trouves">${jeux
      .map((j, i) => `<li><label class="case"><input type="checkbox" data-jeu="${i}" checked> <span><strong>${esc(j.nom)}</strong> <span class="discret">· ${esc(j.source)} · ${esc(j.exes.join(', '))}</span></span></label></li>`)
      .join('')}</ul>`,
    'Ajouter'
  );
  if (!ok) return;
  const choisis = jeux.filter((_, i) => document.querySelector(`[data-jeu="${i}"]`)?.checked);
  await ajouterJeux(choisis);
  toast(`${choisis.length} jeu${choisis.length > 1 ? 'x' : ''} ajouté${choisis.length > 1 ? 's' : ''}. Clique sur l'éclair pour les optimiser.`);
  chargerVignettes(true);
});

/** Une fois par semaine, à l'ouverture : les jeux installés depuis sont ajoutés tout seuls */
async function detecterJeuxAuto() {
  let derniere = 0;
  try {
    derniere = Number(localStorage.getItem('jeuxDetectes')) || 0;
  } catch {}
  if (Date.now() - derniere < 7 * 864e5) return;
  const jeux = await nouveauxJeux().catch(() => []);
  await ajouterJeux(jeux);
  if (jeux.length) toast(`${jeux.length} nouveau${jeux.length > 1 ? 'x' : ''} jeu${jeux.length > 1 ? 'x' : ''} ajouté${jeux.length > 1 ? 's' : ''} au boost : ${jeux.map((j) => j.nom).slice(0, 4).join(', ')}${jeux.length > 4 ? '…' : ''}`, 8000);
}


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
  $('#priorite-basse').checked = reglages.priorite_basse;
  $('#discord').checked = reglages.discord;
  $('#clips-auto').checked = reglages.clips_auto;
  $('#ligne-discord').hidden = !etat.discord;
  $('#fond-liste').innerHTML = reglages.priorite_basse_applis.length
    ? reglages.priorite_basse_applis.map((a, i) => `<span class="puce">${esc(a)}<button data-retirer="${i}" title="Retirer">✕</button></span>`).join('')
    : '<span class="discret">Aucun programme.</span>';
  $('#fermer-liste').innerHTML = reglages.fermer_applis.length
    ? reglages.fermer_applis.map((a, i) => `<span class="puce">${esc(a)}<button data-retirer="${i}" title="Retirer">✕</button></span>`).join('')
    : '<span class="discret">Aucun programme.</span>';
}

for (const [id, champ] of [['#boost-auto', 'boost_auto'], ['#boost-max', 'boost_max'], ['#mesure-fps', 'mesure_fps'], ['#overlay', 'overlay'], ['#maj-auto', 'maj_auto'], ['#couleurs-en-jeu', 'couleurs_en_jeu'], ['#priorite-basse', 'priorite_basse'], ['#discord', 'discord'], ['#clips-auto', 'clips_auto']]) {
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
  const options = '<option value="">Choisir un programme ouvert…</option>' + noms.filter((n) => !systeme.test(n)).map((n) => `<option>${esc(n)}</option>`).join('');
  $('#fermer-choix').innerHTML = options;
  $('#fond-choix').innerHTML = options;
}

$('#fond-liste').addEventListener('click', (e) => {
  const i = e.target.dataset.retirer;
  if (i === undefined) return;
  reglages.priorite_basse_applis.splice(Number(i), 1);
  sauverReglages();
  afficherReglages();
});

$('#btn-fond-ajout').addEventListener('click', () => {
  const n = $('#fond-choix').value;
  if (!n || reglages.priorite_basse_applis.some((a) => a.toLowerCase() === n.toLowerCase())) return;
  reglages.priorite_basse_applis.push(n);
  sauverReglages();
  afficherReglages();
});

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
  afficherTempJeu(jeu.temp_gpu);
}

function afficherTempJeu(t) {
  const p = $('#jeu-temp');
  p.hidden = !t;
  if (!t) return;
  p.textContent = `Carte graphique : ${t} °C${t >= 85 ? ' — elle chauffe beaucoup : vérifie la ventilation du boîtier' : ''}`;
  p.className = t >= 85 ? 'alerte' : 'discret';
}
listen('temp_gpu', (e) => afficherTempJeu(e.payload));

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
    html = `<p><strong>Curve Optimizer</strong> dans Ryzen Master (réglage auto « Per Core ») ou dans le BIOS : PBO → Curve Optimizer → All Cores → Negative → 15. Si ça plante, baisse à 10.</p>`;
    boutons = p.ryzen_master ? '<button class="principal" data-cpu="ryzen">Ouvrir AMD Ryzen Master</button>' : '<button class="principal" data-cpu-lien="amd">Télécharger AMD Ryzen Master</button>';
  } else if (intelK) {
    badge = 'Overclockable';
    html = `<p>Un léger <strong>undervolt</strong> avec Intel XTU : moins de chaleur, fréquences plus stables.${intel1314 ? ' Mets d\'abord ton BIOS à jour (correctif Intel 13e/14e génération).' : ''}</p>`;
    boutons = '<button class="principal" data-cpu-lien="intel">Télécharger Intel XTU</button>';
  } else {
    badge = 'Non overclockable';
    html = `<p>Les optimisations de FPS Cagoule suffisent pour ce processeur.</p>`;
  }
  $('#cpu-badge').textContent = badge;
  $('#cpu-conseils').innerHTML = html;
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
  $('#oc-amd').hidden = true;
  if (carte?.fabricant === 'amd') {
    $('#oc-contenu').hidden = true;
    $('#oc-indisponible').hidden = true;
    return chargerAmd();
  }
  try {
    oc = await invoke('oc_etat');
  } catch (e) {
    $('#oc-contenu').hidden = true;
    $('#oc-indisponible').hidden = false;
    const intel = carte?.fabricant === 'intel';
    $('#oc-indisponible-texte').textContent = intel
      ? `Ta carte graphique est une ${carte.nom}. Intel ne permet pas de la régler depuis une autre appli : utilise Intel Graphics Software (ou Arc Control) pour la limite de puissance et les performances.`
      : `${e}. L'overclocking est proposé pour les cartes NVIDIA et AMD.`;
    $('#btn-oc-logiciel').hidden = !intel;
    $('#btn-oc-logiciel').textContent = 'Ouvrir le logiciel Intel';
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
  $('#oc-protection').checked = reglages?.oc_protection !== false;
  majCurseursOc();
  majTempOc();
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

/** Après un plantage en jeu : -25 MHz GPU et -100 MHz mémoire sur le réglage gardé */
$('#btn-oc-baisser').addEventListener('click', (e) =>
  avecOccupation(e.target, async () => {
    const r = oc?.enregistre;
    const coeur = r ? r.coeur_mhz : Number($('#oc-coeur').value);
    const memoire = r ? r.memoire_mhz : Number($('#oc-memoire').value);
    if (coeur <= 0 && memoire <= 0) {
      toast("Aucun overclocking à baisser : la carte est déjà aux fréquences d'origine.");
      return;
    }
    const reglage = {
      coeur_mhz: coeur > 0 ? Math.max(0, coeur - 25) : coeur,
      memoire_mhz: memoire > 0 ? Math.max(0, memoire - 100) : memoire,
      puissance_pct: r ? r.puissance_pct : valeursOc().puissance,
      au_demarrage: $('#oc-demarrage').checked,
    };
    await appel('oc_enregistrer', { reglage });
    toast(`Baissé : GPU ${signe(reglage.coeur_mhz)} MHz, mémoire ${signe(reglage.memoire_mhz)} MHz. Relance ton jeu ; si ça plante encore, rebaisse.`, 9000);
  })
);

$('#oc-demarrage').addEventListener('change', async (e) => {
  if (!oc?.enregistre) return;
  await appel('oc_enregistrer', { reglage: { ...oc.enregistre, au_demarrage: e.target.checked } }).catch(() => {});
});

$('#oc-protection').addEventListener('change', (e) => {
  reglages.oc_protection = e.target.checked;
  sauverReglages();
});

async function majTempOc() {
  const t = await invoke('temperature_gpu').catch(() => null);
  for (const b of [$('#oc-temp'), $('#amd-temp')]) {
    b.hidden = !t;
    if (!t) continue;
    b.textContent = `${t} °C`;
    b.className = `badge ${t >= 85 ? 'rouge' : t >= 75 ? 'orange' : ''}`;
  }
  return t;
}
setInterval(() => {
  if (!document.hidden && $('#page-oc').classList.contains('actif') && (oc || amd)) majTempOc();
}, 2000);

/* Cartes AMD : préréglages officiels d'Adrenalin */
let amd = null;
const PREREGLAGES_AMD = [
  ['Économie', 'Consomme et chauffe le moins possible, au prix de quelques FPS.'],
  ['Silencieux', 'Ventilateurs plus calmes, fréquences un peu plus basses.'],
  ['Équilibré', 'Le réglage d\'usine.'],
  ['Turbo', 'Limite de puissance un peu relevée : quelques FPS en plus, un peu plus de bruit.'],
  ['Rage', 'L\'overclocking officiel d\'AMD : limite de puissance au maximum autorisé. Le plus de FPS.'],
];

async function chargerAmd() {
  try {
    amd = await invoke('amd_etat');
  } catch (e) {
    $('#oc-indisponible').hidden = false;
    $('#oc-indisponible-texte').textContent = `${e}. Les préréglages se trouvent aussi dans AMD Adrenalin → Performances → Réglage.`;
    $('#btn-oc-logiciel').hidden = false;
    $('#btn-oc-logiciel').textContent = 'Ouvrir AMD Adrenalin';
    return;
  }
  $('#oc-amd').hidden = false;
  $('#amd-nom').textContent = amd.nom;
  $('#amd-resume').textContent = amd.actuel != null ? `Préréglage : ${PREREGLAGES_AMD[amd.actuel][0]}` : 'Réglage personnalisé';
  $('#amd-resume').className = `badge ${amd.actuel >= 3 ? 'accent' : ''}`;
  $('#btn-amd-adrenalin').hidden = !amd.logiciel;
  $('#amd-prereglages').innerHTML = amd.prereglages.length
    ? amd.prereglages
        .map((i) => `<button class="choix-carte ${amd.actuel === i ? 'actif' : ''}" data-prereglage="${i}"><strong>${PREREGLAGES_AMD[i][0]}</strong><span>${PREREGLAGES_AMD[i][1]}</span></button>`)
        .join('')
    : '<p class="discret">Ta carte ne propose pas de préréglages : règle-la dans AMD Adrenalin → Performances → Réglage.</p>';
  majTempOc();
}

$('#amd-prereglages').addEventListener('click', async (e) => {
  const b = e.target.closest('[data-prereglage]');
  if (!b) return;
  try {
    await appel('amd_prereglage', { index: Number(b.dataset.prereglage) }, b);
    toast(`Préréglage ${PREREGLAGES_AMD[b.dataset.prereglage][0]} appliqué.`);
  } catch {}
  chargerAmd();
});
$('#btn-amd-usine').addEventListener('click', async (e) => {
  try {
    await appel('amd_usine', {}, e.target);
    toast("Réglages d'usine de la carte remis.");
  } catch {}
  chargerAmd();
});
$('#btn-amd-adrenalin').addEventListener('click', () => appel('ouvrir_logiciel_gpu', { fabricant: 'amd' }).catch(() => {}));
$('#btn-oc-logiciel').addEventListener('click', () => appel('ouvrir_logiciel_gpu', { fabricant: carte?.fabricant || '' }).catch(() => {}));

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
        const t = await majTempOc();
        journal(`GPU +${c} MHz : stable (${Math.round(r.images_s)} images/s${t ? `, ${t} °C` : ''})`, 'ok');
        coeur = c;
        if (t >= 87) {
          journal(`Carte à ${t} °C : je m'arrête là pour ne pas la faire chauffer plus`, 'ko');
          break;
        }
      }
      // Les jeux poussent la carte autrement que ce test (pics de boost, tension) : grosse marge
      if (coeur > 0) {
        coeur = Math.max(0, Math.min(coeur - 50, Math.floor((coeur * 0.75) / 5) * 5));
        journal(`Marge de sécurité pour les jeux : GPU +${coeur} MHz`);
      }
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
      if (memoire > 0) {
        memoire = Math.max(0, Math.min(memoire - 300, Math.floor((memoire * 0.75) / 25) * 25));
        journal(`Marge de sécurité pour les jeux : mémoire +${memoire} MHz`);
      }
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
    journal('Si un jeu plante ou affiche des artefacts, clique sur « Baisser d\'un cran ».');
    toast(`Overclocking trouvé et appliqué : GPU +${coeur} MHz, mémoire +${memoire} MHz. Si un jeu plante, clique sur « Baisser d'un cran ».`, 10000);
  });
});

/* ---------- Statistiques ---------- */

const moyenne = (l, f) => l.reduce((s, x) => s + f(x), 0) / l.length;
const duree = (s) => (s >= 3600 ? `${Math.floor(s / 3600)} h ${String(Math.floor((s % 3600) / 60)).padStart(2, '0')}` : `${Math.max(1, Math.round(s / 60))} min`);

let sessionsFps = [];

const dateCourte = (t) => new Date(t * 1000).toLocaleString('fr-CH', { dateStyle: 'short', timeStyle: 'short' });
const minutes = (sec) => (sec >= 3600 ? `${Math.floor(sec / 3600)} h ${String(Math.floor((sec % 3600) / 60)).padStart(2, '0')}` : `${Math.floor(sec / 60)}:${String(Math.round(sec % 60)).padStart(2, '0')}`);

/** Pas « rond » pour l'axe des FPS */
function pasAxe(max) {
  for (const p of [10, 20, 25, 50, 100, 200, 250, 500]) if (max / p <= 5) return p;
  return 1000;
}

/** Courbe d'une partie : FPS moyen par tranche de temps, moyenne de la partie et creux le plus bas */
function afficherCourbe(s) {
  const carte = $('#courbe-partie');
  if (!s?.courbe?.length) return (carte.hidden = true);
  carte.hidden = false;
  const L = 800, H = 230, g = 44, d = 14, h = 14, b = 30;
  const max = Math.max(...s.courbe, s.moyenne);
  const pas = pasAxe(max);
  const haut = Math.ceil((max * 1.08) / pas) * pas;
  const n = s.courbe.length;
  const x = (i) => g + (n === 1 ? 0 : (i / (n - 1)) * (L - g - d));
  const y = (v) => h + (1 - v / haut) * (H - h - b);
  const points = s.courbe.map((v, i) => `${x(i).toFixed(1)},${y(v).toFixed(1)}`);
  const lignes = [];
  for (let v = 0; v <= haut; v += pas) lignes.push(`<line class="grille" x1="${g}" x2="${L - d}" y1="${y(v)}" y2="${y(v)}"/><text class="axe" x="${g - 8}" y="${y(v) + 4}" text-anchor="end">${v}</text>`);
  // Repères de temps : 5 au plus, en minutes
  const total = n * s.pas_s;
  const pasT = [60, 120, 300, 600, 900, 1800, 3600].find((p) => total / p <= 6) || 7200;
  for (let t = 0; t <= total; t += pasT) {
    const i = Math.min(n - 1, Math.round(t / s.pas_s));
    lignes.push(`<text class="axe" x="${x(i)}" y="${H - 8}" text-anchor="${t === 0 ? 'start' : 'middle'}">${t === 0 ? '0' : `${Math.round(t / 60)} min`}</text>`);
  }
  const iMin = s.courbe.indexOf(Math.min(...s.courbe));
  const yMoy = y(s.moyenne);
  carte.innerHTML = `
    <div class="courbe-entete">
      <div><h2>${esc(s.jeu)}</h2><span class="discret">${dateCourte(s.debut)} · ${duree(s.duree_s)}</span></div>
      <div class="courbe-chiffres">
        <span><strong>${Math.round(s.moyenne)}</strong> FPS moyen</span>
        <span><strong>${Math.round(s.low1)}</strong> 1 % low</span>
        ${s.temp_gpu ? `<span><strong class="${s.temp_gpu >= 85 ? 'perte' : ''}">${s.temp_gpu} °C</strong> GPU max</span>` : ''}
        ${s.boost ? '<span class="badge accent">Boosté</span>' : '<span class="badge">Sans boost</span>'}
      </div>
    </div>
    <div class="courbe-zone">
      <svg viewBox="0 0 ${L} ${H}" class="courbe" role="img" aria-label="FPS pendant la partie de ${esc(s.jeu)} : ${Math.round(s.moyenne)} en moyenne, creux à ${s.courbe[iMin]}">
        ${lignes.join('')}
        <path class="aire" d="M${x(0)},${y(0)} L${points.join(' L')} L${x(n - 1)},${y(0)} Z"/>
        <line class="moyenne" x1="${g}" x2="${L - d}" y1="${yMoy}" y2="${yMoy}"/>
        <text class="etiquette" x="${L - d}" y="${yMoy - 6}" text-anchor="end">moyenne ${Math.round(s.moyenne)}</text>
        <polyline class="trace" points="${points.join(' ')}"/>
        <circle class="creux" cx="${x(iMin)}" cy="${y(s.courbe[iMin])}" r="4.5"/>
        <text class="etiquette" x="${Math.min(Math.max(x(iMin), g + 40), L - d - 40)}" y="${Math.min(y(s.courbe[iMin]) + 18, H - b - 4)}" text-anchor="middle">creux ${s.courbe[iMin]}</text>
        <g class="survol" hidden><line class="viseur" y1="${h}" y2="${H - b}"/><circle class="point" r="5"/></g>
        <rect class="capteur" x="${g}" y="0" width="${L - g - d}" height="${H}" tabindex="0" aria-label="Parcourir la courbe avec les flèches"/>
      </svg>
      <div class="infobulle-courbe" hidden><strong></strong><span></span></div>
    </div>`;
  const svg = carte.querySelector('svg');
  const survol = svg.querySelector('.survol');
  const bulle = carte.querySelector('.infobulle-courbe');
  let courant = n - 1;
  const montrer = (i) => {
    courant = Math.max(0, Math.min(n - 1, i));
    survol.hidden = false;
    survol.querySelector('.viseur').setAttribute('x1', x(courant));
    survol.querySelector('.viseur').setAttribute('x2', x(courant));
    survol.querySelector('.point').setAttribute('cx', x(courant));
    survol.querySelector('.point').setAttribute('cy', y(s.courbe[courant]));
    bulle.hidden = false;
    bulle.querySelector('strong').textContent = `${s.courbe[courant]} FPS`;
    bulle.querySelector('span').textContent = `à ${minutes(courant * s.pas_s)}`;
    const r = svg.getBoundingClientRect();
    const px = (x(courant) / L) * r.width;
    bulle.style.left = `${Math.min(Math.max(px, 50), r.width - 50)}px`;
    bulle.style.top = `${(y(s.courbe[courant]) / H) * r.height}px`;
  };
  const cacher = () => {
    survol.hidden = true;
    bulle.hidden = true;
  };
  const capteur = svg.querySelector('.capteur');
  capteur.addEventListener('pointermove', (e) => {
    const r = svg.getBoundingClientRect();
    const xs = ((e.clientX - r.left) / r.width) * L;
    montrer(Math.round(((xs - g) / (L - g - d)) * (n - 1)));
  });
  capteur.addEventListener('pointerleave', cacher);
  capteur.addEventListener('blur', cacher);
  capteur.addEventListener('focus', () => montrer(courant));
  capteur.addEventListener('keydown', (e) => {
    if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
      e.preventDefault();
      montrer(courant + (e.key === 'ArrowRight' ? 1 : -1) * Math.max(1, Math.round(n / 60)));
    }
  });
  document.querySelectorAll('#sessions tr[data-s]').forEach((tr) => tr.classList.toggle('choisie', sessionsFps[tr.dataset.s] === s));
}

async function chargerSessions() {
  const sessions = (await appel('sessions')).reverse();
  sessionsFps = sessions;
  $('#sessions tbody').innerHTML = sessions.length
    ? sessions
        .map(
          (s, i) => `<tr data-s="${i}" class="${s.courbe?.length ? 'cliquable' : ''}"><td>${new Date(s.debut * 1000).toLocaleString('fr-CH', { dateStyle: 'short', timeStyle: 'short' })}</td><td>${esc(s.jeu)}</td><td>${duree(s.duree_s)}</td><td>${s.boost ? '<span class="badge accent">Oui</span>' : '<span class="badge">Non</span>'}</td><td>${Math.round(s.moyenne)}</td><td>${Math.round(s.low1)}</td><td>${Math.round(s.low01)}</td><td>${s.temp_gpu ? `<span class="${s.temp_gpu >= 85 ? 'perte' : ''}">${s.temp_gpu} °C</span>` : '–'}</td></tr>`
        )
        .join('')
    : '<tr><td colspan="8" class="discret">Aucune partie mesurée pour l\'instant.</td></tr>';
  afficherCourbe(sessions.find((s) => s.courbe?.length));

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

$('#sessions').addEventListener('click', (e) => {
  const s = sessionsFps[e.target.closest('tr[data-s]')?.dataset.s];
  if (s?.courbe?.length) {
    afficherCourbe(s);
    $('#courbe-partie').scrollIntoView({ behavior: calme() ? 'auto' : 'smooth', block: 'nearest' });
  }
});

/* ---------- Clips ---------- */

let clipsEtat = null;
let clipsListe = [];
let clipsInstallation = false;

async function chargerClips() {
  [clipsEtat, clipsListe] = await Promise.all([invoke('clips_etat').catch(() => null), invoke('clips_liste').catch(() => [])]);
  // Images des jeux pour les dossiers de la galerie
  if (!vignettes.size) vignettes = new Map((await invoke('jeux_cartes').catch(() => [])).map((c) => [c.nom, c]));
  afficherClips();
}

const taillePetite = (o) => (o >= Go ? `${(o / Go).toFixed(1).replace('.', ',')} Go` : `${Math.max(1, Math.round(o / 1024 ** 2))} Mo`);

function afficherClips() {
  if (!reglages) return;
  const touche = reglages.clips_touche?.nom || 'F8';
  $('#clips-actif').checked = reglages.clips;
  $('#clips-touche').textContent = touche;
  document.querySelectorAll('#clips-duree button').forEach((b) => b.classList.toggle('actif', Number(b.dataset.duree) === reglages.clips_duree_s));
  const texte = $('#clips-etat-texte');
  texte.classList.toggle('enregistre', !!(reglages.clips && clipsEtat?.enregistrement));
  texte.textContent = !reglages.clips
    ? 'Coupé'
    : clipsInstallation
      ? 'Installation…'
      : clipsEtat?.enregistrement
        ? `Enregistrement en cours${clipsEtat.encodeur ? ` (${clipsEtat.encodeur})` : ''} · ${touche} pour garder le moment${clipsEtat.alerte_replays ? ` · ⚠ Clips auto : ${clipsEtat.alerte_replays}` : ''}`
        : clipsEtat?.probleme
          ? `Enregistrement impossible : ${clipsEtat.probleme}`
          : `Prêt : appuie sur ${touche} en jeu`;
  document.querySelectorAll('.clips-grise').forEach((o) => o.classList.toggle('inactif', !reglages.clips));
  document.querySelectorAll('#clips-qualite button').forEach((b) => b.classList.toggle('actif', b.dataset.qualite === (reglages.clips_qualite || 'haute')));
  // Poids d'une minute de clip : débit (Mbit/s) × 60 s ÷ 8, plus le son
  const debit = { standard: 12, haute: 20, ultra: 40 }[reglages.clips_qualite] || 20;
  $('#clips-poids').textContent = `≈ ${Math.round((debit * 60) / 8 + 1)} Mo par minute`;
  document.querySelectorAll('#clips-kills button').forEach((b) => b.classList.toggle('actif', (reglages.clips_auto_kills || []).includes(Number(b.dataset.kills))));
  const sons = (reglages.clips_sons || ['pc']).filter((s) => s !== 'discord');
  document.querySelectorAll('#clips-sons button').forEach((b) => b.classList.toggle('actif', sons.includes(b.dataset.son)));
  $('#clips-sons-aide').textContent = sons.length > 1 ? 'Jeu et micro sur deux pistes : volumes réglables dans l\'éditeur' : sons.length ? 'Discord est compris dans le son du jeu' : 'Clips sans son';
  const webhook = discordRegle();
  document.querySelectorAll('#discord-kills button').forEach((b) => b.classList.toggle('actif', webhook && (reglages.discord_auto_kills || []).includes(Number(b.dataset.kills))));
  $('#discord-auto').classList.toggle('sans-webhook', !webhook);
  $('#discord-auto').title = webhook ? '' : 'Règle d\'abord le partage avec un webhook Discord';
  // Résumé des réglages (réglages repliés)
  const multis = (l) => [...l].sort().map((n) => (n >= 5 ? 'ACE' : `${n}K`)).join(' ');
  const resume = [
    dureeTexte(reglages.clips_duree_s),
    reglages.clips_touche?.nom || 'F8',
    { standard: 'Standard', haute: 'Haute', ultra: 'Ultra' }[reglages.clips_qualite] || 'Haute',
    sons.length ? sons.map((s) => ({ pc: 'Jeu', micro: 'Micro' })[s]).join(' + ') : 'Sans son',
    reglages.clips_auto && reglages.clips_auto_kills?.length ? `Auto ${multis(reglages.clips_auto_kills)}` : 'Clips auto coupés',
    webhook && reglages.discord_auto_kills?.length ? `Discord ${multis(reglages.discord_auto_kills)}` : null,
  ].filter(Boolean);
  $('#clips-resume').innerHTML = resume.map((t) => `<span>${esc(t)}</span>`).join('');
  $('#clips-resume').classList.toggle('inactif', !reglages.clips);
  afficherPartage();
  if (clipsEtat) {
    $('#clips-dossier-chemin').textContent = clipsEtat.dossier;
    $('#clips-dossier-chemin').title = clipsEtat.dossier;
    $('#clips-dossier-chemin').classList.toggle('injoignable', !clipsEtat.accessible);
    $('#btn-clips-defaut').hidden = !clipsEtat.personnalise;
    if (!clipsEtat.accessible) $('#clips-dossier-chemin').title = 'Injoignable pour l\'instant : les clips iront dans Vidéos\\FPS Cagoule';
  }
  const grille = $('#clips-grille');
  grille.innerHTML = '';
  if (!clipsListe.length) {
    $('#clips-outils').hidden = true;
    grille.innerHTML = `<div class="clips-vide"><svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="6" width="13" height="12" rx="2"/><path d="M16 10.5l5-3v9l-5-3z"/></svg><span>${reglages.clips ? `En jeu, appuie sur <kbd>${esc(touche)}</kbd> pour garder les ${dureeTexte(reglages.clips_duree_s)} d'avant.` : 'Active l\'enregistrement pour garder tes meilleurs moments.'}</span></div>`;
    return;
  }
  $('#clips-outils').hidden = false;
  const recherche = vueClips.recherche.trim().toLowerCase();
  const filtres = (c) => {
    const d = detailsClip(c);
    return (
      (!vueClips.filtre || genreClip(c) === vueClips.filtre) &&
      (!vueClips.carte || d.carte === vueClips.carte) &&
      (!vueClips.operateur || d.operateur === vueClips.operateur) &&
      (!recherche || `${c.nom} ${c.jeu}`.toLowerCase().includes(recherche))
    );
  };
  document.querySelectorAll('#clips-filtres button').forEach((b) => b.classList.toggle('actif', b.dataset.filtre === vueClips.filtre));
  remplirMenus(clipsListe.filter((c) => !vueClips.jeu || c.jeu === vueClips.jeu));
  // Accueil de la galerie : un dossier par jeu (sauf pendant une recherche ou un filtre)
  if (!vueClips.jeu && !recherche && !vueClips.filtre && !vueClips.carte && !vueClips.operateur) {
    $('#clips-fil').innerHTML = `<strong>Tous les jeux</strong><span class="discret">${clipsListe.length} clip${clipsListe.length > 1 ? 's' : ''}</span>`;
    const jeux = [...new Set(clipsListe.map((c) => c.jeu))];
    jeux.forEach((jeu, i) => grille.append(dossierJeu(jeu, clipsListe.filter((c) => c.jeu === jeu), i)));
    return;
  }
  const visibles = clipsListe.filter((c) => (!vueClips.jeu || c.jeu === vueClips.jeu) && filtres(c));
  $('#clips-fil').innerHTML = `<button class="petit discret-btn" data-retour>← Tous les jeux</button><strong>${esc(vueClips.jeu || 'Résultats')}</strong><span class="discret">${visibles.length} clip${visibles.length > 1 ? 's' : ''}</span>`;
  if (!visibles.length) {
    grille.innerHTML = '<div class="clips-vide"><span>Aucun clip ne correspond.</span></div>';
    return;
  }
  visibles.forEach((c, i) => grille.append(tuileClip(c, i)));
}

/** Type de multi-kill d'après le nom du clip (« 3K · Ace · … ») */
const genreClip = (c) => c.nom.match(/^(2K|3K|4K|ACE)\b/)?.[1] || '';

/** Dossier d'un jeu : image du jeu (ou de son dernier clip) et nombre de clips */
function dossierJeu(jeu, clips, i) {
  const tuile = document.createElement('button');
  tuile.className = 'dossier-jeu';
  tuile.style.animationDelay = `${Math.min(i, 12) * 50}ms`;
  const v = vignettes.get(jeu);
  const image = v?.image && v.genre !== 'icone' ? v.image : clips.find((c) => c.miniature)?.miniature;
  const fond = document.createElement('span');
  fond.className = 'dossier-image';
  if (image) fond.style.backgroundImage = `url("${image}")`;
  const multi = ['ACE', '4K', '3K', '2K'].map((g) => [g, clips.filter((c) => genreClip(c) === g).length]).filter(([, n]) => n);
  const pied = document.createElement('span');
  pied.className = 'dossier-pied';
  pied.innerHTML = `<strong></strong><small>${clips.length} clip${clips.length > 1 ? 's' : ''}${multi.length ? ' · ' + multi.map(([g, n]) => `${n} ${g}`).join(' · ') : ''}</small>`;
  pied.querySelector('strong').textContent = jeu;
  tuile.append(fond, pied);
  tuile.addEventListener('click', () => {
    vueClips.jeu = jeu;
    afficherClips();
  });
  return tuile;
}

function tuileClip(c, i) {
  const tuile = document.createElement('div');
  tuile.className = 'clip';
  tuile.style.animationDelay = `${Math.min(i, 12) * 40}ms`;
  const visuel = document.createElement('button');
  visuel.className = 'clip-visuel';
  visuel.title = 'Regarder et monter';
  if (c.miniature) visuel.style.backgroundImage = `url("${c.miniature}")`;
  const compte = (g) => c.marqueurs.filter((m) => m.genre === g).length;
  const resume = [['kill', compte('kill')], ['ko', compte('ko')], ['mort', compte('mort')]].filter(([, n]) => n);
  visuel.innerHTML = `<span class="clip-lecture">${JOUER}</span>${
    resume.length
      ? `<span class="clip-reperes">${resume.map(([g, n]) => `<span class="pastille-repere ${g}" title="${n} ${NOMS_REPERES[g]}${n > 1 && g !== 'ko' ? 's' : ''}">${n}</span>`).join('')}</span>`
      : ''
  }`;
  visuel.addEventListener('click', () => ouvrirEditeur(c));
  const pied = document.createElement('div');
  pied.className = 'clip-pied';
  const textes = document.createElement('div');
  const nom = document.createElement('strong');
  nom.textContent = c.nom;
  nom.title = c.nom;
  const infos = document.createElement('small');
  const d = new Date(c.date * 1000);
  // Dans le dossier d'un jeu, son nom est déjà en haut : pas besoin de le répéter sur chaque clip
  infos.textContent = `${vueClips.jeu ? '' : `${c.jeu} · `}${d.toLocaleDateString('fr-CH', { day: '2-digit', month: '2-digit' })} ${d.toLocaleTimeString('fr-CH', { hour: '2-digit', minute: '2-digit' })} · ${taillePetite(c.octets)}`;
  textes.append(nom, infos);
  const suppr = document.createElement('button');
  suppr.className = 'petit discret-btn clip-suppr';
  suppr.title = 'Supprimer';
  suppr.setAttribute('aria-label', `Supprimer ${c.nom}`);
  suppr.textContent = '✕';
  suppr.addEventListener('click', async () => {
    if (!(await confirmer('Supprimer ce clip', `<p>${esc(c.nom)}</p>`, 'Supprimer'))) return;
    await appel('clips_supprimer', { chemin: c.chemin }).catch(() => {});
    chargerClips();
  });
  const partager = document.createElement('button');
  partager.className = `petit discret-btn clip-partager${c.lien ? ' actif' : ''}`;
  partager.title = c.lien ? 'Partagé : copier le lien' : 'Partager (copie un lien)';
  partager.setAttribute('aria-label', partager.title);
  partager.innerHTML = ICONE_LIEN;
  partager.addEventListener('click', async () => {
    await partagerClip(c, partager);
    partager.classList.toggle('actif', !!c.lien);
  });
  pied.append(textes, partager, suppr);
  tuile.append(visuel, pied);
  return tuile;
}

/* ---------- Partage des clips par un lien (dossier publié par ton site, ex. clips.lucas-chvnn.ch) ---------- */
const ICONE_LIEN = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M10 14a4.5 4.5 0 0 0 6.4 0l3-3a4.5 4.5 0 0 0-6.4-6.4l-1 1"/><path d="M14 10a4.5 4.5 0 0 0-6.4 0l-3 3a4.5 4.5 0 0 0 6.4 6.4l1-1"/></svg>';
const partageRegle = () => !!(reglages?.partage_dossier && reglages?.partage_url);

async function copierLien(lien) {
  try {
    await navigator.clipboard.writeText(lien);
    toast(`Lien copié : ${lien}`, 5000);
  } catch {
    toast(lien, 9000);
  }
}

async function partagerClip(c, bouton) {
  if (!partageRegle()) {
    editerPartage(true);
    $('#partage-dossier').scrollIntoView({ behavior: 'smooth', block: 'center' });
    return toast('Règle d\'abord où partager tes clips.');
  }
  if (c.lien) return copierLien(c.lien);
  dernierPourcent = -1;
  toast('Préparation du clip…', 20000);
  try {
    const r = await appel('clips_partager', { chemin: c.chemin, titre: c.nom }, bouton);
    c.lien = r.lien;
    try { await navigator.clipboard.writeText(r.lien); } catch {}
    if (!r.discord) toast(`Lien copié : ${r.lien}`, 5000);
    else if ('Ok' in r.discord) toast('Posté sur Discord, lien copié.', 5000);
    else toast(`Lien copié, mais Discord a refusé : ${r.discord.Err}`, 8000);
  } catch {}
}

// Avancement de la préparation du clip (conversion pour le web puis copie sur le serveur)
let dernierPourcent = -1;
listen('partage_progression', (e) => {
  const p = Math.round(e.payload.fraction * 100);
  if (p === dernierPourcent) return;
  dernierPourcent = p;
  toast(p === 50 ? 'Copie du clip sur le serveur…' : p < 92 ? `Préparation du clip… ${p} %` : 'Envoi sur le serveur…', 20000);
});

function afficherPartage() {
  $('#partage-resume').textContent = partageRegle() ? `${reglages.partage_url}${reglages.partage_discord ? ' · posté sur Discord' : ''}` : 'Pas encore réglé';
}
function editerPartage(ouvrir) {
  $('#partage-edition').hidden = !ouvrir;
  $('.clips-partage').hidden = ouvrir;
  if (ouvrir) {
    $('#partage-dossier').value = reglages.partage_dossier || '';
    $('#partage-url').value = reglages.partage_url || '';
    $('#partage-discord').value = reglages.partage_discord || '';
    $('#partage-dossier').focus();
  }
}
$('#btn-partage-regler').addEventListener('click', () => editerPartage(true));
$('#btn-partage-annuler').addEventListener('click', () => editerPartage(false));
$('#btn-partage-valider').addEventListener('click', async (e) => {
  const dossier = $('#partage-dossier').value.trim().replace(/^"|"$/g, '') || null;
  const url = $('#partage-url').value.trim().replace(/\/+$/, '') || null;
  const discord = $('#partage-discord').value.trim() || null;
  try {
    const r = await appel('partage_regler', { dossier, url, discord }, e.currentTarget);
    reglages.partage_dossier = r[0];
    reglages.partage_url = r[1];
    reglages.partage_discord = r[2];
    editerPartage(false);
    afficherPartage();
    toast(partageRegle() ? 'Partage prêt : clique sur 🔗 sur un clip.' : 'Partage coupé.');
    chargerClips();
  } catch {}
});

const vueClips = { jeu: null, filtre: '', recherche: '', carte: '', operateur: '' };

/** Cartes de R6 (le reste d'un nom de clip auto est l'opérateur) */
const CARTES_R6 = new Set(['Bank', 'Bartlett', 'Border', 'Chalet', 'Clubhouse', 'Coastline', 'Consulate', 'Emerald Plains', 'Favela', 'Fortress', 'Hereford', 'House', 'Kafe', 'Kanal', 'Lair', 'Nighthaven Labs', 'Oregon', 'Outback', 'Presidential Plane', 'Skyscraper', 'Stadium', 'Stadium Bravo', 'Theme Park', 'Tower', 'Villa', 'Yacht', 'Litoral']);

/** Carte et opérateur d'après le nom d'un clip auto (« 3K · Rauora · Outback · Pseudo », opérateur ou carte parfois absents) */
function detailsClip(c) {
  const t = c.nom.split(' · ').map((x) => x.trim());
  if (t.length < 3 || !/^(2K|3K|4K|ACE)$/.test(t[0])) return { carte: '', operateur: '' };
  const milieu = t.slice(1, -1);
  const carte = milieu.find((x) => CARTES_R6.has(x)) || (milieu.length === 2 ? milieu[1] : '');
  const operateur = milieu.find((x) => x !== carte) || '';
  return { carte, operateur };
}

/** Menus Carte / Opérateur : remplis avec ce qu'il y a dans les clips (du jeu ouvert), les plus fréquents d'abord */
function remplirMenus(clips) {
  for (const [cle, id, tous] of [['carte', '#clips-carte', 'Toutes les cartes'], ['operateur', '#clips-operateur', 'Tous les opérateurs']]) {
    const comptes = new Map();
    for (const c of clips) {
      const v = detailsClip(c)[cle];
      if (v) comptes.set(v, (comptes.get(v) || 0) + 1);
    }
    if (vueClips[cle] && !comptes.has(vueClips[cle])) comptes.set(vueClips[cle], 0);
    const menu = $(id);
    menu.innerHTML = `<option value="">${tous}</option>` + [...comptes].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map(([v, n]) => `<option value="${esc(v)}">${esc(v)} (${n})</option>`).join('');
    menu.value = vueClips[cle];
    menu.hidden = comptes.size === 0;
    menu.classList.toggle('actif', !!vueClips[cle]);
  }
}
for (const [cle, id] of [['carte', '#clips-carte'], ['operateur', '#clips-operateur']]) {
  $(id).addEventListener('change', (e) => {
    vueClips[cle] = e.target.value;
    afficherClips();
  });
}

$('#clips-recherche').addEventListener('input', (e) => {
  vueClips.recherche = e.target.value;
  afficherClips();
});
$('#clips-filtres').addEventListener('click', (e) => {
  const b = e.target.closest('[data-filtre]');
  if (!b) return;
  vueClips.filtre = vueClips.filtre === b.dataset.filtre ? '' : b.dataset.filtre;
  afficherClips();
});
$('#clips-fil').addEventListener('click', (e) => {
  if (!e.target.closest('[data-retour]')) return;
  Object.assign(vueClips, { jeu: null, filtre: '', recherche: '', carte: '', operateur: '' });
  $('#clips-recherche').value = '';
  afficherClips();
});

const dureeTexte = (s) => (s >= 60 ? `${s / 60} min` : `${s} s`);

async function reglerClips(changements) {
  const voulu = { actif: reglages.clips, dureeS: reglages.clips_duree_s, touche: reglages.clips_touche, ...changements };
  try {
    await appel('clips_regler', voulu);
    Object.assign(reglages, { clips: voulu.actif, clips_duree_s: voulu.dureeS, clips_touche: voulu.touche });
    return true;
  } catch {
    return false;
  } finally {
    afficherClips();
  }
}

$('#clips-actif').addEventListener('change', async (e) => {
  const actif = e.target.checked;
  if (actif && !clipsEtat?.installe) {
    clipsInstallation = true;
    $('#clips-installation').hidden = false;
    afficherClips();
    try {
      await invoke('clips_installer');
      clipsEtat = await invoke('clips_etat');
    } catch (err) {
      toast(String(err), 8000);
      e.target.checked = false;
      return;
    } finally {
      clipsInstallation = false;
      $('#clips-installation').hidden = true;
    }
  }
  if (await reglerClips({ actif })) toast(actif ? `Clips activés : en jeu, ${reglages.clips_touche.nom} garde les ${dureeTexte(reglages.clips_duree_s)} d'avant.` : 'Clips coupés.', 5000);
});

listen('clips_installation', (e) => {
  const { recu, total } = e.payload;
  $('#clips-barre').style.width = `${Math.round((recu / total) * 100)}%`;
  $('#clips-installation-texte').textContent = `Téléchargement de l'enregistreur… ${Math.round(recu / 1024 ** 2)} / ${Math.round(total / 1024 ** 2)} Mo`;
});

$('#clips-duree').addEventListener('click', (e) => {
  const b = e.target.closest('[data-duree]');
  if (b) reglerClips({ dureeS: Number(b.dataset.duree) });
});

/** Touches utilisables seules (comme côté moteur) : F1–F24, Pause, Arrêt défil, Inser, Début, Fin, Page, pavé numérique */
const toucheSeulePermise = (vk) => (vk >= 0x70 && vk <= 0x87) || [0x13, 0x91, 0x2d, 0x24, 0x23, 0x21, 0x22].includes(vk) || (vk >= 0x60 && vk <= 0x6f);

$('#clips-touche').addEventListener('click', (e) => {
  const bouton = e.currentTarget;
  bouton.classList.add('ecoute');
  bouton.textContent = 'Appuie sur une touche…';
  const ecouter = async (ev) => {
    if (['Shift', 'Control', 'Alt', 'Meta', 'AltGraph'].includes(ev.key)) return;
    ev.preventDefault();
    ev.stopPropagation();
    window.removeEventListener('keydown', ecouter, true);
    bouton.classList.remove('ecoute');
    if (ev.key === 'Escape') return afficherClips();
    const modificateurs = (ev.altKey ? 1 : 0) | (ev.ctrlKey ? 2 : 0) | (ev.shiftKey ? 4 : 0);
    const vk = ev.keyCode;
    if (!modificateurs && !toucheSeulePermise(vk)) {
      toast('Choisis une touche F1 à F12, ou une touche avec Ctrl, Alt ou Maj : une lettre seule bloquerait ta frappe.', 7000);
      return afficherClips();
    }
    const nomTouche = ev.code.startsWith('Numpad') ? `Pavé ${ev.code.slice(6)}` : ev.key.length === 1 ? ev.key.toUpperCase() : ev.key;
    const nom = [ev.ctrlKey && 'Ctrl', ev.altKey && 'Alt', ev.shiftKey && 'Maj', nomTouche].filter(Boolean).join(' + ');
    if (await reglerClips({ touche: { modificateurs, vk, nom } })) toast(`Touche des clips : ${nom}`);
  };
  window.addEventListener('keydown', ecouter, true);
});

$('#clips-qualite').addEventListener('click', (e) => {
  const b = e.target.closest('[data-qualite]');
  if (!b) return;
  reglages.clips_qualite = b.dataset.qualite;
  sauverReglages();
  afficherClips();
  if (clipsEtat?.enregistrement) toast('Nouvelle qualité dès ta prochaine partie.');
});

function discordRegle() {
  return !!(partageRegle() && reglages.partage_discord);
}

/* Réglages des clips : repliés en un résumé, dépliés en trois blocs */
function ouvrirReglagesClips(ouvert) {
  $('#clips-corps').hidden = !ouvert;
  $('#clips-resume').hidden = ouvert;
  $('#btn-clips-reglages').setAttribute('aria-expanded', String(ouvert));
  $('#btn-clips-reglages').classList.toggle('actif', ouvert);
  try { localStorage.setItem('clipsReglages', ouvert ? '1' : '0'); } catch {}
}
$('#btn-clips-reglages').addEventListener('click', () => ouvrirReglagesClips($('#clips-corps').hidden));
$('#clips-resume').addEventListener('click', () => ouvrirReglagesClips(true));
try { ouvrirReglagesClips(localStorage.getItem('clipsReglages') === '1'); } catch { ouvrirReglagesClips(false); }

$('#clips-sons').addEventListener('click', (e) => {
  const b = e.target.closest('[data-son]');
  if (!b) return;
  const liste = new Set(reglages.clips_sons || ['pc']);
  liste.has(b.dataset.son) ? liste.delete(b.dataset.son) : liste.add(b.dataset.son);
  reglages.clips_sons = ['pc', 'micro'].filter((s) => liste.has(s));
  sauverReglages();
  afficherClips();
  if (clipsEtat?.enregistrement) toast('Nouveaux sons dès ta prochaine partie.');
});

$('#discord-kills').addEventListener('click', (e) => {
  const b = e.target.closest('[data-kills]');
  if (!b) return;
  if (!discordRegle()) return toast('Règle d\'abord le partage avec un webhook Discord (Partage → Régler).');
  const n = Number(b.dataset.kills);
  const liste = new Set(reglages.discord_auto_kills || []);
  liste.has(n) ? liste.delete(n) : liste.add(n);
  reglages.discord_auto_kills = [...liste].sort();
  sauverReglages();
  afficherClips();
});

listen('clip_poste', (e) => {
  const { titre, erreur } = e.payload;
  toast(erreur ? `Discord : ${erreur}` : `« ${titre} » posté sur Discord.`, 6000);
  chargerClips();
});

$('#clips-kills').addEventListener('click', (e) => {
  const b = e.target.closest('[data-kills]');
  if (!b) return;
  const n = Number(b.dataset.kills);
  const liste = new Set(reglages.clips_auto_kills || []);
  liste.has(n) ? liste.delete(n) : liste.add(n);
  reglages.clips_auto_kills = [...liste].sort();
  sauverReglages();
  afficherClips();
});

// Dossier des clips : chemin tapé ou collé (\\serveur\partage), ou choisi avec Parcourir
function editerDossier(ouvrir) {
  $('#clips-dossier-edition').hidden = !ouvrir;
  $('#clips-connexion').hidden = true;
  $('.clips-dossier').hidden = ouvrir;
  if (ouvrir) {
    const saisie = $('#clips-dossier-saisie');
    saisie.value = clipsEtat?.dossier || '';
    saisie.focus();
    saisie.select();
  }
}
async function validerDossier(bouton, connexion = false) {
  const dossier = $('#clips-dossier-saisie').value.trim().replace(/^"|"$/g, '');
  if (!dossier) return;
  const args = { dossier, identifiant: null, motDePasse: null };
  if (connexion) {
    args.identifiant = $('#clips-identifiant').value.trim();
    args.motDePasse = $('#clips-mdp').value;
    if (!args.identifiant) return $('#clips-identifiant').focus();
  }
  bouton?.classList.add('chargement');
  try {
    const garde = await invoke('clips_dossier_regler', args);
    reglages.clips_dossier = garde;
    $('#clips-mdp').value = '';
    $('#clips-connexion').hidden = true;
    toast(`Les clips iront dans ${garde}`, 5000);
    editerDossier(false);
    chargerClips();
  } catch (e) {
    const msg = String(e);
    if (msg.startsWith('connexion:')) {
      // Le serveur veut un identifiant : on le demande une fois, Windows le retient ensuite
      $('#clips-dossier-saisie').value = msg.slice('connexion:'.length);
      ouvrirReglagesClips(true);
      $('#clips-connexion').hidden = false;
      $('#clips-identifiant').focus();
    } else toast(`Erreur : ${msg}`, 7000);
  } finally {
    bouton?.classList.remove('chargement');
  }
}
$('#btn-clips-changer').addEventListener('click', () => editerDossier(true));
$('#btn-clips-annuler-dossier').addEventListener('click', () => editerDossier(false));
$('#btn-clips-connexion').addEventListener('click', (e) => validerDossier(e.currentTarget, true));
['#clips-identifiant', '#clips-mdp'].forEach((id) =>
  $(id).addEventListener('keydown', (e) => e.key === 'Enter' && validerDossier($('#btn-clips-connexion'), true))
);
$('#btn-clips-valider').addEventListener('click', (e) => validerDossier(e.currentTarget));
$('#clips-dossier-saisie').addEventListener('keydown', (e) => {
  if (e.key === 'Enter') validerDossier($('#btn-clips-valider'));
  if (e.key === 'Escape') editerDossier(false);
});
$('#btn-clips-parcourir').addEventListener('click', async (e) => {
  const choix = await appel('clips_parcourir', {}, e.currentTarget).catch(() => null);
  if (choix) $('#clips-dossier-saisie').value = choix;
});
$('#btn-clips-defaut').addEventListener('click', async (e) => {
  await appel('clips_dossier_regler', { dossier: null }, e.currentTarget).catch(() => {});
  reglages.clips_dossier = null;
  chargerClips();
});

$('#btn-clips-dossier').addEventListener('click', (e) => appel('clips_ouvrir', { chemin: null }, e.currentTarget).catch(() => {}));
$('#btn-clips-journal').addEventListener('click', (e) => appel('clips_journal', {}, e.currentTarget).catch(() => {}));

listen('clip', () => {
  toast('Clip enregistré', 3000);
  if ($('#page-clips').classList.contains('actif')) chargerClips();
});
listen('clip_erreur', (e) => toast(`Pas de clip : ${e.payload}`, 9000));
// Miniatures faites pour des vidéos ajoutées à la main : on rafraîchit la galerie
listen('clips_miniatures', () => {
  if ($('#page-clips').classList.contains('actif')) chargerClips();
});
listen('clip_secours', () => toast('Dossier des clips injoignable (serveur éteint ?) : clip gardé dans Vidéos\\FPS Cagoule.', 7000));

/* ---------- Éditeur de clips ---------- */

const ICONES_REPERES = {
  kill: '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="7"/><path d="M12 2v5M12 17v5M2 12h5M17 12h5"/></svg>',
  ko: '<svg viewBox="0 0 24 24"><path d="M12 4v13M6 12l6 6 6-6"/></svg>',
  mort: '<svg viewBox="0 0 24 24"><path d="M12 3a7 7 0 0 0-7 7c0 2.4 1.2 4 2.5 5v3h9v-3c1.3-1 2.5-2.6 2.5-5a7 7 0 0 0-7-7z"/><circle cx="9.5" cy="10.5" r="1.3"/><circle cx="14.5" cy="10.5" r="1.3"/></svg>',
};
const NOMS_REPERES = { kill: 'Kill', ko: 'KO', mort: 'Mort' };
const ed = { clip: null, duree: 0, passages: [], choisi: -1, historique: [], zone: null };
const video = $('#ed-video');

const horloge = (s) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`;

function majPartageEditeur() {
  $('#ed-partager').title = ed.clip?.lien ? 'Copier le lien du clip' : 'Copier un lien vers ce clip';
  $('#ed-partager').lastChild.textContent = ed.clip?.lien ? ' Copier le lien' : ' Partager';
  $('#ed-plus-partager').hidden = !ed.clip?.lien;
  $('#ed-discord').hidden = !(ed.clip?.lien && reglages?.partage_discord);
}
$('#ed-discord').addEventListener('click', async (e) => {
  try {
    await appel('clips_poster_discord', { chemin: ed.clip.chemin, titre: ed.clip.nom }, e.currentTarget);
    toast('Posté sur Discord.');
  } catch {}
});
$('#ed-partager').addEventListener('click', async (e) => {
  await partagerClip(ed.clip, e.currentTarget);
  majPartageEditeur();
});
$('#ed-plus-partager').addEventListener('click', async (e) => {
  try {
    await appel('clips_ne_plus_partager', { chemin: ed.clip.chemin }, e.currentTarget);
    ed.clip.lien = null;
    majPartageEditeur();
    toast('Le lien ne marche plus.');
    chargerClips();
  } catch {}
});

function ouvrirEditeur(c) {
  arreterSons();
  Object.assign(ed, { clip: c, duree: 0, passages: [], choisi: -1, historique: [], zone: null });
  $('#editeur-titre').textContent = c.nom;
  majPartageEditeur();
  $('#ed-progression').hidden = true;
  video.src = window.__TAURI__.core.convertFileSrc(c.chemin);
  $('#editeur').hidden = false;
  dessinerEditeur();
  chargerSons(c);
}

/* Sons séparés (jeu, Discord, micro) : chacun joué à part, calé sur la vidéo, avec son volume */
async function chargerSons(c) {
  let r;
  try {
    r = await invoke('clips_pistes', { chemin: c.chemin });
  } catch {
    return;
  }
  if (ed.clip !== c || !r.titres.length) return;
  const audios = r.fichiers.map((f) => {
    const a = new Audio(window.__TAURI__.core.convertFileSrc(f));
    a.preload = 'auto';
    return a;
  });
  const volumes = r.volumes.map((v) => Math.round(v * 100));
  ed.sons = { audios, volumes, initiaux: [...volumes] };
  $('#ed-sons').innerHTML =
    '<span class="ed-zone-titre">Sons</span>' +
    r.titres
      .map((t, i) => `<label class="ed-son">${esc(t || `Son ${i + 1}`)}<input type="range" min="0" max="150" step="5" value="${volumes[i]}" data-i="${i}" aria-label="Volume : ${esc(t)}"><output>${volumes[i]} %</output></label>`)
      .join('');
  $('#ed-sons').hidden = false;
  appliquerVolume();
  synchroniserSons(true);
  dessinerEditeur();
}

function arreterSons() {
  for (const a of ed.sons?.audios || []) {
    a.pause();
    a.removeAttribute('src');
    a.load();
  }
  ed.sons = null;
  $('#ed-sons').hidden = true;
  $('#ed-sons').innerHTML = '';
  appliquerVolume(); // le son revient à la vidéo
}

function sonsModifies() {
  return !!ed.sons && ed.sons.volumes.some((v, i) => v !== ed.sons.initiaux[i]);
}

function synchroniserSons(forcer) {
  if (!ed.sons) return;
  for (const a of ed.sons.audios) {
    if (forcer || Math.abs(a.currentTime - video.currentTime) > 0.15) {
      try { a.currentTime = video.currentTime; } catch {}
    }
    a.playbackRate = video.playbackRate;
    if (video.paused || video.seeking) a.pause();
    else if (a.paused) a.play().catch(() => {});
  }
}
for (const ev of ['play', 'playing', 'pause', 'seeked']) video.addEventListener(ev, () => synchroniserSons(true));
video.addEventListener('timeupdate', () => synchroniserSons(false));
video.addEventListener('ratechange', () => synchroniserSons(false));
video.addEventListener('waiting', () => ed.sons?.audios.forEach((a) => a.pause()));

$('#ed-sons').addEventListener('input', (e) => {
  const i = Number(e.target.dataset.i);
  if (!ed.sons || Number.isNaN(i)) return;
  ed.sons.volumes[i] = Number(e.target.value);
  e.target.nextElementSibling.textContent = `${e.target.value} %`;
  appliquerVolume();
  dessinerEditeur();
});

function fermerEditeur() {
  if (pleinEcran) basculerPleinEcran();
  arreterSons();
  video.pause();
  video.removeAttribute('src');
  video.load();
  $('#editeur').hidden = true;
}

video.addEventListener('loadedmetadata', () => {
  ed.duree = video.duration || 0;
  ed.passages = [{ a: 0, b: ed.duree, garde: true }];
  dessinerEditeur();
});

/** Durée gardée et morceaux à exporter (passages gardés voisins fusionnés) */
function morceauxGardes() {
  const m = [];
  for (const p of ed.passages) {
    if (!p.garde) continue;
    const dernier = m[m.length - 1];
    if (dernier && Math.abs(dernier[1] - p.a) < 0.01) dernier[1] = p.b;
    else m.push([p.a, p.b]);
  }
  return m;
}

function dessinerEditeur() {
  const d = ed.duree || 1;
  $('#ed-passages').innerHTML = ed.passages
    .map((p, i) => `<div class="ed-passage${p.garde ? '' : ' retire'}${i === ed.choisi ? ' choisi' : ''}" data-i="${i}" style="left:${(p.a / d) * 100}%;width:${((p.b - p.a) / d) * 100}%"></div>`)
    .join('');
  $('#ed-reperes').innerHTML = (ed.clip?.marqueurs || [])
    .filter((m) => m.t <= d + 0.5)
    .map(
      (m) =>
        `<button class="ed-repere ${m.genre}" style="left:${(m.t / d) * 100}%" data-t="${m.t}" title="${NOMS_REPERES[m.genre] || m.genre} · ${esc(m.detail)}${m.headshot ? ' · headshot' : ''} (${horloge(m.t)})">${ICONES_REPERES[m.genre] || ''}</button>`
    )
    .join('');
  const garde = morceauxGardes().reduce((t, [a, b]) => t + b - a, 0);
  $('#ed-final').textContent = ed.duree ? `Montage : ${horloge(garde)}` : '';
  const sansCoupe = ed.passages.every((p) => p.garde);
  $('#ed-exporter').disabled = !ed.duree || garde < 0.2 || (sansCoupe && !sonsModifies());
  $('#ed-exporter').textContent = sansCoupe && sonsModifies() ? 'Enregistrer les sons' : 'Exporter';
  $('#ed-supprimer').disabled = !ed.duree;
  $('#ed-annuler').disabled = !ed.historique.length;
  dessinerZone();
  majTete();
}

/* Passage choisi (début → fin) : à retirer, ou à garder seul */
const lireHeure = (texte) => {
  const m = String(texte).trim().replace(',', '.').match(/^(?:(\d+):)?(\d+(?:\.\d+)?)$/);
  return m ? Number(m[1] || 0) * 60 + Number(m[2]) : null;
};
const ecrireHeure = (s) => `${Math.floor(s / 60)}:${(s % 60).toFixed(1).padStart(4, '0')}`;

function dessinerZone() {
  const z = ed.zone, d = ed.duree || 1;
  const valide = z && z.b - z.a >= 0.2;
  $('#ed-selection').hidden = !valide;
  if (valide) Object.assign($('#ed-selection').style, { left: `${(z.a / d) * 100}%`, width: `${((z.b - z.a) / d) * 100}%` });
  for (const bord of ['a', 'b']) {
    const champ = $(`#ed-${bord}`);
    if (document.activeElement !== champ) champ.value = z?.[bord] != null ? ecrireHeure(z[bord]) : '';
    champ.classList.remove('faux');
  }
  $('#ed-retirer-zone').disabled = $('#ed-garder-zone').disabled = !valide;
}

function borneZone(bord, t) {
  t = Math.min(Math.max(t, 0), ed.duree);
  const z = ed.zone || { a: bord === 'a' ? t : 0, b: bord === 'b' ? t : ed.duree };
  z[bord] = t;
  if (z.a > z.b) [z.a, z.b] = [z.b, z.a];
  ed.zone = z;
  dessinerZone();
}

/** Coupe les passages aux deux bords puis marque ceux du milieu (retirer) ou ceux d'autour (garder seul) */
function appliquerZone(garderSeul) {
  const z = ed.zone;
  if (!z || z.b - z.a < 0.2) return;
  memoriser();
  for (const t of [z.a, z.b]) {
    const i = passageA(t);
    if (i < 0) continue;
    const p = ed.passages[i];
    if (t - p.a > 0.05 && p.b - t > 0.05) ed.passages.splice(i, 1, { a: p.a, b: t, garde: p.garde }, { a: t, b: p.b, garde: p.garde });
  }
  for (const p of ed.passages) {
    const dedans = p.a >= z.a - 0.01 && p.b <= z.b + 0.01;
    if (garderSeul ? !dedans : dedans) p.garde = false;
  }
  toast(garderSeul ? `Seul le passage ${horloge(z.a)} → ${horloge(z.b)} est gardé.` : `Passage ${horloge(z.a)} → ${horloge(z.b)} retiré.`);
  ed.zone = null;
  ed.choisi = -1;
  video.currentTime = garderSeul ? z.a : Math.min(z.b, ed.duree);
  dessinerEditeur();
}

$('#ed-debut').addEventListener('click', () => borneZone('a', video.currentTime));
$('#ed-fin').addEventListener('click', () => borneZone('b', video.currentTime));
$('#ed-retirer-zone').addEventListener('click', () => appliquerZone(false));
$('#ed-garder-zone').addEventListener('click', () => appliquerZone(true));
for (const bord of ['a', 'b']) {
  const champ = $(`#ed-${bord}`);
  const valider = () => {
    if (!champ.value.trim()) return;
    const t = lireHeure(champ.value);
    if (t == null || t > ed.duree + 0.05) return champ.classList.add('faux');
    borneZone(bord, t);
    video.currentTime = Math.min(t, ed.duree);
  };
  champ.addEventListener('change', valider);
  champ.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') { valider(); champ.blur(); }
    e.stopPropagation();
  });
}
// Poignées du passage : glisser pour ajuster le début ou la fin
$('#ed-selection').addEventListener('pointerdown', (e) => {
  const poignee = e.target.closest('.ed-poignee');
  if (!poignee) return;
  e.stopPropagation();
  e.preventDefault();
  const bord = poignee.dataset.bord;
  const glisser = (ev) => {
    const t = positionTimeline(ev);
    ed.zone[bord] = t;
    if (ed.zone.a > ed.zone.b) [ed.zone.a, ed.zone.b] = [ed.zone.b, ed.zone.a];
    video.currentTime = t;
    dessinerZone();
    majTete();
  };
  window.addEventListener('pointermove', glisser);
  window.addEventListener('pointerup', () => window.removeEventListener('pointermove', glisser), { once: true });
});

/* Volume du clip : retenu d'un clip à l'autre (60 % au départ, les clips sont souvent forts) */
const ICONE_SON = (n) =>
  `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 9h4l5-4v14l-5-4H4z"/>${
    n === 0 ? '<path d="M17 9l5 6M22 9l-5 6"/>' : `<path d="M16.5 9.5a3.5 3.5 0 0 1 0 5"/>${n > 50 ? '<path d="M19 7a7 7 0 0 1 0 10"/>' : ''}`
  }</svg>`;
const son = { niveau: 60, muet: false };
/** Volume général : sur la vidéo, ou sur chaque son séparé (× son propre volume) */
function appliquerVolume() {
  const coupe = son.muet || son.niveau === 0;
  if (ed.sons) {
    video.muted = true;
    ed.sons.audios.forEach((a, i) => (a.volume = coupe ? 0 : Math.min(1, (son.niveau / 100) * (ed.sons.volumes[i] / 100))));
  } else {
    video.volume = son.niveau / 100;
    video.muted = coupe;
  }
}
function regleVolume(niveau, muet = son.muet) {
  niveau = Math.min(Math.max(Math.round(niveau), 0), 100);
  son.niveau = niveau;
  son.muet = muet || niveau === 0;
  appliquerVolume();
  $('#ed-volume').value = niveau;
  $('#ed-volume').style.setProperty('--niveau', `${niveau}%`);
  $('#ed-muet').innerHTML = ICONE_SON(son.muet ? 0 : niveau);
  $('#ed-muet').title = son.muet ? 'Remettre le son (M)' : 'Couper le son (M)';
  try { localStorage.setItem('volumeClips', JSON.stringify({ niveau, muet: son.muet && niveau > 0 })); } catch {}
}
try {
  const v = JSON.parse(localStorage.getItem('volumeClips') || 'null');
  regleVolume(v?.niveau ?? 60, !!v?.muet);
} catch {
  regleVolume(60, false);
}
$('#ed-volume').addEventListener('input', (e) => regleVolume(Number(e.target.value), false));
$('#ed-muet').addEventListener('click', () => regleVolume(Number($('#ed-volume').value) || 60, !son.muet));
video.addEventListener('wheel', (e) => {
  e.preventDefault();
  regleVolume(Number($('#ed-volume').value) + (e.deltaY < 0 ? 5 : -5), false);
}, { passive: false });

/* Plein écran : la fenêtre de l'app passe en plein écran et l'éditeur la remplit */
let pleinEcran = false;
async function basculerPleinEcran() {
  pleinEcran = !pleinEcran;
  $('#editeur').classList.toggle('plein', pleinEcran);
  $('#ed-plein').title = pleinEcran ? 'Quitter le plein écran (F ou Échap)' : 'Plein écran (F)';
  try {
    await window.__TAURI__.window.getCurrentWindow().setFullscreen(pleinEcran);
  } catch {}
}
$('#ed-plein').addEventListener('click', basculerPleinEcran);
video.addEventListener('dblclick', basculerPleinEcran);

function majTete() {
  const d = ed.duree || 1;
  $('#ed-tete').style.left = `${(video.currentTime / d) * 100}%`;
  $('#ed-temps').textContent = `${horloge(video.currentTime)} / ${horloge(ed.duree)}`;
  $('#ed-lecture').innerHTML = video.paused ? JOUER : '<svg viewBox="0 0 24 24"><rect x="6" y="5" width="4" height="14" rx="1"/><rect x="14" y="5" width="4" height="14" rx="1"/></svg>';
}

function memoriser() {
  ed.historique.push(ed.passages.map((p) => ({ ...p })));
  if (ed.historique.length > 50) ed.historique.shift();
}

const passageA = (t) => ed.passages.findIndex((p) => t >= p.a && t < p.b);

function couper() {
  const t = video.currentTime;
  const i = passageA(t);
  if (i < 0) return;
  const p = ed.passages[i];
  if (t - p.a < 0.2 || p.b - t < 0.2) return toast('Trop près d\'une coupe existante.');
  memoriser();
  ed.passages.splice(i, 1, { a: p.a, b: t, garde: p.garde }, { a: t, b: p.b, garde: p.garde });
  ed.choisi = i + 1;
  dessinerEditeur();
}

function basculerPassage() {
  const i = ed.choisi >= 0 ? ed.choisi : passageA(video.currentTime);
  if (i < 0) return;
  memoriser();
  ed.passages[i].garde = !ed.passages[i].garde;
  dessinerEditeur();
}

function annuler() {
  const precedent = ed.historique.pop();
  if (!precedent) return;
  ed.passages = precedent;
  ed.choisi = -1;
  dessinerEditeur();
}

// Lecture : les passages retirés sont sautés
function sauterRetires() {
  const i = passageA(video.currentTime);
  if (i >= 0 && !ed.passages[i].garde) {
    const suivant = ed.passages.slice(i).find((p) => p.garde);
    if (suivant) video.currentTime = suivant.a;
    else video.pause();
  }
  majTete();
}
video.addEventListener('timeupdate', sauterRetires);
video.addEventListener('play', function boucle() {
  if (video.paused || $('#editeur').hidden) return majTete();
  sauterRetires();
  requestAnimationFrame(boucle);
});
video.addEventListener('pause', majTete);

function positionTimeline(e) {
  const r = $('#ed-timeline').getBoundingClientRect();
  return Math.min(Math.max((e.clientX - r.left) / r.width, 0), 1) * ed.duree;
}
$('#ed-timeline').addEventListener('pointerdown', (e) => {
  const repere = e.target.closest('.ed-repere');
  if (repere) {
    video.currentTime = Math.max(0, Number(repere.dataset.t) - 2);
    return majTete();
  }
  const passage = e.target.closest('.ed-passage');
  ed.choisi = passage ? Number(passage.dataset.i) : -1;
  video.currentTime = positionTimeline(e);
  const glisser = (ev) => {
    video.currentTime = positionTimeline(ev);
    majTete();
  };
  window.addEventListener('pointermove', glisser);
  window.addEventListener('pointerup', () => window.removeEventListener('pointermove', glisser), { once: true });
  dessinerEditeur();
});

$('#ed-lecture').addEventListener('click', () => (video.paused ? video.play() : video.pause()));
video.addEventListener('click', () => (video.paused ? video.play() : video.pause()));
$('#ed-couper').addEventListener('click', couper);
$('#ed-supprimer').addEventListener('click', basculerPassage);
$('#ed-annuler').addEventListener('click', annuler);
$('#ed-fermer').addEventListener('click', fermerEditeur);
$('#ed-externe').addEventListener('click', () => appel('clips_ouvrir', { chemin: ed.clip.chemin }).catch(() => {}));
$('#editeur').addEventListener('click', (e) => e.target === e.currentTarget && fermerEditeur());

document.addEventListener('keydown', (e) => {
  if ($('#editeur').hidden || e.target.matches('input, select, textarea')) return;
  const touches = {
    ' ': () => (video.paused ? video.play() : video.pause()),
    s: couper,
    S: couper,
    Delete: basculerPassage,
    Backspace: basculerPassage,
    ArrowLeft: () => (video.currentTime = Math.max(0, video.currentTime - (e.shiftKey ? 0.04 : 1))),
    ArrowRight: () => (video.currentTime = Math.min(ed.duree, video.currentTime + (e.shiftKey ? 0.04 : 1))),
    i: () => borneZone('a', video.currentTime),
    I: () => borneZone('a', video.currentTime),
    o: () => borneZone('b', video.currentTime),
    O: () => borneZone('b', video.currentTime),
    ArrowUp: () => regleVolume(Number($('#ed-volume').value) + 5, false),
    ArrowDown: () => regleVolume(Number($('#ed-volume').value) - 5, false),
    m: () => regleVolume(Number($('#ed-volume').value) || 60, !son.muet),
    M: () => regleVolume(Number($('#ed-volume').value) || 60, !son.muet),
    f: basculerPleinEcran,
    F: basculerPleinEcran,
    Escape: () => (pleinEcran ? basculerPleinEcran() : fermerEditeur()),
  };
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z') {
    e.preventDefault();
    return annuler();
  }
  const f = touches[e.key];
  if (f) {
    e.preventDefault();
    f();
    majTete();
  }
});

try { $('#ed-remplacer').checked = localStorage.getItem('montageRemplace') === '1'; } catch {}
$('#ed-remplacer').addEventListener('change', (e) => {
  try { localStorage.setItem('montageRemplace', e.target.checked ? '1' : '0'); } catch {}
});

$('#ed-exporter').addEventListener('click', async (e) => {
  const bouton = e.currentTarget;
  // Seuls les sons changent : le clip est mis à jour sur place
  const seulementSons = ed.passages.every((p) => p.garde);
  const remplacer = seulementSons || $('#ed-remplacer').checked;
  const volumes = ed.sons ? ed.sons.volumes.map((v) => v / 100) : null;
  video.pause();
  // Le lecteur lâche le fichier : Windows refuse de remplacer un clip encore ouvert
  if (remplacer) {
    video.removeAttribute('src');
    video.load();
  }
  $('#ed-progression').hidden = false;
  $('#ed-barre').style.width = '0%';
  bouton.classList.add('chargement');
  try {
    const sortie = await invoke('clips_exporter', { chemin: ed.clip.chemin, morceaux: morceauxGardes(), remplacer, volumes });
    const aCote = /\(montage( \d+)?\)\.mp4$/i.test(sortie);
    toast(!aCote ? (seulementSons ? 'Sons du clip enregistrés.' : 'Clip raccourci.') : remplacer ? 'Clip encore ouvert : le montage est à côté de l\'original.' : 'Montage exporté à côté du clip d\'origine.', 5000);
    fermerEditeur();
    chargerClips();
  } catch (err) {
    toast(String(err), 8000);
    if (remplacer) video.src = window.__TAURI__.core.convertFileSrc(ed.clip.chemin);
  } finally {
    bouton.classList.remove('chargement');
    $('#ed-progression').hidden = true;
  }
});
listen('clips_export', (e) => ($('#ed-barre').style.width = `${Math.round(e.payload.fraction * 100)}%`));

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

/* ---------- Checklist BIOS ---------- */

// Chemins habituels des menus ; ils bougent un peu selon les versions du BIOS
const GUIDES_BIOS = [
  { re: /asus/i, nom: 'ASUS', touche: 'Suppr (ou F2)', xmp: 'Ai Tweaker → Ai Overclock Tuner → XMP I (ou EXPO)', rebar: 'Advanced → PCI Subsystem Settings → Above 4G Decoding, puis Re-Size BAR Support', pbo: 'Ai Tweaker → Precision Boost Overdrive' },
  { re: /micro-star|\bmsi\b/i, nom: 'MSI', touche: 'Suppr', xmp: 'OC → A-XMP / EXPO (ou le bouton XMP en haut du mode EZ)', rebar: 'Settings → Advanced → PCI Subsystem Settings → Re-Size BAR Support', pbo: 'OC → Advanced CPU Configuration → AMD Overclocking → Precision Boost Overdrive' },
  { re: /gigabyte/i, nom: 'Gigabyte', touche: 'Suppr', xmp: 'Tweaker → Extreme Memory Profile (X.M.P.)', rebar: 'Settings → IO Ports → Above 4G Decoding et Re-Size BAR Support', pbo: 'Tweaker → Advanced CPU Settings → Precision Boost Overdrive' },
  { re: /asrock/i, nom: 'ASRock', touche: 'F2 ou Suppr', xmp: 'OC Tweaker → DRAM Configuration → Load XMP (ou EXPO) Setting', rebar: 'Advanced → Chipset Configuration → Above 4G Decoding et Re-Size BAR (C.A.M.)', pbo: 'OC Tweaker → CPU Configuration → Precision Boost Overdrive' },
];

let bios = null;

async function chargerBios() {
  if (!bios) bios = await invoke('bios_infos').catch(() => null);
  afficherBios();
}

function etatsBios(b) {
  const guide = GUIDES_BIOS.find((g) => g.re.test(b.fabricant));
  const ou = (cle) => (guide ? `Dans ton BIOS ${guide.nom} : ${guide[cle]}.` : 'Cherche ce réglage dans les menus « Overclocking », « Advanced » ou « Tweaker » de ton BIOS.');
  const items = [];

  const ram = b.ram || [];
  const r0 = ram[0];
  // Certains PC (machines virtuelles, PC de marque) ne donnent pas la vitesse de la RAM
  if (r0?.reelle) {
    const ddr4 = r0.type === 26;
    const ddr5 = r0.type === 34;
    const lente = (ddr4 && r0.reelle && r0.reelle <= 2666) || (ddr5 && r0.reelle && r0.reelle <= 4800);
    if ((r0.vitesse && r0.reelle && r0.reelle < r0.vitesse * 0.9) || lente) {
      items.push({ statut: 'a-faire', titre: 'Profil XMP / EXPO de la RAM', texte: `Ta RAM tourne à ${r0.reelle} MHz${r0.vitesse > r0.reelle ? ` alors qu'elle est prévue pour ${r0.vitesse} MHz` : ', la vitesse de base'}. Activer le profil XMP (Intel) ou EXPO (AMD) donne souvent 5 à 15 % de FPS en plus, surtout sur le 1 % low.`, chemin: ou('xmp') });
    } else {
      items.push({ statut: 'ok', titre: 'Profil XMP / EXPO de la RAM', texte: `Ta RAM tourne à ${r0.reelle} MHz : le profil semble actif.` });
    }
    if (ram.length === 1) items.push({ statut: 'a-faire', titre: 'Double canal', texte: 'Une seule barrette de RAM : la bande passante est divisée par deux. Ajoute une deuxième barrette identique (ce n\'est pas un réglage du BIOS).' });
  }

  const gpu = b.gpu || '';
  const compatible = /RTX\s*[345]0\d\d|RX\s*[6-9]\d\d\d|Arc/i.test(gpu);
  if (b.barMo >= 1000) items.push({ statut: 'ok', titre: 'Resizable BAR', texte: `Actif : le processeur voit ${(b.barMo / 1024).toFixed(0)} Go de la mémoire de ta carte graphique d'un coup.` });
  else if (b.barMo >= 200 && b.barMo <= 300 && compatible)
    items.push({ statut: 'a-faire', titre: 'Resizable BAR', texte: `Désactivé alors que ta ${gpu} le prend en charge : 0 à 10 % de FPS selon les jeux. Il faut activer « Above 4G Decoding » puis « Re-Size BAR » (et que Windows démarre en mode UEFI).`, chemin: ou('rebar') });
  else if (b.barMo >= 200 && b.barMo <= 300) items.push({ statut: 'info', titre: 'Resizable BAR', texte: `Désactivé, mais ta carte graphique (${gpu || 'inconnue'}) ne le prend sans doute pas en charge : rien à faire.` });
  else items.push({ statut: 'inconnu', titre: 'Resizable BAR', texte: 'Impossible à lire depuis Windows sur ton PC. Pour vérifier : panneau de configuration NVIDIA → Informations système → « Resizable BAR », ou AMD Adrenalin → Performances → Smart Access Memory.', chemin: compatible ? ou('rebar') : '' });

  if (b.uefi === false) items.push({ statut: 'info', titre: 'Mode de démarrage ancien (CSM / Legacy)', texte: "Windows démarre en mode BIOS ancien : le Resizable BAR a besoin du mode UEFI. Le passage demande de convertir le disque système (outil mbr2gpt de Microsoft) : ne le fais que si tu es à l'aise, ou fais-toi aider." });

  const cpu = b.cpu || '';
  if (/Ryzen/i.test(cpu)) {
    const x3d = /X3D/i.test(cpu);
    items.push({
      statut: 'info',
      titre: x3d ? 'Curve Optimizer (Ryzen X3D)' : 'PBO et Curve Optimizer (Ryzen)',
      texte: x3d
        ? "Sur un X3D, l'overclocking classique est limité, mais un Curve Optimizer négatif (-10 à -20) fait monter un peu les fréquences et baisser la température. À tester par petits pas, avec un test de stabilité."
        : "Precision Boost Overdrive + Curve Optimizer négatif (-10 à -20) : 3 à 8 % de performances en plus et souvent moins de chauffe. À tester par petits pas : si le PC plante, reviens d'un cran.",
      chemin: ou('pbo'),
    });
  }
  const intel1314 = /i[3579]-1[34]\d{3}/i.test(cpu);
  if (intel1314 && b.biosDate && b.biosDate < '2024-09-01')
    items.push({ statut: 'a-faire', titre: 'Mise à jour du BIOS (Intel 13e / 14e génération)', texte: `Ton BIOS date du ${new Date(b.biosDate).toLocaleDateString('fr-CH')}. Ces processeurs ont besoin du microcode Intel 0x12B ou plus récent (BIOS de fin 2024) : sans lui, ils peuvent s'abîmer avec le temps et devenir instables. Télécharge le dernier BIOS sur le site de ta carte mère.` });
  else if (b.biosDate) {
    const ans = (Date.now() - new Date(b.biosDate)) / (365.25 * 864e5);
    if (ans >= 2) items.push({ statut: 'info', titre: 'BIOS ancien', texte: `Ton BIOS date de ${new Date(b.biosDate).getFullYear()} (version ${b.biosVersion}). Une mise à jour améliore souvent la stabilité de la RAM et la compatibilité : à faire depuis le site de ${b.fabricant || 'ta carte mère'}, jamais pendant un orage ni sur batterie.` });
  }
  return { items, guide };
}

function afficherBios() {
  if (!bios) {
    $('#bios-liste').innerHTML = '<li class="discret">Lecture des infos du BIOS impossible.</li>';
    return;
  }
  const { items, guide } = etatsBios(bios);
  const badge = { ok: '<span class="badge accent">OK</span>', 'a-faire': '<span class="badge orange">À activer</span>', info: '<span class="badge bleu">Conseil</span>', inconnu: '<span class="badge">À vérifier</span>' };
  const marque = bios.fabricant ? `${esc(bios.fabricant)} ${esc(bios.modele)}` : 'Carte mère inconnue';
  const oem = !guide && /dell|hp|hewlett|lenovo|acer|medion|fujitsu|alienware|microsoft/i.test(`${bios.fabricant}`);
  $('#bios-liste').innerHTML =
    `<li class="bios-entete discret petit-texte">${marque} · BIOS ${esc(bios.biosVersion)}${bios.biosDate ? ` du ${new Date(bios.biosDate).toLocaleDateString('fr-CH')}` : ''}${guide ? ` · Touche pour entrer dans le BIOS : ${guide.touche}` : ''}</li>` +
    (oem ? '<li class="discret petit-texte">PC de marque : son BIOS propose rarement ces réglages. Ce qui reste possible est indiqué ci-dessous.</li>' : '') +
    items
      .map((i) => `<li class="bios-item" title="${esc(i.texte)}"><div><strong>${esc(i.titre)}</strong>${badge[i.statut]}${i.chemin ? `<p class="chemin">${esc(i.chemin)}</p>` : ''}</div></li>`)
      .join('');
}

$('#btn-bios-redemarrer').addEventListener('click', async (e) => {
  const touche = GUIDES_BIOS.find((g) => g.re.test(bios?.fabricant || ''))?.touche;
  const ok = await confirmer(
    'Redémarrer dans le BIOS',
    `<p>Ton PC va redémarrer dans 5 secondes et s'ouvrir directement sur les réglages du BIOS. Enregistre ton travail avant.</p><p class="discret">Pour revenir à Windows sans rien changer : « Exit » puis « Discard Changes and Exit ».${touche ? ` Si ça ne marche pas, redémarre et appuie sur ${touche} au démarrage.` : ''}</p>`,
    'Redémarrer'
  );
  if (!ok) return;
  try {
    await appel('redemarrer_bios', {}, e.target);
    toast('Redémarrage dans le BIOS dans 5 secondes…', 6000);
  } catch {}
});

/* ---------- Mode simple (retiré : le menu en catégories suffit) ---------- */

function appliquerMode() {
  document.body.classList.remove('simple');
}

/* ---------- Alertes de l'accueil ---------- */

function afficherAlertes() {
  const alertes = [];
  if (etat.oc_baisse_auto) {
    alertes.push(`<div class="carte banniere"><p>${esc(etat.oc_baisse_auto)}</p><div class="boutons"><button class="petit" data-alerte-oc>Voir l'overclocking</button><button class="petit discret-btn" data-alerte-fermer>OK</button></div></div>`);
    toast(etat.oc_baisse_auto, 10000);
  }
  if (etat.alerte_temp) {
    const a = etat.alerte_temp;
    alertes.push(`<div class="carte banniere alerte-chaud"><p><strong>${a.temp} °C</strong> : ta carte graphique a beaucoup chauffé pendant ${esc(a.jeu)}.</p><div class="boutons"><button class="petit" data-alerte-temp>Que faire ?</button><button class="petit discret-btn" data-alerte-fermer>OK</button></div></div>`);
  }
  $('#alertes').innerHTML = alertes.join('');
}

function conseilsTemperature() {
  confirmer(
    'Carte graphique trop chaude',
    `<ul class="conseils-chaud">
      <li><strong>Dépoussière</strong> les ventilateurs du PC et de la carte graphique.</li>
      <li><strong>Laisse respirer le boîtier</strong> : pas collé au mur ni enfermé dans un meuble.</li>
      <li><strong>Limite tes FPS</strong> juste au-dessus de la fréquence de ton écran, dans le jeu.</li>
      <li><strong>Overclocking</strong> : si tu en utilises un, baisse-le d'un cran.</li>
    </ul>`,
    'OK',
    { non: 'Fermer' }
  );
}

$('#alertes').addEventListener('click', (e) => {
  const b = e.target.closest('button');
  if (!b) return;
  if (b.hasAttribute('data-alerte-oc')) allerA('oc');
  if (b.hasAttribute('data-alerte-temp')) return conseilsTemperature();
  b.closest('.banniere').remove();
});

// Carte graphique trop chaude pendant la partie qui vient de finir
listen('alerte_temp', async () => {
  etat = await invoke('etat_app');
  afficherAlertes();
  if (etat.alerte_temp) toast(`Ta carte graphique a atteint ${etat.alerte_temp.temp} °C pendant ${etat.alerte_temp.jeu}.`, 8000);
});

// OC baissé tout seul à la fin d'une partie, pendant que l'interface est ouverte
listen('oc_baisse', async () => {
  etat = await invoke('etat_app');
  afficherAlertes();
  if ($('#page-oc').classList.contains('actif')) chargerOc();
});

/* ---------- DNS ---------- */

let dns = null;

function afficherDns() {
  if (!dns) return;
  const mesures = dns.mesures.filter((m) => m.ms);
  const max = Math.max(...mesures.map((m) => m.ms), 1);
  const meilleur = mesures.reduce((a, m) => (!a || m.ms < a.ms ? m : a), null);
  const actuels = dns.cartes.map((c) => `${c.serveurs.filter((a) => a.includes('.')).join(', ') || '?'} (${esc(c.nom)})`).join(' · ');
  $('#dns-resultats').innerHTML = `
    <p class="discret petit-texte">Actuellement : ${actuels || 'aucune carte réseau connectée'}</p>
    <ul class="liste-dns">${dns.mesures
      .map((m) => {
        const utilise = dns.cartes.some((c) => c.serveurs.includes(m.adresse));
        const droite = utilise
          ? '<span class="badge accent">Utilisé</span>'
          : m.id !== 'actuel' && m.ms
            ? `<button class="petit ${m === meilleur ? 'principal' : ''}" data-dns="${m.id}">Utiliser</button>`
            : '';
        return `<li><strong>${esc(m.nom)}</strong><span class="discret">${esc(m.adresse)}</span>
          <div class="barre-dns"><div style="width:${m.ms ? Math.max(4, (m.ms / max) * 100) : 0}%" class="${m === meilleur ? 'meilleur' : ''}"></div></div>
          <span class="ms">${m.ms ? `${m.ms.toFixed(0)} ms` : 'pas de réponse'}</span>${droite}</li>`;
      })
      .join('')}</ul>
    ${dns.modifie ? '<button class="petit" data-dns-restaurer>Remettre mon DNS d\'origine</button>' : ''}`;
}

async function testerDns(bouton) {
  dns = await appel('dns_etat', {}, bouton);
  afficherDns();
}
$('#btn-dns').addEventListener('click', (e) => testerDns(e.target).catch(() => {}));

$('#dns-resultats').addEventListener('click', async (e) => {
  const b = e.target.closest('button');
  if (!b) return;
  try {
    if (b.dataset.dns) {
      const m = dns.mesures.find((x) => x.id === b.dataset.dns);
      const ok = await confirmer(`DNS ${m.nom}`, `<p>Ta connexion va utiliser le DNS ${esc(m.nom)} (${esc(m.adresse)}). Internet peut se couper une seconde. Tu pourras revenir à ton DNS d'origine ici.</p>`, 'Utiliser');
      if (!ok) return;
      await appel('dns_appliquer', { id: m.id }, b);
      toast(`DNS ${m.nom} activé.`);
    } else if (b.hasAttribute('data-dns-restaurer')) {
      await appel('dns_restaurer', {}, b);
      toast("DNS d'origine remis.");
    }
    await testerDns();
  } catch {}
});

/* ---------- Partager mes réglages ---------- */

const PREFIXE_CODE = 'FPSC1-';

async function codeReglages() {
  if (!tweaks.length) tweaks = await appel('tweaks_liste');
  const donnees = {
    tweaks: tweaks.filter((t) => t.actif && t.niveau !== 'risque').map((t) => t.id),
    reglages: { boost_auto: reglages.boost_auto, boost_max: reglages.boost_max, mesure_fps: reglages.mesure_fps, fermer_applis: reglages.fermer_applis },
    profils: profils.map(({ nom, exes, boost, priorite_haute, gpu_perf, resolution }) => ({ nom, exes, boost, priorite_haute, gpu_perf, resolution })),
  };
  return PREFIXE_CODE + btoa(unescape(encodeURIComponent(JSON.stringify(donnees))));
}

function lireCode(texte) {
  const code = texte.trim().replace(/\s+/g, '');
  if (!code.startsWith(PREFIXE_CODE)) throw new Error('Ce code ne vient pas de FPS Cagoule.');
  const d = JSON.parse(decodeURIComponent(escape(atob(code.slice(PREFIXE_CODE.length)))));
  if (!Array.isArray(d.tweaks) || !Array.isArray(d.profils) || typeof d.reglages !== 'object') throw new Error('Code incomplet.');
  return d;
}

$('#btn-exporter').addEventListener('click', async (e) => {
  const code = await codeReglages();
  let copie = false;
  try {
    await navigator.clipboard.writeText(code);
    copie = true;
  } catch {}
  await confirmer(
    'Mon code de réglages',
    `<p>${copie ? 'Code copié : colle-le à ton pote (Discord, message…).' : 'Copie ce code et envoie-le à ton pote.'} Il l'ouvre dans Réglages → « Coller un code ».</p><textarea class="code" readonly rows="5">${esc(code)}</textarea>`,
    'Fermer',
    { non: 'Retour' }
  );
});

$('#btn-importer').addEventListener('click', async () => {
  const ok = await confirmer('Coller un code', '<p>Colle ici le code reçu :</p><textarea class="code" id="code-import" rows="5" placeholder="FPSC1-…"></textarea>', 'Continuer');
  if (!ok) return;
  let d;
  try {
    d = lireCode($('#code-import').value);
  } catch (err) {
    return toast(`Code illisible : ${err.message}`, 7000);
  }
  if (!tweaks.length) tweaks = await appel('tweaks_liste');
  const ids = d.tweaks.filter((id) => tweaks.some((t) => t.id === id && t.disponible && !t.actif && t.niveau !== 'risque'));
  const nouveaux = d.profils.filter((p) => !profils.some((x) => x.nom === p.nom));
  const confirme = await confirmer(
    'Appliquer ces réglages',
    `<ul><li>${ids.length} optimisation${ids.length > 1 ? 's' : ''} à activer sur ton PC</li><li>Réglages de ${d.profils.length} jeu${d.profils.length > 1 ? 'x' : ''}${nouveaux.length ? ` (dont ${nouveaux.length} nouveau${nouveaux.length > 1 ? 'x' : ''})` : ''}</li><li>Boost pendant les parties : ${d.reglages.boost_auto ? (d.reglages.boost_max ? 'maximum' : 'activé') : 'désactivé'}</li></ul><p>Tout reste annulable.</p>`,
    'Appliquer'
  );
  if (!confirme) return;
  for (const p of d.profils) {
    if (typeof p.nom !== 'string' || !Array.isArray(p.exes)) continue;
    const exes = p.exes.filter((x) => typeof x === 'string' && /^[^\\/:*?"<>|]+\.exe$/i.test(x));
    if (!exes.length) continue;
    const reglage = { exes, boost: !!p.boost, priorite_haute: !!p.priorite_haute, gpu_perf: !!p.gpu_perf, resolution: p.resolution || null };
    const existant = profils.find((x) => x.nom === p.nom);
    if (existant) Object.assign(existant, reglage);
    else profils.push({ nom: p.nom, actif: true, oc: true, ...reglage });
  }
  await sauverProfils();
  reglages.boost_auto = !!d.reglages.boost_auto;
  reglages.boost_max = !!d.reglages.boost_max;
  reglages.mesure_fps = d.reglages.mesure_fps !== false;
  for (const a of d.reglages.fermer_applis || []) {
    if (typeof a === 'string' && !reglages.fermer_applis.some((x) => x.toLowerCase() === a.toLowerCase())) reglages.fermer_applis.push(a);
  }
  await sauverReglages();
  afficherReglages();
  if (ids.length) await appliquerTweaks(ids);
  else toast('Réglages importés.');
});

/* ---------- Tout remettre comme avant ---------- */

$('#btn-tout-remettre').addEventListener('click', async (e) => {
  const ok = await confirmer(
    'Tout remettre comme avant',
    "<p>Toutes les optimisations, l'overclocking, le DNS et les réglages des jeux vont reprendre leur valeur d'origine. Le boost pendant les parties reste actif (tu peux le couper dans « Mes jeux »).</p>",
    'Tout remettre'
  );
  if (!ok) return;
  const b = e.target;
  b.classList.add('chargement');
  const erreurs = [];
  try {
    tweaks = await invoke('tweaks_liste');
    const a = tweaks.filter((t) => t.annulable).map((t) => t.id);
    if (a.length) (await invoke('tweaks_annuler', { ids: a })).filter((r) => !r.ok).forEach((r) => erreurs.push(r.erreur));
    const oc = await invoke('oc_etat').catch(() => null);
    if (oc?.enregistre || oc?.coeur?.actuel || oc?.memoire?.actuel) await invoke('oc_defaut').catch((err) => erreurs.push(String(err)));
    await invoke('dns_restaurer').catch((err) => erreurs.push(String(err)));
    for (const j of await invoke('jeux_configs').catch(() => [])) {
      if (j.sauvegarde && !j.en_cours) await invoke('jeux_config_restaurer', { id: j.id }).catch((err) => erreurs.push(`${j.nom} : ${err}`));
    }
  } finally {
    b.classList.remove('chargement');
  }
  toast(erreurs.length ? `Presque tout est remis, sauf : ${erreurs.join(' ; ')}` : 'Ton PC est revenu comme avant FPS Cagoule. Redémarre-le pour terminer.', 9000);
  chargerTweaks();
});

/* ---------- Assistant de premier lancement ---------- */

const assistant = { ancien: false, etape: 0, niveau: 'sur', jeux: true, nettoyer: true, demarrage: true, jeuxTrouves: null, fini: false };

const ETAPES_ASSISTANT = [
  {
    html: () => `<h2 id="assistant-titre">Bienvenue dans FPS Cagoule</h2>
      <p>3 étapes pour régler ton PC pour jouer.</p>
      <p class="discret">Tout reste annulable.</p>`,
  },
  {
    html: () => `<h2 id="assistant-titre">Jusqu'où on va ?</h2>
      <div class="choix-cartes">
        ${choix('niveau', 'sur', 'Optimisé (conseillé)', 'Les réglages qui font gagner des FPS sans aucun inconvénient. Parfait pour la plupart des PC.')}
        ${choix('niveau', 'max', 'Maximum', 'En plus : réglages agressifs et mode Boost max pendant les parties. Le PC chauffe et consomme un peu plus.')}
      </div>
      ${etat.alimentation?.portable ? '<p class="discret petit-texte">PC portable détecté : branche le chargeur pour jouer, sur batterie Windows bride les performances.</p>' : ''}`,
  },
  {
    html: () => {
      const jeux = (assistant.jeuxTrouves || []).filter((j) => j.fichiers && j.reglages.length && !j.optimise);
      return `<h2 id="assistant-titre">Tes jeux et ton PC</h2>
        <div class="interrupteurs">
          ${jeux.length ? bascule('jeux', `Optimiser les réglages dans ${jeux.map((j) => esc(j.nom)).join(', ')}`, 'V-Sync, flou et effets lourds coupés, distance d\'affichage gardée. Ferme ces jeux avant.') : ''}
          ${bascule('nettoyer', 'Nettoyer les fichiers inutiles', 'Fichiers temporaires, caches et rapports d\'erreurs.')}
          ${bascule('demarrage', 'Lancer FPS Cagoule avec Windows', 'Indispensable pour le boost automatique. Il reste discret : quelques Mo de mémoire.')}
        </div>`;
    },
  },
  { html: () => `<h2 id="assistant-titre">On y va</h2><ul id="assistant-progression" class="etapes assistant-progression"></ul><p id="assistant-fin" class="discret"></p>` },
];

const choix = (champ, valeur, titre, texte) =>
  `<button class="choix-carte ${assistant[champ] === valeur ? 'actif' : ''}" data-champ="${champ}" data-valeur="${valeur}"><strong>${titre}</strong><span>${texte}</span></button>`;
const bascule = (champ, titre, texte) =>
  `<label class="bascule"><input type="checkbox" data-champ="${champ}" ${assistant[champ] ? 'checked' : ''}><span></span><div>${titre}<div class="discret petit-texte">${texte}</div></div></label>`;

function afficherAssistant() {
  const n = ETAPES_ASSISTANT.length;
  $('#assistant-etape').innerHTML = ETAPES_ASSISTANT[assistant.etape].html();
  $('.assistant-points').innerHTML = ETAPES_ASSISTANT.map((_, i) => `<span class="${i <= assistant.etape ? 'actif' : ''}"></span>`).join('');
  const derniere = assistant.etape === n - 1;
  $('#assistant-passer').hidden = derniere;
  $('#assistant-retour').hidden = assistant.etape === 0 || derniere;
  $('#assistant-suivant').textContent = assistant.etape === n - 2 ? 'Optimiser mon PC' : derniere ? 'Terminer' : assistant.etape === 0 ? 'Commencer' : 'Suivant';
  $('#assistant-suivant').disabled = derniere && !assistant.fini;
}

function ouvrirAssistant() {
  Object.assign(assistant, { etape: 0, fini: false });
  invoke('jeux_configs')
    .then((j) => {
      assistant.jeuxTrouves = j;
      assistant.jeux = j.some((x) => x.fichiers && x.reglages.length && !x.optimise);
      if (assistant.etape === 2) afficherAssistant();
    })
    .catch(() => {});
  $('#assistant').hidden = false;
  afficherAssistant();
}
$('#btn-assistant').addEventListener('click', ouvrirAssistant);

$('#assistant-etape').addEventListener('click', (e) => {
  const b = e.target.closest('.choix-carte');
  if (!b) return;
  const v = b.dataset.valeur;
  assistant[b.dataset.champ] = v === 'true' ? true : v === 'false' ? false : v;
  afficherAssistant();
});
$('#assistant-etape').addEventListener('change', (e) => {
  if (e.target.dataset.champ) assistant[e.target.dataset.champ] = e.target.checked;
});

async function terminerAssistant() {
  reglages.assistant_fait = true;
  await sauverReglages().catch(() => {});
  $('#assistant').hidden = true;
}
$('#assistant-passer').addEventListener('click', terminerAssistant);
$('#assistant-retour').addEventListener('click', () => {
  assistant.etape--;
  afficherAssistant();
});
$('#assistant-suivant').addEventListener('click', () => {
  if (assistant.etape === ETAPES_ASSISTANT.length - 1) return terminerAssistant();
  assistant.etape++;
  afficherAssistant();
  if (assistant.etape === ETAPES_ASSISTANT.length - 1) executerAssistant();
});

async function executerAssistant() {
  const liste = $('#assistant-progression');
  const etape = async (nom, f) => {
    const li = document.createElement('li');
    li.innerHTML = `<span class="icone"></span>${esc(nom)}`;
    liste.append(li);
    try {
      const detail = await f();
      li.classList.add('fait');
      if (detail) li.insertAdjacentHTML('beforeend', ` <span class="discret">— ${esc(detail)}</span>`);
    } catch (err) {
      li.classList.add('echec');
      li.insertAdjacentHTML('beforeend', ` <span class="discret">— ${esc(String(err))}</span>`);
    }
  };
  let redemarrage = false;
  // Le score de départ est pris avant de changer quoi que ce soit
  await etape('Analyse de ton PC', async () => {
    if (!derniereAnalyse) await analyser();
  });
  await etape('Point de restauration et optimisations', async () => {
    tweaks = await invoke('tweaks_liste');
    const max = assistant.niveau === 'max';
    const a = tweaks.filter((t) => t.disponible && !t.actif && (max ? !['risque', 'choix'].includes(t.niveau) : t.niveau === 'recommande'));
    if (!a.length) return 'déjà fait';
    const [, resultats] = await invoke('tweaks_appliquer', { ids: a.map((t) => t.id), restauration: true });
    const ok = resultats.filter((r) => r.ok).length;
    redemarrage = a.some((t) => t.redemarrage && resultats.some((r) => r.ok && r.id === t.id));
    return `${ok} réglage${ok > 1 ? 's' : ''} appliqué${ok > 1 ? 's' : ''}`;
  });
  await etape('Recherche de tes jeux installés', async () => {
    const jeux = await nouveauxJeux();
    await ajouterJeux(jeux);
    return jeux.length ? `${jeux.length} jeu${jeux.length > 1 ? 'x' : ''} ajouté${jeux.length > 1 ? 's' : ''}` : 'déjà tous dans la liste';
  });
  await etape('Boost automatique de tes jeux', async () => {
    reglages.boost_auto = true;
    reglages.boost_max = assistant.niveau === 'max';
    reglages.mode_simple = false;
    await sauverReglages();
    afficherReglages();
    appliquerMode();
    return reglages.boost_max ? 'mode Boost max' : '';
  });
  const jeux = (assistant.jeuxTrouves || []).filter((j) => j.fichiers && j.reglages.length && !j.optimise && !j.en_cours);
  if (assistant.jeux && jeux.length) {
    await etape('Réglages dans tes jeux', async () => {
      let n = 0;
      for (const j of jeux) n += await invoke('jeux_config_appliquer', { id: j.id }).catch(() => 0);
      return `${n} réglage${n > 1 ? 's' : ''} changé${n > 1 ? 's' : ''}`;
    });
  }
  if (assistant.nettoyer) {
    await etape('Nettoyage', async () => {
      nettoyageAnalyse = await invoke('nettoyage_analyser');
      return `${formatOctets(await nettoyerRapide())} libérés`;
    });
  }
  if (assistant.demarrage) {
    await etape('Lancement avec Windows', async () => {
      await invoke('demarrage_auto_set', { actif: true });
      $('#demarrage-auto').checked = true;
    });
  }
  assistant.fini = true;
  reglages.assistant_fait = true;
  await sauverReglages().catch(() => {});
  $('#assistant-fin').innerHTML = redemarrage
    ? 'C\'est fait ! <strong>Redémarre ton PC</strong> pour que tout prenne effet. Ensuite, lance simplement tes jeux : FPS Cagoule s\'occupe du reste.'
    : 'C\'est fait ! Lance simplement tes jeux : FPS Cagoule les repère et les boost tout seul.';
  if (redemarrage) $('#assistant-fin').insertAdjacentHTML('beforeend', '<div class="boutons"><button class="principal petit" id="assistant-redemarrer">Redémarrer maintenant</button></div>');
  $('#assistant-redemarrer')?.addEventListener('click', () => $('#btn-redemarrer').click());
  afficherAssistant();
  analyser();
}

/* ---------- Démarrage ---------- */

async function init() {
  const debutSplash = performance.now();
  const etape = (f, t) => window.splashEtape?.(f, t);
  etape(0.15, 'Connexion au moteur…');
  try {
    etat = await appel('etat_app');
    etape(0.45, 'Chargement de tes jeux…');
    $('#version').textContent = `Version ${etat.version}`;
    $('#maj-texte').textContent = `Version installée : ${etat.version}.`;
    afficherJeu(etat.jeu);
    [profils, reglages] = await Promise.all([appel('profils_get'), appel('reglages_get')]);
    etape(0.8, 'Préparation de ton PC…');
    afficherProfils();
    afficherReglages();
    appliquerMode();
    afficherAlertes();
    invoke('demarrage_auto_get').then((v) => ($('#demarrage-auto').checked = v));
    invoke('carte_graphique').then((c) => (carte = c)).catch(() => {});
    try {
      derniereAnalyse = JSON.parse(localStorage.getItem('analyse'));
      // Déjà utilisé avant l'assistant : l'interface complète est proposée par défaut
      assistant.ancien = !!derniereAnalyse;
      extraAnalyse = JSON.parse(localStorage.getItem('extra')) || {};
    } catch {}
  } finally {
    // L'intro reste au moins 2 s, le temps de son animation (un clic la passe)
    etape(1, 'Prêt');
    const calme = matchMedia('(prefers-reduced-motion: reduce)').matches;
    const reste = (calme ? 600 : 2000) - (performance.now() - debutSplash);
    let fini = false;
    const finir = () => {
      if (fini) return;
      fini = true;
      $('#splash').classList.add('fini');
      document.body.classList.add('entree');
      setTimeout(() => $('#splash')?.remove(), 750);
      setTimeout(() => document.body.classList.remove('entree'), 1400);
    };
    $('#splash').addEventListener('click', finir);
    setTimeout(finir, Math.max(0, reste));
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
  if (!reglages?.assistant_fait) setTimeout(ouvrirAssistant, 1300);
  else {
    setTimeout(montrerNouveautes, 2400);
    setTimeout(detecterJeuxAuto, 4000);
  }
  // Première installation : les nouveautés de cette version ne sont pas montrées
  if (!reglages?.assistant_fait) montrerNouveautes();
}

/* ---------- Nouveautés après une mise à jour ---------- */

const ICONES_NOUVEAUTES = {
  jouer: JOUER,
  courbe: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 17l5-6 4 3 5-7 4 4"/><path d="M3 21h18"/></svg>',
  chaud: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M10 14.5V5a2 2 0 1 1 4 0v9.5a4 4 0 1 1-4 0z"/><path d="M12 9v7"/></svg>',
  discord: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 7.5c3-1.5 7-1.5 10 0l1.5 9c-1.5 1.3-3 2-4.5 2l-.8-1.6M7 7.5 5.5 16.5c1.5 1.3 3 2 4.5 2l.8-1.6M8.5 15c2.3 1 4.7 1 7 0"/><circle cx="9.5" cy="12" r="1"/><circle cx="14.5" cy="12" r="1"/></svg>',
  reglages: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h10M18 7h2M4 17h4M12 17h8"/><circle cx="16" cy="7" r="2"/><circle cx="10" cy="17" r="2"/></svg>',
  clips: '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="6" width="13" height="12" rx="2"/><path d="M16 10.5l5-3v9l-5-3z"/></svg>',
  montage: '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="6" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="M8.1 8.1 20 20M8.1 15.9 20 4"/></svg>',
  dossier: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>',
};

const NOUVEAUTES = {
  '0.28.2': [
    { icone: 'clips', titre: 'Clips à la bonne taille', texte: 'Fini les clips d\'1 Go qui durent des heures : le son s\'arrête avec l\'image. Discord revient dans le son du jeu.' },
  ],
  '0.28.1': [
    { icone: 'clips', titre: 'Aperçu Discord', texte: 'Les longs clips s\'affichent maintenant aussi dans Discord, avec leur vignette.' },
  ],
  '0.28.0': [
    { icone: 'clips', titre: 'Réglages plus clairs', texte: 'Les réglages des clips tiennent en une ligne, et s\'ouvrent en trois blocs quand tu en as besoin.' },
    { icone: 'clips', titre: 'Filtre carte et opérateur', texte: 'Retrouve tous tes clips sur Outback ou avec Rauora en deux clics.' },
  ],
  '0.27.9': [
    { icone: 'clips', titre: 'Choisis tes sons', texte: 'Jeu, voix Discord et micro, chacun sur sa piste : règle leurs volumes dans l\'éditeur.' },
    { icone: 'clips', titre: 'Discord tout seul', texte: 'Tes 3K, 4K ou ACE partent sur ton salon Discord sans rien toucher.' },
  ],
  '0.27.8': [
    { icone: 'clips', titre: 'Un son propre', texte: 'Fini les petites coupures dans le son de tes clips, même quand le jeu fait travailler le PC à fond.' },
  ],
  '0.27.7': [
    { icone: 'clips', titre: 'Des clips vraiment fluides', texte: 'Tes clips gardent enfin leurs 60 images par seconde, en plein écran comme en fenêtré. Le son reste calé.' },
  ],
  '0.27.6': [
    { icone: 'clips', titre: 'Des clips vraiment fluides', texte: 'Tes clips gardent enfin leurs 60 images par seconde : avant, la moitié étaient des copies. Le son reste calé.' },
  ],
  '0.27.0': [
    { icone: 'clips', titre: 'Des clips bien plus nets', texte: 'Nouvel encodage pensé pour la qualité : moins de blocs et de flou quand ça bouge, à taille égale. Pour tes prochains clips.' },
    { icone: 'reglages', titre: '4 optimisations de plus', texte: 'Tâches de télémétrie coupées, logiciels installés tout seuls bloqués, stockage réservé récupéré et recherche du menu Démarrer allégée.' },
  ],
  '0.26.6': [
    { icone: 'discord', titre: 'Partage tes clips', texte: 'Le bouton 🔗 met ton clip en ligne sur ton site et copie le lien. Avec un webhook, il est posté direct dans ton salon Discord.' },
    { icone: 'clips', titre: 'Le son, à ton goût', texte: 'Règle le volume dans le lecteur (molette ou ↑ ↓, M pour couper). Il est retenu pour les prochains clips.' },
  ],
  '0.26.5': [
    { icone: 'clips', titre: 'Clips auto réparés', texte: 'Les replays de R6 sont maintenant trouvés dans le dossier du jeu : tes 2K à ACE se gardent tout seuls, avec Solid Snake, Noor et les nouvelles maps.' },
    { icone: 'montage', titre: 'Un vrai montage', texte: 'Plein écran (F), passage de 0:20 à 0:30 à retirer ou à garder seul, et « Remplacer l\'original » pour raccourcir un clip.' },
    { icone: 'dossier', titre: 'Galerie plus propre', texte: 'Pleine largeur, noms en entier, et les clips copiés de Medal apparaissent avec leurs miniatures.' },
  ],
  '0.26.0': [
    { icone: 'clips', titre: 'Clips automatiques dans R6', texte: 'Tes 2K, 3K, 4K ou ACE (au choix) sont gardés tout seuls : « 3K · Ace · Clubhouse · TonPseudo ». Laisse le Match Replay activé dans R6.' },
    { icone: 'montage', titre: 'Monte tes clips', texte: 'Tes clips sont rangés par jeu, avec une recherche et des filtres 2K à ACE. Clique sur un clip pour le couper : tes kills, KO et morts sont marqués sur la timeline.' },
    { icone: 'dossier', titre: 'Clips sur ton serveur', texte: 'Choisis le dossier des clips, même un dossier réseau, et leur qualité (jusqu\'à Ultra en 120 images/s).' },
  ],
  '0.25.0': [
    { icone: 'clips', titre: 'Tes clips, comme Medal', texte: 'Jeux → Clips : active l\'enregistrement, choisis ta touche et la durée. En jeu, elle garde les 30 s, 1 min ou 3 min d\'avant.' },
  ],
  '0.24.0': [
    { icone: 'jouer', titre: 'Lance tes jeux depuis l\'app', texte: 'Bouton « Jouer » sur chaque jeu. Plusieurs comptes Steam ? Choisis lequel avant de lancer.' },
    { icone: 'courbe', titre: 'La courbe de tes FPS', texte: 'Dans Statistiques, vois chaque partie seconde par seconde et repère les chutes.' },
    { icone: 'chaud', titre: 'Alerte température', texte: 'Si ta carte graphique dépasse 85 °C en jeu, l\'app te prévient avec des conseils.' },
    { icone: 'discord', titre: 'Statut Discord', texte: 'Tes potes voient à quoi tu joues et tes FPS, boostés par FPS Cagoule.' },
    { icone: 'reglages', titre: '3 jeux de plus optimisés', texte: 'Overwatch 2, GTA V et Minecraft ont maintenant leurs réglages en jeu, en un clic sur l\'éclair.' },
  ],
};

const nouveautes = { liste: [], etape: 0 };

function afficherNouveautes() {
  const n = nouveautes.liste.length;
  const e = nouveautes.liste[nouveautes.etape];
  $('#nouveautes-etape').innerHTML = `<div class="nouveaute-icone">${ICONES_NOUVEAUTES[e.icone] || ''}</div>
    <p class="nouveaute-version">Nouveau dans la ${esc(etat.version)}</p>
    <h2 id="nouveautes-titre">${esc(e.titre)}</h2>
    <p>${esc(e.texte)}</p>`;
  $('.nouveautes-points').innerHTML = nouveautes.liste.map((_, i) => `<span class="${i === nouveautes.etape ? 'actif' : ''}"></span>`).join('');
  $('#nouveautes-suivant').textContent = nouveautes.etape === n - 1 ? 'C\'est parti' : 'Suivant';
  $('#nouveautes-passer').hidden = nouveautes.etape === n - 1;
}

/** Une fois par version, si ce n'est pas la première installation */
function montrerNouveautes() {
  let vue = null;
  try {
    vue = localStorage.getItem('versionVue');
    localStorage.setItem('versionVue', etat.version);
  } catch {}
  const liste = (NOUVEAUTES[etat.version] || []).filter((e) => e.icone !== 'discord' || etat.discord);
  if (!liste.length || vue === etat.version || !reglages?.assistant_fait) return;
  Object.assign(nouveautes, { liste, etape: 0 });
  afficherNouveautes();
  $('#nouveautes').hidden = false;
  $('#nouveautes-suivant').focus();
}
$('#nouveautes-suivant').addEventListener('click', () => {
  if (nouveautes.etape >= nouveautes.liste.length - 1) return ($('#nouveautes').hidden = true);
  nouveautes.etape++;
  afficherNouveautes();
});
$('#nouveautes-passer').addEventListener('click', () => ($('#nouveautes').hidden = true));

// Icône du menu devant le titre de chaque page
document.querySelectorAll('button[data-page]').forEach((b) => {
  const h1 = document.querySelector(`#page-${b.dataset.page} > header h1`);
  const svg = b.querySelector('svg');
  if (!h1 || !svg) return;
  const titre = document.createElement('div');
  titre.className = 'titre-page';
  titre.innerHTML = `<span class="icone-page">${svg.outerHTML}</span>`;
  h1.replaceWith(titre);
  titre.append(h1);
});

init();
