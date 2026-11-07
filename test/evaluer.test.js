import { test } from 'node:test';
import assert from 'node:assert/strict';
import { evaluer } from '../ui/evaluer.js';

const Go = 1024 ** 3;
const pcSain = {
  ram: [
    { capacite: 16 * Go, vitesse: 6000, vitesseReelle: 6000, type: 34 },
    { capacite: 16 * Go, vitesse: 6000, vitesseReelle: 6000, type: 34 },
  ],
  gpus: [{ nom: 'NVIDIA GeForce RTX 4070', hz: 165, hzMax: 165, datePilote: '2026-10-01' }],
  plan: { guid: '8c5e7fd1-ce92-466d-be55-c3230bc7bdb8', nom: 'Performances élevées' },
  reglages: { modeJeu: 1, gameDvr: 0, hags: 2, accelerationSouris: '0' },
  volumes: [{ lettre: 'C:', taille: 1000 * Go, libre: 400 * Go }],
  lecteurSysteme: 'C:',
  typeDisqueSysteme: 'SSD',
  disques: [{ nom: 'Samsung 990', sante: 'Healthy' }],
  demarrage: [],
  os: { joursAllume: 1 },
  vbs: 0,
};
const maintenant = new Date('2026-11-07');

test('un PC bien réglé a 100', () => {
  assert.deepEqual(evaluer(pcSain, { maintenant }), { score: 100, problemes: [] });
});

test('les problèmes graves passent en premier et baissent le score', () => {
  const pc = { ...pcSain, ram: [pcSain.ram[0]], gpus: [{ ...pcSain.gpus[0], hz: 60 }], reglages: { ...pcSain.reglages, modeJeu: 0 } };
  const { score, problemes } = evaluer(pc, { maintenant });
  assert.deepEqual(problemes.map((p) => p.id), ['ram-canal', 'ecran-hz', 'mode-jeu']);
  assert.equal(score, 100 - 15 - 15 - 8);
  assert.equal(problemes[2].correctif, 'mode-jeu');
});

test('le plan créé par l\'app compte comme Performances élevées', () => {
  const pc = { ...pcSain, plan: { guid: 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee', nom: 'Copie' } };
  assert.equal(evaluer(pc, { maintenant }).problemes[0].id, 'plan');
  assert.equal(evaluer(pc, { maintenant, planPerso: 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee' }).score, 100);
});

test('RAM DDR4 sans XMP et disque presque plein', () => {
  const pc = {
    ...pcSain,
    ram: pcSain.ram.map((b) => ({ ...b, type: 26, vitesse: 2133, vitesseReelle: 2133 })),
    volumes: [{ lettre: 'C:', taille: 500 * Go, libre: 12 * Go }],
  };
  const ids = evaluer(pc, { maintenant }).problemes.map((p) => p.id);
  assert.ok(ids.includes('ram-xmp'));
  assert.ok(ids.includes('disque-plein'));
});

test('une analyse incomplète ne plante pas', () => {
  assert.equal(evaluer({}, { maintenant }).score, 98); // seule la planification GPU inconnue est signalée
});

test('copie du plan Performances élevées et carte graphique virtuelle ignorées', () => {
  const pc = {
    ...pcSain,
    plan: { guid: '11111111-2222-3333-4444-555555555555', nom: 'High performance' },
    gpus: [{ nom: 'Microsoft Hyper-V Video', datePilote: '2006-06-21', hz: 60, hzMax: 0 }],
  };
  assert.equal(evaluer(pc, { maintenant }).score, 100);
});

test('le mode « Meilleures performances » compte comme un plan performances', () => {
  const pc = { ...pcSain, plan: { guid: '381b4222-f694-41f0-9685-ff5bb260df2e', nom: 'Utilisation normale', mode: 'ded574b5-45a0-4f42-8737-46345c09c238' } };
  assert.equal(evaluer(pc, { maintenant }).score, 100);
  assert.equal(evaluer({ ...pc, plan: { ...pc.plan, mode: '' } }, { maintenant }).problemes[0].id, 'plan');
});
