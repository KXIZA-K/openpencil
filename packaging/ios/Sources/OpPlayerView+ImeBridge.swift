import UIKit

/// The UITextViewDelegate half of the IME bridge. The conduit text view
/// itself (`ImeConduitTextView`, including its `deleteBackward` override)
/// lives in OpPlayerView.swift; this extension owns the composition and
/// insertion forwarding, and enforces the conduit's empty invariant:
/// outside a composition the conduit holds no text, so plain backspace
/// always reaches `deleteBackward` on an empty view and is forwarded to
/// the engine. Before the drain calls below existed, every composition
/// commit accumulated in the conduit; once non-empty, Chinese IMEs'
/// backspace deleted that invisible text instead of reaching the engine —
/// the on-device "typed text cannot be deleted" defect.
extension OpPlayerView {
    /// The engine's IME focus flipped; show or hide the system keyboard.
    func imeFocusChanged(_ focused: Bool) {
        if focused {
            if !imeTextView.isFirstResponder {
                imeTextView.becomeFirstResponder()
            }
        } else {
            if imeTextView.isFirstResponder {
                imeTextView.resignFirstResponder()
            }
        }
    }

    func textViewDidChange(_ textView: UITextView) {
        guard host.editorMode else { return }
        let marked = markedText(of: textView)
        if let marked {
            // Composition update: forward the marked text + caret.
            wasComposing = true
            let markedRange = textView.markedTextRange
            let selection = textView.selectedTextRange
            var selStart = 0
            var selEnd = 0
            if let markedRange, let selection {
                let startOffset = textView.offset(from: textView.beginningOfDocument, to: selection.start)
                let endOffset = textView.offset(from: textView.beginningOfDocument, to: selection.end)
                let markedStartOffset = textView.offset(from: textView.beginningOfDocument, to: markedRange.start)
                selStart = max(0, startOffset - markedStartOffset)
                selEnd = max(0, endOffset - markedStartOffset)
            }
            host.editorImePreedit(marked, selection: selStart..<selEnd)
        } else if wasComposing {
            // The composition ended. The conduit is empty outside a
            // composition (drained below and in shouldChangeTextIn), so the
            // full conduit text IS the commit payload — the candidate the
            // user chose, which can differ from the last preedit string
            // ("si" → "四"). An empty conduit means the composition was
            // cancelled; the empty commit clears the engine's preedit.
            wasComposing = false
            host.editorImeCommit(textView.text ?? "")
            drainImeConduit(textView)
        } else if !(textView.text ?? "").isEmpty {
            // Safety net: text landed without a composition and without
            // being forwarded from shouldChangeTextIn (some IMEs and
            // dictation insert directly). Forward it, then restore the
            // empty invariant so backspace keeps reaching the engine.
            host.editorText(textView.text)
            drainImeConduit(textView)
        }
        host.requestImmediateFrame()
    }

    func textView(
        _ textView: UITextView,
        shouldChangeTextIn range: NSRange,
        replacementText text: String
    ) -> Bool {
        guard host.editorMode else { return true }
        // While composing, the system owns the edit (preedit updates flow
        // through textViewDidChange).
        if markedText(of: textView) != nil {
            return true
        }
        if text == "\n" {
            host.editorKey(Int32(OpKey_Enter))
            host.requestImmediateFrame()
            return false
        }
        if range.length == 0 {
            // Plain insertion (typing / paste).
            host.editorText(text)
        } else {
            // Deletion fallback for IME paths that edit through the
            // delegate rather than UIKeyInput. With the empty invariant
            // holding, this branch is normally unreachable outside a
            // composition (there is nothing to delete).
            let atEnd = range.location + range.length >= (textView.text as NSString).length
            host.editorKey(Int32(atEnd ? OpKey_Backspace : OpKey_Delete))
        }
        host.requestImmediateFrame()
        return false
    }

    /// Restore the conduit's empty invariant. The engine owns the real
    /// text; any character left in the conduit makes the next backspace
    /// delete invisible conduit text instead of reaching the engine. Only
    /// drains while no composition is active (clearing under a marked
    /// range would yank the preedit out from under the IME); programmatic
    /// `text` writes do not re-enter the delegate and leave
    /// first-responder status untouched.
    private func drainImeConduit(_ textView: UITextView) {
        guard textView.markedTextRange == nil else { return }
        if !(textView.text ?? "").isEmpty {
            textView.text = ""
        }
    }

    private func markedText(of textView: UITextView) -> String? {
        guard let range = textView.markedTextRange else { return nil }
        return textView.text(in: range)
    }
}
