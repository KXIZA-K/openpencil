// ../../platform/lib/prototype-source-fonts.ts
var MAX_FONT_BYTES = 2 * 1024 * 1024;
var MAX_TOTAL_BYTES = 4 * 1024 * 1024;
async function loadPrototypeSourceFont(font) {
  if (!/^data:(?:font\/(?:ttf|otf|woff|woff2)|application\/(?:font-woff|x-font-ttf|x-font-opentype));base64,[A-Za-z0-9+/]+={0,2}$/i.test(font.dataUrl) || font.dataUrl.length > MAX_FONT_BYTES * 4 / 3 + 100)
    throw Error("Font loader requires a bounded embedded data source");
  return new FontFace(font.family, `url("${font.dataUrl}")`, {
    weight: font.weight,
    style: font.style,
    stretch: font.stretch,
    unicodeRange: font.unicodeRange
  }).load();
}

// ../../platform/lib/prototype-font-session.ts
function createPrototypeFontSession(target = document.fonts, decode = loadPrototypeSourceFont) {
  let generation = 0, owned = [], disposed = false;
  return {
    cancelPending() {
      generation++;
    },
    async replace(fonts) {
      if (disposed)
        throw Error("Font session is disposed");
      if (fonts.length > 64)
        throw Error("Too many checkpoint font faces");
      const current = ++generation;
      const next = await Promise.all(fonts.map((font) => decode(font)));
      if (disposed || current !== generation)
        return false;
      const added = [];
      try {
        for (const font of next) {
          target.add(font);
          added.push(font);
        }
      } catch (error) {
        for (const font of added)
          target.delete(font);
        throw error;
      }
      for (const font of owned)
        target.delete(font);
      owned = next;
      return true;
    },
    dispose() {
      disposed = true;
      generation++;
      for (const font of owned)
        target.delete(font);
      owned = [];
    }
  };
}

// ../../platform/lib/prototype-native-font-runtime.ts
var session;
var families = new Set;
var fontGeneration = 0;
var requestGeneration = 0;
var disposed = false;
var hasSourceFontFamily = (name) => families.has(name.trim().toLowerCase());
var sourceFontsGeneration = () => fontGeneration;
async function installSourceFonts(raw) {
  const current = ++requestGeneration;
  session?.cancelPending();
  if (disposed)
    throw Error("Font runtime is disposed");
  if (raw.length > 6 * 1024 * 1024)
    throw Error("Font manifest too large");
  const fonts = JSON.parse(raw);
  if (!Array.isArray(fonts) || fonts.length > 64)
    throw Error("Invalid font manifest");
  let bytesTotal = 0;
  for (const font of fonts) {
    if (!font || typeof font !== "object" || ["family", "weight", "style", "stretch", "unicodeRange"].some((k) => typeof font[k] !== "string" || !font[k] || font[k].length > (k === "unicodeRange" ? 2000 : 160)) || typeof font.dataUrl !== "string" || !/^data:(?:font\/(?:ttf|otf|woff|woff2)|application\/(?:font-woff|x-font-ttf|x-font-opentype));base64,[A-Za-z0-9+/]+={0,2}$/i.test(font.dataUrl))
      throw Error("Invalid font descriptor");
    const encoded = font.dataUrl.slice(font.dataUrl.indexOf(",") + 1), binary = atob(encoded);
    bytesTotal += binary.length;
    if (btoa(binary) !== encoded || binary.length > 2 * 1024 * 1024 || bytesTotal > 4 * 1024 * 1024)
      throw Error("Invalid font budget");
    const digest = await crypto.subtle.digest("SHA-256", Uint8Array.from(binary, (c) => c.charCodeAt(0)));
    if (Array.from(new Uint8Array(digest), (n) => n.toString(16).padStart(2, "0")).join("") !== font.sha256)
      throw Error("Font integrity mismatch");
  }
  if (disposed || current !== requestGeneration)
    return false;
  session ??= createPrototypeFontSession();
  const installed = await session.replace(fonts);
  if (!installed)
    return false;
  families = new Set(fonts.map((font) => font.family.trim().toLowerCase()));
  fontGeneration++;
  return true;
}
if (typeof addEventListener === "function")
  addEventListener("pagehide", () => {
    disposed = true;
    requestGeneration++;
    session?.dispose();
    families.clear();
    fontGeneration++;
  }, { once: true });
export {
  sourceFontsGeneration,
  installSourceFonts,
  hasSourceFontFamily
};
