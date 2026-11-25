// Portraits des opérateurs de R6 (fenêtres des dégâts et du tableau)
// Icônes des opérateurs (r6operators, licence MIT, redessinées par Marco Vockner ; Noor et Solid Snake ajoutés à part)
const ICONES = new Set('ace alibi amaru aruni ash azami bandit blackbeard blitz brava buck capitao castle caveira clash deimos denari doc dokkaebi echo ela fenrir finka flores frost fuze glaz goyo gridlock grim hibana iana iq jackal jager kaid kali kapkan lesion lion maestro maverick melusi mira montagne mozzie mute nokk nomad noor oryx osa pulse ram rauora recrue rook sens sentry skopos sledge smoke solidsnake solis striker tachanka thatcher thermite thorn thunderbird tubarao twitch valkyrie vigil wamai warden ying zero zofia'.split(' '));
const escOp = (t) => String(t ?? '').replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);

function icone(op) {
  const cle = (op || '').normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase().replace(/ø/g, 'o').replace(/[^a-z0-9]/g, '');
  if (ICONES.has(cle)) return `<img class="op" src="operateurs/${cle}.webp" alt="${escOp(op)}" title="${escOp(op)}">`;
  const lettres = (op || '?').split(/\s+/).map((m) => m[0]).join('').slice(0, 2).toUpperCase();
  return `<span class="op lettres" title="${escOp(op || '')}">${escOp(lettres)}</span>`;
}
