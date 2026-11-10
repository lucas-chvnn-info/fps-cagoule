// Fond animé de l'écran de démarrage : grille néon en perspective et étincelles.
// Un seul canvas 2D, arrêté et retiré de la page dès que l'écran de démarrage disparaît.
(() => {
  const canvas = document.getElementById('splash-fond');
  const ctx = canvas.getContext('2d');
  const calme = matchMedia('(prefers-reduced-motion: reduce)').matches;
  let l = 0, h = 0, r = 1;

  function taille() {
    r = Math.min(devicePixelRatio || 1, 2);
    l = innerWidth;
    h = innerHeight;
    canvas.width = l * r;
    canvas.height = h * r;
  }
  taille();
  addEventListener('resize', taille);

  const etincelles = Array.from({ length: 70 }, () => nouvelle(true));
  function nouvelle(partout) {
    return { x: Math.random(), y: partout ? Math.random() : 1.05, v: 0.0015 + Math.random() * 0.004, t: 0.6 + Math.random() * 1.8, a: 0.25 + Math.random() * 0.6 };
  }

  const debut = performance.now();
  function image(maintenant) {
    if (!canvas.isConnected) return;
    const t = (maintenant - debut) / 1000;
    ctx.setTransform(r, 0, 0, r, 0, 0);
    ctx.clearRect(0, 0, l, h);

    // Horizon lumineux
    const horizon = h * 0.62;
    const lueur = ctx.createRadialGradient(l / 2, horizon, 0, l / 2, horizon, l * 0.55);
    lueur.addColorStop(0, 'rgba(70, 120, 255, .28)');
    lueur.addColorStop(1, 'rgba(70, 120, 255, 0)');
    ctx.fillStyle = lueur;
    ctx.fillRect(0, 0, l, h);

    // Grille au sol qui défile vers nous
    const apparition = Math.min(1, t / 0.8);
    ctx.lineWidth = 1;
    const sol = h - horizon;
    for (let i = 0; i < 18; i++) {
      const p = ((i + (calme ? 0 : t * 1.6)) % 18) / 18;
      const y = horizon + sol * p * p;
      ctx.strokeStyle = `rgba(90, 140, 255, ${0.5 * p * apparition})`;
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(l, y);
      ctx.stroke();
    }
    for (let i = -14; i <= 14; i++) {
      const x = l / 2 + i * l * 0.09;
      const fond = l / 2 + i * l * 0.012;
      const g = ctx.createLinearGradient(0, horizon, 0, h);
      g.addColorStop(0, 'rgba(90, 140, 255, 0)');
      g.addColorStop(1, `rgba(90, 140, 255, ${0.45 * apparition})`);
      ctx.strokeStyle = g;
      ctx.beginPath();
      ctx.moveTo(fond, horizon);
      ctx.lineTo(x, h);
      ctx.stroke();
    }

    // Étincelles qui montent
    for (const e of etincelles) {
      if (!calme) e.y -= e.v;
      if (e.y < -0.05) Object.assign(e, nouvelle(false));
      const scint = 0.5 + 0.5 * Math.sin(t * 4 + e.x * 40);
      ctx.fillStyle = `rgba(140, 180, 255, ${e.a * scint * apparition})`;
      ctx.beginPath();
      ctx.arc(e.x * l, e.y * h, e.t, 0, Math.PI * 2);
      ctx.fill();
    }
    requestAnimationFrame(image);
  }
  requestAnimationFrame(image);

  // Progression réelle du chargement, appelée par app.js
  window.splashEtape = (fraction, texte) => {
    const barre = document.getElementById('splash-progression');
    if (barre) barre.style.width = `${Math.round(fraction * 100)}%`;
    const statut = document.getElementById('splash-statut');
    if (statut && texte) statut.textContent = texte;
  };
})();
