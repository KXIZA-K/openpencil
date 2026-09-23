//! Browser Studio adapter over the shared Preview runtime.
use crate::PreviewSession;
use op_editor_ui::layout_scene::LayoutScene;
#[cfg(target_arch = "wasm32")]
use op_editor_ui::layout_scene::SceneNode;
impl PreviewSession {
    pub fn use_authored_scene(&mut self, scene: LayoutScene) {
        self.scene = scene;
    }

    /// Semantic state for the browser's accessibility/testing adapter. This is
    /// ephemeral preview state, never written back to the design document.
    #[cfg(target_arch = "wasm32")]
    pub fn interaction_snapshot(&self) -> serde_json::Value {
        fn walk(nodes: &[SceneNode], out: &mut Vec<serde_json::Value>) {
            for node in nodes {
                if let Some(widget) = &node.widget {
                    out.push(serde_json::json!({ "id": node.id, "kind": widget.kind, "value": widget.value_str,
                        "checked": widget.checked, "x": node.bounds.origin.x, "y": node.bounds.origin.y,
                        "width": node.bounds.size.x, "height": node.bounds.size.y }));
                }
                walk(&node.children, out);
            }
        }
        let mut out = Vec::new();
        if let Some(page) = self.overlay_runtime_state(&self.scene).active_page() {
            walk(&page.children, &mut out);
        }
        serde_json::json!(out)
    }
}
