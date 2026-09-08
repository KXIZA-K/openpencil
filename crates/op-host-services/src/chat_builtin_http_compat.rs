//! Bounded compatibility negotiation for LiteLLM's OpenAI SDK transport.
//!
//! Direct GLM endpoints accept top-level `thinking`. Some LiteLLM deployments
//! reject it before inference and require SDK extra parameters in `extra_body`.
//! Only negotiate after that exact validation rejection, never after a partial
//! stream or an ambiguous failure (which could duplicate a design operation).

use reqwest::{RequestBuilder, Response, StatusCode};
use serde_json::Value;

fn sdk_body(builder: &RequestBuilder) -> Option<Value> {
    let request = builder.try_clone()?.build().ok()?;
    let mut body: Value = serde_json::from_slice(request.body()?.as_bytes()?).ok()?;
    let obj = body.as_object_mut()?;
    let thinking = obj.remove("thinking")?;
    let extra = obj
        .entry("extra_body")
        .or_insert_with(|| serde_json::json!({}));
    let extra = extra.as_object_mut()?;
    // Do not silently overwrite conflicting caller-specified parameters.
    if extra
        .get("thinking")
        .is_some_and(|existing| existing != &thinking)
    {
        return None;
    }
    extra.insert("thinking".into(), thinking);
    Some(body)
}

fn thinking_validation_error(bytes: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return false;
    };
    let Some(message) = value.pointer("/error/message").and_then(Value::as_str) else {
        return false;
    };
    message.contains("litellm.UnsupportedParamsError:")
        && message.contains("does not support parameters: ['thinking']")
}

pub(super) async fn send(
    label: &str,
    build: &impl Fn() -> RequestBuilder,
) -> Result<Response, reqwest::Error> {
    let builder = build();
    let adapted = (label == "openai-compatible")
        .then(|| sdk_body(&builder))
        .flatten();
    let mut response = builder.send().await?;
    if response.status() != StatusCode::BAD_REQUEST || adapted.is_none() {
        return Ok(response);
    }
    // Never surface the untrusted body: providers can echo credentials in it.
    // Reading is bounded, and a malformed/oversized error remains the original
    // HTTP 400. Callers use the status, not this consumed error response body.
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > 32 * 1024 {
            return Ok(response);
        }
        bytes.extend_from_slice(&chunk);
    }
    if !thinking_validation_error(&bytes) {
        return Ok(response);
    }
    build().json(&adapted.unwrap()).send().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn negotiates_once_after_validation_failure_and_preserves_auth() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/chat/completions", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for attempt in 0..2 {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0; 1];
                while !request.ends_with(b"\r\n\r\n") {
                    socket.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let headers = String::from_utf8(request).unwrap().to_lowercase();
                assert!(headers.contains("authorization: bearer test-key"));
                let length: usize = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                let mut body = vec![0; length];
                socket.read_exact(&mut body).unwrap();
                let body: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(body["model"], "glm-5.3-flash");
                let (status, response) = if attempt == 0 {
                    assert_eq!(body["thinking"]["type"], "disabled");
                    (
                        "400 Bad Request",
                        r#"{"error":{"message":"litellm.UnsupportedParamsError: openai does not support parameters: ['thinking']"}}"#,
                    )
                } else {
                    assert!(body.get("thinking").is_none());
                    assert_eq!(body["extra_body"]["thinking"]["type"], "disabled");
                    ("200 OK", "data: [DONE]\n\n")
                };
                write!(socket, "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{response}", response.len()).unwrap();
            }
        });
        let client = reqwest::Client::new();
        let response = send("openai-compatible", &|| {
            client
                .post(&url)
                .bearer_auth("test-key")
                .json(&serde_json::json!({"model":"glm-5.3-flash","thinking":{"type":"disabled"}}))
        })
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), "data: [DONE]\n\n");
        server.join().unwrap();
    }

    #[test]
    fn preserves_model_messages_and_reasoning_without_overwriting_extra_fields() {
        let builder = reqwest::Client::new()
            .post("https://example.com/v1/chat/completions")
            .json(&serde_json::json!({"model":"glm-5.3-flash", "messages":[],
                "thinking":{"type":"disabled"}, "extra_body":{"another":true}}));
        let result = sdk_body(&builder).unwrap();
        assert!(result.get("thinking").is_none());
        assert_eq!(result["model"], "glm-5.3-flash");
        assert_eq!(result["messages"], serde_json::json!([]));
        assert_eq!(result["extra_body"]["thinking"]["type"], "disabled");
        assert_eq!(result["extra_body"]["another"], true);
    }

    #[test]
    fn only_recognizes_the_single_unsupported_thinking_parameter() {
        let error = |message| {
            serde_json::to_vec(&serde_json::json!({"error":{"message":message}})).unwrap()
        };
        assert!(thinking_validation_error(&error(
            "litellm.UnsupportedParamsError: openai does not support parameters: ['thinking'], for model=glm-5.3-flash"
        )));
        for message in [
            "Bad request",
            "thinking is invalid",
            "litellm.UnsupportedParamsError: openai does not support parameters: ['thinking', 'tools']",
        ] {
            assert!(!thinking_validation_error(&error(message)));
        }
        assert!(!thinking_validation_error(b"not JSON"));
    }

    #[test]
    fn ignores_standard_requests_and_conflicting_extra_body() {
        for body in [
            serde_json::json!({"reasoning_effort":"low"}),
            serde_json::json!({"thinking":{"type":"disabled"},"extra_body":{"thinking":{"type":"enabled"}}}),
            serde_json::json!({"thinking":{"type":"disabled"},"extra_body":"invalid"}),
        ] {
            let builder = reqwest::Client::new()
                .post("https://example.com")
                .json(&body);
            assert!(sdk_body(&builder).is_none());
        }
    }
}
