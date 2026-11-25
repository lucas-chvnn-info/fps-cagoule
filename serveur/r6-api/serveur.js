// r6-api : stats Ubisoft des joueurs de Rainbow Six pour FPS Cagoule, avec un seul compte Ubisoft gardé ici
// (personne n'a à se connecter dans l'app). Node 20, aucune dépendance.
//   GET /sante                         { ok, connecte }
//   GET /v1/stats?profils=a,b,…        { profil: stats | null }   (20 profils au plus, cache 20 min)
//   GET /v1/chercher?pseudo=…          [[profil, pseudo], …]
//   GET /v1/saisons?profil=…           [stats de chaque saison vue, la plus récente d'abord]
// Ubisoft ne donne que la saison en cours : chaque saison vue est gardée dans .data/saisons.json, pour tous
// les joueurs vus par tous les utilisateurs. Adresses et identifiants d'application d'après siegeapi (MIT).
// Variables : UBI_EMAIL, UBI_MOT_DE_PASSE (compte sans double authentification), PORT (3103 par défaut).
'use strict';
const http = require('http');
const fs = require('fs');
const path = require('path');

const PORT = Number(process.env.PORT || 3103);
const DONNEES = path.join(__dirname, '.data');
const APPID = '4391c956-8943-48eb-8859-07b0778f47b9';
const APPID_RANGS = 'e3d5ea9e-50bd-43b7-88bf-39794f4e3d40';
const ESPACE_R6 = '0d2ae42d-4c27-4cb7-af6c-2099062302bb';
// Client annoncé à Ubisoft (l'ancien « UbiServices_SDK… » de siegeapi reçoit une page anti-robots)
const AGENT = process.env.UBI_AGENT || 'r6-stats/1.0';
const DUREE_SESSION = (2 * 60 + 45) * 60e3;
const DUREE_CACHE = 20 * 60e3;
const RANG_MAX = 40;
const PROFIL = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

fs.mkdirSync(DONNEES, { recursive: true });
const fichierSaisons = path.join(DONNEES, 'saisons.json');
let saisons = {};
try {
  saisons = JSON.parse(fs.readFileSync(fichierSaisons, 'utf8'));
} catch {}
let aSauver = false;
setInterval(() => {
  if (!aSauver) return;
  aSauver = false;
  fs.writeFileSync(fichierSaisons + '.tmp', JSON.stringify(saisons));
  fs.renameSync(fichierSaisons + '.tmp', fichierSaisons);
}, 30e3).unref();

// --- Sessions Ubisoft (la seconde sert aux rangs) ---
const sessions = { base: null, rangs: null };
let ouverture = null;

async function ouvrir(autorisation, appid) {
  const r = await fetch('https://public-ubiservices.ubi.com/v3/profiles/sessions', {
    method: 'POST',
    headers: { Authorization: autorisation, 'Ubi-AppId': appid, 'User-Agent': AGENT, 'Content-Type': 'application/json' },
    body: JSON.stringify({ rememberMe: true }),
    signal: AbortSignal.timeout(20e3),
  });
  const texte = await r.text();
  let v = {};
  try {
    v = JSON.parse(texte);
  } catch {}
  if (!r.ok) {
    // Raison donnée par Ubisoft (jamais les identifiants) : message JSON, sinon le début de la page reçue
    const detail = v.message || v.errorCode ? `code ${v.errorCode ?? '?'} : ${v.message ?? ''}` : `${r.headers.get('server') || ''} ${texte.replace(/\s+/g, ' ').slice(0, 160)}`;
    throw new Error(`Ubisoft refuse la connexion (${r.status}, ${detail.trim()})`);
  }
  if (!v.ticket) throw new Error(v.twoFactorAuthenticationTicket ? 'Compte avec double authentification : il en faut un sans' : 'Connexion refusée');
  return { ticket: v.ticket, id: v.sessionId, debut: Date.now() };
}

const valide = (s) => s && Date.now() - s.debut < DUREE_SESSION;

async function sessionsOuvertes() {
  if (valide(sessions.base) && valide(sessions.rangs)) return;
  // Une seule ouverture à la fois, même si plusieurs demandes arrivent ensemble
  ouverture ||= (async () => {
    if (!valide(sessions.base)) {
      const { UBI_EMAIL: email, UBI_MOT_DE_PASSE: mdp } = process.env;
      if (!email || !mdp) throw new Error('Compte Ubisoft non configuré');
      sessions.base = await ouvrir('Basic ' + Buffer.from(`${email}:${mdp}`).toString('base64'), APPID);
      sessions.rangs = null;
    }
    if (!valide(sessions.rangs)) sessions.rangs = await ouvrir(`Ubi_v1 t=${sessions.base.ticket}`, APPID_RANGS);
  })().finally(() => (ouverture = null));
  return ouverture;
}

async function lire(url, s, appid) {
  const r = await fetch(url, {
    headers: { Authorization: `Ubi_v1 t=${s.ticket}`, 'Ubi-AppId': appid, 'Ubi-SessionId': s.id, 'Ubi-LocaleCode': 'en-US', 'User-Agent': AGENT },
    signal: AbortSignal.timeout(20e3),
  });
  if (r.status === 401) {
    sessions.base = sessions.rangs = null;
    throw new Error('Session Ubisoft expirée, réessaie');
  }
  if (!r.ok) throw new Error(`Ubisoft répond ${r.status}`);
  return r.status === 204 ? {} : r.json();
}

// --- Stats ---
function lireRangs(v) {
  const out = {}, rapide = {};
  for (const t of v?.platform_families_full_profiles?.[0]?.board_ids_full_profiles || []) {
    if (t.board_id !== 'ranked' && t.board_id !== 'standard') continue;
    for (const fp of t.full_profiles || []) {
      const id = String(fp.profile?.id || '').toLowerCase();
      if (!id) continue;
      const st = fp.season_statistics || {}, mo = st.match_outcomes || {};
      const s = {
        rang: Math.min(RANG_MAX, fp.profile.rank || 0), points: fp.profile.rank_points || 0, pic_saison: Math.min(RANG_MAX, fp.profile.max_rank || 0),
        pic_general: 0, niveau: null, kills: st.kills || 0, morts: st.deaths || 0, victoires: mo.wins || 0, defaites: mo.losses || 0,
        abandons: mo.abandons || 0, classe: t.board_id === 'ranked', saison: fp.profile.season_id || 0,
      };
      (t.board_id === 'ranked' ? out : rapide)[id] = s;
    }
  }
  // Aucune partie classée cette saison : KD et victoires de la partie rapide
  for (const [id, r] of Object.entries(rapide)) {
    const s = (out[id] ||= { ...r });
    if (!(s.victoires + s.defaites + s.abandons)) Object.assign(s, { kills: r.kills, morts: r.morts, victoires: r.victoires, defaites: r.defaites, abandons: r.abandons, classe: false });
  }
  return out;
}

const cache = new Map(); // profil → { quand, stats | null }

async function stats(profils) {
  const manquants = profils.filter((p) => !(cache.get(p)?.quand > Date.now() - DUREE_CACHE));
  if (manquants.length) {
    await sessionsOuvertes();
    const liste = manquants.join(',');
    const [rangs, niveaux] = await Promise.all([
      lire(`https://public-ubiservices.ubi.com/v2/spaces/${ESPACE_R6}/title/r6s/skill/full_profiles?profile_ids=${liste}&platform_families=pc`, sessions.rangs, APPID_RANGS),
      lire(`https://public-ubiservices.ubi.com/v1/profiles/stats?profileIds=${liste}&spaceId=${ESPACE_R6}&statNames=PClearanceLevel`, sessions.base, APPID).catch(() => ({})),
    ]);
    const recues = lireRangs(rangs);
    for (const p of niveaux.profiles || []) {
      const s = recues[String(p.profileId || '').toLowerCase()];
      const n = Number(p.stats?.PClearanceLevel?.value);
      if (s && n) s.niveau = n;
    }
    for (const p of manquants) {
      const s = recues[p] || null;
      if (s && s.saison) {
        (saisons[p] ||= {})[s.saison] = s;
        aSauver = true;
      }
      cache.set(p, { quand: Date.now(), stats: s });
    }
  }
  const out = {};
  for (const p of profils) {
    const s = cache.get(p)?.stats;
    // Pic général : meilleur rang de toutes les saisons gardées
    out[p] = s ? { ...s, pic_general: Math.max(s.pic_saison, ...Object.values(saisons[p] || {}).map((x) => Math.max(x.pic_saison, x.rang))) } : null;
  }
  return out;
}

async function chercher(pseudo) {
  await sessionsOuvertes();
  const v = await lire(`https://public-ubiservices.ubi.com/v3/profiles?nameOnPlatform=${encodeURIComponent(pseudo)}&platformType=uplay`, sessions.base, APPID);
  return (v.profiles || []).filter((p) => p.profileId && p.nameOnPlatform).map((p) => [p.profileId.toLowerCase(), p.nameOnPlatform]);
}

// --- Limite : 60 demandes par minute et par adresse ---
const demandes = new Map();
function trop(ip) {
  const maintenant = Date.now();
  const l = (demandes.get(ip) || []).filter((t) => t > maintenant - 60e3);
  l.push(maintenant);
  demandes.set(ip, l);
  return l.length > 60;
}
setInterval(() => demandes.clear(), 10 * 60e3).unref();

function repondre(res, code, corps) {
  res.writeHead(code, { 'Content-Type': 'application/json; charset=utf-8', 'Cache-Control': 'no-store' });
  res.end(JSON.stringify(corps));
}

http
  .createServer(async (req, res) => {
    const url = new URL(req.url, 'http://local');
    const ip = String(req.headers['x-forwarded-for'] || req.socket.remoteAddress).split(',')[0].trim();
    if (req.method !== 'GET') return repondre(res, 405, { erreur: 'GET seulement' });
    if (url.pathname === '/sante') return repondre(res, 200, { ok: true, connecte: !!valide(sessions.base) });
    if (trop(ip)) return repondre(res, 429, { erreur: 'Trop de demandes, réessaie dans une minute' });
    try {
      if (url.pathname === '/v1/stats') {
        const profils = [...new Set((url.searchParams.get('profils') || '').toLowerCase().split(',').filter((p) => PROFIL.test(p)))].slice(0, 20);
        return repondre(res, 200, await stats(profils));
      }
      if (url.pathname === '/v1/chercher') {
        const pseudo = (url.searchParams.get('pseudo') || '').trim().slice(0, 40);
        return repondre(res, 200, pseudo ? await chercher(pseudo) : []);
      }
      if (url.pathname === '/v1/saisons') {
        const p = (url.searchParams.get('profil') || '').toLowerCase();
        return repondre(res, 200, PROFIL.test(p) ? Object.values(saisons[p] || {}).sort((a, b) => b.saison - a.saison) : []);
      }
      repondre(res, 404, { erreur: 'Introuvable' });
    } catch (e) {
      console.error(new Date().toISOString(), url.pathname, e.message);
      repondre(res, 502, { erreur: e.message });
    }
  })
  .listen(PORT, '127.0.0.1', () => console.log(`r6-api sur 127.0.0.1:${PORT}`));
