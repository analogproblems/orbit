/* Bit's orb looks, ported from the sketches drawn for Lita, LITAgent's
   companion: little beings of light that show how things are going through
   shape, colour and motion. Every look is a list of numbers (PARAMS). Each orb
   springs its numbers toward the target for its being and mood, so every
   change flows smoothly, even when a new one starts halfway through. Nothing
   here touches the page: Bit.tsx owns the canvases and the clock. */

type Vals = Float64Array;
type Lab = [number, number, number];
type V3 = [number, number, number];
type Pt = number[];
type Mat = [number, number, number, number, number, number];
type Ctx = CanvasRenderingContext2D;

const TAU = Math.PI * 2;
const clamp = (v: number, a: number, b: number) => Math.min(b, Math.max(a, v));
const smooth = (t: number) => {
  t = clamp(t, 0, 1);
  return t * t * (3 - 2 * t);
};
const hash = (n: number) => {
  const s = Math.sin(n * 12.9898 + 78.233) * 43758.5453;
  return s - Math.floor(s);
};

/* Colour lives in OKLab so blends between two colours stay clear instead of going muddy. */
const lin = (c: number) =>
  c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
const gam = (c: number) =>
  c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055;
function rgbLab(R: number, G: number, B: number): Lab {
  const [r, g, b] = [R, G, B].map((v) => lin(v / 255));
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
}
const toLab = (hex: string) =>
  rgbLab(...([1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)) as V3));
function labRgb(L: number, a: number, b: number): V3 {
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3,
    m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3,
    s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ].map((v) => Math.round(255 * clamp(gam(Math.max(0, v)), 0, 1))) as V3;
}
function rgba(L: number, a: number, b: number, alpha = 1) {
  const c = labRgb(L, a, b);
  return `rgba(${c[0]},${c[1]},${c[2]},${clamp(alpha, 0, 1).toFixed(3)})`;
}
const lch = ([L, a, b]: Lab): Lab => [L, Math.hypot(a, b), Math.atan2(b, a)];
const unlch = ([L, C, h]: Lab): Lab => [L, C * Math.cos(h), C * Math.sin(h)];

/* Each being has four colours: light (L), middle (M), deep (D) and its glow (G). A mood shines a
   coloured light on the being from above instead of repainting it, so each one keeps its own colours
   underneath: amber light when it needs you, lavender when it's thinking, dim blue dusk when paused,
   brighter when working, and no light at all (grey) when not connected. */
type Role = "L" | "M" | "D" | "G";
const ROLES: Role[] = ["L", "M", "D", "G"];
/** A being's four colours, as #rrggbb. */
export type Palette = Record<Role, string>;
/** How Lita's states map onto words: Bit uses four of the six. */
export type OrbMood =
  | "idle"
  | "working"
  | "awaiting_input"
  | "paused"
  | "disconnected"
  | "thinking";
export const ORB_MOODS: OrbMood[] = [
  "idle",
  "working",
  "awaiting_input",
  "paused",
  "disconnected",
  "thinking",
];
/** A being's own say in how a mood's light falls on it. */
interface OwnLight {
  L?: number;
  M?: number;
  D?: number;
  G?: number;
  pal?: Palette;
  dim?: number;
  sat?: number;
  turn?: number;
}
const LIGHT: Partial<Record<OrbMood, Palette & { k: Record<Role, number> }>> = {
  awaiting_input: {
    L: "#fff1c6",
    M: "#ffc04c",
    D: "#ee8a2a",
    G: "#ffc660",
    k: { L: 0.78, M: 0.32, D: 0.15, G: 1 },
  },
  thinking: {
    L: "#f2eaff",
    M: "#bca6ff",
    D: "#7c5de0",
    G: "#c5b1ff",
    k: { L: 0.72, M: 0.35, D: 0.12, G: 1 },
  },
};
const GREY: Record<Role, number> = { L: 0.91, M: 0.8, D: 0.64, G: 0.75 };
function moodColour(
  hex: string,
  role: Role,
  mood: OrbMood,
  own: OwnLight = {},
): Lab {
  let c = toLab(hex);
  const f = LIGHT[mood];
  if (f) {
    const t = toLab(f[role]),
      k = own[role] ?? f.k[role];
    c = c.map((v, i) => v + (t[i] - v) * k) as Lab;
  }
  if (mood === "working") {
    const [L, C, h] = lch(c);
    c = unlch([Math.min(0.97, L + 0.02), C * 1.18, h]);
  }
  if (mood === "paused" && own.pal) c = toLab(own.pal[role]);
  else if (mood === "paused") {
    // Dusk: dimmer and softer, and turned a little toward evening blue.
    const [L, C, h] = lch(c),
      dusk = (275 * Math.PI) / 180;
    let d = dusk - h;
    d = Math.atan2(Math.sin(d), Math.cos(d));
    c = unlch([
      L * (role === "L" ? 0.9 : 0.85) * (own.dim ?? 1),
      C * (own.sat ?? 0.72),
      h + clamp(d, -(own.turn ?? 0.38), own.turn ?? 0.38),
    ]);
  }
  if (mood === "disconnected") {
    const h = lch(c)[2];
    c = unlch([GREY[role], 0.012, h]);
  }
  return c;
}

/** A palette built around one colour, for a being in a colour you chose:
 * light, middle and deep shades of it, the deep one turned a little warmer,
 * and a glow. A grey stays grey. */
export function tint(r: number, g: number, b: number): Palette {
  const [, C, h] = lch(rgbLab(r, g, b));
  const chroma = C < 0.03 ? C : clamp(C * 1.4, 0.11, 0.17);
  const hex = (L: number, k: number, turn: number) =>
    "#" +
    labRgb(...unlch([L, chroma * k, h + (turn * Math.PI) / 180]))
      .map((v) => v.toString(16).padStart(2, "0"))
      .join("");
  return {
    L: hex(0.95, 0.3, 8),
    M: hex(0.78, 1, 0),
    D: hex(0.56, 1.05, -16),
    G: hex(0.83, 0.85, 0),
  };
}

/* Every number an orb can change, grouped by how its spring behaves: shapes overshoot a little like
   jelly, colours glide, faces are quick, and speeds wind up and down gently. The defaults for the
   round-six shapes (star, ring, friend, cloud lobes) sit where those beings keep them, so another
   being turning into one of them just grows the part in place. */
type Group = "shape" | "pos" | "motion" | "rate" | "colour" | "face" | "extra";
const PARAMS: Record<string, { i: number; g: Group; v: number }> = {};
let NP = 0;
const group = (g: Group, entries: Record<string, number>) => {
  for (const [k, v] of Object.entries(entries)) PARAMS[k] = { i: NP++, g, v };
};
const LOBES = [
  [-0.95, 0.14, 0.4],
  [-0.5, -0.4, 0.5],
  [0.06, -0.6, 0.6],
  [0.6, -0.36, 0.48],
  [0.97, 0.15, 0.38],
];
group("shape", {
  size: 1,
  sx: 1,
  sy: 1,
  h2: 0,
  h3: 0,
  h4: 0,
  h6: 0,
  o2: 0,
  o3: 0,
  o4: 0,
  o6: 0,
  petals: 0,
  tip: 0,
  tipDir: -Math.PI / 2,
  ripple: 0,
  star: 0,
  starRot: 0,
  bell: 0,
  hem: 0,
  hole: 0,
  holeY: -0.24,
  holeSy: 1,
  gap: 0,
  duo: 0,
  duoA: Math.PI - 0.42,
  duoD: 1.24,
  duoR: 0.46,
  duoNeck: 0.12,
  puff: 0,
  cut: 3,
  billow: 0,
  ...Object.fromEntries(
    LOBES.flatMap(([x, y, r], i) => [
      ["lx" + i, x],
      ["ly" + i, y],
      ["lr" + i, r],
    ]),
  ),
  // Round seven: how much of each regular polygon (3, 4, 5, 6 and 8 sides), its turn and corner rounding, gear
  // teeth, and the solid shapes (jewel, spinning top, box, paper) with the angles they're seen and folded at.
  tri: 0,
  sq: 0,
  pent: 0,
  hex: 0,
  oct: 0,
  polyRot: 0,
  polyK: 0.26,
  cog: 0,
  gem: 0,
  top: 0,
  cube: 0,
  fold: 0,
  geoRoll: 0,
  gemYaw: 0,
  gemPitch: 0.34,
  cubeYaw: -0.55,
  cubePitch: 0.42,
  lid: 0,
  flapT: 0,
  flapR: 0,
  flapB: 0,
  flapL: 0,
  tent: 0,
  lie: 0,
  foldYaw: 0,
});
group("pos", { lift: 1, tilt: 0, shiftX: 0 });
group("motion", {
  bobAmp: 0,
  breathAmp: 0,
  wobAmp: 0,
  swayAmp: 0,
  bounce: 0,
  swirl: 0,
  pulse: 0,
  duoSwing: 0,
  tendAmp: 0,
  prec: 0,
  foldWork: 0,
  wave: 0,
});
group("rate", {
  bobRate: 0,
  breathRate: 0,
  wobRate: 0,
  swayRate: 0,
  bounceRate: 0,
  swirlRate: 0,
  spin: 0,
  rippleRate: 0,
  sparkRate: 0,
  thinkRate: 0,
  qRate: 0,
  zRate: 0,
  tendRate: 0,
  fizzRate: 0,
  rollRate: 0,
  precRate: 0,
});
group("colour", {
  glow: 0.5,
  alpha: 1,
  dash: 0,
  sheen: 1,
  glass: 0,
  ...Object.fromEntries(ROLES.flatMap((r) => [0, 1, 2].map((j) => [r + j, 0]))),
});
group("face", {
  eyeOpen: 1,
  eyeCurve: 0,
  lookX: 0,
  lookY: 0,
  blinks: 1,
  cheeks: 0.5,
  eyeY: 0.02,
  eyeGap: 0.3,
});
group("extra", {
  q: 0,
  z: 0,
  sparks: 0,
  think: 0,
  moons: 0,
  mOrbit: 0,
  mRest: 0,
  mTrail: 0,
  mAsk: 0,
  sprout: 0,
  perk: 0,
  glitter: 0,
  ringLight: 0,
  trail: 0,
  steam: 0,
  tendrils: 0,
  beads: 0,
  raise: 0,
  curl: 0,
  fizz: 0,
  bub: 0,
  prism: 0,
  rainbow: 0,
  rainQ: 0,
  puddle: 0,
  glints: 0,
  gemLight: 0,
  creaseGlow: 0,
  boxGlow: 0,
  arcs: 0,
});
const I: Record<string, number> = Object.fromEntries(
  Object.entries(PARAMS).map(([k, p]) => [k, p.i]),
);
const SPRING: Record<Group, [number, number]> = {
  shape: [7, 0.5],
  pos: [4.2, 0.62],
  motion: [2.6, 1],
  rate: [2.2, 1],
  colour: [3.6, 1],
  face: [11, 0.8],
  extra: [4.5, 1],
};
const OMEGA = new Float64Array(NP),
  ZETA = new Float64Array(NP),
  FADES = new Uint8Array(NP);
for (const p of Object.values(PARAMS)) [OMEGA[p.i], ZETA[p.i]] = SPRING[p.g];
for (const k of [
  "q",
  "z",
  "sparks",
  "think",
  "glitter",
  "ringLight",
  "trail",
  "steam",
  "beads",
  "rainbow",
  "glints",
  "gemLight",
  "boxGlow",
  "arcs",
])
  FADES[I[k]] = 1;

type Layer = Record<string, number>;
/* How every being behaves when idle: floating, breathing slowly, blinking now and then. */
const BASE: Layer = {
  lift: 1,
  bobAmp: 0.05,
  bobRate: 0.16,
  breathAmp: 0.026,
  breathRate: 0.19,
  wobAmp: 0.012,
  wobRate: 0.06,
  swayAmp: 0.05,
  swayRate: 0.11,
  swirl: 0.4,
  swirlRate: 0.025,
  glow: 0.5,
  sheen: 1,
  eyeOpen: 1,
  blinks: 1,
  cheeks: 0.55,
  qRate: 0.3,
  zRate: 0.42,
  thinkRate: 0.55,
};

/* What each mood changes. Only working moves with purpose. Waiting asks once (a hop on arrival, see
   Orb.set) and then rocks gently. Not connected winds every motion down to nothing. */
const MOOD: Record<OrbMood, Layer> = {
  idle: {},
  working: {
    bobAmp: 0.035,
    bobRate: 0.45,
    breathAmp: 0.02,
    breathRate: 0.45,
    wobRate: 0.2,
    swirl: 0.85,
    swirlRate: 0.22,
    glow: 0.85,
    sparks: 1,
    sparkRate: 0.32,
    eyeOpen: 0.78,
    lookX: -0.15,
    lookY: 0.4,
    cheeks: 0.45,
    swayAmp: 0.1,
    swayRate: 0.5,
  },
  awaiting_input: {
    bobAmp: 0.03,
    tilt: 0.16,
    sx: 0.97,
    sy: 1.05,
    eyeOpen: 1.15,
    lookX: 0.15,
    lookY: -0.1,
    q: 1,
    glow: 0.65,
  },
  paused: {
    lift: 0,
    bobAmp: 0,
    breathAmp: 0.03,
    breathRate: 0.13,
    wobAmp: 0.006,
    wobRate: 0.04,
    swayAmp: 0.02,
    swayRate: 0.06,
    sx: 1.12,
    sy: 0.87,
    eyeOpen: 0,
    eyeCurve: -1,
    blinks: 0,
    z: 1,
    glow: 0.18,
    swirl: 0.18,
    swirlRate: 0.01,
    cheeks: 0.35,
    sheen: 0.6,
  },
  disconnected: {
    lift: 0,
    bobAmp: 0,
    bobRate: 0,
    breathAmp: 0,
    breathRate: 0,
    wobAmp: 0,
    wobRate: 0,
    swayAmp: 0,
    swayRate: 0,
    swirl: 0,
    swirlRate: 0,
    qRate: 0,
    zRate: 0,
    thinkRate: 0,
    sx: 1.07,
    sy: 0.9,
    eyeOpen: 0,
    eyeCurve: 0,
    blinks: 0,
    cheeks: 0,
    glow: 0,
    alpha: 0.62,
    dash: 1,
    sheen: 0.25,
    tendRate: 0,
    fizzRate: 0,
    tendAmp: 0,
    pulse: 0,
    duoSwing: 0,
    rollRate: 0,
    precRate: 0,
    prec: 0,
    foldWork: 0,
    wave: 0,
  },
  thinking: {
    bobAmp: 0.04,
    bobRate: 0.2,
    tilt: -0.07,
    eyeOpen: 0.95,
    lookX: 0.45,
    lookY: -0.5,
    think: 1,
    glow: 0.6,
    swirl: 0.55,
    swirlRate: 0.08,
    h6: 0.02,
  },
};
const MULT = new Set(["size", "sx", "sy"]);

interface Being {
  name: string;
  kind: string;
  pal: Palette;
  base: Layer;
  light?: Partial<Record<OrbMood, OwnLight>>;
  moods: Partial<Record<OrbMood, Layer>>;
  boop?: (o: Orb) => void;
}
/* The family. Each has its own shape and its own way of showing each mood. */
const BEINGS = {
  pip: {
    name: "Pip",
    kind: "the bubble",
    pal: { L: "#ffe8c8", M: "#ff9d5c", D: "#ee5b4d", G: "#ffae70" },
    base: { h2: 0.012, h3: 0.008, o3: 0.6, wobAmp: 0.016 },
    moods: {
      working: { bounce: 1, bounceRate: 1.05, bobAmp: 0 },
      awaiting_input: { sy: 1.08, sx: 0.95 },
      paused: { sx: 1.08, sy: 0.92 },
    },
  },
  tide: {
    name: "Tide",
    kind: "the drop",
    pal: { L: "#d4fbf3", M: "#45d1c6", D: "#2b7fe0", G: "#5fdfd5" },
    base: { tip: 0.42, sy: 1.0, h2: -0.02, eyeY: 0.12, swayAmp: 0.08 },
    light: { awaiting_input: { L: 0.5, M: 0.15, D: 0.1 } },
    moods: {
      working: {
        ripple: 0.013,
        rippleRate: 0.45,
        swayAmp: 0.22,
        swayRate: 0.55,
      },
      awaiting_input: { tip: 0.5 },
      paused: { tip: 0.1 },
      disconnected: { tip: 0.16, tipDir: -Math.PI / 2 - 0.35 },
      thinking: { tipDir: -Math.PI / 2 - 0.28 },
    },
  },
  bloom: {
    name: "Bloom",
    kind: "the flower",
    pal: { L: "#ffe3f1", M: "#ff88bf", D: "#b456df", G: "#ff9acb" },
    base: { petals: 0.2, size: 1.06, swayAmp: 0.05 },
    moods: {
      working: { spin: 0.07, swayAmp: 0 },
      awaiting_input: { petals: 0.26 },
      paused: { petals: 0.05 },
      disconnected: { petals: 0.1 },
      thinking: { petals: 0.17 },
    },
  },
  moss: {
    name: "Moss",
    kind: "the pebble",
    pal: { L: "#effbcc", M: "#a6dc60", D: "#3f9e66", G: "#b4e87c" },
    base: {
      h4: 0.065,
      o4: Math.PI / 4,
      sx: 1.07,
      sy: 0.95,
      sprout: 1,
      perk: 0.35,
      swayAmp: 0.1,
    },
    light: { thinking: { L: 0.5, M: 0.15, D: 0.08 } },
    moods: {
      working: { swayAmp: 0.32, swayRate: 0.7 },
      awaiting_input: { perk: 1 },
      paused: { perk: -0.55 },
      disconnected: { perk: -1 },
      thinking: { perk: 0.5 },
    },
  },
  glim: {
    name: "Glim",
    kind: "the little planet",
    pal: { L: "#e6e9ff", M: "#909cff", D: "#4c4cd2", G: "#a3adff" },
    base: { size: 0.84, moons: 1, h3: 0.01 },
    light: { awaiting_input: { L: 0.6, M: 0.18, D: 0.08 } },
    moods: {
      working: { sparks: 0, mOrbit: 1 },
      awaiting_input: { mAsk: 1 },
      paused: { mRest: 1 },
      disconnected: { mRest: 1 },
      thinking: { think: 0, mTrail: 1 },
    },
  },

  /* Round six: new shapes, each with its own trick for showing work. */
  nova: {
    name: "Nova",
    kind: "the star",
    pal: { L: "#fff7d1", M: "#ffd34d", D: "#f39a1e", G: "#ffe07a" },
    base: { star: 0.4, size: 0.9 },
    // Yellow can't dim toward blue without going khaki, so a sleeping Nova glows like a sunset instead.
    light: {
      paused: {
        pal: { L: "#ffe8d6", M: "#f4b48f", D: "#d9806e", G: "#f6c3a0" },
      },
      thinking: { L: 0.62, M: 0.16, D: 0.06 },
    },
    moods: {
      working: { sparks: 0, glitter: 1, spin: 0.11, swayAmp: 0 },
      awaiting_input: { star: 0.56 },
      paused: { star: 0.12, starRot: Math.PI / 4 },
      disconnected: { star: 0.16, starRot: Math.PI / 4 },
      thinking: { starRot: 0.2 },
    },
    // A tap sets it twirling: it jumps back a quarter turn (which looks the same) and springs round to rest.
    boop: (o) => {
      o.cur[I.starRot] -= Math.PI / 2;
    },
  },
  duo: {
    name: "Duo",
    kind: "the two friends",
    pal: { L: "#e0fff3", M: "#5fdfb4", D: "#14a088", G: "#7ff0c9" },
    // The friend's place is an angle round the big one, from the left (pi) over the top (1.5 pi) to the
    // right, so every change of place takes the short way round and never swings underneath.
    base: { duo: 1, size: 0.9, shiftX: 0.32 },
    moods: {
      working: {
        sparks: 0,
        duoA: 1.5 * Math.PI,
        duoD: 1.3,
        duoSwing: 1.15,
        duoNeck: 0.1,
        trail: 1,
        bounceRate: 0.5,
      },
      awaiting_input: {
        duoA: TAU - 2.25,
        duoD: 1.62,
        duoNeck: 0.13,
        duoR: 0.42,
      },
      paused: { duoA: Math.PI - 0.6, duoD: 1.02, duoNeck: 0.2 },
      disconnected: { duoA: Math.PI - 0.25, duoD: 1.72, duoNeck: 0 },
      thinking: { duoA: TAU - 2.45, duoD: 1.46, duoNeck: 0.06 },
    },
    boop: (o) => {
      o.vel[I.duoD] += 2.4;
    },
  },
  puff: {
    name: "Puff",
    kind: "the cloud",
    pal: { L: "#ffffff", M: "#d9eeff", D: "#78acec", G: "#c2e1ff" },
    base: {
      puff: 1,
      cut: 0.55,
      sx: 1.08,
      sy: 0.74,
      eyeY: 0.12,
      billow: 0.035,
      swirl: 0.25,
    },
    moods: {
      working: { sparks: 0, steam: 1, billow: 0.06, swirl: 0.4 },
      awaiting_input: { ly2: -0.95, lr2: 0.6, ly1: -0.5, ly3: -0.46 },
      paused: {
        lx0: -1.2,
        ly0: 0.2,
        lx1: -0.62,
        ly1: -0.2,
        lr1: 0.44,
        ly2: -0.32,
        lr2: 0.5,
        lx3: 0.66,
        ly3: -0.18,
        lr3: 0.42,
        lx4: 1.2,
        ly4: 0.22,
      },
      disconnected: { ly1: -0.26, ly2: -0.38, ly3: -0.24, lr2: 0.52 },
    },
    boop: (o) => {
      for (let i = 0; i < LOBES.length; i++) o.vel[I["lr" + i]] += 1.4;
    },
  },
  jelly: {
    name: "Jelly",
    kind: "the jellyfish",
    pal: { L: "#f2e6ff", M: "#b48cff", D: "#6a44d6", G: "#86ecff" },
    base: {
      bell: 1,
      hem: 0.04,
      tendrils: 1,
      size: 0.88,
      lift: 1.5,
      tendAmp: 0.2,
      tendRate: 0.28,
      glass: 0.45,
      eyeY: -0.04,
    },
    moods: {
      working: {
        sparks: 0,
        pulse: 1,
        bounceRate: 0.7,
        beads: 1,
        tendRate: 0.9,
        tendAmp: 0.3,
        bobAmp: 0.02,
      },
      awaiting_input: { raise: 1 },
      paused: { curl: 0.6, tendAmp: 0.08, tendRate: 0.08, tendrils: 1.3 },
      thinking: { curl: 1 },
    },
    boop: (o) => {
      o.vel[I.lift] += 1.6;
    },
  },
  fizz: {
    name: "Fizz",
    kind: "the soda bubble",
    pal: { L: "#ffe3ee", M: "#ff7aa8", D: "#d23c78", G: "#ff9bbd" },
    base: { glass: 1, fizz: 0.8, bub: 1, fizzRate: 0.16 },
    moods: {
      working: { sparks: 0, fizz: 1, fizzRate: 0.5 },
      awaiting_input: { q: 0 },
      paused: { z: 0, fizz: 0.5, fizzRate: 0.06 },
      disconnected: { fizz: 0 },
      thinking: { think: 0 },
    },
    boop: (o) => {
      o.pop = 1;
    },
  },

  /* Round seven: geometric shapes, each with its own signs. */
  poly: {
    name: "Poly",
    kind: "the shape-shifter",
    pal: { L: "#e2eaff", M: "#5f8bff", D: "#2c4fd6", G: "#8eb0ff" },
    base: { hex: 1, size: 0.96, eyeY: 0.04 },
    light: {
      awaiting_input: { L: 0.6, M: 0.2, D: 0.08 },
      thinking: { L: 0.55, M: 0.15, D: 0.06 },
    },
    moods: {
      working: { sparks: 0, hex: 0, cog: 0.12, spin: 0.13, size: 0.92 },
      awaiting_input: { hex: 0, sq: 1, polyRot: Math.PI / 4, size: 0.9 },
      paused: { hex: 0, sq: 1, size: 0.96 },
      disconnected: { hex: 0, oct: 1 },
      thinking: { hex: 0, pent: 1, polyRot: 0.1 },
    },
    // A tap turns it one side (or one tooth) round: it jumps back a step and springs forward.
    boop: (o) => {
      o.cur[I.polyRot] -= polyStep(o.tgt);
    },
  },
  prism: {
    name: "Prism",
    kind: "the glass prism",
    pal: { L: "#ffffff", M: "#ece8ff", D: "#9e90f2", G: "#d8cfff" },
    base: {
      tri: 1,
      size: 0.82,
      glass: 0.75,
      prism: 1,
      eyeY: 0.22,
      eyeGap: 0.27,
      swayAmp: 0.03,
    },
    moods: {
      working: { sparks: 0, rainbow: 1, sparkRate: 0.45 },
      awaiting_input: { rainQ: 1 },
      paused: { puddle: 1 },
      disconnected: { prism: 0, glass: 0.4 },
      thinking: { glints: 1 },
    },
    boop: (o) => {
      o.pop = 1;
    },
  },
  gem: {
    name: "Gem",
    kind: "the jewel",
    pal: { L: "#d0ffe8", M: "#1fc78a", D: "#066b4b", G: "#6cf0ba" },
    base: { gem: 1, eyeY: 0.2 },
    moods: {
      working: { sparks: 0, spin: 0.11, gemLight: 1, sparkRate: 0.6 },
      awaiting_input: { gemPitch: 0.62 },
      paused: { geoRoll: -0.9 },
      disconnected: { geoRoll: -0.9 },
      thinking: { gemLight: 0.7, sparkRate: 0.16, gemYaw: 0.3 },
    },
    // A tap sets it turning: a quarter turn back (two facets, so it looks the same) and it springs round.
    boop: (o) => {
      o.cur[I.gemYaw] -= TAU / 4;
    },
  },
  twirl: {
    name: "Twirl",
    kind: "the spinning top",
    pal: { L: "#fff3e6", M: "#ff5b5f", D: "#c22c47", G: "#ff9a88" },
    base: { top: 1, eyeY: -0.05 },
    moods: {
      working: { sparks: 0, spin: 1.3, prec: 0.035, precRate: 0.8, arcs: 1 },
      awaiting_input: { tilt: 0.3 },
      paused: { geoRoll: 0.74 },
      disconnected: { geoRoll: 0.74 },
      thinking: { prec: 0.14, precRate: 0.2 },
    },
    boop: (o) => {
      o.vel[I.spin] += 6;
    },
  },
  cubby: {
    name: "Cubby",
    kind: "the box",
    pal: { L: "#fff1d8", M: "#e3ad6b", D: "#ad773d", G: "#f6cf94" },
    base: { cube: 1, eyeY: 0 },
    moods: {
      working: { sparks: 0, rollRate: 0.6 },
      awaiting_input: { lid: 1, boxGlow: 1 },
      paused: { cubeYaw: -0.32 },
      disconnected: { lid: 0.2, cubeYaw: -0.2 },
      thinking: { lid: 0.08, boxGlow: 0.55, cubeYaw: -0.15 },
    },
    // A tap makes it peek: the flaps pop up and drop shut.
    boop: (o) => {
      o.vel[I.lid] += 5;
    },
  },
  fold: {
    name: "Fold",
    kind: "the folded paper",
    pal: { L: "#fbfdff", M: "#7fb6ff", D: "#3c7be0", G: "#a9cfff" },
    base: { fold: 1, flapR: 0.16, eyeY: 0 },
    light: { awaiting_input: { L: 0.45, M: 0.14, D: 0.06 } },
    moods: {
      working: { sparks: 0, foldWork: 1, sparkRate: 0.42, flapR: 0 },
      awaiting_input: { flapT: 0.6, wave: 0.06, flapR: 0 },
      paused: { tent: 0.8, foldYaw: 0.42, flapR: 0 },
      disconnected: { lie: 1.22, flapR: 0 },
      thinking: { creaseGlow: 1 },
    },
    // A tap flips it right round, so you see its white back go by.
    boop: (o) => {
      o.cur[I.foldYaw] -= TAU;
    },
  },
} satisfies Record<string, Being>;

/** One of the orb looks. */
export type Look = keyof typeof BEINGS;
/** Every look, in the order the sketches introduced them. */
export const LOOKS = Object.keys(BEINGS) as Look[];
/** A look's name and what it is, such as Glim, the little planet. */
export const lookNames = Object.fromEntries(
  LOOKS.map((k) => [k, { name: BEINGS[k].name, kind: BEINGS[k].kind }]),
) as Record<Look, { name: string; kind: string }>;
/** A look's own colours. */
export const ownPalette = (look: Look): Palette => BEINGS[look].pal;

const TARGETS = new Map<string, Vals>();
/* The numbers a being heads for in a mood. With its own palette it also takes its own light (Nova's sunset
   when it sleeps); in a colour you chose, only the strength of each light. */
function target(bk: Look, mood: OrbMood, pal: Palette): Vals {
  const own = pal === BEINGS[bk].pal,
    key =
      bk + ":" + mood + ":" + (own ? "own" : ROLES.map((r) => pal[r]).join(""));
  const hit = TARGETS.get(key);
  if (hit) return hit;
  const B: Being = BEINGS[bk],
    v: Record<string, number> = {};
  for (const k in PARAMS) v[k] = PARAMS[k].v;
  Object.assign(v, BASE, B.base);
  for (const layer of [MOOD[mood], B.moods[mood] || {}])
    for (const [k, x] of Object.entries(layer))
      v[k] = MULT.has(k) ? v[k] * x : x;
  const light = { ...B.light?.[mood] };
  if (!own) delete light.pal;
  for (const r of ROLES)
    lch(moodColour(pal[r], r, mood, light)).forEach((c, j) => (v[r + j] = c));
  const out = new Float64Array(NP);
  for (const k in PARAMS) out[PARAMS[k].i] = v[k];
  TARGETS.set(key, out);
  return out;
}

/* Fizz's little bubbles. Each mood has a formation (none, a swirl, a question mark, a z, fallen on the
   floor, a trail of thoughts); every bubble belongs a little to each formation, and those shares spring
   on each bubble's own clock, so the bubbles fly to a new shape one after another like a flock. */
const NB = 12,
  NF = 6;
const FORM_OF: Record<OrbMood, number> = {
  idle: 0,
  working: 1,
  awaiting_input: 2,
  paused: 3,
  disconnected: 4,
  thinking: 5,
};
const ASK = [
  [-0.262, -0.785],
  [-0.147, -0.946],
  [0.047, -0.986],
  [0.216, -0.882],
  [0.269, -0.692],
  [0.174, -0.513],
  [0.044, -0.368],
  [0, -0.2],
  [0, 0.07],
];
const ZED = [
  [-0.3, -0.27],
  [-0.1, -0.27],
  [0.1, -0.27],
  [0.3, -0.27],
  [0.15, -0.135],
  [0, 0],
  [-0.15, 0.135],
  [-0.3, 0.27],
  [-0.1, 0.27],
  [0.1, 0.27],
  [0.3, 0.27],
];
const SWIRL_R = [0.11, 0.07, 0.09, 0.065, 0.1, 0.075, 0.085, 0.06, 0.095];
const FALLEN = [
  [1.16, 0.1],
  [1.4, 0.075],
  [1.62, 0.09],
  [1.85, 0.065],
  [1.29, 0.06],
];
const THOUGHT = [
  [0.8, -0.05, 0.065],
  [1.1, -0.33, 0.09],
  [1.45, -0.66, 0.125],
];
const BUB_W = Array.from({ length: NB }, (_, i) => 6.2 - 0.26 * i);

type Phase = Record<
  | "bob"
  | "breath"
  | "wob"
  | "sway"
  | "bounce"
  | "swirl"
  | "spin"
  | "ripple"
  | "spark"
  | "think"
  | "q"
  | "z"
  | "tend"
  | "fizz"
  | "roll"
  | "prec",
  number
>;

/** One living orb: its current numbers, their speeds, and the clocks that drive its motion. */
export class Orb {
  cur = new Float64Array(NP);
  vel = new Float64Array(NP);
  tgt = new Float64Array(NP);
  ph: Phase;
  blinkIn: number;
  blinkT = -1;
  happy = 0;
  blink2 = -1;
  pop = 0;
  bw = new Float64Array(NB * NF);
  bv = new Float64Array(NB * NF);
  bk: Look = "glim";
  mood: OrbMood = "idle";
  pal: Palette = BEINGS.glim.pal;
  constructor(bk: Look, mood: OrbMood, pal: Palette = BEINGS[bk].pal) {
    const r = Math.random;
    this.ph = {
      bob: r(),
      breath: r(),
      wob: r(),
      sway: r(),
      bounce: 0,
      swirl: r(),
      spin: 0,
      ripple: 0,
      spark: r(),
      think: 0,
      q: 0,
      z: 0,
      tend: 0,
      fizz: 0,
      roll: 0,
      prec: 0,
    };
    this.blinkIn = 1 + r() * 3;
    this.set(bk, mood, pal, true);
  }
  set(bk: Look, mood: OrbMood, pal: Palette = BEINGS[bk].pal, instant = false) {
    const arriving = mood !== this.mood;
    this.bk = bk;
    this.mood = mood;
    this.pal = pal;
    this.tgt.set(target(bk, mood, pal));
    // Hues take the short way round the colour wheel from wherever they are now.
    for (const r of ROLES) {
      const i = I[r + "2"],
        d = this.tgt[i] - this.cur[i];
      this.tgt[i] = this.cur[i] + Math.atan2(Math.sin(d), Math.cos(d));
    }
    if (instant) {
      this.cur.set(this.tgt);
      this.vel.fill(0);
      this.snapBubbles();
      return;
    }
    // Turning shapes head for the nearest angle that looks right, rather than winding all the way back.
    for (const k of [I.polyRot, I.gemYaw]) {
      const q = turnStep(this.tgt, k) || turnStep(this.cur, k);
      if (q) {
        const d = this.tgt[k] - this.cur[k];
        this.tgt[k] = this.cur[k] + d - q * Math.round(d / q);
      }
    }
    if (arriving && mood === "awaiting_input") {
      this.vel[I.lift] += 2.4;
      this.vel[I.tilt] += 1.4;
    }
    if (arriving && mood === "paused") this.ph.z = 0;
  }
  snapBubbles() {
    this.bw.fill(0);
    this.bv.fill(0);
    for (let i = 0; i < NB; i++) this.bw[i * NF + FORM_OF[this.mood]] = 1;
  }
  /** A tap: a squish and a happy squint. Nothing happens while not connected, and a paused orb only stirs. */
  boop() {
    if (this.mood === "disconnected") return;
    this.vel[I.sx] += 2.2;
    this.vel[I.sy] -= 2.2;
    if (this.mood !== "paused") {
      this.happy = 1;
      const own = (BEINGS[this.bk] as Being).boop;
      if (own) own(this);
    }
  }
  step(dt: number) {
    const c = this.cur,
      v = this.vel,
      t = this.tgt;
    for (let n = Math.ceil(dt / (1 / 120)), h = dt / n; n > 0; n--) {
      for (let i = 0; i < NP; i++) {
        const w = OMEGA[i] * (FADES[i] && t[i] < c[i] ? 2.4 : 1);
        v[i] += (w * w * (t[i] - c[i]) - 2 * ZETA[i] * w * v[i]) * h;
        c[i] += v[i] * h;
      }
    }
    const ph = this.ph;
    ph.bob += c[I.bobRate] * dt;
    ph.breath += c[I.breathRate] * dt;
    ph.wob += c[I.wobRate] * dt;
    ph.sway += c[I.swayRate] * dt;
    ph.bounce += c[I.bounceRate] * dt;
    ph.swirl += c[I.swirlRate] * dt;
    ph.spin += c[I.spin] * dt;
    ph.ripple += c[I.rippleRate] * dt;
    ph.spark += c[I.sparkRate] * dt;
    ph.think += c[I.thinkRate] * dt;
    ph.q += c[I.qRate] * dt;
    ph.z += c[I.zRate] * dt;
    ph.tend += c[I.tendRate] * dt;
    ph.fizz += c[I.fizzRate] * dt;
    ph.prec += c[I.precRate] * dt;
    // Cubby tumbles a quarter turn at a time; when it stops working it finishes the turn it has started.
    if (t[I.rollRate] > 0) ph.roll += c[I.rollRate] * dt;
    else if (ph.roll !== Math.floor(ph.roll)) {
      const f = ph.roll - Math.floor(ph.roll);
      ph.roll =
        f < 0.45
          ? Math.floor(ph.roll)
          : Math.min(Math.ceil(ph.roll), ph.roll + 0.9 * dt);
    }
    if (c[I.blinks] > 0.5 && this.blinkT < 0 && (this.blinkIn -= dt) <= 0) {
      this.blinkT = 0;
      this.blinkIn =
        2.6 + Math.random() * 3.4 + (this.mood === "working" ? 2 : 0);
      this.blink2 = 0;
    }
    if (this.blinkT >= 0 && (this.blinkT += dt) > 0.18) this.blinkT = -1;
    // Duo's little friend copies each blink a moment later.
    if (this.blink2 >= 0 && (this.blink2 += dt) > 0.27) this.blink2 = -1;
    this.happy = Math.max(0, this.happy - dt / 1.1);
    this.pop = Math.max(0, this.pop - dt / 0.9);
    // A star that stops spinning settles on the nearest quarter turn instead of stopping crooked.
    if (t[I.spin] === 0 && c[I.star] > 0.01 && ph.spin !== 0) {
      const q = Math.PI / 2,
        d = c[I.starRot] + TAU * ph.spin - t[I.starRot];
      c[I.starRot] = t[I.starRot] + ((((d % q) + q * 1.5) % q) - q / 2);
      ph.spin = 0;
    }
    // When Poly or Gem stops turning (Twirl once it has nearly wound down), the turn so far becomes part of its
    // angle and it settles on the nearest side, facet or stripe, so nothing jumps.
    if (t[I.spin] === 0 && ph.spin !== 0) {
      const k =
        polyW(t) > 0.01
          ? I.polyRot
          : t[I.gem] > 0.01 || (t[I.top] > 0.01 && Math.abs(c[I.spin]) < 0.05)
            ? I.gemYaw
            : -1;
      if (k >= 0) {
        const q = turnStep(t, k),
          base = target(this.bk, this.mood, this.pal)[k];
        c[k] += TAU * ph.spin;
        ph.spin = 0;
        t[k] = base + q * Math.round((c[k] - base) / q);
      }
    }
    if (c[I.bub] > 0.001 || t[I.bub] > 0) this.stepBubbles(dt);
  }
  stepBubbles(dt: number) {
    const f = FORM_OF[this.mood],
      w = this.bw,
      u = this.bv;
    for (let n = Math.ceil(dt / (1 / 120)), h = dt / n; n > 0; n--) {
      for (let i = 0; i < NB; i++) {
        const om = BUB_W[i],
          z = 0.72;
        for (let k = i * NF, e = k + NF, j = 0; k < e; k++, j++) {
          u[k] +=
            (om * om * ((j === f ? 1 : 0) - w[k]) - 2 * z * om * u[k]) * h;
          w[k] += u[k] * h;
        }
      }
    }
  }
  /** The one still picture each mood is judged by (reduced motion, and pictures that hold still). */
  pose() {
    this.cur.set(this.tgt);
    this.vel.fill(0);
    this.blinkT = -1;
    this.happy = 0;
    this.blink2 = -1;
    this.pop = 0;
    Object.assign(this.ph, {
      bob: 0,
      breath: 0,
      wob: 0.15,
      sway: 0,
      bounce: 0.5,
      swirl: 0.2,
      spin: 0,
      ripple: 0.2,
      spark: 0.04,
      think: 0.3,
      q: 0,
      z: 0.3,
      tend: 0.2,
      fizz: 0.35,
      roll: this.tgt[I.rollRate] > 0 ? 0.62 : 0,
      prec: 0.25,
    });
    this.snapBubbles();
  }
}

/** What a picture is drawn over: the shadow's colour as "r,g,b", the z's and the dashed outline as rgba(),
 * and whether to light the floor and draw faces. */
export interface Env {
  dark: boolean;
  faces: boolean;
  pool: boolean;
  shadow: string;
  z: string;
  dash: string;
}
/** Where the top of an orb's head was drawn, how far it leaned and how big its body was, so a hat can sit there. */
export interface Head {
  x: number;
  y: number;
  angle: number;
  size: number;
}
/** A shape traced from a distance field: Duo's friend, Puff's lobes, a ring's hole. */
/** One of the solid shapes, drawn by its own draw, with the face painted on through face. */
interface Solid {
  pts: Pt[];
  face: Mat | null;
  fs: number;
  faceA: number;
  shut: number;
  hop: number;
  draw: (ctx: Ctx, alpha: number, env: Env) => void;
}
interface Field {
  friend: number[];
  lobes: number[][];
  top: number;
  hole: number[] | null;
}
/* The outline, as a distance from the centre for each direction (1 = a plain circle). */
function radius(c: Vals, ph: Phase, th: number) {
  const turn = TAU * ph.spin + 0.6 * c[I.swayAmp] * Math.sin(TAU * ph.sway),
    a = th - turn;
  let r =
    1 +
    c[I.h2] * Math.cos(2 * (a - c[I.o2])) +
    c[I.h3] * Math.cos(3 * (a - c[I.o3])) +
    c[I.h4] * Math.cos(4 * (a - c[I.o4])) +
    c[I.h6] * Math.cos(6 * (a - c[I.o6]));
  // Five petals pointing up: round lobes with narrow dips between them (0.737 keeps the average size).
  if (c[I.petals])
    r +=
      c[I.petals] *
      (Math.abs(Math.cos(2.5 * (a + Math.PI / 2))) ** 0.55 - 0.737);
  // Four soft star points (0.34 keeps the average size).
  if (c[I.star])
    r += c[I.star] * (Math.abs(Math.cos(2 * (a - c[I.starRot]))) ** 5 - 0.34);
  // Round seven: regular polygons, which blend into each other and into the circle, and gear teeth.
  for (const [i, n, ai] of POLY)
    if (c[i]) r += c[i] * (polyR(n, ai, a - c[I.polyRot], c[I.polyK]) - 1);
  if (c[I.cog])
    r +=
      (c[I.cog] * Math.tanh(3.2 * Math.sin(10 * (a - c[I.polyRot])))) /
      Math.tanh(3.2);
  const d = Math.cos(th - c[I.tipDir] - c[I.swayAmp] * Math.sin(TAU * ph.sway));
  if (d > 0) r += c[I.tip] * d ** 9;
  r += c[I.ripple] * Math.sin(9 * th - TAU * ph.ripple);
  r +=
    c[I.wobAmp] *
    (0.6 * Math.sin(2 * th + TAU * ph.wob + 1.3) +
      0.4 * Math.sin(3 * th - TAU * 1.4 * ph.wob + 0.4));
  // A jellyfish bell: the lower half is cut flat, softly, with a scalloped hem.
  if (c[I.bell] > 0.001) {
    const s = Math.sin(th);
    if (s > 0.02) {
      const b = 1 - 0.55 * c[I.bell],
        k = 0.3 * c[I.bell],
        f = b / s,
        h = Math.max(k - Math.abs(r - f), 0) / k;
      r = Math.min(r, f) - h * h * k * 0.25;
      r +=
        c[I.hem] *
        smooth((s - 0.55) / 0.3) *
        (0.5 + 0.5 * Math.cos(11 * (th - Math.PI / 2)));
    }
  }
  return r;
}

function addLoop(ctx: Ctx, pts: Pt[]) {
  const n = pts.length;
  ctx.moveTo((pts[n - 1][0] + pts[0][0]) / 2, (pts[n - 1][1] + pts[0][1]) / 2);
  for (let i = 0; i < n; i++) {
    const p = pts[i],
      q = pts[(i + 1) % n];
    ctx.quadraticCurveTo(p[0], p[1], (p[0] + q[0]) / 2, (p[1] + q[1]) / 2);
  }
  ctx.closePath();
}
function smoothPath(ctx: Ctx, pts: Pt[]) {
  ctx.beginPath();
  addLoop(ctx, pts);
}

/* Shapes that aren't one simple outline (a ring with a hole or a break, two friends joined by goo, a
   cloud of lobes) are measured instead: how far each point on a fine grid is from the edge, with the
   edge traced where that distance crosses zero. Soft unions (smin) make the goo between parts. */
const smin = (a: number, b: number, k: number) => {
  const h = Math.max(k - Math.abs(a - b), 0) / k;
  return Math.min(a, b) - h * h * k * 0.25;
};
const smax = (a: number, b: number, k: number) => -smin(-a, -b, k);
function segDist(x: number, y: number, bx: number, by: number) {
  const t = clamp((x * bx + y * by) / (bx * bx + by * by || 1), 0, 1);
  return Math.hypot(x - bx * t, y - by * t);
}

function shapeField(c: Vals, ph: Phase, Rs: number, sx: number, sy: number) {
  const k = Math.min(sx, sy),
    sc = Math.sqrt(sx * sy);
  const hole = c[I.hole] * Rs,
    hy = c[I.holeY] * Rs * sy,
    hsy = Math.max(0.08, c[I.holeSy]),
    gap = c[I.gap];
  const hasHole = hole > 0.3,
    hasGap = hasHole && gap > 0.004,
    kh = 0.08 * Rs,
    kg = 0.32 * Rs * smooth(gap / 0.12);
  let fx = 0,
    fy = 0,
    fr = 0,
    nr = 0;
  const duo = c[I.duo];
  if (duo > 0.004) {
    const a = c[I.duoA] + c[I.duoSwing] * Math.sin(TAU * (ph.bounce + 0.1)),
      d = c[I.duoD] * Rs;
    fx = Math.cos(a) * d * sx;
    fy = Math.sin(a) * d * sy;
    fr = Math.max(0, c[I.duoR]) * Rs * duo * sc;
    nr = Math.max(0, c[I.duoNeck]) * Rs * duo * sc;
  }
  const kd = 0.26 * Rs,
    lobes: number[][] = [],
    puff = c[I.puff];
  let top = 0;
  if (puff > 0.004)
    for (let i = 0; i < LOBES.length; i++) {
      const b = 1 + c[I.billow] * Math.sin(TAU * (ph.breath + i * 0.21)),
        lr = Math.max(0, c[I["lr" + i]]) * Rs * puff * b * sc;
      const lx = c[I["lx" + i]] * Rs * sx,
        ly = c[I["ly" + i]] * Rs * sy;
      lobes.push([lx, ly, lr]);
      top = Math.max(top, lr - ly);
    }
  const cutY = c[I.cut] * Rs * sy,
    kp = 0.13 * Rs,
    kc = 0.16 * Rs;
  // The outline's distance from the centre, measured once in 256 directions and looked up after that.
  const NT = 256,
    rt = new Float64Array(NT + 1);
  for (let i = 0; i <= NT; i++)
    rt[i] = radius(c, ph, -Math.PI + (TAU * i) / NT) * Rs;
  const f = (x: number, y: number) => {
    const qx = x / sx,
      qy = y / sy,
      u = ((Math.atan2(qy, qx) + Math.PI) / TAU) * NT,
      i = Math.min(NT - 1, u | 0);
    let d = (Math.hypot(qx, qy) - (rt[i] + (rt[i + 1] - rt[i]) * (u - i))) * k;
    if (lobes.length) {
      for (const [lx, ly, lr] of lobes)
        if (Math.max(Math.abs(x - lx), Math.abs(y - ly)) - lr < d + kp)
          d = smin(d, Math.hypot(x - lx, y - ly) - lr, kp);
      d = smax(d, y - cutY, kc);
    }
    if (fr > 0.2) {
      if (Math.max(Math.abs(x - fx), Math.abs(y - fy)) - fr < d + kd)
        d = smin(d, Math.hypot(x - fx, y - fy) - fr, kd);
      if (nr > 0.2) d = smin(d, segDist(x, y, fx, fy) - nr, kd);
    }
    if (hasHole)
      d = smax(
        d,
        -(Math.hypot(x / hole, (y - hy) / (hole * hsy)) - 1) *
          hole *
          Math.min(1, hsy),
        kh,
      );
    if (hasGap) {
      // A break in the ring at the top, with rounded ends.
      const uy = y - hy,
        rho = Math.hypot(x, uy);
      let del = Math.abs(Math.atan2(uy, x) + Math.PI / 2);
      if (del > Math.PI) del = TAU - del;
      const dw =
        del < gap
          ? -rho * Math.sin(Math.min(gap - del, Math.PI / 2))
          : del - gap < Math.PI / 2
            ? rho * Math.sin(del - gap)
            : rho;
      d = kg > 0.01 ? smax(d, -dw, kg) : Math.max(d, -dw);
    }
    return d;
  };
  const e = Rs * 1.25 * Math.max(sx, sy),
    box = [-e, -e, e, e];
  const grow = (x: number, y: number, r: number) => {
    box[0] = Math.min(box[0], x - r);
    box[1] = Math.min(box[1], y - r);
    box[2] = Math.max(box[2], x + r);
    box[3] = Math.max(box[3], y + r);
  };
  if (fr > 0.2) grow(fx, fy, fr + kd);
  for (const [lx, ly, lr] of lobes) grow(lx, ly, lr + kp);
  if (lobes.length) box[3] = Math.min(box[3], cutY + kc);
  return {
    f,
    box,
    friend: [fx, fy, fr],
    lobes,
    top,
    hole: hasHole ? [hy, hole, hsy] : null,
  };
}

/* Traces the edge (where f crosses zero) through a grid with marching squares, joining the pieces
   into closed loops. Inside is f < 0. */
const TB = {
  n: 0,
  v: new Float64Array(0),
  px: new Float64Array(0),
  py: new Float64Array(0),
  link: new Int32Array(0),
  seen: new Uint8Array(0),
};
function traceBuffers(nV: number) {
  if (TB.n < nV)
    Object.assign(TB, {
      n: nV,
      v: new Float64Array(nV),
      px: new Float64Array(2 * nV),
      py: new Float64Array(2 * nV),
      link: new Int32Array(4 * nV),
      seen: new Uint8Array(2 * nV),
    });
  return TB;
}
function trace(
  f: (x: number, y: number) => number,
  x0: number,
  y0: number,
  x1: number,
  y1: number,
  st: number,
) {
  const nx = Math.max(2, Math.ceil((x1 - x0) / st)),
    ny = Math.max(2, Math.ceil((y1 - y0) / st)),
    W = nx + 1;
  const { v, px, py, link, seen } = traceBuffers(W * (ny + 1));
  for (let j = 0, k = 0; j <= ny; j++)
    for (let i = 0; i <= nx; i++, k++) v[k] = f(x0 + i * st, y0 + j * st);
  // Edge ids: the edge from grid point k to its right neighbour is 2k; to the one below it is 2k + 1.
  const nE = 2 * W * (ny + 1);
  link.fill(-1, 0, 2 * nE);
  seen.fill(0, 0, nE);
  const at = (e: number) => {
    const k = e >> 1,
      i = k % W,
      j = (k - i) / W,
      q = e & 1 ? k + W : k + 1,
      t = v[k] / (v[k] - v[q]);
    px[e] = x0 + (i + (e & 1 ? 0 : t)) * st;
    py[e] = y0 + (j + (e & 1 ? t : 0)) * st;
  };
  const join = (a: number, b: number) => {
    if (link[2 * a] < 0) {
      link[2 * a] = b;
      at(a);
    } else link[2 * a + 1] = b;
    if (link[2 * b] < 0) {
      link[2 * b] = a;
      at(b);
    } else link[2 * b + 1] = a;
  };
  for (let j = 0; j < ny; j++)
    for (let i = 0; i < nx; i++) {
      const k = j * W + i,
        a = v[k],
        b = v[k + 1],
        cc = v[k + W + 1],
        d = v[k + W];
      const cs =
        (a < 0 ? 8 : 0) | (b < 0 ? 4 : 0) | (cc < 0 ? 2 : 0) | (d < 0 ? 1 : 0);
      if (cs === 0 || cs === 15) continue;
      const T = 2 * k,
        B = 2 * (k + W),
        L = 2 * k + 1,
        Rt = 2 * (k + 1) + 1,
        mid = a + b + cc + d < 0;
      switch (cs) {
        case 1:
        case 14:
          join(L, B);
          break;
        case 2:
        case 13:
          join(B, Rt);
          break;
        case 3:
        case 12:
          join(L, Rt);
          break;
        case 4:
        case 11:
          join(T, Rt);
          break;
        case 6:
        case 9:
          join(T, B);
          break;
        case 7:
        case 8:
          join(T, L);
          break;
        case 5:
          if (mid) {
            join(T, L);
            join(B, Rt);
          } else {
            join(T, Rt);
            join(L, B);
          }
          break;
        case 10:
          if (mid) {
            join(T, Rt);
            join(L, B);
          } else {
            join(T, L);
            join(B, Rt);
          }
          break;
      }
    }
  const loops = [],
    near = (st * 0.3) ** 2;
  for (let e = 0; e < nE; e++) {
    if (link[2 * e] < 0 || seen[e]) continue;
    const pts = [];
    let prev = -1,
      cur = e;
    while (cur >= 0 && !seen[cur]) {
      seen[cur] = 1;
      const p = pts[pts.length - 1];
      if (!p || (px[cur] - p[0]) ** 2 + (py[cur] - p[1]) ** 2 > near)
        pts.push([px[cur], py[cur]]);
      const n0 = link[2 * cur],
        n1 = link[2 * cur + 1],
        nxt = n0 !== prev ? n0 : n1;
      prev = cur;
      cur = nxt;
    }
    if (pts.length > 2) loops.push(pts);
  }
  return loops;
}

/* Eyes morph between open ovals, ^ ^ (happy), ‿ ‿ (asleep) and – – (not connected) with two numbers. */
function eye(
  ctx: Ctx,
  x: number,
  y: number,
  w: number,
  h: number,
  open: number,
  curve: number,
) {
  const o = clamp(open, 0, 1.3),
    shut = clamp(1 - o / 0.6, 0, 1);
  const b = (h / 2) * o,
    bend = curve * h * 0.34 * shut,
    th = Math.max(h * 0.24, 1.2) * shut;
  const top = -b - bend,
    bot = b - bend + th;
  ctx.beginPath();
  ctx.moveTo(x - w / 2, y);
  ctx.bezierCurveTo(
    x - w / 2,
    y + (top * 4) / 3,
    x + w / 2,
    y + (top * 4) / 3,
    x + w / 2,
    y,
  );
  ctx.bezierCurveTo(
    x + w / 2,
    y + (bot * 4) / 3,
    x - w / 2,
    y + (bot * 4) / 3,
    x - w / 2,
    y,
  );
  ctx.fill();
  return b;
}

const tone = (c: Vals, r: Role): Lab => [
  c[I[r + "0"]],
  Math.max(0, c[I[r + "1"]]),
  c[I[r + "2"]],
];
const paint = (c: Vals, r: Role, a = 1) => rgba(...unlch(tone(c, r)), a);
function glowDot(
  ctx: Ctx,
  x: number,
  y: number,
  r: number,
  core: string,
  halo: string,
  a: number,
) {
  const g = ctx.createRadialGradient(x, y, 0, x, y, r * 2.4);
  g.addColorStop(0, core.replace(/[\d.]+\)$/, a + ")"));
  g.addColorStop(0.35, halo.replace(/[\d.]+\)$/, a * 0.8 + ")"));
  g.addColorStop(1, halo.replace(/[\d.]+\)$/, "0)"));
  ctx.fillStyle = g;
  ctx.beginPath();
  ctx.arc(x, y, r * 2.4, 0, TAU);
  ctx.fill();
}
function miniOrb(
  ctx: Ctx,
  c: Vals,
  x: number,
  y: number,
  r: number,
  a: number,
) {
  const g = ctx.createRadialGradient(
    x - r * 0.35,
    y - r * 0.4,
    r * 0.05,
    x,
    y,
    r * 1.3,
  );
  g.addColorStop(0, paint(c, "L", a));
  g.addColorStop(0.5, paint(c, "M", a));
  g.addColorStop(1, paint(c, "D", a));
  ctx.fillStyle = g;
  ctx.beginPath();
  ctx.arc(x, y, r, 0, TAU);
  ctx.fill();
  ctx.fillStyle = `rgba(255,255,255,${0.5 * a * c[I.sheen]})`;
  ctx.beginPath();
  ctx.ellipse(x - r * 0.35, y - r * 0.42, r * 0.28, r * 0.17, -0.6, 0, TAU);
  ctx.fill();
}
/* A little soap bubble: see-through in the middle, coloured at the rim, with a bright glint. */
function bubbleDot(
  ctx: Ctx,
  c: Vals,
  x: number,
  y: number,
  r: number,
  a: number,
) {
  const g = ctx.createRadialGradient(
    x - r * 0.3,
    y - r * 0.35,
    r * 0.1,
    x,
    y,
    r,
  );
  g.addColorStop(0, paint(c, "L", a * 0.6));
  g.addColorStop(0.7, paint(c, "M", a * 0.7));
  g.addColorStop(1, paint(c, "D", a * 0.95));
  ctx.fillStyle = g;
  ctx.beginPath();
  ctx.arc(x, y, r, 0, TAU);
  ctx.fill();
  if (r > 2.2) {
    ctx.fillStyle = `rgba(255,255,255,${0.75 * a * c[I.sheen]})`;
    ctx.beginPath();
    ctx.arc(x - r * 0.38, y - r * 0.38, r * 0.24, 0, TAU);
    ctx.fill();
  }
}
/* A tiny cloud of three puffs. */
function cloudlet(
  ctx: Ctx,
  c: Vals,
  x: number,
  y: number,
  s: number,
  a: number,
) {
  const g = ctx.createLinearGradient(x, y - s, x, y + s * 0.75);
  g.addColorStop(0, paint(c, "L", a));
  g.addColorStop(0.55, paint(c, "M", a));
  g.addColorStop(1, paint(c, "D", a));
  ctx.fillStyle = g;
  ctx.beginPath();
  for (const [dx, dy, r] of [
    [-0.55, 0.12, 0.55],
    [0.05, -0.2, 0.72],
    [0.62, 0.15, 0.5],
  ]) {
    ctx.moveTo(x + (dx + r) * s, y + dy * s);
    ctx.arc(x + dx * s, y + dy * s, r * s, 0, TAU);
  }
  ctx.fill("nonzero");
  ctx.strokeStyle = paint(c, "D", a * 0.35);
  ctx.lineWidth = Math.max(0.8, s * 0.08);
  ctx.stroke();
}
/* A four-pointed sparkle. */
function sparkle(ctx: Ctx, x: number, y: number, s: number, rot: number) {
  const k = 0.16 * s;
  ctx.save();
  ctx.translate(x, y);
  ctx.rotate(rot);
  ctx.beginPath();
  ctx.moveTo(0, -s);
  ctx.quadraticCurveTo(k, -k, s, 0);
  ctx.quadraticCurveTo(k, k, 0, s);
  ctx.quadraticCurveTo(-k, k, -s, 0);
  ctx.quadraticCurveTo(-k, -k, 0, -s);
  ctx.fill();
  ctx.restore();
}
/* Jelly swims in pulses: a quick squeeze, then a slow release. */
function pulseAt(p: number) {
  const u = ((p % 1) + 1) % 1;
  return u < 0.22 ? smooth(u / 0.22) : 1 - smooth((u - 0.22) / 0.78);
}

/* Draws one orb over a floor at floorY. env: { dark, faces, pool, shadow, z, dash } (shadow is "r,g,b"; z and dash are rgba strings). */
export function drawOrb(
  ctx: Ctx,
  o: Orb,
  cx: number,
  floorY: number,
  R: number,
  env: Env,
): Head {
  const c = o.cur,
    ph = o.ph,
    small = R < 22;
  const happy =
    o.happy * (o.mood === "disconnected" || o.mood === "paused" ? 0 : 1);
  cx += c[I.shiftX] * R;
  // Size, squash, breath and the steady bounce Pip does while working.
  const hopU = Math.abs(Math.sin(Math.PI * ph.bounce));
  const squash = c[I.bounce] * 0.1 * Math.max(0, 1 - 3.5 * hopU);
  const breath = c[I.breathAmp] * Math.sin(TAU * ph.breath);
  const Rs = R * c[I.size];
  // Coming down to rest overshoots the floor a touch; that turns into a little landing squish.
  const land = Math.max(0, -c[I.lift]) * 0.6;
  let sx = c[I.sx] * (1 + squash + land) * (1 + 0.6 * breath),
    sy = c[I.sy] * (1 - squash - land) * (1 + breath);
  let swim = 0;
  if (c[I.pulse] > 0.001) {
    const p = c[I.pulse] * pulseAt(ph.bounce - 0.3);
    sx *= 1 - 0.13 * p;
    sy *= 1 + 0.1 * p;
    swim = c[I.pulse] * pulseAt(ph.bounce - 0.42) * 0.1 * R;
  }
  // Polygons and gears need more points to keep their corners and teeth crisp.
  const pw = polyW(c),
    N =
      (pw > 0.01 || c[I.cog] > 0.005 ? 2 : 1) *
      (R > 60 ? 128 : R > 26 ? 84 : 60),
    pts: Pt[] = [];
  let maxY = 0;
  for (let i = 0; i < N; i++) {
    const th = (TAU * i) / N,
      r = radius(c, ph, th) * Rs;
    const p = [r * Math.cos(th) * sx, r * Math.sin(th) * sy];
    pts.push(p);
    if (p[1] > maxY) maxY = p[1];
  }
  let topR = radius(c, ph, -Math.PI / 2) * Rs * sy;
  // Rings, friends and clouds are traced from a distance field instead of one outline.
  let loops: Pt[][] | null = null,
    F: Field | null = null,
    minX = 0,
    maxX = 0;
  if (c[I.hole] > 0.004 || c[I.duo] > 0.004 || c[I.puff] > 0.004) {
    if (c[I.gap] <= 0.004 && c[I.duo] <= 0.004 && c[I.puff] <= 0.004) {
      // An unbroken ring is just its outline plus a hole, so it needs no tracing.
      const hole = c[I.hole] * Rs,
        hy = c[I.holeY] * Rs * sy,
        hsy = Math.max(0.08, c[I.holeSy]),
        hp = [];
      for (let i = 0; i < 48; i++)
        hp.push([
          hole * Math.cos((TAU * i) / 48),
          hy + hole * hsy * Math.sin((TAU * i) / 48),
        ]);
      F = {
        friend: [0, 0, 0],
        lobes: [],
        top: 0,
        hole: hole > 0.3 ? [hy, hole, hsy] : null,
      };
      loops = hole > 0.3 ? [pts, hp] : [pts];
    } else {
      const field = shapeField(c, ph, Rs, sx, sy);
      F = field;
      const st = Math.max(Rs / 13, 1.25),
        [bx0, by0, bx1, by1] = field.box;
      loops = trace(
        field.f,
        bx0 - 2 * st,
        by0 - 2 * st,
        bx1 + 2 * st,
        by1 + 2 * st,
        st,
      );
    }
    if (loops.length) {
      maxY = -Infinity;
      minX = Infinity;
      maxX = -Infinity;
      for (const L of loops)
        for (const [x, y] of L) {
          if (y > maxY) maxY = y;
          if (x < minX) minX = x;
          if (x > maxX) maxX = x;
        }
      if (F.lobes.length) topR = Math.max(topR, F.top);
    } else loops = null;
  }
  // The shadow follows the shape's real width when it isn't round.
  let shX = loops ? [minX, maxX] : null;
  if (!loops && pw > 0.01) {
    let a = Infinity,
      b = -Infinity;
    for (const [x] of pts) {
      if (x < a) a = x;
      if (x > b) b = x;
    }
    shX = [a, b];
  }
  // Round seven's solid shapes (jewel, top, box, paper) blend with the outline; a roll lays the whole body on
  // its side, so its reach is measured after the roll and the lowest point still rests on the floor.
  const G = geoBodies(c, ph, Rs, sx, sy),
    roll = c[I.geoRoll];
  if (G || roll) {
    const g = G ? G.w : 0,
      b = extent(loops ? loops.flat() : pts, roll);
    for (let j = 0; j < 4; j++) b[j] *= 1 - g;
    if (G)
      for (const s of G.list) {
        const e = extent(s.pts, roll);
        for (let j = 0; j < 4; j++) b[j] += e[j] * s.w;
      }
    shX = [b[0], b[1]];
    topR = -b[2];
    maxY = b[3];
  }
  // Twirl's slow wobble while it thinks (and a small one while it spins) leans it on its tip.
  const lift = clamp(c[I.lift], 0, 1.6),
    tilt = c[I.prec]
      ? c[I.tilt] + c[I.prec] * Math.sin(TAU * ph.prec)
      : c[I.tilt];
  let baseY =
    floorY -
    lift * 0.42 * R +
    c[I.bobAmp] * R * Math.sin(TAU * ph.bob) -
    c[I.bounce] * 0.22 * R * hopU -
    swim;
  if (G) baseY -= G.hop;
  // Body centre in page space (the body leans on its lowest point).
  const ccx = cx + Math.sin(tilt) * maxY,
    ccy = baseY - Math.cos(tilt) * maxY;
  const alpha = c[I.alpha];
  // Extras sit by the head; a tall tip (Tide) shouldn't push them out of the picture.
  const topY =
    loops && F!.lobes.length
      ? ccy - Math.min(topR, 1.08 * Rs)
      : Math.max(ccy - topR, ccy - 1.08 * Rs * sy);

  // A soft pool of the orb's own light on the floor (stage views only).
  if (env.pool && c[I.glow] > 0.01) {
    ctx.save();
    ctx.translate(cx, floorY);
    ctx.scale(1, 0.2);
    const pg = ctx.createRadialGradient(0, 0, 0, 0, 0, Rs * 2.3);
    pg.addColorStop(0, paint(c, "G", c[I.glow] * (env.dark ? 0.3 : 0.22)));
    pg.addColorStop(1, paint(c, "G", 0));
    ctx.fillStyle = pg;
    ctx.beginPath();
    ctx.arc(0, 0, Rs * 2.3, 0, TAU);
    ctx.fill();
    ctx.restore();
  }
  // Shadow on the floor: darker and tighter as the orb comes down to rest.
  const sh = clamp(1 - lift, 0, 1);
  const scx = shX ? cx + (shX[0] + shX[1]) / 2 : cx,
    sw = shX ? (shX[1] - shX[0]) / 2 : Rs * sx;
  const sg = ctx.createRadialGradient(scx, floorY, 0, scx, floorY, sw);
  sg.addColorStop(
    0,
    `rgba(${env.shadow},${(0.16 + 0.14 * sh) * (env.dark ? 1.5 : 1)})`,
  );
  sg.addColorStop(1, `rgba(${env.shadow},0)`);
  ctx.save();
  ctx.translate(scx, floorY);
  ctx.scale(1, 0.17 + 0.03 * sh);
  ctx.translate(-scx, -floorY);
  ctx.fillStyle = sg;
  ctx.beginPath();
  ctx.arc(scx, floorY, sw * (0.95 - 0.2 * lift), 0, TAU);
  ctx.fill();
  ctx.restore();
  // Prism, resting, leaves a little rainbow on the floor beside it.
  if (c[I.puddle] > 0.01)
    rainbowPuddle(ctx, cx + 1.28 * R, floorY, R, c[I.puddle] * alpha);

  // Glow around the body.
  if (c[I.glow] > 0.01) {
    const gg = ctx.createRadialGradient(ccx, ccy, Rs * 0.4, ccx, ccy, Rs * 2.1);
    gg.addColorStop(0, paint(c, "G", c[I.glow] * (env.dark ? 0.5 : 0.36)));
    gg.addColorStop(1, paint(c, "G", 0));
    ctx.fillStyle = gg;
    ctx.beginPath();
    ctx.arc(ccx, ccy, Rs * 2.1, 0, TAU);
    ctx.fill();
  }

  // Sparks (working) and moons (Glim) pass behind the body for half of each circle.
  const front: (() => void)[] = [];
  const behind = (z: number, fn: () => void) => (z < 0 ? fn() : front.push(fn));
  if (c[I.sparks] > 0.01) {
    // Two sparks circle wide around the body, so they never cross the face; nearer means a little bigger.
    const a = c[I.sparks],
      rx = 1.42 * Rs * Math.max(sx, 0.9),
      ry = 1.12 * Rs * Math.max(sy, 0.9),
      rot = -0.3,
      sr = Math.max(Rs * 0.1, 1.7);
    const at = (ang: number) => {
      const x0 = rx * Math.cos(ang),
        y0 = ry * Math.sin(ang);
      return [
        ccx + x0 * Math.cos(rot) - y0 * Math.sin(rot),
        ccy + x0 * Math.sin(rot) + y0 * Math.cos(rot),
      ];
    };
    for (let s = 0; s < 2; s++) {
      const head = TAU * (ph.spark + s / 2);
      for (let j = 20; j >= 1; j--) {
        const a0 = head - j * 0.05,
          a1 = a0 + 0.05,
          [x0, y0] = at(a0),
          [x1, y1] = at(a1),
          z = Math.sin(a1);
        behind(z, () => {
          ctx.strokeStyle = paint(c, "G", a * 0.6 * (1 - j / 21));
          ctx.lineWidth = sr * (0.9 + 0.2 * z) * (1 - j / 22);
          ctx.lineCap = "butt";
          ctx.beginPath();
          ctx.moveTo(x0, y0);
          ctx.lineTo(x1, y1);
          ctx.stroke();
        });
      }
      const [x, y] = at(head),
        z = Math.sin(head),
        rr = sr * (1 + 0.2 * z);
      behind(z, () => {
        glowDot(ctx, x, y, rr, paint(c, "G", 1), paint(c, "G", 1), a * 0.55);
        ctx.fillStyle = paint(c, "D", a);
        ctx.beginPath();
        ctx.arc(x, y, rr, 0, TAU);
        ctx.fill();
        ctx.fillStyle = `rgba(255,255,255,${0.9 * a})`;
        ctx.beginPath();
        ctx.arc(x - rr * 0.25, y - rr * 0.25, rr * 0.45, 0, TAU);
        ctx.fill();
      });
    }
  }
  if (c[I.moons] > 0.01) {
    const m = c[I.moons],
      sizes = [0.25, 0.19, 0.145];
    const wO = clamp(c[I.mOrbit], 0, 1),
      wR = clamp(c[I.mRest], 0, 1),
      wT = clamp(c[I.mTrail], 0, 1),
      wA = clamp(c[I.mAsk], 0, 1);
    const wH = Math.max(0, 1 - wO - wR - wT - wA);
    const hover = [
        [-1.38, -0.5],
        [1.32, -0.78],
        [1.12, 0.42],
      ],
      ask = [
        [-1.32, -0.98],
        [-1.62, -0.4],
        [-1.5, 0.2],
      ];
    const trail = [
        [1.62, -1.62],
        [1.24, -1.2],
        [0.92, -0.88],
      ],
      restX = [-1.5, 1.45, 1.9];
    sizes.forEach((s0, i) => {
      const s = s0 * R * (0.4 + 0.6 * m),
        bob = 0.07 * R * Math.sin(TAU * (ph.bob * 1.3 + i / 3));
      const ang = TAU * (ph.spark * 0.75 + i / 3),
        oz = Math.sin(ang),
        ox = 1.55 * R * Math.cos(ang),
        oy = 0.42 * R * oz;
      const tb = 0.05 * R * Math.sin(TAU * (ph.think - i * 0.18));
      const pos = [
        [hover[i][0] * R, hover[i][1] * R + bob, wH],
        [
          ox * Math.cos(-0.2) - oy * Math.sin(-0.2),
          ox * Math.sin(-0.2) + oy * Math.cos(-0.2),
          wO,
        ],
        [ask[i][0] * R, ask[i][1] * R + bob * 0.5, wA],
        [trail[i][0] * R, trail[i][1] * R + tb, wT],
        [restX[i] * R, floorY - s - ccy, wR],
      ];
      let x = 0,
        y = 0;
      for (const [px, py, w] of pos) {
        x += px * w;
        y += py * w;
      }
      const z = wO * oz - 0.0001;
      behind(z, () =>
        miniOrb(
          ctx,
          c,
          ccx + x * (0.5 + 0.5 * m),
          ccy + y * (0.5 + 0.5 * m),
          s * (1 + 0.15 * wO * oz),
          m * (0.55 + 0.45 * alpha),
        ),
      );
    });
  }
  // Jelly's tendrils hang from the bell, wave, curl, lie on the floor, and one rises like a hand.
  const arms =
    c[I.tendrils] > 0.01
      ? tendrilPaths(c, ph, cx, baseY, maxY, tilt, sx, sy, Rs, R, floorY)
      : null;
  if (arms)
    arms.forEach((pts, t) => drawTendril(ctx, c, ph, pts, t, 1, R, alpha, 1));
  // Puff's little clouds rise off its top-left while it works.
  if (c[I.steam] > 0.01) {
    for (let k = 0; k < 3; k++) {
      const u = (((ph.spark * 0.75 + k / 3) % 1) + 1) % 1,
        fade = c[I.steam] * Math.min(1, u / 0.15, (1 - u) / 0.4);
      cloudlet(
        ctx,
        c,
        ccx - R * (0.28 + 0.45 * u),
        topY + R * (0.3 - 1.0 * u),
        Math.max(R * (0.13 + 0.15 * u), 2.4),
        fade * alpha,
      );
    }
  }
  // Fizz's bubbles: drawn now if they're behind the body, after it if in front.
  if (c[I.bub] > 0.01)
    fizzBubbles(
      o,
      c,
      ph,
      ccx,
      ccy,
      topY,
      floorY,
      R,
      Rs,
      sx,
      sy,
      alpha,
      (z, fn) => behind(z, () => fn(ctx)),
    );

  // The body itself, leaning on its lowest point (and, when rolled, lying over on its side).
  ctx.save();
  ctx.translate(cx, baseY);
  ctx.rotate(tilt);
  ctx.translate(0, -maxY);
  if (roll) ctx.rotate(roll);
  // Prism's beam of light passes behind the glass.
  if (c[I.rainbow] > 0.01) prismBeam(ctx, c, ph, Rs, sx, sy, alpha);
  const outline = () => {
    if (loops) {
      ctx.beginPath();
      for (const L of loops) addLoop(ctx, L);
    } else smoothPath(ctx, pts);
  };
  const rule = loops ? "evenodd" : "nonzero";
  // The soft outline fades out where a solid shape takes over.
  const blobA = G ? 1 - G.w : 1;
  if (blobA > 0.002) {
    if (G) {
      ctx.save();
      ctx.globalAlpha *= blobA;
    }
    if (loops && c[I.trail] > 0.01) friendTrail(ctx, c, ph, F!, Rs, sx, sy);
    outline();
    const body = ctx.createRadialGradient(
      -0.32 * Rs,
      -0.42 * Rs,
      Rs * 0.05,
      0,
      0,
      Rs * 1.75,
    );
    if (c[I.glass] > 0.001) {
      // See-through in the middle (Fizz, and a little for Jelly), so what's behind shows faintly.
      const g = c[I.glass];
      body.addColorStop(0, paint(c, "L", alpha * (1 - 0.55 * g)));
      body.addColorStop(0.42, paint(c, "M", alpha * (1 - 0.42 * g)));
      body.addColorStop(0.95, paint(c, "D", alpha * (1 - 0.1 * g)));
    } else {
      body.addColorStop(0, paint(c, "L", alpha));
      body.addColorStop(0.42, paint(c, "M", alpha));
      body.addColorStop(0.95, paint(c, "D", alpha));
    }
    ctx.fillStyle = body;
    ctx.fill(rule);
    ctx.save();
    ctx.clip(rule);
    if (c[I.swirl] > 0.01) {
      const s = c[I.swirl],
        blobs: [number, number, number, Role, number][] = [
          [
            0.34 * Math.cos(TAU * ph.swirl),
            0.26 * Math.sin(TAU * 1.3 * ph.swirl) - 0.1,
            0.62,
            "L",
            0.5,
          ],
          [
            -0.3 * Math.cos(TAU * 0.8 * ph.swirl + 2),
            0.3 * Math.sin(TAU * ph.swirl + 1),
            0.55,
            "G",
            0.38,
          ],
          [0.22 * Math.sin(TAU * 0.6 * ph.swirl + 4), 0.5, 0.6, "D", 0.3],
        ];
      for (const [bx, by, br, role, a] of blobs) {
        const g = ctx.createRadialGradient(
          bx * Rs,
          by * Rs,
          0,
          bx * Rs,
          by * Rs,
          br * Rs,
        );
        g.addColorStop(0, paint(c, role, a * s * alpha));
        g.addColorStop(1, paint(c, role, 0));
        ctx.fillStyle = g;
        ctx.fillRect(-2 * Rs, -2 * Rs, 4 * Rs, 4 * Rs);
      }
    }
    if (loops || c[I.glass] > 0.001 || c[I.bell] > 0.01 || c[I.fizz] > 0.01) {
      innerLights(ctx, c, ph, F, Rs, sx, sy, alpha);
      outline();
    }
    ctx.lineWidth = Rs * 0.1;
    ctx.strokeStyle = paint(c, "L", 0.3 * c[I.sheen] * alpha);
    ctx.stroke();
    // Rings, friends and clouds keep their shine inside the body, so it can't land in Loop's hole.
    if (loops) sheen(ctx, c, Rs, sx, sy, alpha);
    ctx.restore();
    if (!loops) sheen(ctx, c, Rs, sx, sy, alpha);
    if (c[I.dash] > 0.01) {
      outline();
      ctx.setLineDash([Math.max(2, Rs * 0.17), Math.max(1.5, Rs * 0.13)]);
      ctx.lineWidth = Math.max(1, Rs * 0.055);
      ctx.lineCap = "round";
      ctx.strokeStyle = env.dash.replace(/[\d.]+\)$/, c[I.dash] + ")");
      ctx.stroke();
      ctx.setLineDash([]);
    }

    // The sprout (Moss): a stem with two leaves that stands up, sways or droops.
    if (c[I.sprout] > 0.01) {
      const s = c[I.sprout],
        perk = c[I.perk],
        sway = c[I.swayAmp] * Math.sin(TAU * ph.sway);
      const len = Rs * 0.42 * s * (1 + 0.25 * Math.max(perk, 0)),
        bend = Math.max(-perk, 0) * 1.25;
      const a0 = -Math.PI / 2 + 0.12 + sway * 0.8,
        a1 = a0 + bend + sway * 0.6;
      const bx = 0,
        by = -topR + Rs * 0.04;
      const mx = bx + Math.cos(a0) * len * 0.55,
        my = by + Math.sin(a0) * len * 0.55;
      const tx = mx + Math.cos(a1) * len * 0.5,
        ty = my + Math.sin(a1) * len * 0.5;
      ctx.strokeStyle = paint(c, "D", alpha);
      ctx.lineWidth = Math.max(1.2, Rs * 0.06);
      ctx.lineCap = "round";
      ctx.beginPath();
      ctx.moveTo(bx, by);
      ctx.quadraticCurveTo(mx, my, tx, ty);
      ctx.stroke();
      const spread = 0.75 + 0.35 * perk,
        droop = Math.max(-perk, 0) * 0.9;
      for (const side of [-1, 1]) {
        const la = a1 + side * spread + droop * (side > 0 ? 0.6 : 1.1),
          ll = Rs * 0.27 * s,
          lw = Rs * 0.13 * s;
        ctx.save();
        ctx.translate(tx, ty);
        ctx.rotate(la);
        const lg = ctx.createLinearGradient(0, 0, ll, 0);
        lg.addColorStop(0, paint(c, "D", alpha));
        lg.addColorStop(1, paint(c, "M", alpha));
        ctx.fillStyle = lg;
        ctx.beginPath();
        ctx.moveTo(0, 0);
        ctx.quadraticCurveTo(ll * 0.5, -lw, ll, 0);
        ctx.quadraticCurveTo(ll * 0.5, lw, 0, 0);
        ctx.fill();
        ctx.restore();
      }
    }
    if (G) ctx.restore();
  }
  if (G)
    for (const s of G.list) {
      ctx.save();
      ctx.globalAlpha *= s.a;
      s.draw(ctx, alpha, env);
      ctx.restore();
    }

  // The face: two eyes and rosy cheeks, painted onto the box or the paper as they turn.
  if (env.faces) {
    if (!G) drawFace(ctx, o, c, Rs, alpha, small, happy, 1);
    else if (G.fa > 0.01) {
      ctx.save();
      ctx.transform(...G.m);
      ctx.globalAlpha *= Math.min(1, G.fa);
      drawFace(ctx, o, c, G.fs, alpha, small, happy, G.shut);
      ctx.restore();
    }
    if (loops && F!.friend[2] > 1 && c[I.duo] > 0.3)
      friendFace(ctx, o, c, F!.friend, small, happy, alpha);
  }
  if (c[I.glints] > 0.01) prismGlints(ctx, c, ph, Rs, sx, sy, alpha);
  ctx.restore();

  // A raised tendril is waving, so it comes out in front of the bell.
  if (arms && c[I.raise] > 0.02)
    drawTendril(
      ctx,
      c,
      ph,
      arms[3],
      3,
      3,
      R,
      alpha * smooth(c[I.raise] * 1.6),
      0,
    );
  for (const fn of front) fn();

  // Nova's glitter: little sparkles thrown outward while it works, each one fading as it goes.
  if (c[I.glitter] > 0.01) {
    const a = c[I.glitter],
      K = 8;
    for (let k = 0; k < K; k++) {
      const t = ph.spark * 1.25 + k / K,
        n = Math.floor(t),
        u = t - n,
        h1 = hash(n * 13 + k * 7),
        h2 = hash(n * 7 + k * 3 + 1);
      const ang = -Math.PI / 2 + (h1 - 0.5) * 1.6 * Math.PI,
        dist = Rs * (1.02 + 0.9 * u) * (0.95 + 0.25 * h2);
      const x = ccx + Math.cos(ang) * dist * Math.max(sx, 0.9),
        y = ccy + Math.sin(ang) * dist * 0.92;
      const fade = a * Math.min(1, u / 0.12, (1 - u) / 0.45),
        s = Math.max(2.6, Rs * (0.24 - 0.1 * u) * (0.75 + 0.5 * h2)),
        rot = 0.3 * (h2 - 0.5) + TAU * 0.05 * u;
      glowDot(
        ctx,
        x,
        y,
        s * 0.45,
        paint(c, "G", 1),
        paint(c, "G", 1),
        fade * 0.6,
      );
      ctx.fillStyle = paint(c, "D", fade);
      sparkle(ctx, x, y, s, rot);
      ctx.fillStyle = `rgba(255,255,255,${0.95 * fade})`;
      sparkle(ctx, x, y, s * 0.5, rot);
    }
  }
  // A tap on Fizz lets out a little burst of bubbles.
  if (o.pop > 0.01 && c[I.bub] > 0.01) {
    for (let k = 0; k < 6; k++) {
      const ang = TAU * (k / 6) + 0.4,
        d = Rs * (1.02 + 0.75 * (1 - o.pop));
      bubbleDot(
        ctx,
        c,
        ccx + Math.cos(ang) * d * sx,
        ccy + Math.sin(ang) * d * sy,
        Math.max(R * 0.07 * (0.6 + 0.4 * o.pop), 1.2),
        o.pop ** 0.7,
      );
    }
  }
  // A tap on Prism scatters a ring of rainbow sparkles.
  if (o.pop > 0.01 && c[I.prism] > 0.01)
    prismBurst(ctx, ccx, ccy, Rs, sx, sy, o.pop);

  // Thinking: a trail of thought dots to the upper right.
  if (c[I.think] > 0.01) {
    const a = c[I.think];
    THOUGHT.forEach(([dx, dy, dr], i) => {
      const r = Math.max(dr * R, 1.5),
        y = topY + dy * R + 0.05 * R * Math.sin(TAU * (ph.think - i * 0.18));
      miniOrb(ctx, c, ccx + dx * R, y, r, a);
    });
  }
  // Paused: z's drift up slowly and fade. Each z is born, rises and goes; none flash.
  if (c[I.z] > 0.01) {
    const a = c[I.z],
      zs = Math.max(R * 0.22, 4);
    for (let k = 0; k < 2; k++) {
      const u = (ph.z + k * 0.5) % 1,
        fade = Math.min(1, u / 0.2, (1 - u) / 0.35);
      const x = ccx + R * (0.62 + 0.5 * u),
        y = topY + R * (0.12 - 0.85 * u),
        s = zs * (0.75 + 0.45 * u);
      ctx.strokeStyle = env.z.replace(/[\d.]+\)$/, a * fade + ")");
      ctx.lineWidth = Math.max(1.1, s * 0.2);
      ctx.lineCap = "round";
      ctx.lineJoin = "round";
      ctx.beginPath();
      ctx.moveTo(x - s / 2, y - s / 2);
      ctx.lineTo(x + s / 2, y - s / 2);
      ctx.lineTo(x - s / 2, y + s / 2);
      ctx.lineTo(x + s / 2, y + s / 2);
      ctx.stroke();
    }
  }
  // Waiting: one question mark that rocks gently on its dot (Prism's comes out in rainbow colours).
  if (c[I.q] > 0.01) {
    const a = c[I.q] * (1 - clamp(c[I.rainQ], 0, 1)),
      s = Math.max(R * 0.46, 9),
      x = ccx + R * 1.2,
      y = topY + R * 0.18,
      rock = 0.13 * Math.sin(TAU * ph.q);
    if (a > 0.004) {
      const hg = ctx.createRadialGradient(
        x,
        y - s * 0.35,
        0,
        x,
        y - s * 0.35,
        s * 0.95,
      );
      hg.addColorStop(0, `rgba(255,196,92,${0.3 * a})`);
      hg.addColorStop(1, "rgba(255,196,92,0)");
      ctx.fillStyle = hg;
      ctx.beginPath();
      ctx.arc(x, y - s * 0.35, s * 0.95, 0, TAU);
      ctx.fill();
      ctx.save();
      ctx.translate(x, y);
      ctx.rotate(rock);
      ctx.strokeStyle = `rgba(232,138,36,${a})`;
      ctx.fillStyle = `rgba(232,138,36,${a})`;
      ctx.lineWidth = s * 0.17;
      ctx.lineCap = "round";
      ctx.beginPath();
      ctx.arc(0, -s * 0.72, s * 0.27, Math.PI * 1.08, Math.PI * 2.28);
      ctx.quadraticCurveTo(0, -s * 0.38, 0, -s * 0.2);
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(0, s * 0.06, s * 0.1, 0, TAU);
      ctx.fill();
      ctx.restore();
    }
    if (c[I.rainQ] > 0.004)
      rainbowQ(ctx, x, y, s, rock, c[I.q] * clamp(c[I.rainQ], 0, 1));
  }
  return {
    x: ccx + Math.sin(tilt) * topR,
    y: ccy - Math.cos(tilt) * topR,
    angle: tilt,
    size: Rs,
  };
}

/* The face: two eyes and rosy cheeks, sized by Rs. shut closes the eyes (Cubby blinks as it turns over). */
function drawFace(
  ctx: Ctx,
  o: Orb,
  c: Vals,
  Rs: number,
  alpha: number,
  small: boolean,
  happy: number,
  shut: number,
) {
  const k = small ? Math.max(1, 2.6 / (0.15 * Rs)) : 1;
  const w = 0.15 * Rs * k,
    h = 0.21 * Rs * k;
  const ex = c[I.eyeGap] * Rs * Math.max(1, k * 0.85) + c[I.lookX] * 0.09 * Rs,
    ey = c[I.eyeY] * Rs + c[I.lookY] * 0.07 * Rs;
  const blink = o.blinkT >= 0 ? 1 - Math.sin((Math.PI * o.blinkT) / 0.18) : 1;
  const open = c[I.eyeOpen] * blink * (1 - happy) * shut,
    curve = c[I.eyeCurve] * (1 - happy) + happy;
  const ink = tone(c, "D");
  ctx.fillStyle = rgba(
    ...unlch([0.27, ink[1] * 0.45, ink[2]]),
    Math.max(alpha, 0.8),
  );
  if (c[I.cheeks] > 0.02 && !small) {
    ctx.save();
    ctx.fillStyle = `rgba(255,104,146,${0.62 * c[I.cheeks] * alpha})`;
    for (const s of [-1, 1]) {
      ctx.beginPath();
      ctx.ellipse(
        s * (ex + 0.13 * Rs) + c[I.lookX] * 0.05 * Rs,
        ey + 0.16 * Rs,
        0.085 * Rs,
        0.05 * Rs,
        0,
        0,
        TAU,
      );
      ctx.fill();
    }
    ctx.restore();
  }
  let b = 0;
  for (const s of [-1, 1]) b = eye(ctx, s * ex, ey, w, h, open, curve);
  if (b > h * 0.2 && w > 2.4) {
    ctx.fillStyle = `rgba(255,255,255,${clamp(b / (h * 0.5), 0, 1)})`;
    for (const s of [-1, 1]) {
      ctx.beginPath();
      ctx.arc(
        s * ex + w * 0.16,
        ey - b * 0.42,
        Math.max(0.7, w * 0.21),
        0,
        TAU,
      );
      ctx.fill();
      if (w > 5) {
        ctx.beginPath();
        ctx.arc(s * ex - w * 0.14, ey + b * 0.42, w * 0.09, 0, TAU);
        ctx.fill();
      }
    }
  }
}

function sheen(
  ctx: Ctx,
  c: Vals,
  Rs: number,
  sx: number,
  sy: number,
  alpha: number,
) {
  ctx.fillStyle = `rgba(255,255,255,${0.55 * c[I.sheen] * alpha})`;
  ctx.beginPath();
  ctx.ellipse(
    -0.36 * Rs * sx,
    -0.5 * Rs * sy,
    0.2 * Rs,
    0.12 * Rs,
    -0.55,
    0,
    TAU,
  );
  ctx.fill();
  ctx.beginPath();
  ctx.arc(-0.1 * Rs * sx, -0.68 * Rs * sy, 0.045 * Rs, 0, TAU);
  ctx.fill();
}

/* Light and detail inside the body (already clipped to it): Loop's inner shade and running light, the
   friend's and the cloud lobes' own roundness, Jelly's hem lights, and Fizz's glassy rim and inner fizz. */
function innerLights(
  ctx: Ctx,
  c: Vals,
  ph: Phase,
  F: Field | null,
  Rs: number,
  sx: number,
  sy: number,
  alpha: number,
) {
  if (F && F.hole) {
    const [hy, hole, hsy] = F.hole;
    ctx.save();
    ctx.translate(0, hy);
    ctx.scale(1, hsy);
    const g = ctx.createRadialGradient(0, 0, hole * 0.95, 0, 0, hole * 1.6);
    g.addColorStop(0, paint(c, "D", 0.5 * alpha));
    g.addColorStop(1, paint(c, "D", 0));
    ctx.fillStyle = g;
    ctx.fillRect(-2 * hole, -2 * hole, 4 * hole, 4 * hole);
    ctx.restore();
  }
  if (c[I.ringLight] > 0.01) {
    // A light runs round the outer edge of the ring, below the face, with a fading tail.
    // Each pass is one tapered ribbon filled with a gradient that fades round the ring, so the tail has no seams.
    const a = c[I.ringLight] * alpha,
      rr = 0.85 * Rs,
      head = TAU * ph.spark * 1.6 - 0.15,
      len = 2.55,
      N = 40;
    for (const [role, wide, k] of [
      ["D", 0.3, 0.35],
      ["G", 0.22, 0.75],
      ["L", 0.11, 1],
    ] as [Role, number, number][]) {
      const col = (f: number) =>
        role === "L"
          ? `rgba(255,255,255,${a * k * f})`
          : paint(c, role, a * k * f);
      let fill: string | CanvasGradient = col(0.5);
      if (ctx.createConicGradient) {
        fill = ctx.createConicGradient(head - len, 0, 0);
        fill.addColorStop(0, col(0));
        fill.addColorStop(len / TAU, col(1));
        fill.addColorStop(len / TAU + 0.002, col(0));
      }
      const edge = (j: number, side: number) => {
        const t = head - len + (len * j) / N,
          w = (side * Math.max(1.2, Rs * wide * (0.25 + (0.75 * j) / N))) / 2;
        ctx.lineTo(Math.cos(t) * (rr * sx + w), Math.sin(t) * (rr * sy + w));
      };
      ctx.fillStyle = fill;
      ctx.beginPath();
      for (let j = 0; j <= N; j++) edge(j, 1);
      for (let j = N; j >= 0; j--) edge(j, -1);
      ctx.closePath();
      ctx.fill();
    }
    const hx = Math.cos(head) * rr * sx,
      hy = Math.sin(head) * rr * sy;
    glowDot(
      ctx,
      hx,
      hy,
      Math.max(Rs * 0.12, 1.8),
      "rgba(255,255,255,1)",
      paint(c, "G", 1),
      a,
    );
  }
  if (F && F.friend[2] > 0.5) {
    const [fx, fy, fr] = F.friend,
      g = ctx.createRadialGradient(
        fx - 0.3 * fr,
        fy - 0.38 * fr,
        fr * 0.05,
        fx,
        fy,
        fr * 1.15,
      );
    g.addColorStop(0, paint(c, "L", 0.85 * alpha));
    g.addColorStop(0.55, paint(c, "M", 0.55 * alpha));
    g.addColorStop(1, paint(c, "M", 0));
    ctx.fillStyle = g;
    ctx.fillRect(fx - 1.3 * fr, fy - 1.3 * fr, 2.6 * fr, 2.6 * fr);
    ctx.fillStyle = `rgba(255,255,255,${0.5 * c[I.sheen] * alpha})`;
    ctx.beginPath();
    ctx.ellipse(
      fx - 0.36 * fr,
      fy - 0.48 * fr,
      0.22 * fr,
      0.13 * fr,
      -0.55,
      0,
      TAU,
    );
    ctx.fill();
  }
  if (F)
    for (const [lx, ly, lr] of F.lobes) {
      if (lr < 0.5) continue;
      const g = ctx.createRadialGradient(
        lx - 0.3 * lr,
        ly - 0.4 * lr,
        0,
        lx,
        ly,
        lr * 1.05,
      );
      g.addColorStop(0, paint(c, "L", 0.7 * alpha));
      g.addColorStop(1, paint(c, "L", 0));
      ctx.fillStyle = g;
      ctx.fillRect(lx - 1.2 * lr, ly - 1.2 * lr, 2.4 * lr, 2.4 * lr);
    }
  if (c[I.bell] > 0.01) {
    // Soft lights along Jelly's hem; while it works they light up in turn with each pulse.
    const b = (1 - 0.55 * c[I.bell]) * Rs * sy;
    for (let k = 0; k < 5; k++) {
      const x = (k - 2) * 0.3 * Rs * sx,
        y = b - 0.13 * Rs,
        wave = 0.5 + 0.5 * Math.sin(TAU * (ph.bounce * 1 - k * 0.12));
      const a = alpha * (0.28 + 0.55 * c[I.beads] * wave);
      glowDot(
        ctx,
        x,
        y,
        Math.max(Rs * 0.045, 0.9),
        "rgba(255,255,255,1)",
        paint(c, "G", 1),
        a,
      );
    }
  }
  if (c[I.fizz] > 0.01) {
    // Fizz's inner fizz: tiny bubbles rising inside, faster while it works.
    for (let k = 0; k < 7; k++) {
      const u = (((ph.fizz + k / 7) % 1) + 1) % 1,
        h1 = hash(k * 3.1 + 0.7),
        x =
          ((h1 - 0.5) * 1.05 + 0.06 * Math.sin(TAU * (u * 1.5 + h1))) * Rs * sx;
      const y = (0.72 - 1.45 * u) * Rs * sy,
        r = (0.035 + 0.03 * hash(k + 9.2) + 0.02 * u) * Rs,
        a = c[I.fizz] * alpha * Math.min(1, u / 0.15, (1 - u) / 0.2);
      if (r < 0.5) continue;
      ctx.fillStyle = `rgba(255,255,255,${0.5 * a})`;
      ctx.beginPath();
      ctx.arc(x, y, r, 0, TAU);
      ctx.fill();
      ctx.strokeStyle = paint(c, "L", 0.8 * a);
      ctx.lineWidth = Math.max(0.6, r * 0.25);
      ctx.stroke();
    }
  }
  if (c[I.prism] > 0.01) prismInner(ctx, c, Rs, sx, sy, alpha);
  // A glassy rim and a curved glint low on the right, like a real bubble (round bodies only; Prism has its own).
  const g = c[I.glass] * (1 - clamp(polyW(c), 0, 1));
  if (g > 0.001) {
    ctx.strokeStyle = `rgba(255,255,255,${0.32 * g * alpha})`;
    ctx.lineWidth = Math.max(1, Rs * 0.06);
    ctx.beginPath();
    ctx.ellipse(0, 0, Rs * sx * 0.95, Rs * sy * 0.95, 0, 0, TAU);
    ctx.stroke();
    ctx.strokeStyle = `rgba(255,255,255,${0.5 * g * alpha * c[I.sheen]})`;
    ctx.lineWidth = Math.max(1, Rs * 0.07);
    ctx.lineCap = "round";
    ctx.beginPath();
    ctx.ellipse(
      0,
      0,
      Rs * sx * 0.74,
      Rs * sy * 0.74,
      0,
      0.12 * Math.PI,
      0.36 * Math.PI,
    );
    ctx.stroke();
  }
}

/* Duo's little friend: its own small eyes (blinking a moment after the big one) and rosy cheeks. */
function friendFace(
  ctx: Ctx,
  o: Orb,
  c: Vals,
  [fx, fy, fr]: number[],
  small: boolean,
  happy: number,
  alpha: number,
) {
  const k = small ? Math.max(1, 1.6 / (0.2 * fr)) : 1;
  const w = 0.2 * fr * k,
    h = 0.28 * fr * k;
  const ex = 0.36 * fr * Math.max(1, k * 0.8) + c[I.lookX] * 0.1 * fr,
    ey = fy - 0.02 * fr + c[I.lookY] * 0.08 * fr;
  const t = o.blink2 - 0.09,
    blink = t >= 0 && t <= 0.18 ? 1 - Math.sin((Math.PI * t) / 0.18) : 1;
  const open = c[I.eyeOpen] * blink * (1 - happy),
    curve = c[I.eyeCurve] * (1 - happy) + happy;
  const ink = tone(c, "D");
  if (c[I.cheeks] > 0.02 && !small) {
    ctx.fillStyle = `rgba(255,104,146,${0.62 * c[I.cheeks] * alpha})`;
    for (const s of [-1, 1]) {
      ctx.beginPath();
      ctx.ellipse(
        fx + s * (ex + 0.14 * fr),
        ey + 0.22 * fr,
        0.12 * fr,
        0.07 * fr,
        0,
        0,
        TAU,
      );
      ctx.fill();
    }
  }
  ctx.fillStyle = rgba(
    ...unlch([0.27, ink[1] * 0.45, ink[2]]),
    Math.max(alpha, 0.8),
  );
  let b = 0;
  for (const s of [-1, 1]) b = eye(ctx, fx + s * ex, ey, w, h, open, curve);
  if (b > h * 0.2 && w > 2.4) {
    ctx.fillStyle = `rgba(255,255,255,${clamp(b / (h * 0.5), 0, 1)})`;
    for (const s of [-1, 1]) {
      ctx.beginPath();
      ctx.arc(
        fx + s * ex + w * 0.16,
        ey - b * 0.42,
        Math.max(0.6, w * 0.22),
        0,
        TAU,
      );
      ctx.fill();
    }
  }
}

/* While Duo works, a soft swoosh follows the little friend along its hop. */
function friendTrail(
  ctx: Ctx,
  c: Vals,
  ph: Phase,
  F: Field,
  Rs: number,
  sx: number,
  sy: number,
) {
  const fr = F.friend[2];
  if (fr < 0.5) return;
  const d = c[I.duoD] * Rs,
    at = (p: number) => {
      const a = c[I.duoA] + c[I.duoSwing] * Math.sin(TAU * (p + 0.1));
      return [Math.cos(a) * d * sx, Math.sin(a) * d * sy];
    };
  ctx.lineCap = "round";
  for (let j = 16; j >= 1; j--) {
    const [x0, y0] = at(ph.bounce - (j + 1) * 0.013),
      [x1, y1] = at(ph.bounce - j * 0.013);
    ctx.strokeStyle = paint(c, "G", c[I.trail] * 0.5 * (1 - j / 17));
    ctx.lineWidth = fr * 1.5 * (1 - j / 19);
    ctx.beginPath();
    ctx.moveTo(x0, y0);
    ctx.lineTo(x1, y1);
    ctx.stroke();
  }
}

/* Jelly's four tendrils. Each one is drawn by walking down from the hem, turning a little at each step:
   a slow wave, a curl at the tip, and for the right-hand one a reach out and up like a raised hand. Where
   a tendril reaches the floor it lies along it, tip curled. While Jelly works, beads of light run down them. */
function tendrilPaths(
  c: Vals,
  ph: Phase,
  cx: number,
  baseY: number,
  maxY: number,
  tilt: number,
  sx: number,
  sy: number,
  Rs: number,
  R: number,
  floorY: number,
) {
  const hem = (1 - 0.55 * c[I.bell]) * Rs * sy - 0.06 * Rs,
    xs = [-0.5, -0.17, 0.17, 0.5],
    seg = 12;
  const stepL = (0.66 * R * c[I.tendrils]) / seg,
    fl = floorY - Math.max(1, R * 0.03),
    co = Math.cos(tilt),
    si = Math.sin(tilt),
    up = clamp(c[I.raise], 0, 1.2);
  return xs.map((x0, t) => {
    const lx = x0 * Rs * sx,
      ly = hem - maxY,
      side = Math.sign(x0),
      len = t === 3 ? stepL * (1 + 0.62 * up) : stepL;
    let x = cx + co * lx - si * ly,
      y = baseY + si * lx + co * ly,
      down = -1;
    const pts = [[x, y]];
    for (let j = 1; j <= seg; j++) {
      const s = j / seg;
      let a =
        Math.PI / 2 +
        tilt * 0.6 +
        x0 * 0.4 +
        c[I.tendAmp] *
          Math.sin(TAU * (ph.tend + t * 0.27) - 4.2 * s) *
          (0.3 + s) -
        side * c[I.curl] * 4.2 * s * s;
      // Out to the side first, then up.
      if (t === 3)
        a -=
          c[I.raise] *
          (1.5 * smooth(s / 0.26) + 1.7 * smooth((s - 0.36) / 0.3));
      if (down >= 0) {
        const k = (j - down) / seg;
        a =
          side > 0
            ? -c[I.curl] * 6 * Math.max(0, k - 0.45) ** 2 * 4
            : Math.PI + c[I.curl] * 6 * Math.max(0, k - 0.45) ** 2 * 4;
      }
      x += Math.cos(a) * len;
      y += Math.sin(a) * len;
      if (y > fl) {
        y = fl;
        if (down < 0) down = j;
      }
      pts.push([x, y]);
    }
    return pts;
  });
}
function drawTendril(
  ctx: Ctx,
  c: Vals,
  ph: Phase,
  pts: Pt[],
  t: number,
  from: number,
  R: number,
  alpha: number,
  beads: number,
) {
  const seg = pts.length - 1,
    wide = Math.max(1.1, R * 0.075);
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  for (let j = from; j <= seg; j++) {
    const s = j / seg;
    ctx.strokeStyle = paint(c, j < 4 ? "M" : "D", alpha * (0.95 - 0.35 * s));
    ctx.lineWidth = wide * (1 - 0.6 * s);
    ctx.beginPath();
    ctx.moveTo(pts[j - 1][0], pts[j - 1][1]);
    ctx.lineTo(pts[j][0], pts[j][1]);
    ctx.stroke();
  }
  if (beads && c[I.beads] > 0.01) {
    const u = (((ph.spark * 1.5 + t * 0.25) % 1) + 1) % 1,
      f = u * seg,
      j = Math.min(seg - 1, Math.floor(f)),
      r = f - j;
    const bx = pts[j][0] + (pts[j + 1][0] - pts[j][0]) * r,
      by = pts[j][1] + (pts[j + 1][1] - pts[j][1]) * r;
    glowDot(
      ctx,
      bx,
      by,
      Math.max(wide * 0.6, 1),
      "rgba(255,255,255,1)",
      paint(c, "G", 1),
      c[I.beads] * alpha * Math.min(1, u / 0.1, (1 - u) / 0.25),
    );
  }
}

/* Where each of Fizz's bubbles sits in a formation, in page space: [x, y, radius, z (behind < 0)]. */
function formation(
  f: number,
  i: number,
  ph: Phase,
  ccx: number,
  ccy: number,
  topY: number,
  floorY: number,
  R: number,
  Rs: number,
  sx: number,
  sy: number,
): number[] {
  const none = [
    ccx + 0.3 * R * Math.cos(i * 2.4),
    ccy + 0.1 * R + 0.3 * R * Math.sin(i * 2.4),
    0,
    1,
  ];
  if (f === 1 && i < SWIRL_R.length) {
    // A swirl round the body, wide enough to miss the face, passing behind it on the far side.
    const ang = TAU * (ph.spark * 0.8 + i / SWIRL_R.length),
      wob = 1 + 0.06 * Math.sin(TAU * (ph.spark * 2 + i * 0.37));
    const rx = 1.45 * Rs * Math.max(sx, 0.9) * wob,
      ry = 1.12 * Rs * Math.max(sy, 0.9) * wob,
      x0 = rx * Math.cos(ang),
      y0 = ry * Math.sin(ang),
      rot = -0.3;
    return [
      ccx + x0 * Math.cos(rot) - y0 * Math.sin(rot),
      ccy + x0 * Math.sin(rot) + y0 * Math.cos(rot),
      SWIRL_R[i] * R,
      Math.sin(ang),
    ];
  }
  if (f === 2 && i < ASK.length) {
    // A question mark of bubbles that rocks gently on its dot.
    const s = 0.78 * R,
      x = ccx + 1.24 * R,
      y = topY + 0.3 * R,
      rock = 0.13 * Math.sin(TAU * ph.q),
      [px, py] = ASK[i];
    return [
      x + s * (px * Math.cos(rock) - py * Math.sin(rock)),
      y + s * (px * Math.sin(rock) + py * Math.cos(rock)),
      (i === ASK.length - 1 ? 0.11 : 0.07) * R,
      1,
    ];
  }
  if (f === 3 && i < ZED.length) {
    // A z of bubbles, bobbing slowly while it dozes.
    const s = 0.86 * R,
      x = ccx + 1.12 * R,
      y = topY - 0.12 * R + 0.04 * R * Math.sin(TAU * ph.z * 0.5),
      [px, py] = ZED[i];
    return [x + s * px, y + s * py, 0.066 * R, 1];
  }
  if (f === 4 && i < FALLEN.length) {
    // Fallen to the floor and lying still.
    const [dx, r] = FALLEN[i],
      up = i === 4 ? 0.075 * 2 * R + 0.01 * R : 0;
    return [ccx + dx * R, floorY - r * R - up, r * R, 1];
  }
  if (f === 5 && i < THOUGHT.length) {
    const [dx, dy, dr] = THOUGHT[i];
    return [
      ccx + dx * R,
      topY + dy * R + 0.05 * R * Math.sin(TAU * (ph.think - i * 0.18)),
      dr * R,
      1,
    ];
  }
  return none;
}
function fizzBubbles(
  o: Orb,
  c: Vals,
  ph: Phase,
  ccx: number,
  ccy: number,
  topY: number,
  floorY: number,
  R: number,
  Rs: number,
  sx: number,
  sy: number,
  alpha: number,
  place: (z: number, fn: (ctx: Ctx) => void) => void,
) {
  const amt = c[I.bub],
    minR = Math.max(1.15, R * 0.045);
  for (let i = 0; i < NB; i++) {
    let x = 0,
      y = 0,
      r = 0,
      z = 0;
    for (let f = 0; f < NF; f++) {
      const w = o.bw[i * NF + f];
      if (Math.abs(w) < 0.0005) continue;
      const p = formation(f, i, ph, ccx, ccy, topY, floorY, R, Rs, sx, sy);
      x += p[0] * w;
      y += p[1] * w;
      r += p[2] * w;
      z += p[3] * w;
    }
    r *= amt;
    if (r < 0.15) continue;
    const rr = r < minR ? minR * Math.min(1, r / (0.5 * minR)) : r;
    place(z, (ctx) =>
      bubbleDot(ctx, c, x, y, rr, Math.min(1, amt) * (0.6 + 0.4 * alpha)),
    );
  }
}

/* ---------- Round seven: geometric shapes ---------- */

/* A regular polygon with n sides and rounded corners, standing on one flat side, as a distance from its
   centre. ai is the distance to each side, chosen so every polygon covers the same area as the circle. */
const POLY = (
  [
    ["tri", 3],
    ["sq", 4],
    ["pent", 5],
    ["hex", 6],
    ["oct", 8],
  ] as const
).map(([k, n]): [number, number, number] => [
  I[k],
  n,
  Math.sqrt(Math.PI / (n * Math.tan(Math.PI / n))),
]);
const polyW = (c: Vals) => c[I.tri] + c[I.sq] + c[I.pent] + c[I.hex] + c[I.oct];
function polyR(n: number, ai: number, a: number, k: number) {
  const s = TAU / n;
  let u = a - Math.PI / 2;
  u -= s * Math.round(u / s);
  const kr = k * ai,
    ii = ai - kr,
    tn = Math.tan(Math.PI / n),
    cu = Math.cos(u),
    su = Math.abs(Math.sin(u));
  if (ai * su <= ii * tn * cu) return ai / cu;
  // Past the flat part the ray meets the round corner, a circle of radius kr.
  const cd = ii * (cu + tn * su);
  return (
    cd + Math.sqrt(Math.max(0, cd * cd - ii * ii * (1 + tn * tn) + kr * kr))
  );
}
/* How far a turning shape turns before it looks the same again (0 if the numbers in v show no such shape):
   Poly by a gear tooth or a side, Gem by a facet, Twirl by a stripe. */
function turnStep(v: Vals, k: number): number {
  if (k === I.polyRot)
    return polyW(v) > 0.01 || v[I.cog] > 0.001 ? polyStep(v) : 0;
  return v[I.gem] > 0.01 ? TAU / 8 : v[I.top] > 0.01 ? TAU / 4 : 0;
}
/* How far Poly turns before it looks the same again: one gear tooth, or one side of the polygon it's becoming. */
function polyStep(t: Vals) {
  if (t[I.cog] > 0.001) return TAU / 10;
  let n = 6,
    best = 0;
  for (const [i, m] of POLY)
    if (t[i] > best) {
      best = t[i];
      n = m;
    }
  return TAU / n;
}

/* Small 3D helpers. Models use y up and z toward you; the light comes from the upper left, in front. */
const norm3 = (v: V3): V3 => {
  const n = Math.hypot(v[0], v[1], v[2]) || 1;
  return [v[0] / n, v[1] / n, v[2] / n];
};
const dot3 = (a: V3, b: V3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const sub3 = (a: V3, b: V3): V3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const cross3 = (a: V3, b: V3): V3 => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];
const rotX = (p: V3, a: number): V3 => {
  const co = Math.cos(a),
    si = Math.sin(a);
  return [p[0], p[1] * co - p[2] * si, p[1] * si + p[2] * co];
};
const rotY = (p: V3, a: number): V3 => {
  const co = Math.cos(a),
    si = Math.sin(a);
  return [p[0] * co + p[2] * si, p[1], -p[0] * si + p[2] * co];
};
const LIGHT3 = norm3([-0.42, 0.72, 0.55]),
  HALF3 = norm3([LIGHT3[0], LIGHT3[1], LIGHT3[2] + 1]);
/* Shading runs from the being's deep colour (0) through its middle (0.5) to its light colour (1). */
const tonesOf = (c: Vals) =>
  (["D", "M", "L"] as Role[]).map((r) => unlch(tone(c, r)));
function toneAt(T: Lab[], k: number, a: number) {
  k = clamp(k, 0, 1) * 2;
  const i = k < 1 ? 0 : 1,
    t = k - i,
    p = T[i],
    q = T[i + 1];
  return rgba(
    p[0] + (q[0] - p[0]) * t,
    p[1] + (q[1] - p[1]) * t,
    p[2] + (q[2] - p[2]) * t,
    a,
  );
}
function polyPath(ctx: Ctx, pts: Pt[]) {
  ctx.beginPath();
  pts.forEach(([x, y], i) => (i ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
  ctx.closePath();
}
/* The outline around a cloud of points (the smallest convex shape holding them all). */
function hull2(P: Pt[]) {
  const p = P.slice().sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  if (p.length < 3) return p;
  const cr = (o: Pt, a: Pt, b: Pt) =>
      (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]),
    lo = [],
    hi = [];
  for (const q of p) {
    while (lo.length > 1 && cr(lo[lo.length - 2], lo[lo.length - 1], q) <= 0)
      lo.pop();
    lo.push(q);
  }
  for (let i = p.length - 1; i >= 0; i--) {
    const q = p[i];
    while (hi.length > 1 && cr(hi[hi.length - 2], hi[hi.length - 1], q) <= 0)
      hi.pop();
    hi.push(q);
  }
  lo.pop();
  hi.pop();
  return lo.concat(hi);
}
/* The reach of some points after turning them by roll: [left, right, top, bottom]. */
function extent(pts: Pt[], roll: number) {
  const co = Math.cos(roll),
    si = Math.sin(roll);
  let x0 = Infinity,
    x1 = -Infinity,
    y0 = Infinity,
    y1 = -Infinity;
  for (const [x, y] of pts) {
    const X = x * co - y * si,
      Y = x * si + y * co;
    if (X < x0) x0 = X;
    if (X > x1) x1 = X;
    if (Y < y0) y0 = Y;
    if (Y > y1) y1 = Y;
  }
  return [x0, x1, y0, y1];
}
function dashOutline(ctx: Ctx, c: Vals, env: Env, Rs: number, pts: Pt[]) {
  polyPath(ctx, pts);
  ctx.lineJoin = "round";
  ctx.setLineDash([Math.max(2, Rs * 0.17), Math.max(1.5, Rs * 0.13)]);
  ctx.lineWidth = Math.max(1, Rs * 0.055);
  ctx.lineCap = "round";
  ctx.strokeStyle = env.dash.replace(/[\d.]+\)$/, c[I.dash] + ")");
  ctx.stroke();
  ctx.setLineDash([]);
}

/* The solid shapes in play, each a little 3D model with its weight. They grow in as they arrive, and one face
   is shared: blended between the soft outline's face and each shape's (painted on, for the box and the paper). */
const GEO: [
  string,
  (c: Vals, ph: Phase, Rs: number, sx: number, sy: number, k: number) => Solid,
][] = [
  ["gem", gemShape],
  ["top", topShape],
  ["cube", cubeShape],
  ["fold", foldShape],
];
function geoBodies(c: Vals, ph: Phase, Rs: number, sx: number, sy: number) {
  let tot = 0;
  for (const [k] of GEO) tot += clamp(c[I[k]], 0, 1);
  if (tot < 0.002) return null;
  const w = Math.min(1, tot),
    list: (Solid & { a: number; w: number })[] = [];
  for (const [k, fn] of GEO) {
    const a = clamp(c[I[k]], 0, 1);
    if (a < 0.002) continue;
    list.push({
      ...fn(c, ph, Rs, sx, sy, 0.55 + 0.45 * a),
      a,
      w: (a * w) / tot,
    });
  }
  const G = {
    w,
    list,
    m: [1 - w, 0, 0, 1 - w, 0, 0] as Mat,
    fs: Rs * (1 - w),
    fa: 1 - w,
    shut: 1 - w,
    hop: 0,
  };
  for (const s of list) {
    const f = s.face || [1, 0, 0, 1, 0, 0];
    for (let j = 0; j < 6; j++) G.m[j] += f[j] * s.w;
    G.fs += s.fs * s.w;
    G.fa += s.faceA * s.w;
    G.shut += s.shut * s.w;
    G.hop += s.hop * s.w;
  }
  return G;
}

/* Gem: a cut jewel with a flat table on top, eight sides, and a point below. It turns while it works, with a
   light bouncing around inside. */
const GEM_PATH = [
  [-0.5, -0.12],
  [0.36, 0.34],
  [0.06, -0.52],
  [-0.3, 0.44],
  [0.52, -0.06],
  [-0.16, -0.46],
];
function gemShape(
  c: Vals,
  ph: Phase,
  Rs: number,
  sx: number,
  sy: number,
  k: number,
): Solid {
  const S = Rs * k,
    yaw = c[I.gemYaw] + TAU * ph.spin,
    pitch = c[I.gemPitch],
    n = 8;
  const V = (x: number, y: number, z: number) =>
      rotX(rotY([x, y, z], yaw), pitch),
    scr = (p: V3): Pt => [p[0] * S * sx, -p[1] * S * sy];
  const top = [],
    gir = [];
  for (let i = 0; i < n; i++) {
    const a = (TAU * i) / n,
      b = a + Math.PI / n;
    gir.push(V(Math.cos(a), 0.4, Math.sin(a)));
    top.push(V(0.54 * Math.cos(b), 0.8, 0.54 * Math.sin(b)));
  }
  const cul = V(0, -0.95, 0),
    mid = V(0, 0.1, 0),
    tris = [top];
  for (let i = 0; i < n; i++) {
    const j = (i + 1) % n,
      h = (i + n - 1) % n;
    tris.push(
      [gir[i], gir[j], top[i]],
      [top[h], gir[i], top[i]],
      [gir[i], cul, gir[j]],
    );
  }
  const vis: { p: Pt[]; k: number; s: number }[] = [];
  tris.forEach((f, i) => {
    let nv = norm3(cross3(sub3(f[1], f[0]), sub3(f[2], f[0])));
    const cen = f.reduce(
      (s, p) => [
        s[0] + p[0] / f.length,
        s[1] + p[1] / f.length,
        s[2] + p[2] / f.length,
      ],
      [0, 0, 0],
    );
    if (dot3(nv, sub3(cen, mid)) < 0) nv = [-nv[0], -nv[1], -nv[2]];
    if (nv[2] <= 0.002) return;
    // Neighbouring facets alternate a little lighter and darker, the way a cut stone flashes.
    const kf =
      0.5 + 0.5 * dot3(nv, LIGHT3) + (i === 0 ? 0.08 : i % 2 ? 0.07 : -0.07);
    vis.push({ p: f.map(scr), k: kf, s: Math.max(0, dot3(nv, HALF3)) ** 18 });
  });
  const hull = hull2([...top, ...gir, cul].map(scr));
  const draw = (ctx: Ctx, alpha: number, env: Env) => {
    const T = tonesOf(c);
    ctx.lineJoin = "round";
    for (const f of vis) {
      polyPath(ctx, f.p);
      ctx.fillStyle = toneAt(T, f.k, alpha);
      ctx.fill();
      ctx.strokeStyle = ctx.fillStyle;
      ctx.lineWidth = 0.6;
      ctx.stroke();
      if (f.s > 0.03) {
        ctx.fillStyle = `rgba(255,255,255,${0.75 * f.s * alpha})`;
        ctx.fill();
      }
    }
    ctx.strokeStyle = toneAt(T, 1, 0.5 * alpha * c[I.sheen]);
    ctx.lineWidth = Math.max(0.6, S * 0.022);
    for (const f of vis) {
      polyPath(ctx, f.p);
      ctx.stroke();
    }
    if (c[I.gemLight] > 0.01) {
      // The light inside bounces from facet to facet, leaving a short trail.
      ctx.save();
      polyPath(ctx, hull);
      ctx.clip();
      const at = (u: number) => {
        u = ((u % 1) + 1) % 1;
        const s = u * GEM_PATH.length,
          i = Math.floor(s),
          f = s - i,
          a = GEM_PATH[i],
          b = GEM_PATH[(i + 1) % GEM_PATH.length];
        return [
          (a[0] + (b[0] - a[0]) * f) * S * sx,
          (a[1] + (b[1] - a[1]) * f) * S * sy,
        ];
      };
      const u0 = ph.spark * 0.9,
        la = c[I.gemLight] * alpha;
      for (let j = 7; j >= 1; j--) {
        const [x, y] = at(u0 - j * 0.012);
        glowDot(
          ctx,
          x,
          y,
          Math.max(0.8, S * 0.06 * (1 - j / 9)),
          "rgba(255,255,255,1)",
          paint(c, "G", 1),
          la * 0.5 * (1 - j / 8),
        );
      }
      const [x, y] = at(u0);
      glowDot(
        ctx,
        x,
        y,
        Math.max(1.2, S * 0.09),
        "rgba(255,255,255,1)",
        paint(c, "G", 1),
        la,
      );
      ctx.restore();
    }
    if (c[I.dash] > 0.01) dashOutline(ctx, c, env, Rs, hull);
  };
  return { pts: hull, face: null, fs: S, faceA: 1, shut: 1, hop: 0, draw };
}

/* Twirl: a spinning top, red with cream stripes round its lower half, a cream dome for its face and a handle. */
function topShape(
  c: Vals,
  ph: Phase,
  Rs: number,
  sx: number,
  sy: number,
  k: number,
): Solid {
  const S = Rs * k,
    turn = c[I.gemYaw] + TAU * ph.spin,
    pitch = 0.28;
  const at = (phi: number, y: number, rho: number): Pt => {
    const p = rotX([rho * Math.sin(phi), y, rho * Math.cos(phi)], pitch);
    return [p[0] * S * sx, -p[1] * S * sy];
  };
  // The profile: down from the rim to the tip, and up from the rim to the handle.
  const low: number[][] = [],
    dome: number[][] = [];
  for (let i = 0; i <= 8; i++) {
    const t = i / 8;
    low.push([-0.02 - 0.96 * t, (1 - t) ** 0.62 * (1 - 0.22 * t)]);
  }
  for (let i = 0; i <= 6; i++) {
    const s = i / 6;
    dome.push([-0.02 + 0.6 * s, 0.88 * Math.sqrt(1 - s * s) + 0.12]);
  }
  const cloud = [];
  for (const [y, rho] of [...low, ...dome])
    for (let j = 0; j < 24; j++) cloud.push(at((TAU * j) / 24, y, rho));
  const hull = hull2(cloud),
    base = at(0, 0.56, 0),
    tip = at(0, 0.86, 0),
    knobR = Math.max(1.4, 0.14 * S);
  const rim: Pt[] = [];
  for (let j = 0; j <= 16; j++)
    rim.push(at(-Math.PI / 2 + (Math.PI * j) / 16, -0.02, 1));
  const draw = (ctx: Ctx, alpha: number, env: Env) => {
    const T = tonesOf(c);
    const lg = ctx.createLinearGradient(-S * sx, 0, S * sx, 0);
    lg.addColorStop(0, toneAt(T, 0.6, alpha));
    lg.addColorStop(1, toneAt(T, 0.24, alpha));
    polyPath(ctx, hull);
    ctx.fillStyle = lg;
    ctx.fill();
    ctx.save();
    polyPath(ctx, hull);
    ctx.clip();
    // Cream stripes on the lower half, on the side facing you; they sweep round as it spins.
    for (let i = 0; i < 4; i++) {
      let a0 = ((((i * Math.PI) / 2 - turn) % TAU) + TAU) % TAU;
      if (a0 > Math.PI) a0 -= TAU;
      const lo = Math.max(-Math.PI / 2, a0),
        hi = Math.min(Math.PI / 2, a0 + Math.PI / 4);
      if (hi <= lo) continue;
      ctx.beginPath();
      for (const [y, rho] of low) {
        const p = at(lo, y, rho);
        ctx.lineTo(p[0], p[1]);
      }
      for (let j = low.length - 1; j >= 0; j--) {
        const p = at(hi, low[j][0], low[j][1]);
        ctx.lineTo(p[0], p[1]);
      }
      ctx.closePath();
      ctx.fillStyle = toneAt(
        T,
        0.97 - 0.22 * Math.max(0, Math.sin((lo + hi) / 2)),
        alpha,
      );
      ctx.fill();
    }
    // The dome above the rim.
    ctx.beginPath();
    rim.forEach(([x, y]) => ctx.lineTo(x, y));
    ctx.lineTo(1.3 * S * sx, -1.5 * S * sy);
    ctx.lineTo(-1.3 * S * sx, -1.5 * S * sy);
    ctx.closePath();
    const dg = ctx.createLinearGradient(
      -0.5 * S * sx,
      -0.7 * S * sy,
      0.7 * S * sx,
      0.3 * S * sy,
    );
    dg.addColorStop(0, toneAt(T, 1, alpha));
    dg.addColorStop(1, toneAt(T, 0.84, alpha));
    ctx.fillStyle = dg;
    ctx.fill();
    ctx.beginPath();
    rim.forEach(([x, y]) => ctx.lineTo(x, y));
    ctx.strokeStyle = toneAt(T, 0.3, alpha);
    ctx.lineWidth = Math.max(1, S * 0.1);
    ctx.stroke();
    // Light on the left, shade on the right.
    const sg = ctx.createLinearGradient(-S * sx, 0, S * sx, 0);
    sg.addColorStop(0, `rgba(255,255,255,${0.16 * alpha})`);
    sg.addColorStop(0.45, "rgba(255,255,255,0)");
    sg.addColorStop(0.6, toneAt(T, 0, 0));
    sg.addColorStop(1, toneAt(T, 0, 0.3 * alpha));
    ctx.fillStyle = sg;
    ctx.fillRect(-2 * S, -2 * S, 4 * S, 4 * S);
    ctx.fillStyle = `rgba(255,255,255,${0.5 * c[I.sheen] * alpha})`;
    ctx.beginPath();
    ctx.ellipse(
      -0.42 * S * sx,
      -0.28 * S * sy,
      0.22 * S,
      0.1 * S,
      -0.45,
      0,
      TAU,
    );
    ctx.fill();
    ctx.restore();
    // The handle.
    const hw = Math.max(1.6, 0.19 * S * sx);
    ctx.lineCap = "round";
    ctx.strokeStyle = toneAt(T, 0.36, alpha);
    ctx.lineWidth = hw;
    ctx.beginPath();
    ctx.moveTo(base[0], base[1]);
    ctx.lineTo(tip[0], tip[1]);
    ctx.stroke();
    ctx.fillStyle = toneAt(T, 0.42, alpha);
    ctx.beginPath();
    ctx.arc(tip[0], tip[1] - knobR * 0.6, knobR, 0, TAU);
    ctx.fill();
    ctx.fillStyle = `rgba(255,255,255,${0.55 * c[I.sheen] * alpha})`;
    ctx.beginPath();
    ctx.arc(tip[0] - knobR * 0.35, tip[1] - knobR, knobR * 0.35, 0, TAU);
    ctx.fill();
    // While it spins, speed lines flicker round its sides.
    if (c[I.arcs] > 0.01) {
      for (const s of [-1, 1])
        for (let j = 0; j < 2; j++) {
          const r = S * (1.24 + 0.18 * j),
            fl = 0.55 + 0.45 * Math.sin(TAU * (ph.spin * 1.5 + j * 0.5) + s);
          ctx.strokeStyle = paint(c, "G", c[I.arcs] * alpha * 0.8 * fl);
          ctx.lineWidth = Math.max(1, S * 0.06);
          ctx.beginPath();
          ctx.ellipse(
            0,
            0.08 * S * sy,
            r * sx,
            r * 0.72 * sy,
            0,
            s > 0 ? -0.5 : Math.PI - 0.5,
            s > 0 ? 0.5 : Math.PI + 0.5,
          );
          ctx.stroke();
        }
    }
    if (c[I.dash] > 0.01) dashOutline(ctx, c, env, Rs, hull);
  };
  return {
    pts: [...hull, [tip[0], tip[1] - knobR * 1.6]],
    face: null,
    fs: S,
    faceA: 1,
    shut: 1,
    hop: 0,
    draw,
  };
}

/* Cubby: a cardboard box with a lid hinged at the back. It tumbles over a quarter turn at a time while it works;
   its face is painted on the front, and hops to the next side (with a blink) as the box turns over. */
const CUBE: [V3, V3[]][] = [
  [
    [1, 0, 0],
    [
      [1, -1, -1],
      [1, 1, -1],
      [1, 1, 1],
      [1, -1, 1],
    ],
  ],
  [
    [-1, 0, 0],
    [
      [-1, -1, -1],
      [-1, -1, 1],
      [-1, 1, 1],
      [-1, 1, -1],
    ],
  ],
  [
    [0, -1, 0],
    [
      [-1, -1, -1],
      [1, -1, -1],
      [1, -1, 1],
      [-1, -1, 1],
    ],
  ],
  [
    [0, 0, 1],
    [
      [-1, -1, 1],
      [1, -1, 1],
      [1, 1, 1],
      [-1, 1, 1],
    ],
  ],
  [
    [0, 0, -1],
    [
      [-1, -1, -1],
      [-1, 1, -1],
      [1, 1, -1],
      [1, -1, -1],
    ],
  ],
];
const CUBE_IN: [V3, V3[]][] = [
  // the inside walls and floor, seen through the open lid
  [
    [0, 0, 1],
    [
      [-1, -1, -1],
      [1, -1, -1],
      [1, 1, -1],
      [-1, 1, -1],
    ],
  ],
  [
    [1, 0, 0],
    [
      [-1, -1, -1],
      [-1, 1, -1],
      [-1, 1, 1],
      [-1, -1, 1],
    ],
  ],
  [
    [-1, 0, 0],
    [
      [1, -1, -1],
      [1, -1, 1],
      [1, 1, 1],
      [1, 1, -1],
    ],
  ],
  [
    [0, 0, -1],
    [
      [-1, -1, 1],
      [-1, 1, 1],
      [1, 1, 1],
      [1, -1, 1],
    ],
  ],
  [
    [0, 1, 0],
    [
      [-1, -1, -1],
      [-1, -1, 1],
      [1, -1, 1],
      [1, -1, -1],
    ],
  ],
];
function cubeShape(
  c: Vals,
  ph: Phase,
  Rs: number,
  sx: number,
  sy: number,
  k: number,
): Solid {
  const S = Rs * k,
    h = 0.6,
    u = ph.roll - Math.floor(ph.roll),
    p = smooth((u - 0.45) / 0.5);
  const V = (q: V3) =>
    rotX(rotY(rotX(q, (Math.PI / 2) * p), c[I.cubeYaw]), c[I.cubePitch]);
  const scr = (q: V3): Pt => [q[0] * S * sx, -q[1] * S * sy];
  const P = ([x, y, z]: V3) => scr(V([x * h, y * h, z * h]));
  // The top is two flaps hinged at the left and right edges; they fold up and out to open.
  const al = clamp(c[I.lid], 0, 1.3) * 2.05,
    sa = Math.sin(al),
    ca = Math.cos(al),
    open = al > 0.03;
  const flaps = [-1, 1]
    .map((s) => {
      const pts = (
          [
            [s, 1, -1],
            [s, 1, 1],
            [s * (1 - ca), 1 + sa, 1],
            [s * (1 - ca), 1 + sa, -1],
          ] as V3[]
        ).map(P),
        n = V([s * sa, ca, 0]);
      return { pts, n, z: V([s * (1 - 0.5 * ca), 1 + 0.5 * sa, 0])[2] };
    })
    .sort((a, b) => a.z - b.z);
  const sides: { n: V3; k: number; p: Pt[] }[] = [];
  for (const [n, cs] of CUBE) {
    const nv = V(n);
    if (nv[2] > 0.002)
      sides.push({ n, k: 0.48 + 0.42 * dot3(nv, LIGHT3), p: cs.map(P) });
  }
  const opening = (
      [
        [-1, 1, -1],
        [1, 1, -1],
        [1, 1, 1],
        [-1, 1, 1],
      ] as V3[]
    ).map(P),
    corners: Pt[] = [];
  for (const x of [-1, 1])
    for (const y of [-1, 1])
      for (const z of [-1, 1]) corners.push(P([x, y, z]));
  const hull = hull2([
    ...corners,
    ...flaps.flatMap((f) => [f.pts[2], f.pts[3]]),
  ]);
  // The face sits on the front until halfway through a tumble, then on the side coming round to the front.
  const fc =
    p < 0.5
      ? [
          [0, 0, h],
          [1, 0, 0],
          [0, -1, 0],
        ]
      : [
          [0, h, 0],
          [1, 0, 0],
          [0, 0, 1],
        ];
  const O = V(fc[0] as V3),
    U = V(fc[1] as V3),
    D = V(fc[2] as V3);
  const face: Mat = [
    U[0] * sx,
    -U[1] * sy,
    D[0] * sx,
    -D[1] * sy,
    O[0] * S * sx,
    -O[1] * S * sy,
  ];
  const draw = (ctx: Ctx, alpha: number, env: Env) => {
    const T = tonesOf(c),
      edge = toneAt(T, 0.05, 0.5 * alpha),
      lw = Math.max(0.7, S * 0.03);
    ctx.lineJoin = "round";
    const fill = (pts: Pt[], col: string) => {
      polyPath(ctx, pts);
      ctx.fillStyle = col;
      ctx.fill();
      ctx.strokeStyle = col;
      ctx.lineWidth = 0.6;
      ctx.stroke();
    };
    const rim = (pts: Pt[]) => {
      polyPath(ctx, pts);
      ctx.strokeStyle = edge;
      ctx.lineWidth = lw;
      ctx.stroke();
    };
    for (const s of sides) {
      fill(s.p, toneAt(T, s.k, alpha));
      // Four round air holes in each side wall, set square so they look the same after every tumble.
      if (s.n[0])
        for (const [hy, hz] of [
          [0.3, 0],
          [-0.3, 0],
          [0, 0.3],
          [0, -0.3],
        ]) {
          const hole = [];
          for (let j = 0; j < 10; j++) {
            const t = (TAU * j) / 10;
            hole.push(
              P([s.n[0], hy + 0.1 * Math.sin(t), hz + 0.1 * Math.cos(t)]),
            );
          }
          polyPath(ctx, hole);
          ctx.fillStyle = toneAt(T, 0.04, 0.8 * alpha);
          ctx.fill();
        }
      rim(s.p);
    }
    const glow = c[I.boxGlow];
    if (open) {
      // Inside the box: dark, lit warm by whatever is glowing in there.
      ctx.save();
      polyPath(ctx, opening);
      ctx.clip();
      for (const [n, cs] of CUBE_IN) {
        const nv = V(n);
        if (nv[2] > 0.002)
          fill(
            cs.map(P),
            toneAt(
              T,
              0.04 + 0.16 * Math.max(0, dot3(nv, LIGHT3)) + 0.22 * glow,
              alpha,
            ),
          );
      }
      ctx.restore();
      rim(opening);
    }
    if (glow > 0.01) {
      const [ox, oy] = P([0, 1, 0]),
        gr = ctx.createRadialGradient(ox, oy, 0, ox, oy, 1.05 * S);
      gr.addColorStop(0, paint(c, "G", 0.75 * glow * alpha));
      gr.addColorStop(1, paint(c, "G", 0));
      ctx.fillStyle = gr;
      ctx.beginPath();
      ctx.arc(ox, oy, 1.05 * S, 0, TAU);
      ctx.fill();
      for (let j = -1; j <= 1; j++) {
        const x0 = ox + j * 0.3 * S * sx,
          a = glow * alpha * (0.5 + 0.2 * Math.sin(TAU * ph.q + j * 1.7)),
          lg = ctx.createLinearGradient(0, oy, 0, oy - 0.95 * S);
        lg.addColorStop(0, paint(c, "G", 0.55 * a));
        lg.addColorStop(1, paint(c, "G", 0));
        ctx.fillStyle = lg;
        ctx.beginPath();
        ctx.moveTo(x0 - 0.07 * S, oy);
        ctx.lineTo(x0 + 0.07 * S, oy);
        ctx.lineTo(x0 + j * 0.12 * S + 0.12 * S, oy - 0.95 * S);
        ctx.lineTo(x0 + j * 0.12 * S - 0.12 * S, oy - 0.95 * S);
        ctx.fill();
      }
    }
    // The flaps (or, shut, the plain top): their outside where it faces you, their inside, warmly lit, where it doesn't.
    const tn = V([0, 1, 0]);
    if (!open) {
      fill(opening, toneAt(T, 0.48 + 0.42 * dot3(tn, LIGHT3), alpha));
      rim(opening);
    } else
      for (const f of flaps) {
        fill(
          f.pts,
          f.n[2] > 0
            ? toneAt(T, 0.48 + 0.42 * dot3(f.n, LIGHT3), alpha)
            : toneAt(T, 0.2 + 0.3 * glow, alpha),
        );
        rim(f.pts);
      }
    ctx.strokeStyle = toneAt(T, 1, 0.45 * alpha * c[I.sheen]);
    ctx.lineWidth = lw;
    const [e0, e1] = [P([-1, 1, 1]), P([1, 1, 1])];
    ctx.beginPath();
    ctx.moveTo(e0[0], e0[1]);
    ctx.lineTo(e1[0], e1[1]);
    ctx.stroke();
    if (c[I.dash] > 0.01) dashOutline(ctx, c, env, Rs, hull);
  };
  return {
    pts: hull,
    face,
    fs: S * 0.85,
    faceA: 1,
    shut: smooth(Math.abs(p - 0.5) / 0.3),
    hop: Math.sin(Math.PI * p) * 0.16 * S,
    draw,
  };
}

/* Fold: one square sheet of paper standing on its corner, coloured on the front and white on the back. Each
   corner can fold over its crease toward you, and the lower half can fold behind the upper one. The sheet is cut
   into pieces along those creases, so every piece knows how it is folded; they're drawn back to front. */
const FD = 1.2,
  FQ = 0.68 * FD,
  FE = FD - FQ;
const FOLD_PIECES: [number[][], number, number][] = [
  [
    [
      [-FQ, 0],
      [-FQ, FE],
      [-FE, FQ],
      [FE, FQ],
      [FQ, FE],
      [FQ, 0],
    ],
    -1,
    0,
  ],
  [
    [
      [-FQ, 0],
      [FQ, 0],
      [FQ, -FE],
      [FE, -FQ],
      [-FE, -FQ],
      [-FQ, -FE],
    ],
    -1,
    1,
  ],
  [
    [
      [-FE, FQ],
      [FE, FQ],
      [0, FD],
    ],
    0,
    0,
  ],
  [
    [
      [FQ, 0],
      [FD, 0],
      [FQ, FE],
    ],
    1,
    0,
  ],
  [
    [
      [FQ, 0],
      [FQ, -FE],
      [FD, 0],
    ],
    1,
    1,
  ],
  [
    [
      [-FE, -FQ],
      [0, -FD],
      [FE, -FQ],
    ],
    2,
    1,
  ],
  [
    [
      [-FQ, 0],
      [-FQ, FE],
      [-FD, 0],
    ],
    3,
    0,
  ],
  [
    [
      [-FQ, 0],
      [-FD, 0],
      [-FQ, -FE],
    ],
    3,
    1,
  ],
];
const FOLD_CREASE: [number[], number[], number][] = [
  [[-FE, FQ], [FE, FQ], 0],
  [[FQ, FE], [FQ, 0], 0],
  [[FQ, 0], [FQ, -FE], 1],
  [[FE, -FQ], [-FE, -FQ], 1],
  [[-FQ, -FE], [-FQ, 0], 1],
  [[-FQ, 0], [-FQ, FE], 0],
];
const FOLD_ROT: ((v: V3, a: number) => V3)[] = [
  (v, a) => rotX(v, a),
  (v, a) => rotY(v, -a),
  (v, a) => rotX(v, -a),
  (v, a) => rotY(v, a),
];
const FOLD_AT = [
  [0, FQ],
  [FQ, 0],
  [0, -FQ],
  [-FQ, 0],
];
function foldShape(
  c: Vals,
  ph: Phase,
  Rs: number,
  sx: number,
  sy: number,
  k: number,
): Solid {
  const S = Rs * k;
  // While it works the corners fold in and out in turn (top, right, bottom, left); thinking, the left one tries a little fold.
  const bump = (u: number) => {
    u -= Math.floor(u);
    return u < 0.5
      ? Math.min(smooth(u / 0.14), 1 - smooth((u - 0.34) / 0.16))
      : 0;
  };
  const tryL =
    c[I.creaseGlow] * 0.14 * (0.5 - 0.5 * Math.cos(TAU * ph.think * 0.45));
  const th = [
    c[I.flapT] + c[I.wave] * Math.sin(TAU * ph.q),
    c[I.flapR],
    c[I.flapB],
    c[I.flapL] + tryL,
  ].map(
    (f, i) => (f + c[I.foldWork] * 0.83 * bump(ph.spark - i / 4)) * Math.PI,
  );
  const tent = c[I.tent] * Math.PI,
    yaw = c[I.foldYaw],
    lie = c[I.lie];
  const W = (p: V3) => rotX(rotX(rotY(p, yaw), -lie), 0.14);
  const half = (p: V3, lo: number) => (lo ? rotX(p, tent) : p);
  const place = (x: number, y: number, f: number, lo: number) => {
    let p: V3 = [x, y, 0];
    if (f >= 0) {
      const [ax, ay] = FOLD_AT[f],
        r = FOLD_ROT[f]([x - ax, y - ay, 0], th[f]);
      p = [ax + r[0], ay + r[1], r[2]];
    }
    return W(half(p, lo));
  };
  const scr = (p: V3): Pt => [p[0] * S * sx, -p[1] * S * sy];
  const pieces = FOLD_PIECES.map(([pts, f, lo]) => {
    const v = pts.map(([x, y]) => place(x, y, f, lo));
    return {
      p: v.map(scr),
      z: v.reduce((s, q) => s + q[2], 0) / v.length,
      n: W(half(f >= 0 ? FOLD_ROT[f]([0, 0, 1], th[f]) : [0, 0, 1], lo)),
      f,
      lo,
    };
  }).sort((a, b) => a.z - b.z);
  const all = pieces.flatMap((q) => q.p),
    hull = hull2(all);
  const fy = 0.04 + 0.4 * c[I.tent],
    O = W([0, fy, 0]),
    U = W([1, 0, 0]),
    Dn = W([0, -1, 0]),
    nz = W([0, 0, 1])[2];
  const face: Mat = [
    U[0] * sx,
    -U[1] * sy,
    Dn[0] * sx,
    -Dn[1] * sy,
    O[0] * S * sx,
    -O[1] * S * sy,
  ];
  const draw = (ctx: Ctx, alpha: number, env: Env) => {
    const T = tonesOf(c),
      lw = Math.max(0.7, S * 0.025);
    ctx.lineJoin = "round";
    ctx.lineCap = "round";
    for (const q of pieces) {
      const front = q.n[2] >= 0,
        l = dot3(front ? q.n : [-q.n[0], -q.n[1], -q.n[2]], LIGHT3);
      const col = front
        ? toneAt(T, 0.34 + 0.34 * l, alpha)
        : toneAt(T, 0.9 + 0.1 * l, alpha);
      polyPath(ctx, q.p);
      ctx.fillStyle = col;
      ctx.fill();
      ctx.strokeStyle = col;
      ctx.lineWidth = 0.8;
      ctx.stroke();
      // A folded corner shows its crease.
      if (q.f >= 0 && Math.abs(th[q.f]) > 0.03) {
        const [a, b] = q.p;
        ctx.strokeStyle = toneAt(
          T,
          0.1,
          0.45 * alpha * Math.min(1, Math.abs(th[q.f]) / 0.4),
        );
        ctx.lineWidth = lw;
        ctx.beginPath();
        ctx.moveTo(a[0], a[1]);
        ctx.lineTo(b[0], b[1]);
        ctx.stroke();
      }
    }
    // A sheen across the paper, and its edge.
    ctx.save();
    polyPath(ctx, hull);
    ctx.clip();
    const sh = ctx.createLinearGradient(
      -0.9 * S * sx,
      -0.9 * S * sy,
      0.3 * S * sx,
      0.3 * S * sy,
    );
    sh.addColorStop(0, `rgba(255,255,255,${0.3 * c[I.sheen] * alpha})`);
    sh.addColorStop(1, "rgba(255,255,255,0)");
    ctx.fillStyle = sh;
    ctx.fillRect(-2 * S, -2 * S, 4 * S, 4 * S);
    ctx.restore();
    polyPath(ctx, hull);
    ctx.strokeStyle = toneAt(T, 0.05, 0.28 * alpha);
    ctx.lineWidth = lw;
    ctx.stroke();
    // Thinking: the creases glow, like it's working out where to fold.
    if (c[I.creaseGlow] > 0.01) {
      const g = c[I.creaseGlow] * alpha;
      for (const [w, col] of [
        [S * 0.1, paint(c, "G", 0.35 * g)],
        [Math.max(0.8, S * 0.03), `rgba(255,255,255,${0.9 * g})`],
      ] as [number, string][]) {
        ctx.strokeStyle = col;
        ctx.lineWidth = w;
        ctx.beginPath();
        for (const [a, b, lo] of FOLD_CREASE) {
          const p0 = scr(W(half([a[0], a[1], 0], lo))),
            p1 = scr(W(half([b[0], b[1], 0], lo)));
          ctx.moveTo(p0[0], p0[1]);
          ctx.lineTo(p1[0], p1[1]);
        }
        ctx.stroke();
      }
    }
    if (c[I.dash] > 0.01) dashOutline(ctx, c, env, Rs, hull);
  };
  return {
    pts: hull,
    face,
    fs: S * (0.88 - 0.3 * c[I.tent]),
    faceA: smooth((nz - 0.05) / 0.22),
    shut: 1,
    hop: 0,
    draw,
  };
}

/* Prism: the spectrum, its beam of light, the rainbow inside the glass, glints, the puddle and the tap burst. */
const HUES = [356, 24, 46, 140, 204, 268];
const hue = (j: number, a: number, l = 60) =>
  `hsla(${HUES[j]},90%,${l}%,${clamp(a, 0, 1).toFixed(3)})`;
function prismEdgeX(Rs: number, y: number, sy: number) {
  return (POLY[0][2] * Rs + 0.5 * (y / sy)) / 0.866;
}
function prismBeam(
  ctx: Ctx,
  c: Vals,
  ph: Phase,
  Rs: number,
  sx: number,
  sy: number,
  alpha: number,
) {
  const a = c[I.rainbow] * alpha,
    y = -0.2 * Rs * sy,
    x0 = -prismEdgeX(Rs, y, sy) * sx,
    x1 = -x0;
  // White light comes in from the lower left...
  const L = 1.05 * Rs,
    dx = Math.cos(0.325),
    dy = -Math.sin(0.325),
    bx = x0 - L * dx * sx,
    by = y - L * dy * sy;
  ctx.lineCap = "round";
  for (const [w, col, k] of [
    [Math.max(3, Rs * 0.2), paint(c, "G", 1), 0.35],
    [Math.max(1.4, Rs * 0.075), "rgba(255,255,255,1)", 1],
  ] as [number, string, number][]) {
    const g = ctx.createLinearGradient(bx, by, x0, y);
    g.addColorStop(0, col.replace(/[\d.]+\)$/, "0)"));
    g.addColorStop(0.4, col.replace(/[\d.]+\)$/, a * k + ")"));
    g.addColorStop(1, col.replace(/[\d.]+\)$/, a * k + ")"));
    ctx.strokeStyle = g;
    ctx.lineWidth = w;
    ctx.beginPath();
    ctx.moveTo(bx, by);
    ctx.lineTo(x0, y);
    ctx.stroke();
  }
  // ...and leaves on the right as a fan of colours.
  const Lf = 1.3 * Rs;
  for (let j = 0; j < 6; j++) {
    const t0 = 0.1 + (0.56 * j) / 6,
      t1 = 0.1 + (0.56 * (j + 1)) / 6,
      tm = (t0 + t1) / 2;
    const g = ctx.createLinearGradient(
      x1,
      y,
      x1 + Lf * Math.cos(tm) * sx,
      y + Lf * Math.sin(tm) * sy,
    );
    g.addColorStop(0, hue(j, a));
    g.addColorStop(0.55, hue(j, 0.75 * a));
    g.addColorStop(1, hue(j, 0));
    ctx.fillStyle = g;
    ctx.beginPath();
    ctx.moveTo(x1, y);
    ctx.lineTo(x1 + Lf * Math.cos(t0) * sx, y + Lf * Math.sin(t0) * sy);
    ctx.lineTo(x1 + Lf * Math.cos(t1) * sx, y + Lf * Math.sin(t1) * sy);
    ctx.closePath();
    ctx.fill();
  }
  // Little packets of light travel along the beam and out along the colours.
  for (let j = 0; j < 2; j++) {
    const u = (((ph.spark * 1.6 + j / 2) % 1) + 1) % 1,
      f = a * Math.min(1, u / 0.15, (1 - u) / 0.15);
    glowDot(
      ctx,
      bx + (x0 - bx) * u,
      by + (y - by) * u,
      Math.max(1.1, Rs * 0.055),
      "rgba(255,255,255,1)",
      paint(c, "G", 1),
      f,
    );
  }
  for (const j of [0, 2, 5]) {
    const u = (((ph.spark * 1.6 + j * 0.31) % 1) + 1) % 1,
      tm = 0.1 + (0.56 * (j + 0.5)) / 6,
      f = a * Math.min(1, u / 0.12, (1 - u) / 0.35);
    glowDot(
      ctx,
      x1 + Lf * u * Math.cos(tm) * sx,
      y + Lf * u * Math.sin(tm) * sy,
      Math.max(1.1, Rs * 0.05),
      hue(j, 1, 75),
      hue(j, 1),
      f,
    );
  }
}
function prismInner(
  ctx: Ctx,
  c: Vals,
  Rs: number,
  sx: number,
  sy: number,
  alpha: number,
) {
  const p = c[I.prism] * alpha * (1 - 0.7 * clamp(c[I.rainbow], 0, 1));
  // Light bent inside the glass: a soft band of rainbow.
  const g = ctx.createLinearGradient(
    -0.55 * Rs * sx,
    -0.75 * Rs * sy,
    0.35 * Rs * sx,
    0.1 * Rs * sy,
  );
  g.addColorStop(0.28, hue(0, 0));
  HUES.forEach((_, j) => g.addColorStop(0.36 + j * 0.06, hue(j, 0.32 * p)));
  g.addColorStop(0.74, hue(5, 0));
  ctx.fillStyle = g;
  ctx.fillRect(-2 * Rs, -2 * Rs, 4 * Rs, 4 * Rs);
  // The glass is thick: a bright bevel just inside its edges.
  ctx.beginPath();
  for (let i = 0; i <= 60; i++) {
    const th = (TAU * i) / 60,
      r = polyR(3, POLY[0][2], th, c[I.polyK]) * Rs * 0.84;
    ctx.lineTo(r * Math.cos(th) * sx, r * Math.sin(th) * sy);
  }
  ctx.strokeStyle = `rgba(255,255,255,${0.45 * p})`;
  ctx.lineWidth = Math.max(0.8, Rs * 0.035);
  ctx.stroke();
  if (c[I.rainbow] > 0.01) {
    // The white beam crossing the glass.
    const a = c[I.rainbow] * alpha,
      y = -0.2 * Rs * sy,
      x0 = -prismEdgeX(Rs, y, sy) * sx;
    ctx.lineCap = "round";
    ctx.strokeStyle = paint(c, "G", 0.4 * a);
    ctx.lineWidth = Math.max(2.4, Rs * 0.16);
    ctx.beginPath();
    ctx.moveTo(x0, y);
    ctx.lineTo(-x0, y);
    ctx.stroke();
    ctx.strokeStyle = `rgba(255,255,255,${0.85 * a})`;
    ctx.lineWidth = Math.max(1.2, Rs * 0.06);
    ctx.stroke();
  }
}
function prismGlints(
  ctx: Ctx,
  c: Vals,
  ph: Phase,
  Rs: number,
  sx: number,
  sy: number,
  alpha: number,
) {
  // Thinking: rainbow glints drift slowly round just inside the edges.
  const ii = POLY[0][2] * (1 - c[I.polyK]) * Rs,
    V3 = [
      [0, -2 * ii],
      [1.732 * ii, ii],
      [-1.732 * ii, ii],
    ];
  for (let j = 0; j < 3; j++) {
    const s = ((((ph.think * 0.2 + j / 3) % 1) + 1) % 1) * 3,
      e = Math.floor(s),
      f = s - e,
      P = V3[e],
      Q = V3[(e + 1) % 3];
    const x = (P[0] + (Q[0] - P[0]) * f) * sx,
      y = (P[1] + (Q[1] - P[1]) * f) * sy;
    const a =
      c[I.glints] *
      alpha *
      (0.6 + 0.4 * Math.sin(TAU * (ph.think * 1.3 + j * 0.37)));
    ctx.fillStyle = hue(j * 2, a);
    sparkle(ctx, x, y, Math.max(2.6, Rs * 0.16), 0);
    ctx.fillStyle = `rgba(255,255,255,${0.9 * a})`;
    sparkle(ctx, x, y, Math.max(1.3, Rs * 0.07), 0);
  }
}
function rainbowPuddle(ctx: Ctx, x: number, y: number, R: number, a: number) {
  ctx.save();
  ctx.translate(x, y);
  ctx.scale(1, 0.24);
  for (let j = 0; j < 6; j++) {
    const px = (j - 2.5) * 0.1 * R,
      g = ctx.createRadialGradient(px, 0, 0, px, 0, 0.19 * R);
    g.addColorStop(0, hue(j, 0.5 * a));
    g.addColorStop(1, hue(j, 0));
    ctx.fillStyle = g;
    ctx.beginPath();
    ctx.arc(px, 0, 0.19 * R, 0, TAU);
    ctx.fill();
  }
  ctx.restore();
}
function prismBurst(
  ctx: Ctx,
  x: number,
  y: number,
  Rs: number,
  sx: number,
  sy: number,
  pop: number,
) {
  for (let j = 0; j < 7; j++) {
    const ang = (TAU * j) / 7 - Math.PI / 2,
      d = Rs * (1.05 + 0.85 * (1 - pop)),
      px = x + Math.cos(ang) * d * sx,
      py = y + Math.sin(ang) * d * sy;
    const a = pop ** 0.7,
      s = Math.max(2.4, Rs * 0.17 * (0.6 + 0.4 * pop));
    ctx.fillStyle = hue(j % 6, a);
    sparkle(ctx, px, py, s, 0.3);
    ctx.fillStyle = `rgba(255,255,255,${0.9 * a})`;
    sparkle(ctx, px, py, s * 0.45, 0.3);
  }
}
function rainbowQ(
  ctx: Ctx,
  x: number,
  y: number,
  s: number,
  rock: number,
  a: number,
) {
  const hg = ctx.createRadialGradient(
    x,
    y - s * 0.35,
    0,
    x,
    y - s * 0.35,
    s * 0.95,
  );
  hg.addColorStop(0, `rgba(214,200,255,${0.34 * a})`);
  hg.addColorStop(1, "rgba(214,200,255,0)");
  ctx.fillStyle = hg;
  ctx.beginPath();
  ctx.arc(x, y - s * 0.35, s * 0.95, 0, TAU);
  ctx.fill();
  ctx.save();
  ctx.translate(x, y);
  ctx.rotate(rock);
  const g = ctx.createLinearGradient(-s * 0.3, -s, s * 0.2, s * 0.1);
  HUES.forEach((_, j) => g.addColorStop(j / 5, hue(j, a, 50)));
  ctx.strokeStyle = g;
  ctx.lineWidth = s * 0.17;
  ctx.lineCap = "round";
  ctx.beginPath();
  ctx.arc(0, -s * 0.72, s * 0.27, Math.PI * 1.08, Math.PI * 2.28);
  ctx.quadraticCurveTo(0, -s * 0.38, 0, -s * 0.2);
  ctx.stroke();
  ctx.fillStyle = hue(5, a, 50);
  ctx.beginPath();
  ctx.arc(0, s * 0.06, s * 0.1, 0, TAU);
  ctx.fill();
  ctx.restore();
}
