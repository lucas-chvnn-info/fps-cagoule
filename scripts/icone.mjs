// Génère build/icone.png (1024 px) depuis build/icone.svg ; `tauri icon` en tire ensuite toutes les tailles.
import { Resvg } from '@resvg/resvg-js';
import { readFile, writeFile } from 'node:fs/promises';

const svg = await readFile(new URL('../build/icone.svg', import.meta.url));
const png = new Resvg(svg, { fitTo: { mode: 'width', value: 1024 } }).render().asPng();
await writeFile(new URL('../build/icone.png', import.meta.url), png);
console.log('build/icone.png généré');
