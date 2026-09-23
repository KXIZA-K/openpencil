//! Keep browser Studio hit-testing and focus aligned with the painted overlay.
use crate::PreviewSession;
use op_editor_ui::layout_scene::SceneNode;
use std::collections::HashSet;

impl PreviewSession {
    pub(crate) fn sync_binding_input(&mut self) {
        fn collect(
            session: &PreviewSession,
            node: &SceneNode,
            parent_hidden: bool,
            hidden: &mut HashSet<String>,
        ) {
            let visible = session
                .binding_sites
                .iter()
                .find(|site| {
                    site.node_id == node.id
                        && site.target == jian_core::binding::BindingTarget::Visible
                })
                .and_then(|site| {
                    site.expr
                        .eval(&session.runtime.state, None, Some(&node.id))
                        .0
                        .as_bool()
                })
                .unwrap_or(!node.hidden);
            let is_hidden = parent_hidden || !session.ui_actions.visibility_for(&node.id, visible);
            if is_hidden {
                hidden.insert(node.id.clone());
            }
            for child in &node.children {
                collect(session, child, is_hidden, hidden);
            }
        }
        let mut hidden = HashSet::new();
        if let Some(page) = self.scene.active_page() {
            for node in &page.children {
                collect(self, node, false, &mut hidden);
            }
        }
        if hidden.difference(&self.binding_hidden_ids).next().is_some() {
            self.runtime.gestures.reset();
            self.gesture_mappings.clear();
        }
        self.binding_hidden_ids = hidden;
        self.runtime.rebuild_spatial();
        let Some(document) = self.runtime.document.as_ref() else {
            return;
        };
        let allowed = |key| {
            document.tree.nodes.get(key).is_some_and(|node| {
                !self
                    .binding_hidden_ids
                    .contains(jian_core::document::tree::node_schema_id(&node.schema))
            })
        };
        let chain = self
            .runtime
            .focus
            .chain()
            .iter()
            .copied()
            .filter(|key| allowed(*key))
            .collect();
        self.runtime.spatial.retain(allowed);
        self.runtime.focus.set_chain(chain);
    }

    pub(crate) fn scope_pointer_hit(&mut self, id: Option<&str>) {
        let target = id.and_then(|id| {
            let document = self.runtime.document.as_ref()?;
            let key = *document.tree.by_id.get(id)?;
            let rect = self.runtime.node_scene_rect(key)?;
            Some(jian_core::spatial::NodeBBox { key, rect })
        });
        self.runtime.spatial.rebuild(target);
    }
}
