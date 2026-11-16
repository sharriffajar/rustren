import fs from 'fs';

const inPath = process.argv[2];
const outPath = process.argv[3];
if (!inPath || !outPath) {
  console.error('Usage: node composition_to_rustren.mjs <input.json> <output.json>');
  process.exit(1);
}

const spec = JSON.parse(fs.readFileSync(inPath, 'utf8'));
const comp = spec.composition;
const fps = comp.fps || 30;
const totalFrames = comp.durationInFrames || 300;
const durationSecs = totalFrames / fps;
const width = comp.width || 1080;
const height = comp.height || 1920;

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

function mapEasing(e) {
  if (!e) return 'linear';
  if (e.includes('Expo') || e.includes('Out')) return 'ease_out';
  if (e.includes('InOut')) return 'ease_in_out';
  if (e.includes('In')) return 'ease_in';
  return 'linear';
}

function convertAnims(anims, seqFromFrame) {
  const out = [];
  if (!anims) return out;
  for (const a of anims) {
    const propMap = { opacity: 'opacity', translateX: 'x', translateY: 'y', scale: 'scale', widthPercent: 'width' };
    const prop = propMap[a.property];
    if (!prop) continue;

    if (a.from !== undefined && a.to !== undefined) {
      const delay = a.delay || 0;
      const startSec = (seqFromFrame + delay) / fps;
      const endSec = (seqFromFrame + delay + 18) / fps;
      out.push({
        property: prop,
        from: a.property === 'widthPercent' ? (a.from / 100) * width : a.from,
        to: a.property === 'widthPercent' ? (a.to / 100) * width : a.to,
        start_sec: startSec,
        end_sec: endSec,
        easing: a.spring ? 'spring' : mapEasing(a.easing),
      });
    } else if (a.inputRange && a.outputRange && a.inputRange.length >= 2) {
      for (let i = 0; i < a.inputRange.length - 1; i++) {
        out.push({
          property: prop,
          from: a.property === 'widthPercent' ? (a.outputRange[i] / 100) * width : a.outputRange[i],
          to: a.property === 'widthPercent' ? (a.outputRange[i + 1] / 100) * width : a.outputRange[i + 1],
          start_sec: (seqFromFrame + a.inputRange[i]) / fps,
          end_sec: (seqFromFrame + a.inputRange[i + 1]) / fps,
          easing: mapEasing(a.easing),
        });
      }
    }
  }
  return out;
}

function convertNode(node, seqFromFrame, seqEndSec, parentW = width, parentH = height) {
  if (!node) return null;
  const lay = node.layout || {};
  const st = node.style || {};
  const pos = lay.position === 'absolute' ? 'absolute' : 'relative';
  const w = lay.width === '100%' ? parentW : typeof lay.width === 'number' ? lay.width : parentW;
  const h = lay.height === '100%' ? parentH : typeof lay.height === 'number' ? lay.height : parentH;
  const x = lay.left || 0;
  const y = lay.top !== undefined ? lay.top : lay.bottom !== undefined ? parentH - lay.bottom - (typeof lay.height === 'number' ? lay.height : 80) : 0;
  const startSec = seqFromFrame / fps;
  const anims = convertAnims(node.animations, seqFromFrame);

  if (node.type === 'grid') {
    const cell = st.cellSize || 72;
    const stroke = normalizeColor(st.strokeColor) || '#172033';
    let lines = '';
    for (let gx = 0; gx <= width; gx += cell) lines += `<line x1="${gx}" y1="0" x2="${gx}" y2="${height}" stroke="${stroke}" stroke-width="${st.strokeWidth || 1}"/>`;
    for (let gy = 0; gy <= height; gy += cell) lines += `<line x1="0" y1="${gy}" x2="${width}" y2="${gy}" stroke="${stroke}" stroke-width="${st.strokeWidth || 1}"/>`;
    return {
      type: 'svg', position: 'absolute', start_sec: startSec, end_sec: seqEndSec,
      x: 0, y: 0, width, height, opacity: st.opacity ?? 1.0,
      src: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}">${lines}</svg>`,
    };
  }

  if (node.type === 'path') {
    const stroke = normalizeColor(st.stroke?.color) || '#22D3EE';
    const sw = st.stroke?.width || 4;
    return {
      type: 'svg', position: pos, start_sec: startSec, end_sec: seqEndSec,
      x, y, width: w, height: h, opacity: st.opacity ?? 1.0,
      src: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}"><path d="${node.d}" stroke="${stroke}" stroke-width="${sw}" fill="none" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
    };
  }

  if (node.type === 'circle') {
    const r = (typeof lay.width === 'number' ? lay.width : 52) / 2;
    const fill = typeof st.fill === 'string' ? normalizeColor(st.fill) : st.fill?.stops?.[0]?.color ? normalizeColor(st.fill.stops[0].color) : '#22D3EE';
    return {
      type: 'circle', position: pos, start_sec: startSec, end_sec: seqEndSec,
      cx: x + r, cy: y + r, r, color: fill || '#22D3EE', opacity: st.opacity ?? 1.0, animations: anims,
    };
  }

  if (node.type === 'rect') {
    const fill = typeof st.fill === 'string' ? normalizeColor(st.fill) : '#22D3EE';
    const grad = st.fill?.type === 'linear-gradient' && st.fill.stops?.length >= 2 ? {
      from: normalizeColor(st.fill.stops[0].color) || '#22D3EE',
      to: normalizeColor(st.fill.stops[st.fill.stops.length - 1].color) || '#818CF8',
      angle: st.fill.angle || 90,
    } : undefined;
    return {
      type: 'rect', position: pos, start_sec: startSec, end_sec: seqEndSec,
      x, y, width: w, height: h, radius: st.borderRadius || 0,
      color: fill || '#22D3EE', gradient: grad, opacity: st.opacity ?? 1.0, animations: anims,
    };
  }

  if (node.type === 'text') {
    return {
      type: 'text', position: pos, start_sec: startSec, end_sec: seqEndSec,
      x, y, content: node.content || '',
      font_size: st.fontSize || 32,
      font_weight: (st.fontWeight || 400) >= 600 ? 'bold' : 'normal',
      text_align: st.textAlign || 'left',
      color: normalizeColor(st.color) || '#F8FAFC',
      opacity: st.opacity ?? 1.0,
      max_width: w,
      animations: anims,
    };
  }

  if (node.type === 'container') {
    const pad = lay.padding ? (typeof lay.padding === 'number' ? lay.padding : (lay.padding.left || 0)) : 0;
    const bg = typeof st.fill === 'string' ? normalizeColor(st.fill) : undefined;
    const grad = st.fill?.type === 'linear-gradient' && st.fill.stops?.length >= 2 ? {
      from: normalizeColor(st.fill.stops[0].color) || '#101726',
      to: normalizeColor(st.fill.stops[st.fill.stops.length - 1].color) || '#0F2238',
      angle: st.fill.angle || 135,
    } : undefined;
    const children = (node.children || []).map((c) => convertNode(c, seqFromFrame, seqEndSec, w - pad * 2, h)).filter(Boolean);
    return {
      type: 'container', position: pos, start_sec: startSec, end_sec: seqEndSec,
      x, y, width: w, height: h, radius: st.borderRadius || 0,
      background: bg, gradient: grad,
      border_color: st.stroke?.color ? normalizeColor(st.stroke.color) : undefined,
      border_width: st.stroke?.width || 0,
      opacity: st.opacity ?? 1.0,
      layout: {
        direction: lay.flexDirection === 'row' ? 'row' : 'column',
        justify_content: lay.justifyContent === 'center' ? 'center' : lay.justifyContent === 'space-between' ? 'space_between' : 'start',
        align_items: lay.alignItems === 'center' ? 'center' : 'start',
        gap: lay.gap || 16,
        padding: pad,
      },
      animations: anims,
      children,
    };
  }
  return null;
}

const layers = [];
for (const g of spec.globalLayers || []) {
  const n = convertNode(g, 0, durationSecs, width, height);
  if (n) layers.push(n);
}
for (const seq of spec.sequences || []) {
  const seqEndSec = (seq.from + seq.durationInFrames) / fps;
  const n = convertNode(seq.root, seq.from, seqEndSec, width, height);
  if (n) layers.push(n);
}

const outScene = {
  canvas: { width, height, fps, duration_secs: durationSecs, background: comp.backgroundColor || '#070B14' },
  layers,
};
fs.writeFileSync(outPath, JSON.stringify(outScene, null, 2));
console.log(`composition_to_rustren: converted ${layers.length} root layers -> ${outPath}`);
