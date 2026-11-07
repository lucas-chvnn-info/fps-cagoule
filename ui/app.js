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
    if (b.dataset.page === 'optimisations') chargerTweaks();
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
  const { score, problemes } = evaluer(derniereAnalyse, { planPerso: etat.plan_perso });
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
  if (b.dataset.action === 'demarrage') return appel('ouvrir_demarrage');
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

const gainBadge = { fort: '<span class="badge vert">Gain fort</span>', moyen: '<span class="badge bleu">Gain moyen</span>', faible: '<span class="badge">Gain faible</span>' };

async function chargerTweaks() {
  tweaks = await appel('tweaks_liste');
  $('#tweaks').innerHTML = tweaks
    .map(
      (t) => `<li class="tweak">
        <input type="checkbox" data-id="${esc(t.id)}" ${t.actif ? 'disabled' : ''} aria-label="Sélectionner">
        <div>
          <div class="titre">${esc(t.titre)} ${gainBadge[t.gain] || ''} ${t.redemarrage ? '<span class="badge orange">Après redémarrage</span>' : ''}</div>
          <p>${esc(t.description)}</p>
        </div>
        <div class="droite">
          ${t.actif ? '<span class="badge vert">Actif</span>' : '<span class="badge">Inactif</span>'}
          ${t.annulable ? `<button class="petit" data-annuler="${esc(t.id)}">Annuler</button>` : ''}
        </div>
      </li>`
    )
    .join('');
  majBoutonAppliquer();
  afficherScore();
}

function majBoutonAppliquer() {
  $('#btn-appliquer').disabled = !document.querySelector('#tweaks input:checked');
}

$('#tweaks').addEventListener('change', majBoutonAppliquer);
$('#tweaks').addEventListener('click', async (e) => {
  const id = e.target.closest('button')?.dataset.annuler;
  if (!id) return;
  const [r] = await appel('tweaks_annuler', { ids: [id] }, e.target);
  toast(r.ok ? 'Réglage remis comme avant' : `Impossible d'annuler : ${r.erreur}`);
  chargerTweaks();
});

async function appliquerTweaks(ids, bouton) {
  const [point, resultats] = await appel('tweaks_appliquer', { ids, restauration: $('#restauration').checked }, bouton);
  const echecs = resultats.filter((r) => !r.ok);
  const redemarrage = tweaks.some((t) => ids.includes(t.id) && t.redemarrage);
  let msg = echecs.length ? `${echecs.length} échec(s) : ${echecs.map((r) => r.erreur).join(' ; ')}` : `${resultats.length} optimisation(s) appliquée(s).`;
  if (redemarrage && !echecs.length) msg += ' Certaines prennent effet au prochain redémarrage.';
  if (point) msg = `${point}. ${msg}`;
  toast(msg, 7000);
  await chargerTweaks();
}

$('#btn-appliquer').addEventListener('click', (e) => {
  const ids = [...document.querySelectorAll('#tweaks input:checked')].map((c) => c.dataset.id);
  if (ids.length) appliquerTweaks(ids, e.target);
});

async function nettoyer(bouton) {
  const r = await appel('nettoyer', {}, bouton);
  const texte = `${r.fichiers} fichiers supprimés, ${(r.octets / 1048576).toFixed(0)} Mo libérés.`;
  $('#nettoyage-resultat').textContent = texte;
  toast(texte);
}
$('#btn-nettoyer').addEventListener('click', (e) => nettoyer(e.target));
$('#btn-demarrage').addEventListener('click', () => appel('ouvrir_demarrage'));

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
  $('#mesure-fps').checked = reglages.mesure_fps;
  $('#overlay').checked = reglages.overlay;
  $('#maj-auto').checked = reglages.maj_auto;
  $('#fermer-liste').innerHTML = reglages.fermer_applis.length
    ? reglages.fermer_applis.map((a, i) => `<span class="puce">${esc(a)}<button data-retirer="${i}" title="Retirer">✕</button></span>`).join('')
    : '<span class="discret">Aucun programme.</span>';
}

for (const [id, champ] of [['#boost-auto', 'boost_auto'], ['#mesure-fps', 'mesure_fps'], ['#overlay', 'overlay'], ['#maj-auto', 'maj_auto']]) {
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

/* ---------- Statistiques ---------- */

const moyenne = (l, f) => l.reduce((s, x) => s + f(x), 0) / l.length;
const duree = (s) => (s >= 3600 ? `${Math.floor(s / 3600)} h ${String(Math.floor((s % 3600) / 60)).padStart(2, '0')}` : `${Math.max(1, Math.round(s / 60))} min`);

async function chargerSessions() {
  const sessions = (await appel('sessions')).reverse();
  $('#sessions tbody').innerHTML = sessions.length
    ? sessions
        .map(
          (s) => `<tr><td>${new Date(s.debut * 1000).toLocaleString('fr-CH', { dateStyle: 'short', timeStyle: 'short' })}</td><td>${esc(s.jeu)}</td><td>${duree(s.duree_s)}</td><td>${s.boost ? '<span class="badge vert">Oui</span>' : '<span class="badge">Non</span>'}</td><td>${Math.round(s.moyenne)}</td><td>${Math.round(s.low1)}</td><td>${Math.round(s.low01)}</td></tr>`
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
  invoke('tweaks_liste').then((t) => {
    tweaks = t;
    afficherScore();
  });
  majConso();
  setInterval(majConso, 3000);
}

init();
