// Télécharge PresentMon (Intel, licence MIT) dans resources/ et vérifie son empreinte.
// Lancé avant chaque compilation ; le binaire n'est pas versionné.
import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';

const VERSION = '2.6.0';
const LIEN = `https://github.com/GameTechDev/PresentMon/releases/download/v${VERSION}/PresentMon-${VERSION}-x64.exe`;
const SHA256 = 'b2a706bc6ad475749e3b7e3409263aa1e6906d45bdcf993f6dbc0f660188f1af';
const CIBLE = new URL('../resources/PresentMon.exe', import.meta.url);

const empreinte = (buf) => createHash('sha256').update(buf).digest('hex');

if (existsSync(CIBLE) && empreinte(await readFile(CIBLE)) === SHA256) {
  console.log(`PresentMon ${VERSION} déjà présent`);
  process.exit(0);
}

const rep = await fetch(LIEN);
if (!rep.ok) throw new Error(`Téléchargement de PresentMon impossible : HTTP ${rep.status}`);
const buf = Buffer.from(await rep.arrayBuffer());
if (empreinte(buf) !== SHA256) throw new Error('Empreinte de PresentMon incorrecte, fichier rejeté');

await mkdir(new URL('../resources/', import.meta.url), { recursive: true });
await writeFile(CIBLE, buf);
console.log(`PresentMon ${VERSION} téléchargé (${buf.length} octets)`);
