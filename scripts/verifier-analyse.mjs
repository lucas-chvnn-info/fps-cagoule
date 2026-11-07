// Vérifie que l'analyse produite sur un vrai Windows (CI) passe dans l'évaluation de l'interface.
import { readFile } from 'node:fs/promises';
import { evaluer } from '../ui/evaluer.js';

const info = JSON.parse(await readFile(process.argv[2], 'utf8'));
const { score, problemes } = evaluer(info);
if (!Number.isFinite(score)) throw new Error('Score invalide');
console.log(`CPU : ${info.cpu.nom} | GPU : ${info.gpus.map((g) => g.nom).join(', ')} | RAM : ${info.ram.length} barrette(s)`);
console.log(`Score : ${score}/100`);
for (const p of problemes) console.log(`- [${p.gravite}] ${p.titre}`);
