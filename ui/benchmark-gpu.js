// Test de la carte graphique : une scène 3D calculée entièrement par la carte (raymarching) en 1080p.
// On compte les images réellement terminées (gl.finish), sans la limite de fréquence de l'écran.

const SOMMETS = `#version 300 es
void main() {
  vec2 p = vec2((gl_VertexID << 1) & 2, gl_VertexID & 2);
  gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}`;

const PIXELS = `#version 300 es
precision highp float;
uniform vec2 res;
uniform float t;
out vec4 couleur;
float scene(vec3 p) {
  vec3 q = mod(p + 2.0, 4.0) - 2.0;
  float boule = length(q) - 1.0 + 0.08 * sin(5.0 * p.x + t) * sin(5.0 * p.y) * sin(5.0 * p.z);
  return min(boule, p.y + 1.5);
}
vec3 normale(vec3 p) {
  vec2 e = vec2(0.001, 0.0);
  return normalize(vec3(scene(p + e.xyy) - scene(p - e.xyy), scene(p + e.yxy) - scene(p - e.yxy), scene(p + e.yyx) - scene(p - e.yyx)));
}
float ombre(vec3 o, vec3 d) {
  float r = 1.0, dist = 0.05;
  for (int i = 0; i < 32; i++) {
    float h = scene(o + d * dist);
    r = min(r, 8.0 * h / dist);
    dist += clamp(h, 0.02, 0.5);
    if (h < 0.001 || dist > 12.0) break;
  }
  return clamp(r, 0.0, 1.0);
}
void main() {
  vec2 uv = (gl_FragCoord.xy * 2.0 - res) / res.y;
  vec3 o = vec3(sin(t * 0.3) * 3.0, 1.5, t), d = normalize(vec3(uv, 1.6));
  float dist = 0.0;
  for (int i = 0; i < 96; i++) {
    float h = scene(o + d * dist);
    if (h < 0.001 || dist > 40.0) break;
    dist += h;
  }
  vec3 c = vec3(0.03, 0.04, 0.08);
  if (dist < 40.0) {
    vec3 p = o + d * dist, n = normale(p), l = normalize(vec3(0.6, 0.8, -0.4));
    float lumiere = max(dot(n, l), 0.0) * ombre(p + n * 0.01, l);
    c = mix(vec3(0.18, 0.43, 0.93), vec3(0.45, 0.62, 1.0), 0.5 + 0.5 * n.y) * (0.15 + lumiere);
    c = mix(c, vec3(0.03, 0.04, 0.08), 1.0 - exp(-0.003 * dist * dist));
  }
  couleur = vec4(pow(c, vec3(0.4545)), 1.0);
}`;

/** Points : images 1080p par seconde × 2 (une RTX 3060 fait environ 1000 points) */
export const POINTS_PAR_IMAGE = 2;

/**
 * Lance le test sur `canvas` (1920 × 1080) pendant `duree` ms.
 * `progression(fraction)` est appelée à chaque image. Renvoie { images_s, points, gpu }.
 */
export function testerGpu(canvas, duree, progression) {
  return new Promise((resoudre, rejeter) => {
    const gl = canvas.getContext('webgl2', { powerPreference: 'high-performance', antialias: false, preserveDrawingBuffer: false });
    if (!gl) return rejeter(new Error('WebGL 2 indisponible'));
    const info = gl.getExtension('WEBGL_debug_renderer_info');
    const gpu = info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER);
    const shader = (type, source) => {
      const s = gl.createShader(type);
      gl.shaderSource(s, source);
      gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s));
      return s;
    };
    let prog;
    try {
      prog = gl.createProgram();
      gl.attachShader(prog, shader(gl.VERTEX_SHADER, SOMMETS));
      gl.attachShader(prog, shader(gl.FRAGMENT_SHADER, PIXELS));
      gl.linkProgram(prog);
      if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(prog));
    } catch (e) {
      return rejeter(e);
    }
    gl.useProgram(prog);
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.uniform2f(gl.getUniformLocation(prog, 'res'), canvas.width, canvas.height);
    const uT = gl.getUniformLocation(prog, 't');
    const dessiner = (t) => {
      gl.uniform1f(uT, t);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
      gl.finish();
    };
    const chauffe = 1000;
    const debut = performance.now();
    let images = 0;
    let debutMesure = 0;
    const image = () => {
      const maintenant = performance.now();
      const ecoule = maintenant - debut;
      // Au moins 3 images mesurées, même sur une carte très lente (limite de sécurité : 30 s de plus)
      if ((ecoule >= chauffe + duree && images >= 3) || ecoule >= chauffe + duree + 30000) {
        const secondes = Math.max(0.001, (maintenant - debutMesure) / 1000);
        const images_s = images / secondes;
        return resoudre({ images_s, points: Math.round(images_s * POINTS_PAR_IMAGE), gpu });
      }
      // Autant d'images que possible dans ~12 ms, puis on rend la main pour l'aperçu
      do {
        const t0 = performance.now();
        dessiner(ecoule / 1000 + images * 0.01);
        if (ecoule >= chauffe) {
          if (!debutMesure) debutMesure = t0;
          images++;
        }
      } while (performance.now() - maintenant < 12);
      progression(Math.min(1, ecoule / (chauffe + duree)));
      requestAnimationFrame(image);
    };
    requestAnimationFrame(image);
  });
}

/* ---------- Test de stabilité (overclocking) ---------- */

const ZONE = 256;

/** Prépare la scène sur `canvas` pour des images fixes, comparables pixel par pixel. */
export function preparerStabilite(canvas) {
  const gl = canvas.getContext('webgl2', { powerPreference: 'high-performance', antialias: false, preserveDrawingBuffer: true });
  if (!gl) throw new Error('WebGL 2 indisponible');
  let perdu = false;
  canvas.addEventListener('webglcontextlost', (e) => {
    perdu = true;
    e.preventDefault();
  });
  const compiler = (type, source) => {
    const s = gl.createShader(type);
    gl.shaderSource(s, source);
    gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s));
    return s;
  };
  const prog = gl.createProgram();
  gl.attachShader(prog, compiler(gl.VERTEX_SHADER, SOMMETS));
  gl.attachShader(prog, compiler(gl.FRAGMENT_SHADER, PIXELS));
  gl.linkProgram(prog);
  gl.useProgram(prog);
  gl.viewport(0, 0, canvas.width, canvas.height);
  gl.uniform2f(gl.getUniformLocation(prog, 'res'), canvas.width, canvas.height);
  const uT = gl.getUniformLocation(prog, 't');
  const pixels = new Uint8Array(ZONE * ZONE * 4);
  const x = (canvas.width - ZONE) >> 1;
  const y = (canvas.height - ZONE) >> 1;
  // Image fixe n° i : empreinte (FNV-1a) des pixels du centre
  const empreinte = (i) => {
    gl.uniform1f(uT, i * 1.37);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    gl.readPixels(x, y, ZONE, ZONE, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
    let h = 0x811c9dc5;
    for (let k = 0; k < pixels.length; k++) h = Math.imul(h ^ pixels[k], 0x01000193);
    return h >>> 0;
  };
  return { empreinte, perdu: () => perdu || gl.isContextLost() };
}

/**
 * Fait tourner la scène `duree` ms en comparant chaque image à `reference` (empreintes calculées aux
 * fréquences d'origine). Renvoie { ok, raison, images_s }.
 */
export function testerStabilite(scene, reference, duree, progression) {
  return new Promise((resoudre) => {
    const debut = performance.now();
    let images = 0;
    const image = () => {
      if (scene.perdu()) return resoudre({ ok: false, raison: 'le pilote graphique a planté', images_s: 0 });
      const maintenant = performance.now();
      if (maintenant - debut >= duree) return resoudre({ ok: true, images_s: images / ((maintenant - debut) / 1000) });
      do {
        const i = images % reference.length;
        if (scene.empreinte(i) !== reference[i]) {
          return resoudre({ ok: false, raison: 'erreurs de calcul détectées (artefacts)', images_s: 0 });
        }
        images++;
      } while (performance.now() - maintenant < 12);
      progression?.(Math.min(1, (maintenant - debut) / duree));
      requestAnimationFrame(image);
    };
    requestAnimationFrame(image);
  });
}

/** Empreintes de référence : à calculer aux fréquences d'origine */
export function references(scene, n = 12) {
  return Array.from({ length: n }, (_, i) => scene.empreinte(i));
}
