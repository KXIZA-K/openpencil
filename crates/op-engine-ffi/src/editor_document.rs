//! Sandbox `.op` Save / Save As for the mobile editor shells.
//!
//! The touch shells never expose a directory picker, so documents live in a
//! `documents/` directory under the private storage root every mobile editor
//! already passes to `op_create` (the same root `op-config-store` keys
//! settings persistence on). Saving reuses the exact canonical writer the
//! desktop's `doc_io::save_to_path` wraps —
//! `jian_ops_schema::image_table::write_document_with_extension` with
//! `EditorMeta::from_state` — so a file written here round-trips through
//! every other host.
//!
//! Flow: the More sheet's Save / Save As tile queues
//! `FileAction::Save`/`SaveAs`; `editor_auth::take_shell_action` routes it
//! to [`begin_save`]. A known path saves in place; otherwise the shared
//! save-name dialog opens and [`drain_confirmed_save`] performs the write
//! once the user confirms a name. No new shell action is involved — all
//! three platforms get the feature from the engine alone.

use crate::error::{FfiError, FfiResult};
use crate::lifecycle::Session;
use crate::OpStatus;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Longest accepted file-name stem, in bytes (UTF-8, cut on a char
/// boundary). Keeps names under every mobile filesystem's 255-byte cap
/// with room for the ` NNN.op` dedup suffix.
const STEM_BYTE_CAP: usize = 120;

/// Where the current document lives in the app sandbox, if it has been
/// saved there. Documents opened through the platform picker arrive as
/// bytes without a writable path, so they start (and re-start) as `None`.
#[derive(Default)]
pub(crate) struct DocumentSaveShellState {
    pub(crate) path: Option<PathBuf>,
}

/// Shell-action tail for document lifecycle: drain a confirmed save-name
/// dialog first, then any queued file action. Called by
/// `editor_auth::take_shell_action` after the auth / window / one-shot
/// request drains.
pub(crate) fn drain_document_actions(session: &mut Session) -> FfiResult<i32> {
    // A confirmed save-name dialog writes into the sandbox engine-side; no
    // shell action is involved.
    if drain_confirmed_save(session)? {
        return Ok(crate::editor_auth::SHELL_ACTION_NONE);
    }

    let pending = session
        .editor_mut()?
        .editor_state()
        .editor_ui
        .pending_file_action;
    match pending {
        Some(op_editor_core::FileAction::New) => {
            install_new_document(session)?;
            Ok(crate::editor_auth::SHELL_ACTION_NONE)
        }
        Some(op_editor_core::FileAction::Open) => {
            let host = session.editor_mut()?;
            host.editor_state_mut().editor_ui.pending_file_action = None;
            host.mark_editor_state_dirty();
            Ok(crate::editor_auth::SHELL_ACTION_OPEN_DOCUMENT)
        }
        Some(op_editor_core::FileAction::Save) => begin_save(session, false),
        Some(op_editor_core::FileAction::SaveAs) => begin_save(session, true),
        #[cfg(any(target_os = "ios", target_os = "android", target_env = "ohos", test))]
        Some(op_editor_core::FileAction::ExportImageConfirm)
        | Some(op_editor_core::FileAction::ExportDeckPdfSelection) => {
            crate::editor_export::stage_export(session, pending)
        }
        _ => Ok(crate::editor_auth::SHELL_ACTION_NONE),
    }
}

/// File ▸ New: atomically install the starter document.
fn install_new_document(session: &mut Session) -> FfiResult<()> {
    let starter_document = op_editor_core::EditorState::starter().doc;
    {
        let host = session.editor_mut()?;
        // Consume the one-shot request even when collaboration starts between
        // the press and this drain. A rejected replacement must not retry on
        // every later frame.
        host.editor_state_mut().editor_ui.pending_file_action = None;
        host.install_open_document(starter_document, None, None)
            .map_err(|_| {
                FfiError::new(
                    OpStatus::Busy,
                    "new document is blocked by the collaboration session",
                )
            })?;
    }

    session.selected = None;
    // The starter document has no sandbox binding; drop the outgoing one.
    forget_current_document(session);
    session.gesture.reset();
    session.user_interacted = false;
    session.fit_content_to_viewports();
    // Fitting mutates the host-owned viewport. Clone only afterwards so the
    // lightweight state used by page APIs remains identical to the live host.
    session.state = session
        .editor()
        .ok_or_else(|| FfiError::new(OpStatus::NotReady, "engine is not in editor mode"))?
        .editor_state()
        .clone();
    session.scene = op_pen_loader::editor_state_to_active_page_layout_scene(&session.state);
    session.request_redraw();
    Ok(())
}

/// Handle a queued `FileAction::Save` / `SaveAs`.
///
/// Save with a known sandbox path writes in place. A first save — and every
/// Save As — opens the engine-painted name dialog instead; the write happens
/// when [`drain_confirmed_save`] sees the confirmation.
pub(crate) fn begin_save(session: &mut Session, save_as: bool) -> FfiResult<i32> {
    {
        // Consume the one-shot request first so a failure below cannot
        // retry on every later frame.
        let host = session.editor_mut()?;
        host.editor_state_mut().editor_ui.pending_file_action = None;
    }
    if !save_as && session.document_save.path.is_some() {
        let path = session.document_save.path.clone().expect("checked above");
        write_current_document(session, &path)?;
        finish_successful_save(session, path, false);
        return Ok(crate::editor_auth::SHELL_ACTION_NONE);
    }
    let now_ms = session.now_ms;
    let host = session.editor_mut()?;
    let seed = seed_name(host.editor_state());
    host.editor_state_mut()
        .editor_ui
        .save_name_dialog
        .open_with(&seed, save_as, now_ms);
    host.mark_editor_state_dirty();
    session.request_redraw();
    Ok(crate::editor_auth::SHELL_ACTION_NONE)
}

/// Perform the write for a confirmed save-name dialog. Returns `true` when
/// a confirmation was drained (whether or not the write succeeded — on
/// failure the dialog stays open with the typed name so the user can retry,
/// and the error propagates to the shell).
pub(crate) fn drain_confirmed_save(session: &mut Session) -> FfiResult<bool> {
    // Save-first-time and Save As behave identically at the write: a fresh
    // unique target (dedupe rather than clobber a same-named document).
    let name = {
        let host = session.editor_mut()?;
        let dialog = &mut host.editor_state_mut().editor_ui.save_name_dialog;
        let Some(name) = dialog.take_confirmed_name() else {
            return Ok(false);
        };
        name
    };
    let target = unique_target_path(&documents_dir()?, &sanitize_stem(&name))?;
    write_current_document(session, &target)?;
    finish_successful_save(session, target, true);
    Ok(true)
}

/// Backgrounding flush: overwrite the current sandbox file when the
/// document has one and carries unsaved changes. A document that was never
/// saved is deliberately left alone — silently inventing a file (and a
/// name) for it would surprise more than it protects.
pub(crate) fn flush_on_suspend(session: &mut Session) {
    let Some(path) = session.document_save.path.clone() else {
        return;
    };
    let dirty = session
        .editor
        .as_ref()
        .is_some_and(|host| host.editor_state().is_dirty());
    if !dirty {
        return;
    }
    match write_current_document(session, &path) {
        Ok(()) => {
            if let Some(host) = session.editor.as_mut() {
                host.editor_state_mut().mark_saved_revision();
                host.mark_editor_state_dirty();
            }
        }
        Err(error) => {
            // Backgrounding cannot show UI; leave the document dirty so the
            // next foreground save retries, and surface a diagnostic.
            session.emit_runtime_error(2, &error.message, "op-engine-ffi/save");
        }
    }
}

/// The current document is being replaced (New / platform Open): its
/// sandbox binding and any stale name prompt must not survive onto the
/// incoming document.
pub(crate) fn forget_current_document(session: &mut Session) {
    session.document_save.path = None;
    if let Some(host) = session.editor.as_mut() {
        if host.editor_state().editor_ui.save_name_dialog.open {
            host.editor_state_mut().editor_ui.save_name_dialog.close();
            host.mark_editor_state_dirty();
        }
    }
}

fn finish_successful_save(session: &mut Session, path: PathBuf, close_dialog: bool) {
    if let Ok(host) = session.editor_mut() {
        let state = host.editor_state_mut();
        if close_dialog {
            state.editor_ui.save_name_dialog.close();
        }
        state.editor_ui.file_name_display = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        state.mark_saved_revision();
        host.mark_editor_state_dirty();
    }
    session.document_save.path = Some(path);
    session.request_redraw();
}

/// Stream the live editor state to `path` through the canonical writer,
/// via a sibling temp file so a mid-write crash never leaves a truncated
/// document at the destination.
fn write_current_document(session: &mut Session, path: &Path) -> FfiResult<()> {
    let host = session.editor_mut()?;
    let state = host.editor_state();
    let meta = op_pen_loader::EditorMeta::from_state(state);
    let thumbnails = jian_ops_schema::image_thumbs::capture_snapshot();

    let io_error = |stage: &str, error: std::io::Error| {
        FfiError::new(
            OpStatus::InvalidArg,
            format!("could not {stage} the document file: {error}"),
        )
    };
    let tmp = sibling_temp_path(path);
    let result = (|| {
        let file = std::fs::File::create(&tmp).map_err(|e| io_error("create", e))?;
        let mut writer = std::io::BufWriter::with_capacity(1024 * 1024, file);
        jian_ops_schema::image_table::write_document_with_extension(
            &mut writer,
            &state.doc,
            &thumbnails,
            "editorMeta",
            &meta,
        )
        .map_err(|error| {
            FfiError::new(
                OpStatus::InvalidArg,
                format!("could not encode the document: {error}"),
            )
        })?;
        writer.flush().map_err(|e| io_error("write", e))?;
        drop(writer);
        std::fs::rename(&tmp, path).map_err(|e| io_error("commit", e))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn sibling_temp_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document.op".to_owned());
    name.push_str(".tmp");
    path.with_file_name(format!(".{name}"))
}

/// `documents/` under the shell-provided private storage root. Test
/// binaries have no `op_create`-configured root, so they fall back to the
/// process config dir, which the harness redirects to a scratch directory.
fn documents_dir() -> FfiResult<PathBuf> {
    let root = match op_config_store::configured_user_root() {
        Some(root) => root,
        None => op_config_store::openpencil_dir().map_err(|error| {
            FfiError::new(
                OpStatus::NotReady,
                format!("no private storage root is available: {error}"),
            )
        })?,
    };
    let dir = root.join("documents");
    std::fs::create_dir_all(&dir).map_err(|error| {
        FfiError::new(
            OpStatus::NotReady,
            format!("could not create the documents directory: {error}"),
        )
    })?;
    Ok(dir)
}

/// Seed for the name dialog: the display name minus the canonical
/// extension, else the localized "Untitled" (未命名 for the default zh-CN
/// locale).
fn seed_name(state: &op_editor_core::EditorState) -> String {
    if let Some(name) = state
        .editor_ui
        .file_name_display
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        let lower = name.to_ascii_lowercase();
        let stem = if lower.ends_with(".op") || lower.ends_with(".pen") {
            &name[..name.rfind('.').expect("checked suffix")]
        } else {
            name
        };
        let cleaned = sanitize_stem(stem);
        // "untitled" is the sanitizer's empty-input fallback; only keep it
        // when the display name genuinely says so, otherwise prefer the
        // locale default below.
        if cleaned != "untitled" || stem.trim().eq_ignore_ascii_case("untitled") {
            return cleaned;
        }
    }
    sanitize_stem(op_i18n::translate(
        state.editor_ui.effective_locale(),
        "common.untitled",
    ))
}

/// Make a typed name safe as a file-name stem: strip path separators and
/// characters the mobile filesystems (or later export to other platforms)
/// reject, collapse leading/trailing dots and whitespace, and cap the
/// length. An empty result becomes `untitled`.
fn sanitize_stem(name: &str) -> String {
    let mut cleaned: String = name
        .chars()
        .map(|c| {
            if op_editor_core::save_name_keyboard::is_forbidden_file_name_char(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    cleaned = cleaned.trim().trim_matches('.').trim().to_owned();
    if cleaned.len() > STEM_BYTE_CAP {
        let mut cut = STEM_BYTE_CAP;
        while !cleaned.is_char_boundary(cut) {
            cut -= 1;
        }
        cleaned.truncate(cut);
        cleaned = cleaned.trim_end().to_owned();
    }
    if cleaned.is_empty() {
        "untitled".to_owned()
    } else {
        cleaned
    }
}

/// First free `<stem>.op`, `<stem> 2.op`, … path inside `dir`. Save As to
/// an already-used name never overwrites — it writes the numbered copy.
fn unique_target_path(dir: &Path, stem: &str) -> FfiResult<PathBuf> {
    let first = dir.join(format!("{stem}.op"));
    if !first.exists() {
        return Ok(first);
    }
    for counter in 2..1000 {
        let candidate = dir.join(format!("{stem} {counter}.op"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(FfiError::new(
        OpStatus::InvalidArg,
        format!("too many documents named \"{stem}\""),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_replaces_separators_and_never_returns_empty() {
        assert_eq!(sanitize_stem("my/design:v2"), "my design v2");
        assert_eq!(sanitize_stem("  ..  "), "untitled");
        assert_eq!(sanitize_stem("...hidden"), "hidden");
        assert_eq!(sanitize_stem("海报设计"), "海报设计");
        let long = "长".repeat(200);
        let capped = sanitize_stem(&long);
        assert!(capped.len() <= STEM_BYTE_CAP);
        assert!(capped.chars().all(|c| c == '长'));
    }

    #[test]
    fn unique_target_dedupes_with_numeric_suffixes() {
        let dir = std::env::temp_dir().join(format!(
            "openpencil-ffi-save-unique-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let first = unique_target_path(&dir, "poster").expect("first");
        assert_eq!(first, dir.join("poster.op"));
        std::fs::write(&first, b"x").expect("occupy first");
        let second = unique_target_path(&dir, "poster").expect("second");
        assert_eq!(second, dir.join("poster 2.op"));
        std::fs::write(&second, b"x").expect("occupy second");
        let third = unique_target_path(&dir, "poster").expect("third");
        assert_eq!(third, dir.join("poster 3.op"));
        std::fs::remove_dir_all(&dir).expect("clean temp dir");
    }
}
