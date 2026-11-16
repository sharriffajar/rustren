import { createRequire } from 'module';
import fs from 'fs';
import path from 'path';

const require = createRequire('/root/remotion/package.json');
const esbuild = require('esbuild');

const tsxPath = process.argv[2] || '/root/remotion/src/NvmTeaser.tsx';
const outJsonPath = process.argv[3] || '/root/rustren/examples/nvm_teaser_tsx.json';
const width = 1080;
const height = 1920;
const fps = 30;
const durationSecs = 12.0;
const totalFrames = Math.ceil(durationSecs * fps);

const source = fs.readFileSync(tsxPath, 'utf8');
const transformed = esbuild.transformSync(source, {
  loader: 'tsx',
  format: 'cjs',
  jsx: 'transform',
  jsxFactory: '__h',
  jsxFragment: '__Frag',
});

let currentFrame = 0;
const memoCache = [];
let memoIdx = 0;

function __h(type, props, ...children) {
  return { type, props: props || {}, children: children.flat(Infinity).filter((c) => c !== null && c !== undefined && c !== false && c !== true) };
}
const __Frag = Symbol('Fragment');

const reactShim = {
  createElement: __h,
  Fragment: __Frag,
  useMemo(fn, deps) {
    const idx = memoIdx++;
    if (!memoCache[idx]) {
      memoCache[idx] = { val: fn(), deps };
    }
    return memoCache[idx].val;
  },
};

const remotionShim = {
  useCurrentFrame: () => currentFrame,
  useVideoConfig: () => ({ fps, durationInFrames: totalFrames, width, height }),
  AbsoluteFill: ({ style, children }) => __h('div', { style: { position: 'absolute', left: 0, top: 0, width, height, ...style } }, children),
  Easing: {
    linear: (t) => t,
    ease: (t) => t * t * (3 - 2 * t),
    in: (fn) => (typeof fn === 'function' ? fn : (t) => t),
    out: (fn) => {
      const f = typeof fn === 'function' ? fn : (t) => t;
      return (t) => 1 - f(1 - t);
    },
    inOut: (fn) => {
      const f = typeof fn === 'function' ? fn : (t) => t;
      return (t) => (t < 0.5 ? f(2 * t) / 2 : 1 - f(2 * (1 - t)) / 2);
    },
    cubic: (t) => t * t * t,
    bezier: () => (t) => t,
  },
  interpolate(input, inputRange, outputRange, opts = {}) {
    const [inMin, inMax] = inputRange;
    const [outMin, outMax] = outputRange;
    let t = (input - inMin) / (inMax - inMin || 1);
    if (opts.extrapolateLeft === 'clamp' || opts.extrapolateRight === 'clamp') {
      t = Math.max(0, Math.min(1, t));
    }
    if (opts.easing) t = opts.easing(t);
    return outMin + (outMax - outMin) * t;
  },
  spring({ frame, fps: f, config = {} }) {
    const t = Math.max(0, frame / (f * 0.6));
    if (t >= 1) return 1;
    const damping = config.damping ?? 10;
    const decay = Math.exp(-damping * 0.6 * t);
    return 1 - decay * Math.cos(t * 12);
  },
};

const moduleObj = { exports: {} };
const fn = new Function('require', 'module', 'exports', '__h', '__Frag', transformed.code);
fn(
  (id) => {
    if (id === 'react') return reactShim;
    if (id === 'remotion') return remotionShim;
    throw new Error(`Unsupported import in TSX bridge: ${id}`);
  },
  moduleObj,
  moduleObj.exports,
  __h,
  __Frag
);

const Comp = moduleObj.exports.NvmTeaser || moduleObj.exports.default;

function parsePx(val, fallback = 0) {
  if (typeof val === 'number') return val;
  if (typeof val === 'string') {
    if (val.endsWith('%')) return (parseFloat(val) / 100) * width;
    const n = parseFloat(val);
    return Number.isNaN(n) ? fallback : n;
  }
  return fallback;
}

function normalizeColor(c) {
  if (!c || c === 'transparent' || c === 'none') return null;
  if (c.startsWith('#')) return c;
  const m = c.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)/);
  if (m) {
    const r = Number(m[1]).toString(16).padStart(2, '0');
    const g = Number(m[2]).toString(16).padStart(2, '0');
    const b = Number(m[3]).toString(16).padStart(2, '0');
    const a = m[4] !== undefined ? Math.round(Number(m[4]) * 255).toString(16).padStart(2, '0') : 'ff';
    return `#${r}${g}${b}${a}`;
  }
  return null;
}

function parsePadding(pad) {
  if (typeof pad === 'number') return pad;
  if (!pad) return 0;
  const nums = String(pad).match(/[\d.]+/g);
  if (!nums || nums.length === 0) return 0;
  return nums.reduce((a, b) => a + parseFloat(b), 0) / nums.length;
}

function parseTransform(tf) {
  let tx = 0, ty = 0, scale = 1;
  if (!tf || tf === 'none') return { tx, ty, scale };
  const s = String(tf);
  const mTx = s.match(/translateX\(([-\d.]+)px\)/);
  if (mTx) tx += parseFloat(mTx[1]);
  const mTy = s.match(/translateY\(([-\d.]+)px\)/);
  if (mTy) ty += parseFloat(mTy[1]);
  const mT = s.match(/translate\(([-\d.]+)px(?:,\s*([-\d.]+)px)?\)/);
  if (mT) {
    tx += parseFloat(mT[1]);
    if (mT[2]) ty += parseFloat(mT[2]);
  }
  const mSc = s.match(/scale\(([-\d.]+)\)/);
  if (mSc) scale = parseFloat(mSc[1]);
  return { tx, ty, scale };
}

function parseBorder(borderStr) {
  if (!borderStr || borderStr === 'none') return null;
  const m = String(borderStr).match(/([\d.]+)px\s+\w+\s+(#[0-9a-fA-F]{3,8}|rgba?\([^)]+\))/);
  if (!m) return null;
  const color = normalizeColor(m[2]);
  if (!color) return null;
  return { width: parseFloat(m[1]), color };
}

function parseLinearGradient(bgStr) {
  if (!bgStr || !String(bgStr).includes('linear-gradient')) return null;
  const colors = String(bgStr).match(/#[0-9a-fA-F]{6,8}|rgba?\([^)]+\)/g);
  if (!colors || colors.length < 2) return null;
  const from = normalizeColor(colors[0]);
  const to = normalizeColor(colors[colors.length - 1]);
  if (!from || !to) return null;
  const angleMatch = String(bgStr).match(/([\d.]+)deg/);
  const angle = angleMatch ? parseFloat(angleMatch[1]) : 180;
  return { from, to, angle };
}

function serializeSvg(node) {
  if (typeof node === 'string' || typeof node === 'number') return String(node);
  if (!node || typeof node !== 'object') return '';
  if (node.type === __Frag) return (node.children || []).map(serializeSvg).join('');
  const attrs = Object.entries(node.props || {})
    .filter(([k, v]) => k !== 'style' && k !== 'children' && v !== undefined && v !== null)
    .map(([k, v]) => {
      const attrName = k === 'viewBox' ? 'viewBox' : k.replace(/[A-Z]/g, (m) => '-' + m.toLowerCase());
      return `${attrName}="${String(v).replace(/"/g, '&quot;')}"`;
    })
    .join(' ');
  const inner = (node.children || []).map(serializeSvg).join('');
  const ns = node.type === 'svg' && !String(attrs).includes('xmlns=') ? ' xmlns="http://www.w3.org/2000/svg"' : '';
  return `<${node.type}${ns}${attrs ? ' ' + attrs : ''}>${inner}</${node.type}>`;
}

function toRustrenNode(node, startSec, endSec, parentOp) {
  if (!node || typeof node !== 'object') return null;
  if (typeof node.type === 'function') {
    return toRustrenNode(node.type({ ...node.props, children: node.children }), startSec, endSec, parentOp);
  }
  if (node.type === __Frag) {
    const kids = (node.children || []).map((c) => toRustrenNode(c, startSec, endSec, parentOp)).filter(Boolean);
    if (kids.length === 0) return null;
    if (kids.length === 1) return kids[0];
    return { type: 'container', start_sec: startSec, end_sec: endSec, x: 0, y: 0, width, height, layout: { direction: 'column', justify_content: 'center', align_items: 'center', gap: 16, padding: 0 }, children: kids };
  }

  const style = node.props?.style || {};
  const op = (style.opacity !== undefined ? Number(style.opacity) : 1) * parentOp;
  if (op <= 0.01) return null;

  if (node.type === 'svg') {
    const w = parsePx(node.props?.width ?? style.width, 120);
    const h = parsePx(node.props?.height ?? style.height, 120);
    return {
      type: 'svg',
      start_sec: startSec,
      end_sec: endSec,
      x: parsePx(style.left, 0),
      y: parsePx(style.top, 0),
      width: w,
      height: h,
      src: serializeSvg(node),
      opacity: op,
    };
  }

  const textParts = (node.children || []).filter((c) => typeof c === 'string' || typeof c === 'number').join('');
  const objChildren = (node.children || []).filter((c) => typeof c === 'object');

  const { tx, ty, scale } = parseTransform(style.transform);
  if (textParts.trim().length > 0 && objChildren.length === 0) {
    const color = normalizeColor(style.color) || '#eeeeee';
    const fontSize = parsePx(style.fontSize, 36) * scale;
    const textAlign = style.textAlign === 'center' ? 'center' : style.textAlign === 'right' ? 'right' : 'left';
    return {
      type: 'text',
      start_sec: startSec,
      end_sec: endSec,
      x: parsePx(style.left, 0) + tx,
      y: parsePx(style.top, 0) + ty,
      content: textParts,
      font_size: fontSize,
      font_weight: Number(style.fontWeight) >= 600 || style.fontWeight === 'bold' ? 'bold' : 'normal',
      text_align: textAlign,
      color,
      opacity: op,
      max_width: parsePx(style.width, width - 120),
    };
  }

  const pos = style.position === 'absolute' || style.inset === 0 ? 'absolute' : 'relative';
  const mappedKids = objChildren.map((c) => toRustrenNode(c, startSec, endSec, 1.0)).filter(Boolean);
  const bg = normalizeColor(style.backgroundColor);
  const gradient = parseLinearGradient(style.background || style.backgroundImage);
  const border = parseBorder(style.border);
  const w = parsePx(style.width, width) * scale;
  const h = (style.height !== undefined ? parsePx(style.height, height) : (style.top !== undefined || style.bottom !== undefined) ? 80 : height) * scale;
  const x = parsePx(style.left, 0) + tx;
  const y = (style.top !== undefined ? parsePx(style.top, 0) : style.bottom !== undefined ? height - parsePx(style.bottom, 0) - h : 0) + ty;
  const r = parsePx(style.borderRadius, 0);

  if (mappedKids.length === 0 && !bg && !gradient && !border) return null;
  if (mappedKids.length === 0) {
    return {
      type: 'rect',
      position: pos,
      start_sec: startSec,
      end_sec: endSec,
      x,
      y,
      width: w,
      height: h,
      radius: r,
      color: bg || '#00000000',
      gradient: gradient || undefined,
      border_color: border?.color,
      border_width: border?.width || 0,
      opacity: op,
    };
  }

  const dir = style.flexDirection === 'column' ? 'column' : (style.display === 'flex' || style.flexDirection === 'row') ? 'row' : 'column';
  const justify = style.justifyContent === 'center' ? 'center' : style.justifyContent === 'flex-end' ? 'end' : style.justifyContent === 'space-between' ? 'space_between' : 'start';
  const align = style.alignItems === 'center' ? 'center' : style.alignItems === 'flex-end' ? 'end' : 'start';

  return {
    type: 'container',
    position: pos,
    start_sec: startSec,
    end_sec: endSec,
    x,
    y,
    width: w,
    height: h,
    radius: r,
    background: bg || undefined,
    gradient: gradient || undefined,
    border_color: border?.color,
    border_width: border?.width || 0,
    opacity: op,
    layout: { direction: dir, justify_content: justify, align_items: align, gap: parsePx(style.gap, 16), padding: parsePadding(style.padding) },
    children: mappedKids,
  };
}

const layers = [];
for (let f = 0; f < totalFrames; f++) {
  currentFrame = f;
  memoIdx = 0;
  const tree = Comp({ styleMode: 'snappy' });
  const startSec = f / fps;
  const endSec = (f + 0.99) / fps;
  const root = toRustrenNode(tree, startSec, endSec, 1.0);
  if (root) layers.push(root);
}

const scene = {
  canvas: { width, height, fps, duration_secs: durationSecs, background: '#000000' },
  layers,
};

fs.writeFileSync(outJsonPath, JSON.stringify(scene));
console.log(`tsx_bridge: generated ${layers.length} layers across ${totalFrames} frames -> ${outJsonPath}`);
