// Évaluation de l'analyse du PC : liste des problèmes et score sur 100.
// Pur JavaScript, exécuté dans l'interface uniquement quand elle est ouverte.

export const PLANS_PERF = [
  '8c5e7fd1-ce92-466d-be55-c3230bc7bdb8', // Performances élevées
  'e9a42b02-d5df-448d-aa00-03f14749eb61', // Performances optimales
];

const GO = 1024 ** 3;

// Liste des problèmes détectés : gravite 'haute' | 'moyenne' | 'info'.
// `correctif` renvoie à une optimisation que l'app peut appliquer elle-même.
export function evaluer(info, { planPerso = null, maintenant = new Date() } = {}) {
  const pb = [];
  const ajoute = (id, gravite, titre, detail, correctif) => pb.push({ id, gravite, titre, detail, correctif });

  // Mémoire vive
  const barrettes = info.ram || [];
  const totalRam = barrettes.reduce((s, b) => s + (Number(b.capacite) || 0), 0);
  if (barrettes.length === 1) {
    ajoute('ram-canal', 'haute', 'Mémoire en simple canal',
      "Une seule barrette de RAM : la bande passante est divisée par deux, ce qui coûte souvent 10 à 25 % de FPS dans les jeux qui dépendent du processeur. Ajoute une deuxième barrette identique.");
  }
  if (totalRam && totalRam < 15 * GO) {
    ajoute('ram-quantite', 'moyenne', `Seulement ${Math.round(totalRam / GO)} Go de RAM`,
      'Les jeux récents en demandent 16 Go. En dessous, Windows utilise le disque et ça provoque des saccades.');
  }
  const ram0 = barrettes[0];
  if (ram0) {
    const reelle = Number(ram0.vitesseReelle) || 0;
    const nominale = Number(ram0.vitesse) || 0;
    const ddr5 = Number(ram0.type) === 34;
    const ddr4 = Number(ram0.type) === 26;
    const lente = (ddr4 && reelle > 0 && reelle <= 2666) || (ddr5 && reelle > 0 && reelle <= 4800);
    if ((nominale && reelle && reelle < nominale * 0.9) || lente) {
      ajoute('ram-xmp', 'moyenne', `RAM à ${reelle} MHz : XMP/EXPO probablement désactivé`,
        "Ta mémoire tourne sans doute en dessous de sa vitesse prévue. Active le profil XMP (Intel) ou EXPO (AMD) dans le BIOS. Ça ne se règle pas depuis Windows.");
    }
  }

  // Écran
  for (const g of info.gpus || []) {
    const hz = Number(g.hz) || 0;
    const max = Number(g.hzMax) || 0;
    if (hz && max >= 100 && hz < max - 5) {
      ajoute('ecran-hz', 'haute', `Écran à ${hz} Hz au lieu de ${max} Hz`,
        "Ton écran peut afficher plus d'images par seconde que ce qui est réglé. Paramètres → Système → Écran → Paramètres d'affichage avancés → Fréquence d'actualisation.");
      break;
    }
  }

  // Pilote graphique
  for (const g of info.gpus || []) {
    if (!g.datePilote || /microsoft basic|remote|virtual/i.test(g.nom || '')) continue;
    const mois = (maintenant - new Date(g.datePilote)) / (30 * 24 * 3600 * 1000);
    if (mois > 6) {
      ajoute('pilote-gpu', 'moyenne', `Pilote graphique daté de ${Math.floor(mois)} mois`,
        `${g.nom} : installe le dernier pilote (NVIDIA App, AMD Adrenalin ou Intel). Les nouveaux pilotes contiennent souvent des optimisations pour les jeux récents.`);
      break;
    }
  }

  // Plan d'alimentation
  const guid = (info.plan?.guid || '').toLowerCase();
  if (guid && !PLANS_PERF.includes(guid) && guid !== planPerso) {
    ajoute('plan', 'moyenne', `Plan d'alimentation « ${info.plan.nom || 'inconnu'} »`,
      'Le plan Performances élevées garde le processeur à pleine fréquence et évite les micro-latences.', 'plan-alimentation');
  }

  // Réglages Windows
  const r = info.reglages || {};
  if (r.modeJeu === 0) {
    ajoute('mode-jeu', 'moyenne', 'Mode Jeu désactivé',
      'Le Mode Jeu de Windows donne la priorité au jeu et bloque les mises à jour pendant les parties.', 'mode-jeu');
  }
  if (r.gameDvr !== 0 && r.gameDvr !== null && r.gameDvr !== undefined) {
    ajoute('game-dvr', 'moyenne', "Enregistrement en arrière-plan de la Xbox Game Bar actif",
      "La Game Bar peut enregistrer en continu et consommer des ressources. Medal ou ShadowPlay le font mieux.", 'game-dvr');
  }
  if (r.hags !== 2) {
    ajoute('hags', 'info', 'Planification GPU accélérée désactivée',
      "Réduit un peu la latence sur les cartes récentes (NVIDIA 1000+, AMD 5000+). Nécessaire pour DLSS Frame Generation.", 'hags');
  }
  if (r.accelerationSouris !== undefined && r.accelerationSouris !== null && String(r.accelerationSouris) !== '0') {
    ajoute('souris', 'moyenne', 'Précision du pointeur activée',
      "L'accélération de la souris rend la visée irrégulière dans les FPS.", 'souris');
  }
  if (Number(info.vbs) === 2) {
    ajoute('vbs', 'info', 'Isolation du noyau (VBS) active',
      "Cette protection de Windows coûte quelques pour-cent de FPS. Elle protège contre des attaques réelles : FPS Cagoule ne la désactive pas pour toi. Certains anti-triches (Valorant, FACEIT) l'exigent.");
  }

  // Disques
  const lecteur = (info.lecteurSysteme || 'C:').toUpperCase();
  const vol = (info.volumes || []).find((v) => (v.lettre || '').toUpperCase() === lecteur);
  if (vol && vol.taille) {
    const libre = Number(vol.libre) || 0;
    if (libre < 20 * GO || libre / vol.taille < 0.1) {
      ajoute('disque-plein', 'haute', `Disque ${lecteur} presque plein (${Math.round(libre / GO)} Go libres)`,
        'Un disque système plein ralentit tout Windows. Utilise le nettoyage ou libère de la place.', 'nettoyage');
    }
  }
  if (/hdd/i.test(info.typeDisqueSysteme || '')) {
    ajoute('disque-hdd', 'haute', 'Windows est installé sur un disque dur classique',
      'Un SSD rend Windows et les chargements de jeux plusieurs fois plus rapides.');
  }
  for (const d of info.disques || []) {
    if (d.sante && !/healthy|sain/i.test(d.sante)) {
      ajoute('disque-sante', 'haute', `Disque « ${d.nom} » en mauvaise santé`, 'Sauvegarde tes fichiers et prévois de le remplacer.');
    }
  }

  // Démarrage et entretien
  const nbDemarrage = (info.demarrage || []).length;
  if (nbDemarrage > 8) {
    ajoute('demarrage', 'info', `${nbDemarrage} programmes au démarrage`,
      "Ils tournent en permanence en arrière-plan. Désactive ceux dont tu n'as pas besoin.", 'demarrage');
  }
  if (Number(info.os?.joursAllume) > 7) {
    ajoute('redemarrer', 'info', `PC allumé depuis ${Math.floor(info.os.joursAllume)} jours`,
      "L'arrêt de Windows 11 n'est pas un vrai redémarrage (démarrage rapide). Un redémarrage libère la mémoire et termine les mises à jour.");
  }
  if (Number(info.tempGpu) >= 85) {
    ajoute('temp-gpu', 'haute', `Carte graphique à ${info.tempGpu} °C`,
      'Elle va baisser sa fréquence pour refroidir. Nettoie la poussière et vérifie la ventilation du boîtier.');
  }

  const poids = { haute: 15, moyenne: 8, info: 2 };
  const score = Math.max(0, Math.min(100, 100 - pb.reduce((s, p) => s + poids[p.gravite], 0)));
  const ordre = { haute: 0, moyenne: 1, info: 2 };
  pb.sort((a, b) => ordre[a.gravite] - ordre[b.gravite]);
  return { score, problemes: pb };
}
