use super::{contain_rect, layer::to_skia_blend_mode, to_sk_rect, NativeBackend};
use op_editor_ui::{ImageAdjustments, ImageBlendMode, ImageDrawMode, Point2D, Rect};

const THUMB_CACHE_BYTE_BUDGET: usize = 4 * 1024 * 1024;
const THUMB_CACHE_MAX_ENTRIES: usize = 4096;
const THUMB_ENCODED_BYTE_LIMIT: usize = 4 * 1024;
const THUMB_MAX_EDGE: i32 = 32;

#[derive(Default)]
pub(super) struct ThumbCache {
    entries: std::collections::HashMap<u64, ThumbCacheEntry>,
    bytes: usize,
    tick: u64,
}

struct ThumbCacheEntry {
    image: Option<skia_safe::Image>,
    bytes: usize,
    last_used: u64,
}

/// Aspect-cover (`fill` / `crop`) `img_w × img_h` over `outer`, centered.
/// The result fully covers `outer`; the caller clips to `outer`.
pub(super) fn cover_rect(outer: Rect, img_w: f32, img_h: f32) -> Rect {
    if img_w <= 0.0 || img_h <= 0.0 || outer.size.x <= 0.0 || outer.size.y <= 0.0 {
        return outer;
    }
    let scale = (outer.size.x / img_w).max(outer.size.y / img_h);
    let w = img_w * scale;
    let h = img_h * scale;
    Rect {
        origin: Point2D::new(
            outer.origin.x + (outer.size.x - w) / 2.0,
            outer.origin.y + (outer.size.y - h) / 2.0,
        ),
        size: Point2D::new(w, h),
    }
}

impl NativeBackend {
    /// Decode and draw a bounded blur-up JPEG. Unlike full images, these
    /// <=32px thumbnails are explicitly allowed to decode during paint and
    /// live in a dedicated small LRU so they cannot displace sharp rasters.
    pub fn draw_image_thumb(
        &mut self,
        canvas: &skia_safe::Canvas,
        rect: Rect,
        id: u64,
        jpeg: &[u8],
    ) {
        let Some(image) = self.thumbnail_image(id, jpeg) else {
            return;
        };
        let dst = cover_rect(rect, image.width() as f32, image.height() as f32);
        let paint = skia_safe::Paint::default();
        let sampling = skia_safe::SamplingOptions::new(
            skia_safe::FilterMode::Linear,
            skia_safe::MipmapMode::None,
        );
        let save = canvas.save();
        canvas.clip_rect(to_sk_rect(rect), None, Some(true));
        canvas.draw_image_rect_with_sampling_options(
            &image,
            None,
            to_sk_rect(dst),
            sampling,
            &paint,
        );
        canvas.restore_to_count(save);
        super::image_diagnostics::record_successful_thumbnail_draw();
    }

    fn thumbnail_image(&mut self, id: u64, jpeg: &[u8]) -> Option<skia_safe::Image> {
        self.thumb_cache.tick = self.thumb_cache.tick.wrapping_add(1);
        let tick = self.thumb_cache.tick;
        if let Some(hit) = self.thumb_cache.entries.get_mut(&id) {
            hit.last_used = tick;
            return hit.image.clone();
        }

        let image = (jpeg.len() <= THUMB_ENCODED_BYTE_LIMIT)
            .then(|| skia_safe::Image::from_encoded(skia_safe::Data::new_copy(jpeg)))
            .flatten()
            // Dimensions come from encoded metadata and are checked before
            // Skia expands pixels on the paint thread. A highly compressible
            // large JPEG can otherwise fit under the 4 KiB payload bound.
            .filter(|lazy| {
                lazy.width() > 0
                    && lazy.height() > 0
                    && lazy.width() <= THUMB_MAX_EDGE
                    && lazy.height() <= THUMB_MAX_EDGE
            })
            .and_then(|lazy| lazy.make_raster_image(None, None));
        let bytes = image
            .as_ref()
            .map(|image| {
                (image.width().max(0) as usize)
                    .saturating_mul(image.height().max(0) as usize)
                    .saturating_mul(4)
            })
            .unwrap_or(0);
        self.thumb_cache.bytes = self.thumb_cache.bytes.saturating_add(bytes);
        self.thumb_cache.entries.insert(
            id,
            ThumbCacheEntry {
                image: image.clone(),
                bytes,
                last_used: tick,
            },
        );
        self.evict_thumbnails_over_budget();
        image
    }

    fn evict_thumbnails_over_budget(&mut self) {
        while self.thumb_cache.bytes > THUMB_CACHE_BYTE_BUDGET
            || self.thumb_cache.entries.len() > THUMB_CACHE_MAX_ENTRIES
        {
            let Some((&oldest, _)) = self
                .thumb_cache
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
            else {
                break;
            };
            if let Some(entry) = self.thumb_cache.entries.remove(&oldest) {
                self.thumb_cache.bytes = self.thumb_cache.bytes.saturating_sub(entry.bytes);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn thumb_cache_len(&self) -> usize {
        self.thumb_cache.entries.len()
    }

    #[cfg(test)]
    pub(crate) fn thumb_raster_cache_len(&self) -> usize {
        self.thumb_cache
            .entries
            .values()
            .filter(|entry| entry.image.is_some())
            .count()
    }

    /// Draw the image identified by `id`, aspect-fit + centered
    /// inside `rect`. Kept as the legacy/default image path for chat
    /// attachments and UI previews.
    pub fn draw_image(&mut self, canvas: &skia_safe::Canvas, rect: Rect, id: u64, encoded: &[u8]) {
        self.draw_image_with_mode(canvas, rect, id, encoded, ImageDrawMode::Fit);
    }

    /// Draw the image identified by `id` using the same placement
    /// modes as the TS renderer's image fill path.
    pub fn draw_image_with_mode(
        &mut self,
        canvas: &skia_safe::Canvas,
        rect: Rect,
        id: u64,
        encoded: &[u8],
        mode: ImageDrawMode,
    ) {
        self.draw_image_with_options(
            canvas,
            rect,
            id,
            encoded,
            mode,
            ImageAdjustments::default(),
            1.0,
            0.0,
        );
    }

    /// Draw the image identified by `id` using placement and
    /// adjustment controls from the image-fill popover.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_image_with_options(
        &mut self,
        canvas: &skia_safe::Canvas,
        rect: Rect,
        id: u64,
        encoded: &[u8],
        mode: ImageDrawMode,
        adjustments: ImageAdjustments,
        opacity: f32,
        corner_radius: f32,
    ) {
        self.draw_image_with_options_and_transform(
            canvas,
            rect,
            id,
            encoded,
            mode,
            adjustments,
            opacity,
            corner_radius,
            None,
        );
    }

    /// Draw an image with an optional Figma image-fill transform. The affine
    /// maps the node unit square into image UV; Skia image shaders expect the
    /// inverse mapping (image pixels into node-local coordinates).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_image_with_options_and_transform(
        &mut self,
        canvas: &skia_safe::Canvas,
        rect: Rect,
        id: u64,
        encoded: &[u8],
        mode: ImageDrawMode,
        adjustments: ImageAdjustments,
        opacity: f32,
        corner_radius: f32,
        transform: Option<[f32; 6]>,
    ) {
        self.draw_image_with_options_transform_and_blend(
            canvas,
            rect,
            id,
            encoded,
            mode,
            adjustments,
            opacity,
            corner_radius,
            transform,
            ImageBlendMode::Normal,
        );
    }

    /// Draw an image with affine sampling and explicit backdrop compositing.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_image_with_options_transform_and_blend(
        &mut self,
        canvas: &skia_safe::Canvas,
        rect: Rect,
        id: u64,
        encoded: &[u8],
        mode: ImageDrawMode,
        adjustments: ImageAdjustments,
        opacity: f32,
        corner_radius: f32,
        transform: Option<[f32; 6]>,
        blend_mode: ImageBlendMode,
    ) {
        self.draw_image_with_options_transform_blend_and_tile_scale(
            canvas,
            rect,
            id,
            encoded,
            mode,
            adjustments,
            opacity,
            corner_radius,
            transform,
            blend_mode,
            None,
            1.0,
        );
    }

    /// Draw an image with affine sampling, backdrop compositing, and a
    /// dedicated TILE scale. Affine decal sampling remains exclusive to
    /// non-tile modes; TILE always repeats the scaled source bitmap.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_image_with_options_transform_blend_and_tile_scale(
        &mut self,
        canvas: &skia_safe::Canvas,
        rect: Rect,
        id: u64,
        _encoded: &[u8],
        mode: ImageDrawMode,
        adjustments: ImageAdjustments,
        opacity: f32,
        corner_radius: f32,
        transform: Option<[f32; 6]>,
        blend_mode: ImageBlendMode,
        original_size: Option<[f32; 2]>,
        tile_scale: f32,
    ) {
        let Some(image) = self.raster_image(id) else {
            return;
        };
        super::image_diagnostics::record_sharp_raster_hit();
        // Rounded image nodes clip the BITMAP, not just the
        // placeholder fill/stroke (TS `clipRRect` parity,
        // node-renderer.ts:1093-1104).
        let clip_round = corner_radius > 0.5;
        if clip_round {
            canvas.save();
            let rrect = skia_safe::RRect::new_rect_xy(
                skia_safe::Rect::from_xywh(rect.origin.x, rect.origin.y, rect.size.x, rect.size.y),
                corner_radius,
                corner_radius,
            );
            canvas.clip_rrect(rrect, skia_safe::ClipOp::Intersect, true);
        }
        let mut paint = skia_safe::Paint::default();
        paint.set_anti_alias(true);
        paint.set_blend_mode(to_skia_blend_mode(blend_mode));
        // Node-level opacity dims the raster (rasters carry no fill
        // colour to bake the opacity into at scene-build).
        paint.set_alpha_f(opacity.clamp(0.0, 1.0));
        if let Some(matrix) = image_adjustment_matrix(adjustments) {
            paint.set_color_filter(skia_safe::color_filters::matrix_row_major(&matrix, None));
        }

        // Figma commonly serializes UI-cropped paints as STRETCH plus an
        // affine transform (the Tesla 191×236 card does this). Apply the
        // transform before mode fallback so explicit CROP and transformed
        // STRETCH paints share Fill's exact sampling path.
        let transform = if matches!(mode, ImageDrawMode::Tile | ImageDrawMode::CssRepeat) {
            None
        } else {
            transform
        };
        if let Some(local) = transform.and_then(|affine| {
            figma_image_local_matrix(rect, image.width() as f32, image.height() as f32, affine)
        }) {
            let local_matrix = skia_safe::Matrix::new_all(
                local[0], local[1], local[2], local[3], local[4], local[5], 0.0, 0.0, 1.0,
            );
            let sampling = skia_safe::SamplingOptions::new(
                skia_safe::FilterMode::Linear,
                skia_safe::MipmapMode::None,
            );
            if let Some(shader) = image.to_shader(
                (skia_safe::TileMode::Decal, skia_safe::TileMode::Decal),
                sampling,
                &local_matrix,
            ) {
                paint.set_shader(shader);
                let save = canvas.save();
                canvas.clip_rect(to_sk_rect(rect), None, Some(true));
                canvas.draw_rect(to_sk_rect(rect), &paint);
                canvas.restore_to_count(save);
                if clip_round {
                    canvas.restore();
                }
                return;
            }
        }

        match mode {
            ImageDrawMode::Fit => {
                let dst = contain_rect(rect, image.width() as f32, image.height() as f32);
                canvas.draw_image_rect(&image, None, to_sk_rect(dst), &paint);
            }
            ImageDrawMode::Stretch => {
                canvas.draw_image_rect(&image, None, to_sk_rect(rect), &paint);
            }
            ImageDrawMode::Tile => {
                draw_tiled_image(canvas, rect, &image, &paint, original_size, tile_scale);
            }
            ImageDrawMode::CssRepeat => {
                let [sw, sh] = valid_original_size(original_size)
                    .unwrap_or([image.width() as f32, image.height() as f32]);
                let scale = if tile_scale.is_finite() && tile_scale > 0.0 { tile_scale } else { 1.0 };
                let matrix = skia_safe::Matrix::new_all(
                    sw * scale / image.width() as f32, 0.0, rect.origin.x,
                    0.0, sh * scale / image.height() as f32, rect.origin.y,
                    0.0, 0.0, 1.0,
                );
                let sampling = skia_safe::SamplingOptions::new(skia_safe::FilterMode::Linear, skia_safe::MipmapMode::None);
                if let Some(shader) = image.to_shader(
                    (skia_safe::TileMode::Repeat, skia_safe::TileMode::Repeat), sampling, &matrix,
                ) {
                    paint.set_shader(shader);
                    canvas.draw_rect(to_sk_rect(rect), &paint);
                }
            }
            ImageDrawMode::Fill | ImageDrawMode::Crop => {
                let dst = cover_rect(rect, image.width() as f32, image.height() as f32);
                let save = canvas.save();
                canvas.clip_rect(to_sk_rect(rect), None, Some(true));
                canvas.draw_image_rect(&image, None, to_sk_rect(dst), &paint);
                canvas.restore_to_count(save);
            }
        }
        if clip_round {
            canvas.restore();
        }
    }
}

/// Build Skia's image-shader local matrix from a Figma fill transform.
///
/// Figma maps normalized node coordinates to normalized image UV. A Skia
/// image shader's local matrix maps image pixels to local coordinates, so the
/// order is `node_rect * inverse(figma) * inverse(image_dimensions)`.
pub(super) fn figma_image_local_matrix(
    rect: Rect,
    img_w: f32,
    img_h: f32,
    transform: [f32; 6],
) -> Option<[f32; 6]> {
    if rect.size.x <= 0.0
        || rect.size.y <= 0.0
        || img_w <= 0.0
        || img_h <= 0.0
        || !transform.iter().all(|v| v.is_finite())
    {
        return None;
    }
    let [a, b, tx, c, d, ty] = transform;
    let det = a * d - b * c;
    if !det.is_finite() || det.abs() <= f32::EPSILON {
        return None;
    }
    let inv_det = 1.0 / det;
    let ia = d * inv_det;
    let ib = -b * inv_det;
    let ic = -c * inv_det;
    let id = a * inv_det;
    let itx = (b * ty - d * tx) * inv_det;
    let ity = (c * tx - a * ty) * inv_det;
    Some([
        rect.size.x * ia / img_w,
        rect.size.x * ib / img_h,
        rect.origin.x + rect.size.x * itx,
        rect.size.y * ic / img_w,
        rect.size.y * id / img_h,
        rect.origin.y + rect.size.y * ity,
    ])
}

pub(super) fn image_adjustment_matrix(adjustments: ImageAdjustments) -> Option<[f32; 20]> {
    let exp = adjustments.exposure / 100.0;
    let con = adjustments.contrast / 100.0;
    let sat = adjustments.saturation / 100.0;
    let temp = adjustments.temperature / 100.0;
    let tint = adjustments.tint / 100.0;
    let hi = adjustments.highlights / 100.0;
    let sh = adjustments.shadows / 100.0;
    if adjustments.is_neutral() {
        return None;
    }

    let e = 1.0 + exp * 1.5;
    let c = 1.0 + con;
    let c_off = 0.5 * (1.0 - c);
    let s = 1.0 + sat;
    let (lr, lg, lb) = (0.2126, 0.7152, 0.0722);
    let sr = (1.0 - s) * lr;
    let sg = (1.0 - s) * lg;
    let sb = (1.0 - s) * lb;
    let f = c * e;
    let off_r = c_off + temp * 0.15 + (hi + sh * 0.5) * 0.1;
    let off_g = c_off + tint * 0.15 + (hi + sh * 0.5) * 0.1;
    let off_b = c_off - temp * 0.15 + (hi + sh * 0.5) * 0.1;

    Some([
        f * (sr + s),
        f * sg,
        f * sb,
        0.0,
        off_r,
        f * sr,
        f * (sg + s),
        f * sb,
        0.0,
        off_g,
        f * sr,
        f * sg,
        f * (sb + s),
        0.0,
        off_b,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
    ])
}

fn draw_tiled_image(
    canvas: &skia_safe::Canvas,
    rect: Rect,
    image: &skia_safe::Image,
    paint: &skia_safe::Paint,
    original_size: Option<[f32; 2]>,
    tile_scale: f32,
) {
    let [source_w, source_h] =
        valid_original_size(original_size).unwrap_or([image.width() as f32, image.height() as f32]);
    let tile_scale = effective_tile_scale(rect, source_w, source_h, tile_scale);
    let tile_w = source_w * tile_scale;
    let tile_h = source_h * tile_scale;
    if tile_w <= 0.0 || tile_h <= 0.0 || rect.size.x <= 0.0 || rect.size.y <= 0.0 {
        return;
    }

    let save = canvas.save();
    canvas.clip_rect(to_sk_rect(rect), None, Some(true));

    let centered_x = rect.origin.x + (rect.size.x - tile_w) / 2.0;
    let centered_y = rect.origin.y + (rect.size.y - tile_h) / 2.0;
    let x_steps = ((centered_x - rect.origin.x) / tile_w).ceil().max(0.0);
    let y_steps = ((centered_y - rect.origin.y) / tile_h).ceil().max(0.0);
    let start_x = centered_x - x_steps * tile_w;
    let start_y = centered_y - y_steps * tile_h;

    let right = rect.origin.x + rect.size.x;
    let bottom = rect.origin.y + rect.size.y;
    let mut y = start_y;
    while y < bottom {
        let mut x = start_x;
        while x < right {
            canvas.draw_image_rect(
                image,
                None,
                to_sk_rect(Rect::xywh(x, y, tile_w, tile_h)),
                paint,
            );
            let next_x = x + tile_w;
            if next_x <= x {
                break;
            }
            x = next_x;
        }
        let next_y = y + tile_h;
        if next_y <= y {
            break;
        }
        y = next_y;
    }

    canvas.restore_to_count(save);
}

fn normalized_tile_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

fn valid_original_size(size: Option<[f32; 2]>) -> Option<[f32; 2]> {
    size.filter(|[width, height]| {
        width.is_finite() && height.is_finite() && *width > 0.0 && *height > 0.0
    })
}

/// Keep pathological but positive wire values from producing billions of
/// individual draws. Dense sub-pixel repetition is visually indistinguishable
/// beyond this bound, while the shared multiplier preserves bitmap aspect.
fn effective_tile_scale(rect: Rect, image_w: f32, image_h: f32, scale: f32) -> f32 {
    const MAX_REPEATS_PER_AXIS: f32 = 128.0;
    let authored = normalized_tile_scale(scale);
    let min_for_progress = (1.0 / image_w).max(1.0 / image_h);
    let min_for_x = rect.size.x.abs() / (image_w * MAX_REPEATS_PER_AXIS);
    let min_for_y = rect.size.y.abs() / (image_h * MAX_REPEATS_PER_AXIS);
    authored.max(min_for_progress).max(min_for_x).max(min_for_y)
}

#[cfg(test)]
mod crop_transform_tests {
    use super::*;

    #[test]
    fn css_repeat_keeps_origin_distinct_from_centered_tile() {
        let repeat = render_with_scale(ImageDrawMode::CssRepeat, None, 0.7);
        assert_ne!(repeat, render_with_scale(ImageDrawMode::Tile, None, 0.7));
        assert_eq!(repeat, render_with_options(ImageDrawMode::CssRepeat, None, Some([4.0, 2.0]), 0.7));
        assert_eq!(repeat, render_with_scale(ImageDrawMode::CssRepeat, Some([1.0, 0.0, 0.5, 0.0, 1.0, 0.5]), 0.7));
    }

    fn render_with_scale(
        mode: ImageDrawMode,
        transform: Option<[f32; 6]>,
        tile_scale: f32,
    ) -> Vec<u8> {
        render_with_options(mode, transform, None, tile_scale)
    }

    fn render_with_options(
        mode: ImageDrawMode,
        transform: Option<[f32; 6]>,
        original_size: Option<[f32; 2]>,
        tile_scale: f32,
    ) -> Vec<u8> {
        let mut source = skia_safe::surfaces::raster_n32_premul((4, 2)).unwrap();
        source.canvas().clear(skia_safe::Color::RED);
        let blue = skia_safe::Paint::new(skia_safe::Color4f::new(0.0, 0.0, 1.0, 1.0), None);
        source
            .canvas()
            .draw_rect(skia_safe::Rect::from_xywh(2.0, 0.0, 2.0, 2.0), &blue);

        let mut backend = NativeBackend::with_dpi(1.0);
        backend.install_raster_image(1, source.image_snapshot(), u32::MAX);
        let mut target = skia_safe::surfaces::raster_n32_premul((20, 20)).unwrap();
        target.canvas().clear(skia_safe::Color::TRANSPARENT);
        backend.draw_image_with_options_transform_blend_and_tile_scale(
            target.canvas(),
            Rect::xywh(0.0, 0.0, 20.0, 20.0),
            1,
            &[],
            mode,
            ImageAdjustments::default(),
            1.0,
            0.0,
            transform,
            ImageBlendMode::Normal,
            original_size,
            tile_scale,
        );
        target
            .image_snapshot()
            .encode(None, skia_safe::EncodedImageFormat::PNG, 100)
            .unwrap()
            .as_bytes()
            .to_vec()
    }

    fn render(mode: ImageDrawMode, transform: Option<[f32; 6]>) -> Vec<u8> {
        render_with_scale(mode, transform, 1.0)
    }

    #[test]
    fn crop_uses_figma_transform_identically_to_fill() {
        let transform = Some([0.5, 0.0, 0.0, 0.0, 1.0, 0.0]);
        let transformed_crop = render(ImageDrawMode::Crop, transform);
        assert_eq!(
            transformed_crop,
            render(ImageDrawMode::Fill, transform),
            "Crop and Fill must share Figma's affine sampling path"
        );
        assert_ne!(
            transformed_crop,
            render(ImageDrawMode::Crop, None),
            "the parity assertion must prove the transform affects pixels"
        );
    }

    #[test]
    fn tile_scale_is_distinct_from_affine_decal_sampling() {
        let transform = Some([0.5, 0.0, 0.0, 0.0, 1.0, 0.0]);
        let scaled = render_with_scale(ImageDrawMode::Tile, transform, 0.38618907);
        assert_eq!(
            scaled,
            render_with_scale(ImageDrawMode::Tile, None, 0.38618907),
            "TILE must ignore the affine Decal path and repeat its scaled bitmap"
        );
        assert_ne!(
            scaled,
            render_with_scale(ImageDrawMode::Tile, None, 1.0),
            "the authored tile scale must affect output pixels"
        );
    }

    #[test]
    fn tile_scale_preserves_figma_value_and_bounds_pathological_repetition() {
        let rect = Rect::xywh(0.0, 0.0, 220.0, 220.0);
        assert_eq!(normalized_tile_scale(0.38618907), 0.38618907);
        assert_eq!(normalized_tile_scale(0.0), 1.0);
        assert_eq!(normalized_tile_scale(f32::NAN), 1.0);
        assert_eq!(normalized_tile_scale(f32::INFINITY), 1.0);
        assert_eq!(
            valid_original_size(Some([4096.0, 2048.0])),
            Some([4096.0, 2048.0])
        );
        assert_eq!(valid_original_size(Some([0.0, 2048.0])), None);
        assert_eq!(valid_original_size(Some([f32::NAN, 2048.0])), None);
        assert_eq!(
            effective_tile_scale(rect, 220.0, 220.0, 0.38618907),
            0.38618907
        );

        let bounded = effective_tile_scale(
            Rect::xywh(1.0e20, 1.0e20, 1_000_000.0, 1_000_000.0),
            1.0,
            1.0,
            f32::MIN_POSITIVE,
        );
        assert!(bounded >= 1_000_000.0 / 128.0);
    }

    #[test]
    fn original_size_keeps_tile_period_after_oversized_raster_is_downsampled() {
        // Model a 4096px source decoded to a 256px raster (16x reduction).
        // The authored 1/16 TILE scale must still produce the same destination
        // period as a 256px source at scale 1.0.
        let decoded_period = render_with_options(ImageDrawMode::Tile, None, None, 1.0);
        let original_period =
            render_with_options(ImageDrawMode::Tile, None, Some([64.0, 32.0]), 1.0 / 16.0);
        assert_eq!(original_period, decoded_period);
        assert_eq!(
            render_with_options(ImageDrawMode::Tile, None, Some([0.0, 32.0]), 1.0),
            decoded_period,
            "invalid authored dimensions must fall back to the decoded raster"
        );

        let source = valid_original_size(Some([4096.0, 4096.0])).unwrap();
        assert_eq!([source[0] * 0.25, source[1] * 0.25], [1024.0, 1024.0]);
    }
}
