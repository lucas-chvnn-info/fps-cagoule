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
    if (b.dataset.page === 'optimisations') {
      chargerTweaks();
      chargerDemarrage();
    }
    if (b.dataset.page === 'analyse') chargerCartes();
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
  if (b.dataset.action === 'nettoyer') return allerA('nettoyage');
  if (b.dataset.action === 'demarrage') {
    allerA('optimisations');
    return choisirCategorie('demarrage');
  }
  if (b.dataset.action === 'pilotes') return allerA('pilotes');
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
  return `<li class="tweak">
      <div>
        <div class="titre">${esc(t.titre)} ${gainBadge[t.gain] || ''} ${t.niveau === 'max' ? '<span class="badge violet">Boost max</span>' : ''}${t.niveau === 'risque' ? '<span class="badge rouge">Risqué</span>' : ''} ${redemarrage}</div>
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
  const [point, resultats] = await appel('tweaks_appliquer', { ids, restauration: $('#restauration').checked }, bouton);
  const echecs = resultats.filter((r) => !r.ok);
  const redemarrage = tweaks.some((t) => ids.includes(t.id) && t.redemarrage);
  const reussis = resultats.length - echecs.length;
  let msg = echecs.length ? `${reussis} appliquée(s), ${echecs.length} échec(s) : ${echecs.map((r) => r.erreur).join(' ; ')}` : ids.length > 1 ? `${reussis} optimisations appliquées.` : 'Optimisation appliquée.';
  if (redemarrage && !echecs.length) msg += ' Redémarre ton PC pour terminer (bouton en haut de la page).';
  if (point) msg = `${point}. ${msg}`;
  toast(msg, 7000);
  await chargerTweaks();
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
  const a = tweaks.filter((t) => t.disponible && !t.actif && (max ? t.niveau !== 'risque' : t.niveau === 'recommande'));
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
    afficherCategories();
    afficherScore();
  });
  majConso();
  setInterval(majConso, 3000);
}

init();
