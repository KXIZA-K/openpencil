//! Document sink shared by the web design-agent executor.
use super::*;

pub(super) struct WebDesignDocSink<'a> {
    state: &'a Mutex<WebCanvasState>,
    hub: &'a SseHub,
    write_barrier: Option<&'a crate::web_canvas_server::WriteBarrier>,
    mirror: EditorState,
}

impl<'a> WebDesignDocSink<'a> {
    pub(super) fn new(
        state: &'a Mutex<WebCanvasState>,
        hub: &'a SseHub,
        write_barrier: Option<&'a crate::web_canvas_server::WriteBarrier>,
        mirror: EditorState,
    ) -> Self {
        Self {
            state,
            hub,
            write_barrier,
            mirror,
        }
    }
}

impl DocSink for WebDesignDocSink<'_> {
    fn state(&self) -> &EditorState {
        &self.mirror
    }

    fn apply(&mut self, cmd: EditorCommand) -> bool {
        let Ok(_write_pass) = admit_document_write(self.write_barrier) else {
            return false;
        };
        let (applied, tick, snapshot) = {
            let mut guard = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let applied = guard
                .apply_gated(cmd, op_editor_core::CollabEditSource::Ai)
                .unwrap_or(false);
            let tick = if applied {
                crate::design_session::fit_design_viewport_to_content(
                    &mut guard.editor,
                    1440.0,
                    900.0,
                );
                guard.version += 1;
                Some(guard.sse_tick())
            } else {
                None
            };
            (applied, tick, guard.editor.clone())
        };
        self.mirror = snapshot;
        if let Some(tick) = tick {
            self.hub.broadcast(tick);
        }
        applied
    }

    fn begin_undo_batch(&mut self) {}

    fn end_undo_batch(&mut self) {}
}

