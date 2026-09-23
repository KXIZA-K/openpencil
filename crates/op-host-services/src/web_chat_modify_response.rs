//! Fail closed before applying any edits from an incomplete model stream.
use op_ai::chat_provider::{ChatDelta, StopReason};

#[derive(Debug)]
pub(super) enum ResponseError {
    TokenLimit,
    Incomplete,
    Provider(String),
    Empty,
}

impl std::fmt::Display for ResponseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TokenLimit => f.write_str("Design response exceeded its token limit. No changes were applied. Try a smaller edit."),
            Self::Incomplete => f.write_str("Design response did not complete. No changes were applied."),
            Self::Provider(error) => write!(f, "Design provider failed: {error}. No changes were applied."),
            Self::Empty => f.write_str("The model returned an empty design response. No changes were applied."),
        }
    }
}

impl std::error::Error for ResponseError {}

pub(super) fn collect(stream: impl Iterator<Item = ChatDelta>) -> Result<String, ResponseError> {
    let mut text = String::new();
    let mut thinking_bytes = 0usize;
    let mut stop = None;
    let mut failed = None;
    for delta in stream {
        match delta {
            ChatDelta::TextDelta(part) => text.push_str(&part),
            ChatDelta::Thinking(part) => thinking_bytes += part.len(),
            ChatDelta::ToolUse { .. } => {}
            ChatDelta::Error(error) => {
                failed = Some(error);
                break;
            }
            ChatDelta::Done { stop_reason } => {
                stop = Some(stop_reason);
                break;
            }
        }
    }
    // Metadata only: never log prompts, reasoning, credentials or model output.
    eprintln!(
        "studio_modify_response content_bytes={} thinking_bytes={} stop={stop:?} provider_error={}",
        text.len(),
        thinking_bytes,
        failed.is_some()
    );
    if let Some(error) = failed {
        return Err(ResponseError::Provider(error));
    }
    match stop {
        Some(StopReason::EndTurn) => {}
        Some(StopReason::MaxTokens) => return Err(ResponseError::TokenLimit),
        _ => return Err(ResponseError::Incomplete),
    }
    if text.trim().is_empty() {
        return Err(ResponseError::Empty);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_content_is_accepted() {
        assert_eq!(
            collect(
                [
                    ChatDelta::TextDelta("[]".into()),
                    ChatDelta::Done {
                        stop_reason: StopReason::EndTurn
                    },
                ]
                .into_iter()
            )
            .unwrap(),
            "[]"
        );
    }

    #[test]
    fn reasoning_only_token_exhaustion_is_explicit() {
        assert!(matches!(
            collect(
                [
                    ChatDelta::Thinking("private reasoning".into()),
                    ChatDelta::Done {
                        stop_reason: StopReason::MaxTokens
                    },
                ]
                .into_iter()
            ),
            Err(ResponseError::TokenLimit)
        ));
    }

    #[test]
    fn partial_content_is_never_accepted() {
        for stop_reason in [
            StopReason::MaxTokens,
            StopReason::Aborted,
            StopReason::ToolUse,
        ] {
            assert!(collect(
                [
                    ChatDelta::TextDelta("[]".into()),
                    ChatDelta::Done { stop_reason },
                ]
                .into_iter()
            )
            .is_err());
        }
        assert!(collect([ChatDelta::TextDelta("[]".into())].into_iter()).is_err());
        assert!(collect(
            [
                ChatDelta::TextDelta("[]".into()),
                ChatDelta::Error("upstream".into()),
            ]
            .into_iter()
        )
        .is_err());
    }

    #[test]
    fn empty_completed_response_is_not_success() {
        assert!(matches!(
            collect(
                [ChatDelta::Done {
                    stop_reason: StopReason::EndTurn
                },]
                .into_iter()
            ),
            Err(ResponseError::Empty)
        ));
    }
}
