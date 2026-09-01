// Browser boundary: FileReader, Blob downloads, paste, and drag/drop require
// browser smoke; pure ingest/export behavior is covered in file_actions tests.
//! Browser file-IO glue — the web consumer of
//! `editor_ui.pending_file_action` plus the DOM paste / drag-drop
//! listeners. The pure serialize / ingest logic lives in
//! `crate::file_actions`; this module owns Blob/file-picker reads and routing.
//!
//! Closure lifetime pattern: long-lived listeners go through
//! `crate::listener::add_listener` (stored on the `WebShell` like
//! every mount() listener); the one-shot picker / reader callbacks
//! use the self-dropping `Rc<RefCell<Option<Closure>>>` slot idiom
//! from `raf_pump.rs` so each fires once and frees its own
//! wasm-bindgen closure slot. If the user cancels a file dialog the
//! `change` event never fires and that input + closure leak until
//! page unload in older engines that do not dispatch the modern file-input
//! `cancel` event; current engines clean up on both `change` and `cancel`.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsValue;

use op_editor_core::chat::{ChatAttachment, MAX_ATTACHMENT_BYTES};
use op_editor_core::figma_import_state::ImportSource;
use op_editor_core::KitIoRequest;

use crate::file_actions::{self, DropBatchPlan, DropKind};
use crate::listener::{add_listener, Listener};
use crate::repaint_ctx::RepaintContext;

mod browser_files;
mod document_io;
mod drop_entries;
mod figma_import;
mod html_directory_session;
mod html_zip_session;
mod import_generation;

use browser_files::{html_project_file_path, open_html_project_picker};
pub(crate) use browser_files::{js_bytes, open_file_picker, read_file, ReadMode};
pub(crate) use document_io::drain_pending_file_action;
use figma_import::import_figma;
use import_generation::{
    begin_document_import, clear_document_import_if_owned, document_import_activity,
    document_import_is_current,
};

type InnerRc<C> = Rc<RefCell<C>>;

fn console_error(msg: &str) {
    web_sys::console::error_1(&JsValue::from_str(msg));
}

fn console_warn(msg: &str) {
    web_sys::console::warn_1(&JsValue::from_str(msg));
}

/// Consume a chat attachment-pick request raised by the chat footer.
/// Browser parity for desktop `chat_attachment::drain_attachment_pick`:
/// open an image/PDF picker, read the file bytes, then stage a
/// `ChatAttachment` on the current chat draft.
pub(crate) fn drain_pending_attachment_pick<C: RepaintContext + 'static>(inner: &InnerRc<C>) {
    let pending = std::mem::take(
        &mut inner
            .borrow_mut()
            .host_mut()
            .editor_state_mut()
            .chat
            .pending_attachment_pick,
    );
    if !pending {
        return;
    }
    let inner = inner.clone();
    open_file_picker(
        ".png,.jpg,.jpeg,.gif,.webp,.svg,.pdf,application/pdf",
        Box::new(move |file| stage_chat_attachment_file(&inner, file)),
    );
}

/// Read one browser file and stage it for the next Agent turn. Shared by the
/// paperclip picker and PDF drag/drop so both paths enforce identical limits
/// and produce the same removable attachment chip.
fn stage_chat_attachment_file<C: RepaintContext + 'static>(
    inner: &InnerRc<C>,
    file: web_sys::File,
) {
    let raw_name = file.name();
    if file.size() > MAX_ATTACHMENT_BYTES as f64 {
        let limit_mib = MAX_ATTACHMENT_BYTES / (1024 * 1024);
        console_warn(&format!(
            "[chat-attachment] {raw_name}: file exceeds {limit_mib} MiB limit"
        ));
        return;
    }
    let name = file_actions::attachment_file_name(&raw_name);
    let media_type = file_actions::attachment_media_type_for_name(&name);
    let inner = inner.clone();
    read_file(
        file,
        ReadMode::Bytes,
        Box::new(move |value| {
            let Some(data) = js_bytes(&value) else {
                console_error("[chat-attachment] file read produced no bytes");
                return;
            };
            let mut b = inner.borrow_mut();
            let added = b
                .host_mut()
                .editor_state_mut()
                .chat
                .add_attachment(ChatAttachment {
                    name,
                    media_type,
                    data,
                });
            if added {
                b.host_mut().mark_editor_state_dirty();
                let _ = b.repaint();
            } else {
                console_warn("[chat-attachment] attachment rejected by chat state");
            }
        }),
    );
}

/// Consume a Component-Browser kit import/export request raised by
/// the floating panel header. Browser counterpart of desktop
/// `DesktopApp::drain_kit_io`: native dialogs become hidden file
/// inputs / Blob downloads.
pub(crate) fn drain_pending_kit_io<C: RepaintContext + 'static>(inner: &InnerRc<C>) {
    let request = inner
        .borrow_mut()
        .host_mut()
        .editor_state_mut()
        .editor_ui
        .component_browser_kit_request
        .take();
    match request {
        Some(KitIoRequest::Import) => import_kit_file(inner),
        Some(KitIoRequest::Export) => export_kit_file(inner),
        None => {}
    }
}

/// Component Browser → Import kit: hidden `.op` / `.pen` / `.json`
/// picker → extract reusable components → append session kit.
fn import_kit_file<C: RepaintContext + 'static>(inner: &InnerRc<C>) {
    let inner = inner.clone();
    open_file_picker(
        ".op,.pen,.json",
        Box::new(move |file| {
            let name = file.name();
            let inner2 = inner.clone();
            read_file(
                file,
                ReadMode::Text,
                Box::new(move |value| match value.as_string() {
                    Some(src) => apply_imported_kit(&inner2, &src, &name),
                    None => console_error("[kit-import] file read produced no text"),
                }),
            );
        }),
    );
}

fn apply_imported_kit<C: RepaintContext + 'static>(inner: &InnerRc<C>, src: &str, file_name: &str) {
    match file_actions::import_kit_source(src, mint_web_kit_id()) {
        Ok(Some(kit)) => {
            let mut b = inner.borrow_mut();
            b.host_mut().editor_state_mut().import_kit(kit);
            b.host_mut().mark_editor_state_dirty();
            let _ = b.repaint();
        }
        Ok(None) => {}
        Err(e) => console_error(&format!("[kit-import] {file_name}: {e}")),
    }
}

/// Component Browser → Export kit: collect reusable components into
/// the shared kit file format and download it as `.op`.
fn export_kit_file<C: RepaintContext + 'static>(inner: &InnerRc<C>) {
    let b = inner.borrow();
    match file_actions::export_kit_document(b.host().editor_state()) {
        Ok(Some(export)) => {
            if let Err(e) = crate::web_clipboard::download_bytes(
                &export.file_name,
                "application/json",
                export.json.as_bytes(),
            ) {
                web_sys::console::error_1(&e);
            }
        }
        Ok(None) => {}
        Err(e) => console_error(&format!("[kit-export] {e}")),
    }
}

fn mint_web_kit_id() -> String {
    let millis = js_sys::Date::now() as u64;
    let nonce = (js_sys::Math::random() * 1_000_000_000.0) as u64;
    format!("kit-web-{millis:013x}-{nonce:08x}")
}

/// Pick loose saved-page files or one ZIP project. Loose selections are indexed
/// together, then only the entry and referenced resources are read.
fn import_html_file<C: RepaintContext + 'static>(inner: &InnerRc<C>) {
    let inner = inner.clone();
    open_html_project_picker(Box::new(move |files| {
        import_html_batch(&inner, files);
    }));
}

fn import_html_batch<C: RepaintContext + 'static>(inner: &InnerRc<C>, files: Vec<web_sys::File>) {
    let kinds: Vec<_> = files
        .iter()
        .map(|file| file_actions::drop_kind(&file.name()))
        .collect();
    match file_actions::drop_batch_plan(&kinds) {
        DropBatchPlan::HtmlProject => {
            if files.len() > op_html::MAX_PROJECT_FILES {
                console_error(&format!(
                    "[import-html] project contains {} files; limit is {}",
                    files.len(),
                    op_html::MAX_PROJECT_FILES
                ));
                return;
            }
            let generation = begin_html_document_import(inner);
            let inner2 = inner.clone();
            let files = files
                .into_iter()
                .map(|file| drop_entries::DroppedProjectFile {
                    relative_path: html_project_file_path(&file),
                    file,
                })
                .collect();
            html_directory_session::start(
                files,
                document_import_activity(inner, generation, ImportSource::Html),
                Box::new(move |result| {
                    finish_html_import(&inner2, generation, result);
                }),
            );
        }
        DropBatchPlan::HtmlZip => {
            let Some(file) = files.into_iter().next() else {
                return;
            };
            let generation = begin_html_document_import(inner);
            let inner2 = inner.clone();
            html_zip_session::start(
                file,
                document_import_activity(inner, generation, ImportSource::Html),
                Box::new(move |result| finish_html_import(&inner2, generation, result)),
            );
        }
        DropBatchPlan::InvalidZipMix => {
            console_error(
                "[import-html] select or drop one ZIP by itself; ZIP cannot be mixed with other files",
            );
        }
        DropBatchPlan::InvalidHtmlMix => {
            console_error(
                "[import-html] HTML projects may only contain HTML, CSS, fonts, SVG, and image resources",
            );
        }
        DropBatchPlan::Individual => {
            console_error("[import-html] selected files contain no .html, .htm, or .zip file");
        }
    }
}

fn begin_html_document_import<C: RepaintContext + 'static>(inner: &InnerRc<C>) -> u64 {
    begin_document_import(inner, ImportSource::Html)
}

fn finish_html_import<C: RepaintContext + 'static, E: std::fmt::Display>(
    inner: &InnerRc<C>,
    generation: u64,
    result: Result<file_actions::IngestedDoc, E>,
) {
    finish_document_import(inner, generation, ImportSource::Html, result, "import-html");
}

/// Generic over the ingest error so every import path can hand over its own
/// typed failure; the console line only ever renders it through `Display`.
fn finish_document_import<C: RepaintContext + 'static, E: std::fmt::Display>(
    inner: &InnerRc<C>,
    generation: u64,
    source: ImportSource,
    result: Result<file_actions::IngestedDoc, E>,
    log_tag: &str,
) -> bool {
    if !document_import_is_current(inner, generation, source) {
        return false;
    }
    match result {
        Ok(ingested) => {
            install_ingested_document(inner, ingested, log_tag);
            true
        }
        Err(error) => {
            clear_document_import_if_owned(inner, generation, source);
            console_error(&format!("[{log_tag}] {error}"));
            false
        }
    }
}

fn install_ingested_document<C: RepaintContext + 'static>(
    inner: &InnerRc<C>,
    ingested: file_actions::IngestedDoc,
    log_tag: &str,
) {
    for warning in &ingested.warnings {
        console_warn(&format!("[{log_tag}] warning: {warning}"));
    }
    let diagnostics = ingested.diagnostics;
    let mut b = inner.borrow_mut();
    b.host_mut().install_unsaved_ingested_state(ingested.state);
    // After the install, so the fresh state carries the report rather than
    // having it wiped by the replacement.
    b.host_mut().show_html_import_diagnostics(diagnostics);
    let (w, h) = b.viewport_size();
    b.host_mut().fit_content_to_viewport(w, h);
    let _ = b.repaint();
}

/// Shape picker → Import image or SVG: SVG parses into editable
/// nodes; rasters insert an Image node carrying the file as a data
/// URL (the browser's `readAsDataURL` builds it — no base64 dep).
fn import_image_or_svg<C: RepaintContext + 'static>(inner: &InnerRc<C>) {
    let inner = inner.clone();
    open_file_picker(
        ".png,.jpg,.jpeg,.gif,.webp,.svg",
        Box::new(move |file| {
            let name = file.name();
            if file_actions::drop_kind(&name) == DropKind::Svg {
                let inner2 = inner.clone();
                read_file(
                    file,
                    ReadMode::Text,
                    Box::new(move |value| match value.as_string() {
                        Some(svg) => insert_svg(&inner2, &svg, &name),
                        None => console_error("[import-svg] file read produced no text"),
                    }),
                );
            } else {
                let inner2 = inner.clone();
                read_file(
                    file,
                    ReadMode::DataUrl,
                    Box::new(move |value| match value.as_string() {
                        Some(url) => insert_image(&inner2, &url, &name),
                        None => console_error("[import-image] file read produced no data URL"),
                    }),
                );
            }
        }),
    );
}

/// Fill section 图片 row → picker → write the data URL into the
/// selected node's primary fill (desktop `handle_pick_fill_image`).
fn pick_fill_image<C: RepaintContext + 'static>(inner: &InnerRc<C>) {
    let inner = inner.clone();
    open_file_picker(
        ".png,.jpg,.jpeg,.gif,.webp,.svg",
        Box::new(move |file| {
            let inner2 = inner.clone();
            read_file(
                file,
                ReadMode::DataUrl,
                Box::new(move |value| {
                    let Some(url) = value.as_string() else {
                        console_error("[fill-image] file read produced no data URL");
                        return;
                    };
                    let mut b = inner2.borrow_mut();
                    if file_actions::apply_fill_image_data_url(
                        b.host_mut().editor_state_mut(),
                        &url,
                    ) {
                        // Fill content written outside the command/history
                        // path — bump the revision so the layer-panel cache +
                        // save-dirty tracking (keyed on `document_revision()`)
                        // see it. The relink handler below bumps via
                        // `commit_history()`.
                        b.host_mut().editor_state_mut().mark_document_changed();
                    }
                    b.host_mut().mark_editor_state_dirty();
                    let _ = b.repaint();
                }),
            );
        }),
    );
}

/// Image-section warning row's Relink: pick a replacement image and
/// rewrite the selected image node's `src`. The desktop
/// (`persistence_image::handle_relink_image`) stores a document-relative
/// FILE PATH; the browser has no file paths, so the picked file becomes a
/// data URL — the same divergence-by-platform as `PickFillImage`. The
/// stale-asset check is dropped so the warning row clears on re-probe.
fn relink_image<C: RepaintContext + 'static>(inner: &InnerRc<C>) {
    let inner = inner.clone();
    open_file_picker(
        ".png,.jpg,.jpeg,.gif,.webp,.svg",
        Box::new(move |file| {
            let inner2 = inner.clone();
            read_file(
                file,
                ReadMode::DataUrl,
                Box::new(move |value| {
                    let Some(url) = value.as_string() else {
                        console_error("[relink-image] file read produced no data URL");
                        return;
                    };
                    let mut b = inner2.borrow_mut();
                    let state = b.host_mut().editor_state_mut();
                    let id = state.selection.anchor.clone();
                    if !id.is_real() {
                        return;
                    }
                    state.commit_history();
                    if let Some(jian_ops_schema::node::PenNode::Image(image)) =
                        op_editor_core::walkers::find_node_mut(state.active_children_mut(), &id)
                    {
                        image.src = url.into();
                    }
                    state.editor_ui.image_panel.asset_check = None;
                    b.host_mut().mark_editor_state_dirty();
                    let _ = b.repaint();
                }),
            );
        }),
    );
}

/// Parse SVG source into editable nodes centred near the viewport —
/// mirrors `persistence_image::handle_import_image_or_svg`'s SVG arm.
fn insert_svg<C: RepaintContext + 'static>(inner: &InnerRc<C>, svg: &str, file_name: &str) {
    let mut b = inner.borrow_mut();
    let pan_x = b.host().editor_state().viewport.pan_x as f64;
    let pan_y = b.host().editor_state().viewport.pan_y as f64;
    let zoom = (b.host().editor_state().viewport.zoom as f64).max(0.001);
    let centre_x = -pan_x / zoom;
    let centre_y = -pan_y / zoom;
    let mut next_id = 0u64;
    let stem = file_actions::file_stem(file_name);
    let count = b.host_mut().editor_state_mut().import_svg_named(
        &mut next_id,
        svg,
        (centre_x - 200.0, centre_y - 150.0),
        Some(stem),
    );
    if count == 0 {
        console_warn(&format!("[import-svg] {file_name} yielded no nodes"));
    }
    b.host_mut().mark_editor_state_dirty();
    let _ = b.repaint();
}

/// Insert a raster image (as a data URL) as an Image node centred on
/// the viewport — mirrors the desktop's raster import arm.
fn insert_image<C: RepaintContext + 'static>(inner: &InnerRc<C>, url: &str, file_name: &str) {
    let mut b = inner.borrow_mut();
    let stem = file_actions::file_stem(file_name).to_string();
    let _ = b
        .host_mut()
        .editor_state_mut()
        .insert_image_node_at_viewport(&stem, url);
    b.host_mut().mark_editor_state_dirty();
    let _ = b.repaint();
}

// ---------------------------------------------------------------------
// DOM paste + file drag-drop listeners (task items 3 + 4)
// ---------------------------------------------------------------------

/// Register the paste / dragover / dragleave / drop listeners.
/// Called from `mount()`'s registration closure; the closures are
/// stored on the `WebShell` like every other listener.
pub(crate) fn register_io_listeners<C: RepaintContext + 'static>(
    inner: &InnerRc<C>,
    canvas: &web_sys::HtmlCanvasElement,
    win_target: &web_sys::EventTarget,
    listeners: &mut Vec<Listener>,
) -> Result<(), JsValue> {
    // Paste on the window — the event fires on whatever element owns
    // focus (usually the hidden IME textarea) and bubbles up.
    {
        let inner_p = inner.clone();
        add_listener::<web_sys::ClipboardEvent, _, _>(
            win_target,
            "paste",
            listeners,
            move |evt: web_sys::ClipboardEvent| {
                handle_paste_event(&inner_p, &evt);
            },
        )?;
    }
    let canvas_target: web_sys::EventTarget = canvas.clone().into();
    // dragover fires continuously while a file hovers the canvas. Keep the
    // browser from navigating to a dropped file, but show the full-canvas
    // affordance only for PDFs that will become Agent attachments. Project
    // drag/import is intentionally disabled; File -> Open remains available.
    {
        let inner_d = inner.clone();
        add_listener::<web_sys::DragEvent, _, _>(
            &canvas_target,
            "dragover",
            listeners,
            move |evt: web_sys::DragEvent| {
                evt.prevent_default();
                set_file_drop_active(&inner_d, drag_event_has_pdf(&evt));
            },
        )?;
    }
    {
        let inner_d = inner.clone();
        add_listener::<web_sys::DragEvent, _, _>(
            &canvas_target,
            "dragleave",
            listeners,
            move |_evt: web_sys::DragEvent| {
                set_file_drop_active(&inner_d, false);
            },
        )?;
    }
    {
        let inner_d = inner.clone();
        add_listener::<web_sys::DragEvent, _, _>(
            &canvas_target,
            "drop",
            listeners,
            move |evt: web_sys::DragEvent| {
                evt.prevent_default();
                set_file_drop_active(&inner_d, false);
                handle_drop_event(&inner_d, &evt);
            },
        )?;
    }
    Ok(())
}

fn drag_event_has_pdf(evt: &web_sys::DragEvent) -> bool {
    let Some(transfer) = evt.data_transfer() else {
        return false;
    };
    let items = transfer.items();
    (0..items.length()).any(|index| {
        let Some(item) = items.get(index) else {
            return false;
        };
        if item.kind() != "file" {
            return false;
        }
        item.type_().eq_ignore_ascii_case("application/pdf")
            || item
                .get_as_file()
                .ok()
                .flatten()
                .is_some_and(|file| file.name().to_ascii_lowercase().ends_with(".pdf"))
    })
}

/// Flip the painted file-drop overlay (`paint.rs` already renders it
/// when `file_drop_active` is set); repaints only on change.
fn set_file_drop_active<C: RepaintContext + 'static>(inner: &InnerRc<C>, active: bool) {
    let mut b = inner.borrow_mut();
    if b.host().editor_state().editor_ui.file_drop_active != active {
        b.host_mut().editor_state_mut().editor_ui.file_drop_active = active;
        b.host_mut().mark_editor_state_dirty();
        let _ = b.repaint();
    }
}

/// DOM paste routing — native Cmd+V priority order
/// (`keyboard_input.rs`): Figma clipboard HTML first, then the
/// focused text input, then the internal node clipboard.
fn handle_paste_event<C: RepaintContext + 'static>(
    inner: &InnerRc<C>,
    evt: &web_sys::ClipboardEvent,
) {
    let Some(dt) = evt.clipboard_data() else {
        return;
    };
    // Native parity: a visible non-chat text surface owns Cmd/Ctrl+V before
    // rich HTML, Figma metadata, images, or the internal node clipboard. Rich
    // clipboard payloads normally include text/plain; insert that into the
    // field and never import nodes behind a popover/modal.
    if inner.borrow().host().non_chat_input_owns_keyboard() {
        evt.prevent_default();
        let text = dt.get_data("text/plain").unwrap_or_default();
        if !text.is_empty() {
            let mut b = inner.borrow_mut();
            if b.host_mut().apply_clipboard_text(&text) {
                let _ = b.repaint();
            }
        }
        return;
    }
    let html = dt.get_data("text/html").unwrap_or_default();
    if !html.is_empty() && op_figma::is_figma_clipboard_html(&html) {
        evt.prevent_default();
        if let Some(data) = op_figma::extract_figma_clipboard_data(&html) {
            let result = op_figma::figma_clipboard_to_nodes(&data.buffer, Some(&html));
            if !result.nodes.is_empty() {
                let mut b = inner.borrow_mut();
                let (w, h) = b.viewport_size();
                if b.host_mut().paste_figma_nodes(result.nodes, w, h) {
                    let _ = b.repaint();
                }
                return;
            }
        }
        // Recognized Figma HTML but nothing usable decoded — swallow
        // the paste rather than dumping raw HTML text (native parity).
        return;
    }
    if !html.is_empty() {
        evt.prevent_default();
        let result = op_html::import_html(&html, &op_html::HtmlImportOptions::default());
        for warning in &result.warnings {
            console_warn(&format!("[import-html] {warning}"));
        }
        let mut b = inner.borrow_mut();
        // A pasted page degrades exactly like an imported file, so it raises
        // the same GUI report instead of console-only warnings. Publishing an
        // empty list also clears a stale card, matching the file-ingest path.
        b.host_mut().show_html_import_diagnostics(
            op_editor_core::html_import_diagnostics::rows_from_parts(op_html::diagnostic_parts(
                &result.diagnostics,
            )),
        );
        let inserted = if result.nodes.is_empty() {
            false
        } else {
            let (w, h) = b.viewport_size();
            b.host_mut().paste_figma_nodes(result.nodes, w, h)
        };
        if inserted || !result.diagnostics.is_empty() {
            let _ = b.repaint();
        }
        return;
    }
    // System image/file paste: `clipboardData.files` carries pasted images
    // (PNG/JPEG from another app — Chrome names them `image.png`, so the
    // name-based `drop_kind` routes them to `insert_image`). Route through the
    // same ingestion as drag-drop BEFORE the text / internal-clipboard
    // fallbacks, so an image paste never falls through to the stale internal
    // node clipboard.
    if let Some(files) = dt.files() {
        if files.length() > 0 {
            evt.prevent_default();
            for i in 0..files.length() {
                if let Some(file) = files.get(i) {
                    route_dropped_file(inner, file);
                }
            }
            return;
        }
    }
    let text = dt.get_data("text/plain").unwrap_or_default();
    if !text.is_empty() {
        let mut b = inner.borrow_mut();
        if b.host_mut().apply_clipboard_text(&text) {
            evt.prevent_default();
            let _ = b.repaint();
            return;
        }
    }
    // Nothing textual consumed it — fall back to the internal node
    // clipboard (Cmd+C/V of canvas nodes; lowest priority).
    let mut b = inner.borrow_mut();
    if b.host_mut().apply_paste() {
        let _ = b.repaint();
    }
}

/// Route dropped PDFs into the Agent draft and keep image/SVG canvas drops.
/// Document/project drops are deliberately ignored; opening a project is an
/// explicit File-menu action and must not replace the current design by drag.
fn handle_drop_event<C: RepaintContext + 'static>(inner: &InnerRc<C>, evt: &web_sys::DragEvent) {
    let Some(dt) = evt.data_transfer() else {
        return;
    };
    let Some(files) = dt.files() else {
        return;
    };
    for index in 0..files.length() {
        if let Some(file) = files.get(index) {
            route_dropped_file(inner, file);
        }
    }
}

fn route_dropped_file<C: RepaintContext + 'static>(inner: &InnerRc<C>, file: web_sys::File) {
    let name = file.name();
    match file_actions::drop_kind(&name) {
        DropKind::Pdf => stage_chat_attachment_file(inner, file),
        DropKind::Document
        | DropKind::Figma
        | DropKind::Html
        | DropKind::HtmlResource
        | DropKind::Zip => {
            console_warn(&format!(
                "[drop] project import is disabled; use File -> Open: {name}"
            ));
        }
        DropKind::Svg => {
            let inner2 = inner.clone();
            read_file(
                file,
                ReadMode::Text,
                Box::new(move |value| match value.as_string() {
                    Some(svg) => insert_svg(&inner2, &svg, &name),
                    None => console_error("[import-svg] dropped file read produced no text"),
                }),
            );
        }
        DropKind::Image => {
            let inner2 = inner.clone();
            read_file(
                file,
                ReadMode::DataUrl,
                Box::new(move |value| match value.as_string() {
                    Some(url) => insert_image(&inner2, &url, &name),
                    None => console_error("[import-image] dropped file read produced no data URL"),
                }),
            );
        }
        DropKind::Unsupported => {
            console_warn(&format!("[drop] unsupported file type: {name}"));
        }
    }
}
