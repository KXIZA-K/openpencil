// Conservative device-space backdrop knowledge shared by editor and player.
// This never reads GPU pixels. Unsupported paint only discards knowledge.
const valid = b => b && Object.values(b).every(Number.isFinite) && b.left < b.right && b.top < b.bottom;
const intersects = (a, b) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
const contains = (a, b) => a.left <= b.left && a.top <= b.top && a.right >= b.right && a.bottom >= b.bottom;
const axisAligned = m => [...m].every(Number.isFinite) && m[0] > 0 && m[4] > 0 && m[1] === 0 && m[3] === 0 && m[6] === 0 && m[7] === 0 && m[8] === 1;
const color = (r, g, b, a) => a === 1 && [r, g, b].every(v => Number.isFinite(v) && v >= 0 && v <= 1)
  ? `rgb(${Math.round(r * 255)},${Math.round(g * 255)},${Math.round(b * 255)})` : null;

export function createBackdropTracker(CK, el) {
  let paints = [], blocked = false, clip = null, pending = null, raw = null;
  const stack = [];
  const forget = () => { paints = []; };
  const paint = (bounds, fill = null, regions = null, legacy = null) => {
    if (!valid(bounds)) { forget(); return; }
    // A query hitting a contained older record hits this newer record first.
    // Removing fully superseded records preserves last-paint query semantics.
    paints = paints.filter(previous => !contains(bounds, previous.bounds));
    if (paints.length >= 512) forget();
    paints.push({ bounds: { ...bounds }, fill, regions, legacy });
  };
  const intersectClip = b => clip ? {
    left: Math.max(b.left, clip.left), top: Math.max(b.top, clip.top),
    right: Math.min(b.right, clip.right), bottom: Math.min(b.bottom, clip.bottom),
  } : b;
  const bounds = (x, y, w, h, inset = 0) => {
    const m = raw.getTotalMatrix();
    if (blocked || !axisAligned(m) || ![x, y, w, h, inset].every(Number.isFinite) || w <= 2 * inset || h <= 2 * inset) return null;
    return { left: m[0] * (x + inset) + m[2], top: m[4] * (y + inset) + m[5],
      right: m[0] * (x + w - inset) + m[2], bottom: m[4] * (y + h - inset) + m[5] };
  };
  return {
    wrap(canvas) {
      // A replacement surface has neither old pixels nor old save/clip state.
      forget(); stack.length = 0; blocked = false; clip = null; pending = null; raw = canvas;
      return new Proxy(canvas, { get(target, key) {
        const value = target[key];
        if (typeof value !== 'function') return value;
        return (...args) => {
          const name = String(key);
          if (name === 'save' || name === 'saveLayer') {
            stack.push({ blocked, clip });
            if (name === 'saveLayer') { blocked = true; forget(); }
          } else if (name === 'restore') {
            const old = stack.pop() || { blocked: false, clip: null };
            if (blocked) forget();
            blocked = old.blocked; clip = old.clip;
          } else if ((name === 'clipRect' || name === 'clipRRect') && args[1] === CK.ClipOp.Intersect) {
            const m = target.getTotalMatrix(), r = args[0];
            const rx = name === 'clipRRect' ? Math.max(r[4], r[6], r[8], r[10]) : 0;
            const ry = name === 'clipRRect' ? Math.max(r[5], r[7], r[9], r[11]) : 0;
            if (axisAligned(m) && [...r, rx, ry].every(Number.isFinite)) {
              clip = intersectClip({ left: Math.ceil(m[0] * (r[0] + rx) + m[2]), top: Math.ceil(m[4] * (r[1] + ry) + m[5]),
                right: Math.floor(m[0] * (r[2] - rx) + m[2]), bottom: Math.floor(m[4] * (r[3] - ry) + m[5]) });
            } else { blocked = true; forget(); }
          } else if (name.startsWith('clip')) {
            blocked = true; forget();
          } else if (name.startsWith('draw') || ['clear', 'writePixels', 'discard'].includes(name)) {
            if (pending && !blocked) paint({ left: Math.floor(pending.left) - 1, top: Math.floor(pending.top) - 1,
              right: Math.ceil(pending.right) + 1, bottom: Math.ceil(pending.bottom) + 1 });
            else forget();
            pending = null;
          }
          return value.apply(target, args);
        };
      } });
    },
    prepare(x, y, w, h) { pending = bounds(x, y, w, h); },
    fill(x, y, w, h, inset, r, g, b, a) {
      const fill = color(r, g, b, a);
      if (!fill || !Number.isFinite(inset) || inset < 0 || inset > Math.min(w, h) / 2) return;
      const regions = [];
      const add = (box, margin = 0) => {
        if (!box) return;
        const clipped = intersectClip({ left: Math.ceil(box.left + margin), top: Math.ceil(box.top + margin),
          right: Math.floor(box.right - margin), bottom: Math.floor(box.bottom - margin) });
        if (valid(clipped)) regions.push(clipped);
      };
      // Preserve the original vertical-strip proof for color/emoji runs.
      add(bounds(x + inset, y, w - 2 * inset, h));
      const legacy = regions[0] || null;
      if (inset > 0) {
        const cut = inset * (1 - Math.SQRT1_2);
        // One extra device pixel keeps curve antialiasing outside the proof.
        add(bounds(x + cut, y + cut, w - 2 * cut, h - 2 * cut), 1);
      }
      // Circles previously had only their unknown draw record. Attach optional
      // proof there rather than doubling record pressure and evicting early.
      if (!legacy && regions.length && paints.length && !paints[paints.length - 1].fill) {
        Object.assign(paints[paints.length - 1], { fill, regions, legacy: null });
        return;
      }
      if (regions.length) paint({ left: Math.min(...regions.map(b => b.left)), top: Math.min(...regions.map(b => b.top)),
        right: Math.max(...regions.map(b => b.right)), bottom: Math.max(...regions.map(b => b.bottom)) }, fill, regions, legacy);
    },
    clear(r, g, b, a) {
      forget();
      const fill = color(r, g, b, a);
      if (fill && !blocked) paint(intersectClip({ left: 0, top: 0, right: el.width, bottom: el.height }), fill);
    },
    colorAt(box, allowRounded = true) {
      if (blocked || !valid(box) || (clip && !contains(clip, box))) return null;
      for (let i = paints.length - 1; i >= 0; i--) {
        const stored = paints[i];
        const p = !allowRounded && stored.regions ? { ...stored, regions: stored.legacy ? [stored.legacy] : [] } : stored;
        if (intersects(p.bounds, box)) return p.fill && (p.regions ? p.regions.some(region => contains(region, box)) : contains(p.bounds, box)) ? p.fill : null;
      }
      return null;
    },
  };
}

// Use the browser's opaque text path only when the complete padded bitmap is
// proven flat and the canvas is at native CSS/DPR scale. Other transforms keep
// the transparent path; no geometry, font metrics or layout is adjusted.
export function textBackdrop(tracker, el, matrix, metrics, sz, ss, x, y, phaseX, phaseY, allowRounded = true) {
  const dpr = globalThis.devicePixelRatio;
  if (!axisAligned(matrix) || matrix[0] !== dpr || matrix[4] !== dpr
    || Math.abs(el.width / el.clientWidth - dpr) >= .001 || Math.abs(el.height / el.clientHeight - dpr) >= .001) return null;
  const left = 2 + Math.ceil(Math.max(0, Math.ceil(metrics.actualBoundingBoxLeft || 0)) * ss) / ss;
  const baseline = Math.ceil(metrics.actualBoundingBoxAscent || sz * .8) + 2;
  const width = Math.ceil(Math.max(1, Math.ceil(left + Math.max(metrics.width, metrics.actualBoundingBoxRight || 0) + 2)) * ss);
  const height = Math.ceil(Math.max(1, baseline + Math.ceil(metrics.actualBoundingBoxDescent || sz * .25) + 2) * ss);
  const px = Math.round(matrix[0] * (x - left - phaseX) + matrix[2]);
  const py = Math.round(matrix[4] * (y - baseline - phaseY) + matrix[5]);
  if (px < 0 || py < 0 || px + width > el.width || py + height > el.height || width * height > 65536) return null;
  return tracker.colorAt({ left: px, top: py, right: px + width, bottom: py + height }, allowRounded);
}

// Inspect only our already-rasterized CPU bitmap, once per cache miss. Blank
// opaque pixels equal the proven flat backdrop; transparent pixels paint nothing.
// Readback failure and excessive work retain conservative full-image coverage.
export function textInkPixels(ctx, bitmap, backdropColor) {
  const w = bitmap.width, h = bitmap.height;
  if (w * h > 65536 || w <= 0 || h <= 0) return null;
  const match = backdropColor?.match(/^rgb\((\d+),\s*(\d+),\s*(\d+)\)$/);
  if (backdropColor && !match) return null;
  const bg = match ? match.slice(1).map(Number) : null;
  let pixels;
  try { pixels = ctx.getImageData(0, 0, w, h).data; } catch { return null; }
  let left = w, top = h, right = 0, bottom = 0;
  for (let py = 0; py < h; py++) for (let px = 0; px < w; px++) {
    const i = (py * w + px) * 4;
    const changed = bg ? pixels[i] !== bg[0] || pixels[i + 1] !== bg[1]
      || pixels[i + 2] !== bg[2] || pixels[i + 3] !== 255 : pixels[i + 3] !== 0;
    if (changed) {
      left = Math.min(left, px); top = Math.min(top, py);
      right = Math.max(right, px + 1); bottom = Math.max(bottom, py + 1);
    }
  }
  return right > left && bottom > top ? { left, top, right, bottom } : null;
}

export function prepareTextFootprint(tracker, el, matrix, entry, x, y) {
  const ss = entry.ss || 1, left = x - entry.left, top = y - entry.baseline;
  const px = matrix[0] * left + matrix[2], py = matrix[4] * top + matrix[5];
  const exact = axisAligned(matrix) && matrix[0] === ss && matrix[4] === ss
    && ss === globalThis.devicePixelRatio
    && Math.abs(px - Math.round(px)) < 1e-7 && Math.abs(py - Math.round(py)) < 1e-7
    && Math.abs(el.width / el.clientWidth - ss) < .001
    && Math.abs(el.height / el.clientHeight - ss) < .001;
  const b = exact && entry.inkPixels;
  if (b) tracker.prepare(left + b.left / ss, top + b.top / ss,
    (b.right - b.left) / ss, (b.bottom - b.top) / ss);
  else tracker.prepare(left, top, entry.image.width() / ss, entry.image.height() / ss);
}
