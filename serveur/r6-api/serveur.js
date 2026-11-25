// r6-api : stats des joueurs de Rainbow Six pour FPS Cagoule, via l'API d'Arenyze (clé d'API gardée ici :
// les utilisateurs de l'app n'ont rien à configurer). Node 20, aucune dépendance.
//   GET /sante                                   { ok, cle }
//   GET /v1/stats?joueurs=profil:pseudo,…        { profil: stats | null }   (20 joueurs au plus, cache 6 h)
//   GET /v1/chercher?pseudo=…                    [[profil, pseudo]]
//   GET /v1/saisons?profil=…                     [stats de chaque saison vue, la plus récente d'abord]
// Arenyze cherche par pseudo ; Ubisoft ne donne que la saison en cours, chaque saison vue est donc gardée dans
// .data/saisons.json. Variables : ARENYZE_CLE (clé d'API de r6.arenyze.com), PORT (3103 par défaut).
'use strict';
const http = require('http');
const fs = require('fs');
const path = require('path');

const PORT = Number(process.env.PORT || 3103);
const DONNEES = path.join(__dirname, '.data');
const API = 'https://public-api.arenyze.com/r6/api';
const CLE = process.env.ARENYZE_CLE || '';
const DUREE_CACHE = 6 * 3600e3; // un appel par joueur et par partie environ : le quota gratuit est de 2 000 par mois
const RANG_MAX = 40;
const PROFIL = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const PSEUDO = /^[\w.\-]{2,32}$/;

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

// --- Arenyze ---
let pauseJusqua = 0; // après un 429 : on attend ce que demande Arenyze

async function profilArenyze(pseudo) {
  if (!CLE) throw new Error('Clé Arenyze non configurée');
  if (Date.now() < pauseJusqua) throw new Error('Quota Arenyze atteint, réessaie plus tard');
  const r = await fetch(`${API}/v2/profile?nameOnPlatform=${encodeURIComponent(pseudo)}&platformType=uplay`, {
    headers: { 'api-key': CLE },
    signal: AbortSignal.timeout(20e3),
  });
  if (r.status === 429) {
    pauseJusqua = Date.now() + (Number(r.headers.get('retry-after')) || 60) * 1000;
    throw new Error('Quota Arenyze atteint, réessaie plus tard');
  }
  if (r.status === 404) return null;
  if (!r.ok) throw new Error(`Arenyze répond ${r.status}`);
  return r.json();
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

// Stats d'un profil Arenyze : rang de la saison (classé, sinon partie rapide) et niveau
function depuisProfil(v, profil) {
  const rangs = lireRangs(v?.stats);
  const s = rangs[profil] || Object.values(rangs)[0] || null;
  if (s && Number(v?.account?.level)) s.niveau = Number(v.account.level);
  return s;
}

async function stats(joueurs) {
  // Un à un : Arenyze limite à 30 appels par minute
  for (const [profil, pseudo] of joueurs) {
    if (cache.get(profil)?.quand > Date.now() - DUREE_CACHE) continue;
    const v = await profilArenyze(pseudo);
    const s = v ? depuisProfil(v, profil) : null;
    if (s && s.saison) {
      (saisons[profil] ||= {})[s.saison] = s;
      aSauver = true;
    }
    cache.set(profil, { quand: Date.now(), stats: s });
  }
  const out = {};
  for (const [p] of joueurs) {
    const s = cache.get(p)?.stats;
    // Pic général : meilleur rang de toutes les saisons gardées
    out[p] = s ? { ...s, pic_general: Math.max(s.pic_saison, ...Object.values(saisons[p] || {}).map((x) => Math.max(x.pic_saison, x.rang))) } : null;
  }
  return out;
}

async function chercher(pseudo) {
  const v = await profilArenyze(pseudo);
  const fp = v?.stats?.platform_families_full_profiles?.[0]?.board_ids_full_profiles?.flatMap((t) => t.full_profiles || []) || [];
  const id = String(fp.find((x) => x.profile?.id)?.profile?.id || '').toLowerCase();
  if (!PROFIL.test(id)) return [];
  const s = depuisProfil(v, id);
  if (s) cache.set(id, { quand: Date.now(), stats: s });
  return [[id, v.player?.nameOnPlatform || pseudo]];
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
    if (url.pathname === '/sante') return repondre(res, 200, { ok: true, cle: !!CLE });
    if (trop(ip)) return repondre(res, 429, { erreur: 'Trop de demandes, réessaie dans une minute' });
    try {
      if (url.pathname === '/v1/stats') {
        // joueurs=profil:pseudo,… (le pseudo sert à interroger Arenyze, le profil à ranger les stats)
        const joueurs = (url.searchParams.get('joueurs') || '').split(',').map((x) => x.split(':')).filter(([p, n]) => PROFIL.test((p || '').toLowerCase()) && PSEUDO.test(n || '')).map(([p, n]) => [p.toLowerCase(), n]).slice(0, 20);
        return repondre(res, 200, await stats(joueurs));
      }
      // Réponse brute d'Arenyze, seulement depuis le serveur lui-même (pour suivre l'évolution de leur format)
      if (url.pathname === '/v1/brut' && !req.headers['x-forwarded-for']) {
        return repondre(res, 200, await profilArenyze((url.searchParams.get('pseudo') || '').slice(0, 32)));
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
