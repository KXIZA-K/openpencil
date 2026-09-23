//! Source-owned browser fonts for a managed Studio renderer.
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::MessageEvent;
use crate::repaint_ctx::RepaintContext;

#[wasm_bindgen(module = "/src/op_source_fonts.js")]
extern "C" {
    #[wasm_bindgen(js_name = installSourceFonts)]
    fn install_source_fonts(raw: &str) -> js_sys::Promise;
}

pub(crate) fn install<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    if !crate::platform_chat_bridge::studio_managed() { return; }
    let Some(window) = web_sys::window() else { return; };
    let weak = Rc::downgrade(inner);
    let listener = Closure::<dyn FnMut(MessageEvent)>::wrap(Box::new(move |event: MessageEvent| {
        let Some(parent) = web_sys::window().and_then(|w| w.parent().ok().flatten()) else { return; };
        let Some(source) = event.source() else { return; };
        if !js_sys::Object::is(source.as_ref(), parent.as_ref()) || event.origin() == "null" { return; }
        let Some(raw) = event.data().as_string() else { return; };
        if raw.len() > 6 * 1024 * 1024 { return; }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else { return; };
        if value["type"] != "ids-agent:openpencil-fonts-install" { return; }
        let Some(id) = value["requestId"].as_str().filter(|s| !s.is_empty() && s.len() <= 80 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')) else { return; };
        let Some(fonts) = value["fonts"].as_array().filter(|a| a.len() <= 64) else { return; };
        let (id, origin, fonts, weak) = (id.to_owned(), event.origin(), serde_json::json!(fonts).to_string(), weak.clone());
        wasm_bindgen_futures::spawn_local(async move {
            let result = wasm_bindgen_futures::JsFuture::from(install_source_fonts(&fonts)).await;
            let mut ok = matches!(result, Ok(ref v) if v.as_bool() == Some(true));
            if ok {
                ok = false;
                if let Some(inner) = weak.upgrade() {
                    if let Ok(mut shell) = inner.try_borrow_mut() {
                        shell.host_mut().invalidate_layout_scene();
                        shell.host_mut().mark_editor_state_dirty();
                        ok = shell.repaint().is_ok();
                    }
                }
            }
            let payload = serde_json::json!({"type":"ids-agent:openpencil-fonts-result","requestId":id,"ok":ok});
            let _ = parent.post_message(&JsValue::from_str(&payload.to_string()), &origin);
        });
    }));
    if window.add_event_listener_with_callback("message", listener.as_ref().unchecked_ref()).is_ok() { listener.forget(); }
}
