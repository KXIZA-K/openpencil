//! Live bindings share one visibility decision across painting and input.
use super::{display_string, PreviewSession};
use op_editor_ui::layout_scene::SceneNode;
use std::collections::HashSet;

impl PreviewSession {
    pub(super) fn binding_hidden(&self, node: &SceneNode) -> bool {
        self.binding_sites
            .iter()
            .find(|site| site.node_id == node.id && site.prop == "visible")
            .and_then(|site| {
                site.expr
                    .eval(&self.runtime.state, None, Some(&node.id))
                    .0
                    .as_bool()
            })
            .map(|visible| !visible)
            .unwrap_or(node.hidden)
    }

    pub(super) fn apply_binding_sites(&self, node: &mut SceneNode) {
        node.hidden = self.binding_hidden(node);
        for site in self
            .binding_sites
            .iter()
            .filter(|site| site.node_id == node.id)
        {
            let (value, _) = site.expr.eval(&self.runtime.state, None, Some(&node.id));
            match site.prop.as_str() {
                "content" => {
                    node.text = Some(display_string(&value));
                    node.text_runs.clear();
                }
                "checked" => {
                    if let (Some(widget), Some(checked)) = (node.widget.as_mut(), value.as_bool()) {
                        widget.checked = Some(checked);
                    }
                }
                "value" => {
                    if let Some(widget) = node.widget.as_mut() {
                        if let Some(number) = value
                            .as_f64()
                            .filter(|n| n.is_finite() && n.abs() <= f32::MAX as f64)
                        {
                            widget.value_num = Some(number as f32);
                        } else if let Some(text) = value.as_str() {
                            widget.value_str = Some(text.to_owned());
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn collect_hidden(&self, node: &SceneNode, inherited: bool, ids: &mut HashSet<String>) {
        let hidden = inherited || self.binding_hidden(node);
        if hidden {
            ids.insert(node.id.clone());
        }
        for child in &node.children {
            self.collect_hidden(child, hidden, ids);
        }
    }

    /// Rebuild from the runtime's active tree, preserving tabs/screen filtering.
    /// Mask descendants as well as the bound container; otherwise invisible
    /// child buttons and text fields would remain clickable or tabbable.
    pub(super) fn sync_binding_input(&mut self) {
        if !self.binding_sites.iter().any(|site| site.prop == "visible") {
            return;
        }
        let mut hidden = HashSet::new();
        if let Some(page) = self.scene.active_page() {
            for node in &page.children {
                self.collect_hidden(node, false, &mut hidden);
            }
        }
        if hidden.difference(&self.binding_hidden_ids).next().is_some() {
            self.runtime.gestures.reset();
            self.gesture_mapping = None;
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
}
