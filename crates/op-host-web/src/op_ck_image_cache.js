// CanvasKit image residency and thumbnail drawing.
//
// Full document images and blur-up thumbnails intentionally use independent
// LRU budgets. A large document therefore cannot evict every thumbnail, and
// a thumbnail-heavy document cannot consume the full-resolution raster budget.

const FULL_IMAGE_CACHE_CAP = 4096;
const FULL_IMAGE_CACHE_BYTE_BUDGET = 384 * 1024 * 1024;
const THUMBNAIL_CACHE_CAP = 1024;
const THUMBNAIL_CACHE_BYTE_BUDGET = 4 * 1024 * 1024;
const THUMBNAIL_ENCODED_BYTE_LIMIT = 4 * 1024;
const THUMBNAIL_MAX_EDGE = 32;
const THUMBNAIL_FAILURE_CAP = 1024;
const JPEG_SOF_MARKERS = new Set([
  0xc0, 0xc1, 0xc2, 0xc3, 0xc5, 0xc6, 0xc7,
  0xc9, 0xca, 0xcb, 0xcd, 0xce, 0xcf,
]);

// Read JPEG SOF metadata before CanvasKit expands pixels. The persisted table
// is untrusted input: a very compressible, multi-megapixel JPEG can fit under
// the 4 KiB encoded limit but is not safe to decode synchronously in paint.
const jpegFitsThumbnailBounds = (jpeg) => {
  if (!jpeg || jpeg.byteLength < 4 || jpeg[0] !== 0xff || jpeg[1] !== 0xd8) {
    return false;
  }
  let offset = 2;
  while (offset < jpeg.byteLength) {
    while (offset < jpeg.byteLength && jpeg[offset] === 0xff) offset += 1;
    if (offset >= jpeg.byteLength) return false;
    const marker = jpeg[offset++];
    if (marker === 0xd9 || marker === 0xda) return false;
    if (marker === 0x01 || marker === 0xd8 || (marker >= 0xd0 && marker <= 0xd7)) {
      continue;
    }
    if (offset + 1 >= jpeg.byteLength) return false;
    const length = (jpeg[offset] << 8) | jpeg[offset + 1];
    if (length < 2 || offset + length > jpeg.byteLength) return false;
    if (JPEG_SOF_MARKERS.has(marker)) {
      if (length < 7) return false;
      const height = (jpeg[offset + 3] << 8) | jpeg[offset + 4];
      const width = (jpeg[offset + 5] << 8) | jpeg[offset + 6];
      return (
        width > 0 &&
        height > 0 &&
        width <= THUMBNAIL_MAX_EDGE &&
        height <= THUMBNAIL_MAX_EDGE
      );
    }
    offset += length;
  }
  return false;
};

const imageKey = (lo, hi) => String(hi >>> 0) + ':' + String(lo >>> 0);

const copyBytes = (bytes) =>
  bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);

const decodedRasterBytes = (image) => {
  const width = image.width();
  const height = image.height();
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
    return 0;
  }
  return width * height * 4;
};

const deleteImage = (image) => {
  if (image && image.delete) image.delete();
};

// SVG has no magic bytes — sniff the markup (first 4 KiB decode as text and
// contain an `<svg` tag). Mirrors the native seam's sniff in
// `op-editor-ui/.../svg_raster.rs`.
const sniffsAsSvg = (bytes) => {
  try {
    const head = new TextDecoder().decode(
      bytes.subarray(0, Math.min(bytes.byteLength, 4096)),
    );
    const trimmed = head.replace(/^﻿/, '').trimStart();
    return trimmed.startsWith('<') && trimmed.includes('<svg');
  } catch (_error) {
    return false;
  }
};

const SVG_RASTER_MAX_EDGE = 2048;
const SVG_RASTER_SCALE = 2;
const SVG_FAILURE_CAP = 1024;

export function createWebImageCaches(CK, onImageReady = () => {}) {
  const fullImageCache = new Map();
  let fullImageCacheBytes = 0;
  const thumbnailCache = new Map();
  let thumbnailCacheBytes = 0;
  const thumbnailFailures = new Set();
  // SVG rasterizations in flight / permanently failed. Pending keys report
  // decode success to the Rust side so the id is not negative-cached; paint
  // keeps re-requesting until the browser's async decode lands the raster
  // in `fullImageCache`.
  const svgPending = new Set();
  const svgRequestedEdges = new Map();
  const svgFailures = new Set();

  const rememberSvgFailure = (key) => {
    if (svgFailures.size >= SVG_FAILURE_CAP) {
      svgFailures.delete(svgFailures.values().next().value);
    }
    svgFailures.add(key);
  };

  const installRasterizedSvg = (key, img) => {
    const sourceW = img.naturalWidth || img.width;
    const sourceH = img.naturalHeight || img.height;
    if (!(sourceW > 0) || !(sourceH > 0)) return false;
    const sourceEdge = Math.max(sourceW, sourceH);
    const targetEdge = Math.min(SVG_RASTER_MAX_EDGE,
      Math.max(sourceEdge * SVG_RASTER_SCALE, svgRequestedEdges.get(key) || 0));
    const scale = targetEdge / sourceEdge;
    if (!(scale > 0)) return false;
    const width = Math.max(1, Math.round(sourceW * scale));
    const height = Math.max(1, Math.round(sourceH * scale));
    const surface = document.createElement('canvas');
    surface.width = width;
    surface.height = height;
    const context = surface.getContext('2d');
    if (!context) return false;
    context.drawImage(img, 0, 0, width, height);
    const pixels = context.getImageData(0, 0, width, height);
    if (!CK.MakeImage) return false;
    const image = CK.MakeImage(
      {
        width,
        height,
        colorType: CK.ColorType.RGBA_8888,
        alphaType: CK.AlphaType.Unpremul,
        colorSpace: CK.ColorSpace.SRGB,
      },
      pixels.data,
      4 * width,
    );
    if (!image) return false;
    const bytes = decodedRasterBytes(image);
    if (!(bytes > 0)) {
      deleteImage(image);
      return false;
    }
    const previous = fullImageCache.get(key);
    if (previous) {
      fullImageCacheBytes -= previous.bytes;
      deleteImage(previous.image);
    }
    fullImageCache.set(key, {
      image,
      bytes,
      svg: true,
      coversEdgePx: Math.max(width, height),
    });
    fullImageCacheBytes += bytes;
    evictFullImages();
    return fullImageCache.has(key);
  };

  // The browser is the SVG decoder CanvasKit lacks: load the markup into an
  // `<img>` (async), draw it onto a 2d canvas, and install the pixels as an
  // ordinary CanvasKit image. resvg does this on native; carrying it into
  // the wasm bundle would bust the 6 MiB ceiling for a codec the platform
  // already ships.
  const startSvgRaster = (key, encoded, maxEdgePx) => {
    // Coalesce zoom changes while the browser decodes the vector. Rasterize
    // at the largest requested edge, but keep allocations bounded.
    const requested = Number.isFinite(maxEdgePx) && maxEdgePx > 0
      ? Math.min(SVG_RASTER_MAX_EDGE, maxEdgePx) : 0;
    svgRequestedEdges.set(key, Math.max(svgRequestedEdges.get(key) || 0, requested));
    if (svgPending.has(key)) return;
    svgPending.add(key);
    let url = null;
    const settle = (installed) => {
      if (url) URL.revokeObjectURL(url);
      svgPending.delete(key);
      svgRequestedEdges.delete(key);
      if (!installed) rememberSvgFailure(key);
      // Async completion must wake an idle renderer; it is not a document edit.
      onImageReady();
    };
    try {
      const blob = new Blob([copyBytes(encoded)], { type: 'image/svg+xml' });
      url = URL.createObjectURL(blob);
      const img = new Image();
      img.onload = () => {
        try {
          settle(installRasterizedSvg(key, img));
        } catch (_error) {
          settle(false);
        }
      };
      img.onerror = () => settle(false);
      img.src = url;
    } catch (_error) {
      settle(false);
    }
  };

  const evictFullImages = () => {
    while (
      fullImageCache.size > FULL_IMAGE_CACHE_CAP ||
      fullImageCacheBytes > FULL_IMAGE_CACHE_BYTE_BUDGET
    ) {
      const oldestKey = fullImageCache.keys().next().value;
      const oldest = fullImageCache.get(oldestKey);
      if (!oldest) break;
      fullImageCache.delete(oldestKey);
      fullImageCacheBytes -= oldest.bytes;
      deleteImage(oldest.image);
    }
  };

  const evictThumbnails = () => {
    while (
      thumbnailCache.size > THUMBNAIL_CACHE_CAP ||
      thumbnailCacheBytes > THUMBNAIL_CACHE_BYTE_BUDGET
    ) {
      const oldestKey = thumbnailCache.keys().next().value;
      const oldest = thumbnailCache.get(oldestKey);
      if (!oldest) break;
      thumbnailCache.delete(oldestKey);
      thumbnailCacheBytes -= oldest.bytes;
      deleteImage(oldest.image);
    }
  };

  const rememberThumbnailFailure = (key) => {
    if (thumbnailFailures.has(key)) return;
    if (thumbnailFailures.size >= THUMBNAIL_FAILURE_CAP) {
      thumbnailFailures.delete(thumbnailFailures.values().next().value);
    }
    thumbnailFailures.add(key);
  };

  // A cached raster satisfies a draw only when it is at least as sharp
  // as the view needs. Rastering every visible image at its authored
  // size is what made a zoomed-out image-dense page evict and re-decode
  // continuously; sizing to the view keeps hundreds of thumbnails cheap.
  const hasFullImage = (lo, hi, maxEdgePx) => {
    const key = imageKey(lo, hi);
    const hit = fullImageCache.get(key);
    if (!hit) return false;
    if (!(maxEdgePx > 0)) return true;
    // A failed upgrade must not discard a previously usable raster. Above
    // the SVG safety ceiling, reuse the bounded image instead of decode loops.
    if (hit.svg && svgFailures.has(key)) return true;
    return hit.coversEdgePx >= (hit.svg ? Math.min(maxEdgePx, SVG_RASTER_MAX_EDGE) : maxEdgePx);
  };

  const fullImage = (lo, hi) => {
    const key = imageKey(lo, hi);
    const hit = fullImageCache.get(key);
    if (!hit) return null;
    fullImageCache.delete(key);
    fullImageCache.set(key, hit);
    return hit.image;
  };

  // Raster images decode at the source's own size. Sizing the
  // raster to the view (as the native host does) needs a surface
  // round-trip whose snapshot aliases the surface's pixels, so freeing
  // the surface would dangle the cached image — deferred rather than
  // shipped unsafely. A full raster serves every requested size, so it
  // records itself as covering any edge.
  const installFullImage = (lo, hi, encoded, maxEdgePx) => {
    const key = imageKey(lo, hi);
    if (hasFullImage(lo, hi, maxEdgePx)) return true;
    if (fullImageCache.get(key)?.svg) {
      startSvgRaster(key, encoded, maxEdgePx);
      return true;
    }

    let image = null;
    try {
      image = CK.MakeImageFromEncoded(copyBytes(encoded));
      if (!image) {
        // CanvasKit has no SVG codec — hand the markup to the browser's own
        // decoder. Reporting `true` while the async raster is in flight
        // keeps the id off the Rust side's permanent-failure cache; paint
        // re-requests each frame until the raster lands (or the failure is
        // remembered here and this returns false for good).
        if (svgFailures.has(key)) return false;
        if (sniffsAsSvg(encoded)) {
          startSvgRaster(key, encoded, maxEdgePx);
          return true;
        }
        return false;
      }
      const bytes = decodedRasterBytes(image);
      if (!(bytes > 0)) {
        deleteImage(image);
        return false;
      }
      fullImageCache.set(key, {
        image,
        bytes,
        coversEdgePx: Number.MAX_SAFE_INTEGER,
      });
      fullImageCacheBytes += bytes;
      evictFullImages();
      return fullImageCache.has(key);
    } catch (_error) {
      deleteImage(image);
      return false;
    }
  };

  const thumbnailImage = (lo, hi, jpeg) => {
    const key = imageKey(lo, hi);
    const hit = thumbnailCache.get(key);
    if (hit) {
      thumbnailCache.delete(key);
      thumbnailCache.set(key, hit);
      return hit.image;
    }
    if (thumbnailFailures.has(key)) return null;
    if (!jpeg || jpeg.byteLength > THUMBNAIL_ENCODED_BYTE_LIMIT) {
      rememberThumbnailFailure(key);
      return null;
    }
    if (!jpegFitsThumbnailBounds(jpeg)) {
      rememberThumbnailFailure(key);
      return null;
    }

    let image = null;
    try {
      image = CK.MakeImageFromEncoded(copyBytes(jpeg));
      if (!image) {
        rememberThumbnailFailure(key);
        return null;
      }
      const bytes = decodedRasterBytes(image);
      if (!(bytes > 0) || bytes > THUMBNAIL_CACHE_BYTE_BUDGET) {
        deleteImage(image);
        rememberThumbnailFailure(key);
        return null;
      }
      thumbnailCache.set(key, { image, bytes });
      thumbnailCacheBytes += bytes;
      evictThumbnails();
      return image;
    } catch (_error) {
      deleteImage(image);
      rememberThumbnailFailure(key);
      return null;
    }
  };

  const discardThumbnail = (key) => {
    const entry = thumbnailCache.get(key);
    if (!entry) return;
    thumbnailCache.delete(key);
    thumbnailCacheBytes -= entry.bytes;
    deleteImage(entry.image);
  };

  const drawThumbnailCover = (canvas, lo, hi, jpeg, x, y, w, h) => {
    if (!(w > 0) || !(h > 0)) return false;
    const image = thumbnailImage(lo, hi, jpeg);
    if (!image) return false;

    const imageW = image.width();
    const imageH = image.height();
    if (!(imageW > 0) || !(imageH > 0)) return false;
    const scale = Math.max(w / imageW, h / imageH);
    const drawW = imageW * scale;
    const drawH = imageH * scale;
    const src = CK.LTRBRect(0, 0, imageW, imageH);
    const dst = CK.LTRBRect(
      x + (w - drawW) / 2,
      y + (h - drawH) / 2,
      x + (w + drawW) / 2,
      y + (h + drawH) / 2,
    );
    const clip = CK.LTRBRect(x, y, x + w, y + h);
    const paint = new CK.Paint();
    paint.setAntiAlias(true);

    let saved = false;
    try {
      canvas.save();
      saved = true;
      canvas.clipRect(clip, CK.ClipOp.Intersect, true);
      if (canvas.drawImageRectOptions) {
        canvas.drawImageRectOptions(
          image,
          src,
          dst,
          CK.FilterMode.Linear,
          CK.MipmapMode.None,
          paint,
        );
      } else {
        canvas.drawImageRect(image, src, dst, paint, false);
      }
    } catch (_error) {
      const key = imageKey(lo, hi);
      discardThumbnail(key);
      rememberThumbnailFailure(key);
      return false;
    } finally {
      if (saved) canvas.restore();
      paint.delete();
    }
    return true;
  };

  return {
    hasFullImage,
    fullImage,
    installFullImage,
    drawThumbnailCover,
  };
}

// wasm-bindgen only copies local modules named by Rust attributes; it does not
// recursively package relative imports from another local module. Rust obtains
// this function object and passes it into the bridge during initialization.
export function webImageCacheFactory() {
  return createWebImageCaches;
}
