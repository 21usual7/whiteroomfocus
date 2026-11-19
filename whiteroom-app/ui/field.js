// Живой фон WHITE ROOM: голографический мозг из нейронов на WebGL 1 без зависимостей.
// Это декоративная модель, а не активность мозга пользователя. От данных зависят только
// яркость шести областей (минуты по режимам), частота импульсов (карточки к повторению)
// и вспышки после верных ответов и решённых задач.
(() => {
  const canvas = document.getElementById("field");
  const noop = () => {};
  const api = { setScene: noop, setMode: noop, hover: noop, setData: noop, pulse: noop, activate: noop, setMotion: noop, skip: noop };
  window.field = api;
  const gl = canvas.getContext("webgl", { alpha: false, antialias: true, depth: false, premultipliedAlpha: true });
  if (!gl) {
    document.body.classList.add("no-field");
    return;
  }

  // aMeta: x — случайное число частицы, y — область (-1 вне областей), z — вид:
  // 0 — пыль вокруг, 1 — кора, 2 — нейрон, 3 — свечение, 4 — импульс.
  const vertex = `
    precision highp float;
    attribute vec3 aField;
    attribute vec3 aCore;
    attribute vec3 aMeta;
    attribute float aAct;
    uniform float uTime, uMorph, uLight, uIntro, uQuiet, uDim, uPx, uAspect, uZoom, uMode, uHover, uPulse, uCore, uLine, uYaw, uPitch;
    uniform vec2 uShift;
    uniform float uW[6];
    varying vec4 vColor;
    vec3 rotY(vec3 p, float a) { float c = cos(a), s = sin(a); return vec3(c * p.x + s * p.z, p.y, -s * p.x + c * p.z); }
    vec3 rotX(vec3 p, float a) { float c = cos(a), s = sin(a); return vec3(p.x, c * p.y - s * p.z, s * p.y + c * p.z); }
    void main() {
      float s = aMeta.x, g = aMeta.y, kind = aMeta.z;
      vec3 p = aField;
      if (kind < 0.5) {
        p = rotY(p, uTime * (0.02 + 0.03 * s));
        p.y += 0.2 * sin(uTime * 0.2 + s * 30.0);
      } else {
        p *= 1.0 + 0.01 * sin(uTime * 0.6);
      }
      vec3 c = rotY(aCore, uTime * (0.05 + 0.05 * fract(s * 7.0)));
      float m = clamp(uMorph * 1.5 - s * 0.5, 0.0, 1.0);
      p = mix(p, c, m * m * (3.0 - 2.0 * m));
      p *= (1.0 + 0.12 * uPulse * sin(s * 3.14)) * mix(2.0, 1.0, uIntro) * uZoom;
      p = rotX(rotY(p, uYaw), uPitch);
      float z = p.z + 9.0;
      vec2 xy = p.xy * 2.7 / z;
      xy.x /= uAspect;
      gl_Position = vec4(xy + uShift, 0.0, 1.0);
      float near = clamp(1.0 - (z - 5.0) / 9.0, 0.0, 1.0);
      float w = g >= 0.0 ? uW[int(g + 0.5)] : 0.0;
      float sel = step(abs(g - uMode), 0.25);
      float hov = step(abs(g - uHover), 0.25);
      vec3 silver = vec3(0.70, 0.76, 0.86), cyan = vec3(0.38, 0.86, 0.95), blue = vec3(0.26, 0.48, 1.0), spark = vec3(0.82, 0.97, 1.0);
      vec3 col;
      float a, size;
      if (uLine > 0.5) {
        col = mix(mix(blue, silver, 0.25), cyan, max(aAct, sel * 0.6));
        a = 0.03 + 0.06 * w + 0.05 * sel + 0.03 * hov + 0.8 * aAct;
        a *= 1.0 - uMorph;
        size = 1.0;
      } else if (kind < 0.5) {
        col = silver;
        a = (0.08 + 0.32 * s * s) * (0.4 + 0.6 * near);
        size = 1.4 + 3.0 * s * s;
      } else if (kind < 1.5) {
        col = mix(mix(blue, silver, s), cyan, max(sel, hov * 0.6));
        a = 0.17 + 0.12 * w + 0.14 * sel + 0.08 * hov;
        size = 1.7 + 1.8 * s;
      } else if (kind < 2.5) {
        col = mix(mix(mix(silver, blue, w), cyan, max(sel, hov * 0.6)), spark, aAct);
        a = 0.28 + 0.3 * w + 0.35 * sel + 0.15 * hov + 1.4 * aAct;
        size = (2.2 + 3.2 * s * s) * (1.0 + 0.9 * aAct + 0.25 * sel);
      } else if (kind < 3.5) {
        col = blue;
        a = 0.07 + 0.06 * uCore + 0.025 * sin(uTime * 0.7);
        size = 420.0 * mix(1.0, 0.5, uMorph);
      } else {
        col = spark;
        a = 1.0 - uMorph;
        size = 9.0;
      }
      size *= uPx * 7.0 / z;
      // Слишком близкие частицы гаснут, чтобы не ложиться пятнами на текст.
      a *= smoothstep(3.0, 6.0, z);
      if (kind < 2.5) size = min(size, 18.0 * uPx);
      a *= uIntro * (1.0 - 0.6 * uDim) * mix(1.0, 0.45, uQuiet);
      col = mix(col, vec3(0.09, 0.10, 0.12), uLight);
      // В белой комнате фон едва заметен и не спорит с текстом.
      a *= mix(1.0, 0.16, uLight);
      gl_PointSize = size;
      vColor = vec4(col, a);
    }`;
  const fragment = `
    precision highp float;
    varying vec4 vColor;
    uniform float uLine, uLight;
    void main() {
      float a = vColor.a;
      if (uLine < 0.5) {
        float d = length(gl_PointCoord - 0.5) * 2.0;
        a *= exp(-d * d * 3.0) * step(d, 1.0);
      }
      // В темноте частицы складываются (свечение), в белой комнате ложатся поверх.
      gl_FragColor = vec4(vColor.rgb * a, a * uLight);
    }`;

  function shader(type, source) {
    const node = gl.createShader(type);
    gl.shaderSource(node, source);
    gl.compileShader(node);
    if (!gl.getShaderParameter(node, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(node));
    return node;
  }
  const program = gl.createProgram();
  try {
    gl.attachShader(program, shader(gl.VERTEX_SHADER, vertex));
    gl.attachShader(program, shader(gl.FRAGMENT_SHADER, fragment));
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program));
  } catch (error) {
    console.warn("Фон отключён:", error);
    document.body.classList.add("no-field");
    return;
  }
  gl.useProgram(program);

  // ───── Геометрия мозга. Оси: x — лево/право, y — верх, z — лоб (+) / затылок (−). ─────
  // Детерминированный шум: картина одинакова при каждом запуске.
  let seed = 20261119;
  const rand = () => (seed = seed * 16807 % 2147483647) / 2147483647;
  const gauss = () => Math.sqrt(-2 * Math.log(rand() + 1e-9)) * Math.cos(2 * Math.PI * rand());
  const SCALE = 2.6;

  // Квадрат «эллипсоидного расстояния»: < 1 внутри.
  function cerebrum(x, y, z) {
    const ax = Math.abs(x);
    if (ax < 0.035) return 9;
    const hx = (ax - 0.37) / 0.4, hy = y / (y < 0 ? 0.48 : 0.64), hz = z / (z > 0 ? 0.92 : 0.98);
    const tx = (ax - 0.5) / 0.24, ty = (y + 0.3) / 0.24, tz = (z - 0.12) / 0.5;
    return Math.min(hx * hx + hy * hy + hz * hz, tx * tx + ty * ty + tz * tz);
  }
  function cerebellum(x, y, z) {
    const cx = (Math.abs(x) - 0.2) / 0.32, cy = (y + 0.5) / 0.2, cz = (z + 0.62) / 0.3;
    return cx * cx + cy * cy + cz * cz;
  }
  function stem(x, y, z) {
    if (y > -0.3 || y < -1.05) return 9;
    const cx = x / 0.11, cz = (z + 0.2 + (y + 0.3) * 0.25) / 0.13;
    return cx * cx + cz * cz;
  }
  // Извилины: точки коры ложатся вдоль волнистых линий.
  const fold = (x, y, z) => Math.sin(14 * y + 4 * Math.sin(6 * z + 2 * x)) * Math.cos(12 * z + 3 * Math.sin(7 * y + 3 * x));

  // Области под шесть режимов: лобная, теменная, височная, затылочная, мозжечок, глубинная.
  const regions = [[0.4, 0.25, 0.72], [0.4, 0.5, -0.2], [0.6, -0.3, 0.2], [0.35, 0.1, -0.85], [0.3, -0.55, -0.7], [0.12, -0.05, 0]];
  function region(x, y, z) {
    let best = 0, dist = Infinity;
    regions.forEach(([rx, ry, rz], i) => {
      const d = (Math.abs(x) - rx) ** 2 + (y - ry) ** 2 + (z - rz) ** 2;
      if (d < dist) { dist = d; best = i; }
    });
    return best;
  }
  const box = () => [(rand() * 2 - 1) * 0.8, rand() * 1.75 - 1.05, (rand() * 2 - 1) * 1.0];
  function sample(test, count) {
    const out = [];
    while (out.length < count) {
      const p = box();
      if (test(...p)) out.push(p);
    }
    return out;
  }

  const neurons = [
    ...sample((x, y, z) => { const d = cerebrum(x, y, z); return d > 0.55 && d < 1; }, 1500),
    ...sample((x, y, z) => Math.abs(x) < 0.3 && Math.abs(y + 0.05) < 0.22 && Math.abs(z) < 0.35 && cerebrum(x, y, z) < 1, 170),
    ...sample((x, y, z) => cerebellum(x, y, z) < 1, 180),
    ...sample((x, y, z) => stem(x, y, z) < 1, 50),
  ];
  const deepStart = 1500, deepEnd = 1670;
  const cortex = sample((x, y, z) => {
    const d = cerebrum(x, y, z);
    if (d > 0.8 && d < 1) return Math.abs(fold(x, y, z)) < 0.22 || rand() < 0.05;
    const c = cerebellum(x, y, z);
    if (c > 0.55 && c < 1) return Math.abs(Math.sin(y * 70 + z * 8)) < 0.35;
    return stem(x, y, z) < 1 && rand() < 0.4;
  }, 9000);
  const nN = neurons.length, nD = cortex.length, nA = 2600;

  // Кольца, в которые всё выстраивается при входе в комнату (переход не меняется).
  const tilt = [0, 0.5, -0.35];
  const radius = [1.1, 1.75, 2.5];
  function ring() {
    const k = Math.floor(rand() * 3);
    const phi = rand() * Math.PI * 2;
    const y = gauss() * 0.025;
    const z = Math.sin(phi) * radius[k];
    return [Math.cos(phi) * radius[k], y * Math.cos(tilt[k]) - z * Math.sin(tilt[k]), y * Math.sin(tilt[k]) + z * Math.cos(tilt[k])];
  }
  const scaled = (p) => p.map(v => v * SCALE);

  const STRIDE = 9;
  const total = 1 + nN + nD + nA;
  const data = new Float32Array(total * STRIDE);
  let o = 0;
  const put = (target, f, c, s, g, kind) => { target.set([...f, ...c, s, g, kind], o); o += STRIDE; };
  put(data, [0, 0, 0], [0, 0, 0], 0.5, -1, 3);
  const nodeRegion = new Int8Array(nN);
  const nodePos = neurons.map((p, i) => {
    nodeRegion[i] = region(...p);
    const q = scaled(p);
    put(data, q, ring(), rand(), nodeRegion[i], 2);
    return q;
  });
  for (const p of cortex) put(data, scaled(p), ring(), rand(), region(...p), 1);
  for (let i = 0; i < nA; i++) {
    const r = 4 + Math.pow(rand(), 0.7) * 5;
    const a = rand() * Math.PI * 2;
    put(data, [Math.cos(a) * r, gauss() * (0.5 + r * 0.12), Math.sin(a) * r], ring(), rand(), -1, 0);
  }

  // ───── Граф: соседи в коре, длинные волокна от глубины к коре и через мозолистое тело. ─────
  const edges = [];
  const adjacency = neurons.map(() => []);
  const has = new Set();
  const dist2 = (a, b) => (a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2 + (a[2] - b[2]) ** 2;
  function link(a, b, control) {
    const key = a < b ? a * nN + b : b * nN + a;
    if (a === b || has.has(key)) return;
    has.add(key);
    const pa = nodePos[a], pb = nodePos[b];
    const c = control ?? pa.map((v, i) => (v + pb[i]) / 2);
    const e = edges.length;
    edges.push({ a, b, c, len: Math.sqrt(dist2(pa, pb)) + 0.01, curved: Boolean(control) });
    adjacency[a].push(e);
    adjacency[b].push(e);
  }
  const reach = (0.2 * SCALE) ** 2;
  for (let i = 0; i < nN; i++) {
    const near = [];
    for (let j = 0; j < nN; j++) {
      if (j === i) continue;
      const d = dist2(nodePos[i], nodePos[j]);
      if (d < reach) near.push([d, j]);
    }
    near.sort((x, y) => x[0] - y[0]);
    for (const [, j] of near.slice(0, 2)) link(i, j);
  }
  for (let i = 0; i < 90; i++) {
    const a = deepStart + Math.floor(rand() * (deepEnd - deepStart));
    const b = Math.floor(rand() * deepStart);
    const pa = nodePos[a], pb = nodePos[b];
    link(a, b, pa.map((v, k) => (v + pb[k]) / 2 * 0.55));
  }
  for (let i = 0, made = 0; i < deepStart && made < 60; i++) {
    const p = nodePos[i];
    if (Math.abs(p[0]) > 0.22 * SCALE || p[1] < 0.1 * SCALE) continue;
    let best = -1, bestD = Infinity;
    for (let j = 0; j < deepStart; j++) {
      const q = nodePos[j];
      if (Math.sign(q[0]) === Math.sign(p[0])) continue;
      const d = dist2([-p[0], p[1], p[2]], q);
      if (d < bestD) { bestD = d; best = j; }
    }
    if (best >= 0) {
      link(i, best, [0, (p[1] + nodePos[best][1]) / 2 + 0.12 * SCALE, (p[2] + nodePos[best][2]) / 2]);
      made += 1;
    }
  }

  const bezier = (e, t, out) => {
    const pa = nodePos[e.a], pb = nodePos[e.b], c = e.c, u1 = 1 - t;
    for (let k = 0; k < 3; k++) out[k] = u1 * u1 * pa[k] + 2 * u1 * t * c[k] + t * t * pb[k];
    return out;
  };
  const lineVerts = [];
  const lineEdge = [];
  edges.forEach((e, i) => {
    const steps = e.curved ? 6 : 1;
    for (let s = 0; s < steps; s++) {
      lineVerts.push(bezier(e, s / steps, [0, 0, 0]), bezier(e, (s + 1) / steps, [0, 0, 0]));
      lineEdge.push(i, i);
    }
  });
  const lines = new Float32Array(lineVerts.length * STRIDE);
  o = 0;
  lineVerts.forEach((p, i) => put(lines, p, p, 0.5, nodeRegion[edges[lineEdge[i]].a], 5));

  const MAX_PULSES = 200;
  const pulseData = new Float32Array(MAX_PULSES * STRIDE);

  function buffer(array, usage) {
    const b = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, b);
    gl.bufferData(gl.ARRAY_BUFFER, array, usage);
    return b;
  }
  const pointBuf = buffer(data, gl.STATIC_DRAW);
  const nodeAct = new Float32Array(total);
  const pointAct = buffer(nodeAct, gl.DYNAMIC_DRAW);
  const lineBuf = buffer(lines, gl.STATIC_DRAW);
  const lineAct = new Float32Array(lineVerts.length);
  const lineActBuf = buffer(lineAct, gl.DYNAMIC_DRAW);
  const pulseBuf = buffer(pulseData, gl.DYNAMIC_DRAW);
  const at = {};
  for (const name of ["aField", "aCore", "aMeta", "aAct"]) at[name] = gl.getAttribLocation(program, name);
  function bind(buf, act) {
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    [["aField", 0], ["aCore", 12], ["aMeta", 24]].forEach(([name, offset]) => {
      gl.enableVertexAttribArray(at[name]);
      gl.vertexAttribPointer(at[name], 3, gl.FLOAT, false, STRIDE * 4, offset);
    });
    if (act) {
      gl.bindBuffer(gl.ARRAY_BUFFER, act);
      gl.enableVertexAttribArray(at.aAct);
      gl.vertexAttribPointer(at.aAct, 1, gl.FLOAT, false, 0, 0);
    } else {
      gl.disableVertexAttribArray(at.aAct);
      gl.vertexAttrib1f(at.aAct, 1);
    }
  }
  const u = {};
  for (const name of ["uTime", "uMorph", "uLight", "uIntro", "uQuiet", "uDim", "uPx", "uAspect", "uZoom", "uMode",
    "uHover", "uPulse", "uCore", "uLine", "uYaw", "uPitch", "uShift", "uW"]) u[name] = gl.getUniformLocation(program, name);
  gl.enable(gl.BLEND);
  gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);

  // ───── Сцены и переходы (без изменений по смыслу). ─────
  const scenes = {
    dash: { morph: 0, light: 0, quiet: 0, dim: 0, zoom: 0.88, shift: 0.36 },
    setup: { morph: 0.12, light: 0, quiet: 0, dim: 0.15, zoom: 0.9, shift: -0.4 },
    enter: { morph: 1, light: 0, quiet: 0, dim: 0, zoom: 1.05, shift: 0 },
    room: { morph: 1, light: 1, quiet: 1, dim: 0, zoom: 0.9, shift: 0 },
    tab: { morph: 0, light: 0, quiet: 0.5, dim: 0.65, zoom: 0.75, shift: 0 },
  };
  // Переходы идут по времени, а не по кадрам: на слабом GPU они не растягиваются.
  const cur = { morph: 0, light: 0, intro: 0, quiet: 0, dim: 0, zoom: 0.88, shift: 0.36, core: 0 };
  const goal = { ...cur, intro: 1 };
  let from = { ...cur }, tweenAt = performance.now(), tweenMs = 2200, pulseAt = -1e9;
  const ease = (x) => x < 0.5 ? 4 * x * x * x : 1 - Math.pow(-2 * x + 2, 3) / 2;
  const look = [0, 0], lookGoal = [0, 0];
  const weights = new Float32Array(6);
  let scene = "dash", mode = 0, hover = -1, motion = "live", due = 0;
  let time = 0, last = 0, lastDraw = 0, raf = 0, drawN = total, frames = 0, slow = 0, weak = false;
  const reduce = matchMedia("(prefers-reduced-motion: reduce)");
  const still = () => reduce.matches;
  const calm = () => weak || motion === "quiet" || scene === "room";
  const wide = () => innerWidth > 760;
  const yaw = () => -0.9 + 0.38 * Math.sin(time * 0.06) + look[0];
  const pitch = () => 0.16 + look[1];

  // ───── Импульсы идут только по рёбрам графа. ─────
  const pulses = [];
  const edgeAct = new Float32Array(edges.length);
  const strength = new Float32Array(edges.length);
  let spawnClock = 0, waveClock = 6;
  function pickNode(prefer) {
    // Чаще стартуем в областях с минутами из истории и в выбранной области.
    for (let tries = 0; tries < 12; tries++) {
      const i = Math.floor(rand() * nN);
      const g = nodeRegion[i];
      const chance = prefer === undefined ? 0.25 + weights[g] + (g === mode ? 0.6 : 0) : (g === prefer ? 1 : 0.02);
      if (rand() < chance && adjacency[i].length) return i;
    }
    return Math.floor(rand() * deepStart);
  }
  function fire(node, ttl, mark = false) {
    nodeAct[1 + node] = 1;
    for (const e of adjacency[node]) {
      if (pulses.length >= MAX_PULSES) return;
      if (rand() < 0.6 || adjacency[node].length === 1) pulses.push({ e, from: node, t: 0, ttl, mark });
    }
  }
  function wave(region, size, mark) {
    for (let k = 0; k < size; k++) fire(pickNode(region), 9, mark);
  }
  function step(dt) {
    const rate = (calm() ? 1.2 : 3.5) + Math.min(4, due * 0.35);
    spawnClock += dt * rate;
    while (spawnClock > 1) { spawnClock -= 1; fire(pickNode(), 4 + Math.floor(rand() * 4)); }
    waveClock -= dt;
    if (waveClock < 0 && !calm()) { waveClock = 8 + rand() * 6; wave(rand() < 0.6 ? mode : undefined, 4, false); }
    const fade = Math.exp(-dt * 2.2), fadeE = Math.exp(-dt * 1.4);
    for (let i = 1; i <= nN; i++) nodeAct[i] *= fade;
    for (let i = 0; i < edges.length; i++) edgeAct[i] *= fadeE;
    for (let i = pulses.length - 1; i >= 0; i--) {
      const p = pulses[i];
      const e = edges[p.e];
      p.t += dt * 1.6 / e.len;
      edgeAct[p.e] = Math.max(edgeAct[p.e], 0.7);
      if (p.t < 1) continue;
      const node = e.a === p.from ? e.b : e.a;
      nodeAct[1 + node] = 1;
      if (p.mark) strength[p.e] = Math.min(0.4, strength[p.e] + 0.15);
      pulses.splice(i, 1);
      if (p.ttl <= 0) continue;
      const next = adjacency[node].filter(x => x !== p.e);
      if (!next.length) continue;
      const branches = rand() < 0.18 ? 2 : rand() < 0.8 ? 1 : 0;
      for (let b = 0; b < branches && pulses.length < MAX_PULSES; b++) {
        pulses.push({ e: next[Math.floor(rand() * next.length)], from: node, t: 0, ttl: p.ttl - 1, mark: p.mark });
      }
    }
  }
  const tmp = [0, 0, 0];
  function upload() {
    gl.bindBuffer(gl.ARRAY_BUFFER, pointAct);
    gl.bufferSubData(gl.ARRAY_BUFFER, 0, nodeAct.subarray(0, 1 + nN));
    for (let i = 0; i < lineAct.length; i++) {
      const e = lineEdge[i];
      lineAct[i] = Math.min(1, edgeAct[e] + strength[e]);
    }
    gl.bindBuffer(gl.ARRAY_BUFFER, lineActBuf);
    gl.bufferSubData(gl.ARRAY_BUFFER, 0, lineAct);
    pulses.forEach((p, i) => {
      const e = edges[p.e];
      bezier(e, e.a === p.from ? p.t : 1 - p.t, tmp);
      pulseData.set([tmp[0], tmp[1], tmp[2], tmp[0], tmp[1], tmp[2], 0.5, nodeRegion[p.from], 4], i * STRIDE);
    });
    gl.bindBuffer(gl.ARRAY_BUFFER, pulseBuf);
    gl.bufferSubData(gl.ARRAY_BUFFER, 0, pulseData.subarray(0, pulses.length * STRIDE));
  }

  // Та же проекция, что в шейдере: нужна, чтобы найти нейрон под курсором.
  function project(p) {
    const k = cur.zoom * (wide() ? 1 : 0.8) * (2 - cur.intro);
    let x = p[0] * k, y = p[1] * k, z = p[2] * k;
    const cy = Math.cos(yaw()), sy = Math.sin(yaw());
    [x, z] = [cy * x + sy * z, -sy * x + cy * z];
    const cp = Math.cos(pitch()), sp = Math.sin(pitch());
    [y, z] = [cp * y - sp * z, sp * y + cp * z];
    const d = z + 9;
    const nx = x * 2.7 / d / (innerWidth / innerHeight) + (wide() ? cur.shift : 0);
    const ny = y * 2.7 / d + (wide() ? 0 : 0.18 * (1 - cur.morph));
    return [(nx + 1) / 2 * innerWidth, (1 - ny) / 2 * innerHeight];
  }
  let pointer = null, touchedAt = 0;
  function touch(now) {
    if (!pointer || calm() || cur.morph > 0.2 || now - touchedAt < 140) return;
    touchedAt = now;
    let best = -1, bestD = 36 * 36;
    for (let i = 0; i < nN; i += 2) {
      const [sx, sy] = project(nodePos[i]);
      const d = (sx - pointer[0]) ** 2 + (sy - pointer[1]) ** 2;
      if (d < bestD) { bestD = d; best = i; }
    }
    if (best >= 0 && nodeAct[1 + best] < 0.5) fire(best, 3);
  }

  function resize() {
    const dpr = Math.min(devicePixelRatio || 1, drawN < total ? 1 : 1.5);
    canvas.width = Math.round(innerWidth * dpr);
    canvas.height = Math.round(innerHeight * dpr);
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.uniform1f(u.uPx, dpr);
    gl.uniform1f(u.uAspect, innerWidth / innerHeight);
    wake();
  }

  function draw() {
    const bg = [0.024 + 0.937 * cur.light, 0.028 + 0.937 * cur.light, 0.043 + 0.926 * cur.light];
    gl.clearColor(bg[0], bg[1], bg[2], 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.uniform1f(u.uTime, time);
    gl.uniform1f(u.uMorph, cur.morph);
    gl.uniform1f(u.uLight, cur.light);
    gl.uniform1f(u.uIntro, cur.intro);
    gl.uniform1f(u.uQuiet, cur.quiet);
    gl.uniform1f(u.uDim, wide() ? cur.dim : Math.max(cur.dim, 0.35 * (1 - cur.morph)));
    gl.uniform1f(u.uZoom, cur.zoom * (wide() ? 1 : 0.8));
    gl.uniform1f(u.uMode, mode);
    gl.uniform1f(u.uHover, hover);
    gl.uniform1f(u.uPulse, Math.max(0, 1 - (performance.now() - pulseAt) / 1600));
    gl.uniform1f(u.uCore, cur.core);
    gl.uniform1f(u.uYaw, yaw());
    gl.uniform1f(u.uPitch, pitch());
    gl.uniform2f(u.uShift, wide() ? cur.shift : 0, wide() ? 0 : 0.18 * (1 - cur.morph));
    gl.uniform1fv(u.uW, weights);
    upload();
    gl.uniform1f(u.uLine, 1);
    if (cur.morph < 0.999) {
      bind(lineBuf, lineActBuf);
      gl.drawArrays(gl.LINES, 0, lineVerts.length);
    }
    gl.uniform1f(u.uLine, 0);
    bind(pointBuf, pointAct);
    gl.drawArrays(gl.POINTS, 0, drawN);
    if (pulses.length && cur.morph < 0.999) {
      bind(pulseBuf, null);
      gl.drawArrays(gl.POINTS, 0, pulses.length);
    }
  }

  function frame(now) {
    raf = 0;
    if (motion === "off" || document.hidden) return;
    if (still()) {
      Object.assign(cur, goal);
      draw();
      return;
    }
    // В тихом режиме и в комнате хватает 30 кадров в секунду.
    if (calm() && now - lastDraw < 31) return schedule();
    const dt = Math.min(0.05, (now - (last || now)) / 1000 || 0.016);
    last = now;
    lastDraw = now;
    // Адаптация: если кадр дольше ~22 мс, частиц и плотность пикселей становится меньше.
    const minimum = 1 + nN + 2000;
    if (!calm() && ++frames > 30) {
      if (dt > 1 / 45) slow += 1;
      if (frames >= 150) {
        if (slow > 60 && drawN === minimum) weak = true;
        else if (slow > 60) { drawN = Math.max(minimum, drawN - Math.ceil((drawN - minimum) / 2) - 1); resize(); }
        frames = 0;
        slow = 0;
      }
    }
    time += dt * (calm() ? 0.3 : 1);
    const e = ease(Math.min(1, (now - tweenAt) / tweenMs));
    for (const key in cur) cur[key] = from[key] + (goal[key] - from[key]) * e;
    const lk = 1 - Math.exp(-dt * 2);
    look[0] += (lookGoal[0] - look[0]) * lk;
    look[1] += (lookGoal[1] - look[1]) * lk;
    touch(now);
    step(dt * (calm() ? 0.5 : 1));
    draw();
    schedule();
  }

  function schedule() {
    if (!raf) raf = requestAnimationFrame(frame);
  }
  function wake() {
    last = 0;
    schedule();
  }
  function tween(ms) {
    from = { ...cur };
    tweenAt = performance.now();
    tweenMs = ms;
    wake();
  }

  api.setScene = (name) => {
    if (name === scene) return;
    scene = name;
    Object.assign(goal, scenes[name]);
    tween(name === "room" ? 1300 : 900);
  };
  api.setMode = (index) => {
    if (index !== mode && !still()) wave(index, 3, false);
    mode = index;
    wake();
  };
  api.hover = (index) => { hover = index; wake(); };
  api.setData = (minutes, count) => {
    minutes.forEach((value, i) => { weights[i] = Math.min(1, Math.sqrt(value / 90)); });
    due = count;
    goal.core = Math.min(1, count / 10);
    tween(900);
  };
  // Конец сессии: волна по всей сети.
  api.pulse = () => {
    pulseAt = performance.now();
    if (!still()) wave(undefined, 10, false);
    wake();
  };
  // recall — верный ответ: волна в выбранной области, пройденные связи остаются ярче до закрытия окна.
  // complete — решённая задача: короткая волна по всей сети.
  api.activate = (kind) => {
    if (still()) return;
    if (kind === "recall") wave(mode, 4, true);
    else wave(undefined, 8, false);
    wake();
  };
  api.skip = () => { tweenAt = -1e9; wake(); };
  api.setMotion = (value) => {
    motion = value;
    document.body.classList.toggle("no-field", value === "off");
    wake();
  };

  addEventListener("resize", resize);
  document.addEventListener("visibilitychange", wake);
  reduce.addEventListener?.("change", wake);
  addEventListener("pointermove", (event) => {
    pointer = [event.clientX, event.clientY];
    if (calm()) return;
    lookGoal[0] = (event.clientX / innerWidth - 0.5) * 0.28;
    lookGoal[1] = (event.clientY / innerHeight - 0.5) * 0.12;
  });
  document.addEventListener("pointerleave", () => { pointer = null; });
  canvas.addEventListener("webglcontextlost", (event) => {
    event.preventDefault();
    motion = "off";
    document.body.classList.add("no-field");
  });
  resize();
})();
