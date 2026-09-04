use std::time::Duration;

use serde_json::{json, Value};

use super::model::{ProviderConfig, StepEnvelope};
use super::prompt::{strict_step_schema, CompiledPrompt};
use super::provider::{usage_from_value, InputTokenSemantics, ProviderFailure, ProviderUsage};
use super::validate::validate_provider_config;

pub fn build_openai_request(
    config: &ProviderConfig,
    prompt: &CompiledPrompt,
) -> Result<Value, ProviderFailure> {
    validate_provider_config(config)?;
    let ProviderConfig::OpenAiApi {
        model, cache_ttl, ..
    } = config
    else {
        return Err(ProviderFailure::InvalidConfiguration(
            super::model::RuntimeError::InvalidManifest("OpenAI API provider required".into()),
        ));
    };
    let stable = text(&prompt.stable)?;
    let dynamic = text(&prompt.dynamic)?;
    let schema = strict_step_schema();
    Ok(json!({
        "model": model,
        "max_output_tokens": 4096,
        "store": false,
        "input": [
            {
                "role": "developer",
                "content": [{
                    "type": "input_text",
                    "text": stable,
                    "prompt_cache_breakpoint": {"mode": "explicit"}
                }]
            },
            {
                "role": "user",
                "content": [{"type": "input_text", "text": dynamic}]
            }
        ],
        "prompt_cache_key": format!("codeunlimited:{}", prompt.stable_sha256),
        "prompt_cache_options": {"mode": "explicit", "ttl": cache_ttl.as_str()},
        "text": {
            "format": {
                "type": "json_schema",
                "name": "codeunlimited_step_envelope",
                "strict": true,
                "schema": schema
            }
        }
    }))
}

pub fn build_anthropic_request(
    config: &ProviderConfig,
    prompt: &CompiledPrompt,
) -> Result<Value, ProviderFailure> {
    validate_provider_config(config)?;
    let ProviderConfig::AnthropicApi {
        model, cache_ttl, ..
    } = config
    else {
        return Err(ProviderFailure::InvalidConfiguration(
            super::model::RuntimeError::InvalidManifest("Anthropic API provider required".into()),
        ));
    };
    let stable = text(&prompt.stable)?;
    let dynamic = text(&prompt.dynamic)?;
    let mut schema = strict_step_schema();
    sanitize_anthropic_schema(&mut schema);
    Ok(json!({
        "model": model,
        "max_tokens": 4096,
        "system": [{
            "type": "text",
            "text": stable,
            "cache_control": {"type": "ephemeral", "ttl": cache_ttl.as_str()}
        }],
        "messages": [{
            "role": "user",
            "content": [{"type": "text", "text": dynamic}]
        }],
        "output_config": {
            "format": {"type": "json_schema", "schema": schema}
        }
    }))
}

pub fn parse_api_response(
    config: &ProviderConfig,
    value: &Value,
) -> Result<(StepEnvelope, ProviderUsage), ProviderFailure> {
    let (text, semantics) = match config {
        ProviderConfig::OpenAiApi { .. } => (
            openai_output_text(value),
            InputTokenSemantics::TotalIncludesCache,
        ),
        ProviderConfig::AnthropicApi { .. } => (
            anthropic_output_text(value),
            InputTokenSemantics::UncachedOnly,
        ),
        _ => return Err(ProviderFailure::InvalidOutput),
    };
    let usage = usage_from_value(value.get("usage"), semantics);
    let failure = || ProviderFailure::InvalidOutputWithUsage(Box::new(usage.clone()));
    if matches!(config, ProviderConfig::OpenAiApi { .. })
        && value
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| status != "completed")
        || matches!(config, ProviderConfig::AnthropicApi { .. })
            && value
                .get("stop_reason")
                .and_then(Value::as_str)
                .is_some_and(|reason| reason != "end_turn")
    {
        return Err(failure());
    }
    let text = text.ok_or_else(failure)?;
    let envelope = serde_json::from_str(text).map_err(|_| failure())?;
    Ok((envelope, usage))
}

pub fn invoke_api(
    config: &ProviderConfig,
    prompt: &CompiledPrompt,
    timeout: Duration,
) -> Result<(StepEnvelope, ProviderUsage, usize, u64), ProviderFailure> {
    let (endpoint, key_env, body, anthropic) = match config {
        ProviderConfig::OpenAiApi {
            endpoint,
            api_key_env,
            ..
        } => (
            endpoint,
            api_key_env,
            build_openai_request(config, prompt)?,
            false,
        ),
        ProviderConfig::AnthropicApi {
            endpoint,
            api_key_env,
            ..
        } => (
            endpoint,
            api_key_env,
            build_anthropic_request(config, prompt)?,
            true,
        ),
        _ => return Err(ProviderFailure::InvalidOutput),
    };
    let key = std::env::var(key_env).map_err(|_| ProviderFailure::MissingCredential)?;
    let agent = ureq::AgentBuilder::new()
        .timeout(timeout)
        .redirects(0)
        .build();
    let mut request = agent.post(endpoint).set("content-type", "application/json");
    request = if anthropic {
        request
            .set("x-api-key", &key)
            .set("anthropic-version", "2023-06-01")
    } else {
        request.set("authorization", &format!("Bearer {key}"))
    };
    let started = std::time::Instant::now();
    let response = request.send_json(body).map_err(|_| ProviderFailure::Http)?;
    if !(200..300).contains(&response.status()) {
        return Err(ProviderFailure::Http);
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take((super::model::MAX_PROVIDER_OUTPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ProviderFailure::Http)?;
    if bytes.len() > super::model::MAX_PROVIDER_OUTPUT_BYTES {
        return Err(ProviderFailure::OutputTooLarge);
    }
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| ProviderFailure::InvalidOutput)?;
    let (envelope, usage) = parse_api_response(config, &value)?;
    Ok((
        envelope,
        usage,
        bytes.len(),
        started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    ))
}

fn text(bytes: &[u8]) -> Result<&str, ProviderFailure> {
    std::str::from_utf8(bytes).map_err(|_| ProviderFailure::InvalidOutput)
}

fn openai_output_text(value: &Value) -> Option<&str> {
    value
        .get("output_text")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("output")?
                .as_array()?
                .iter()
                .flat_map(|item| {
                    item.get("content")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                })
                .find_map(|content| content.get("text").and_then(Value::as_str))
        })
}

fn anthropic_output_text(value: &Value) -> Option<&str> {
    value
        .get("content")?
        .as_array()?
        .iter()
        .find(|block| block.get("type").and_then(Value::as_str) == Some("text"))?
        .get("text")?
        .as_str()
}

fn sanitize_anthropic_schema(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for key in ["minimum", "maximum", "minLength", "maxLength"] {
                object.remove(key);
            }
            for child in object.values_mut() {
                sanitize_anthropic_schema(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                sanitize_anthropic_schema(child);
            }
        }
        _ => {}
    }
}

use std::io::Read;

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::thread;
    use std::time::Duration;

    use serde_json::Value;

    use crate::runtime::model::{ApiCacheTtl, ProviderConfig};
    use crate::runtime::prompt::CompiledPrompt;
    use crate::runtime::provider::InputTokenSemantics;

    use super::{build_anthropic_request, build_openai_request, invoke_api, parse_api_response};

    fn prompt() -> CompiledPrompt {
        CompiledPrompt {
            bytes: b"combined-private".to_vec(),
            stable: b"stable-contract".to_vec(),
            dynamic: b"dynamic-state".to_vec(),
            codex_bootstrap: b"bootstrap".to_vec(),
            instructions_path: PathBuf::from("/tmp/provider-instructions.md"),
            stable_bytes: 15,
            dynamic_bytes: 13,
            stable_sha256: "ab".repeat(32),
            dynamic_sha256: "cd".repeat(32),
            prompt_sha256: "ef".repeat(32),
        }
    }

    fn openai() -> ProviderConfig {
        ProviderConfig::OpenAiApi {
            endpoint: "https://api.openai.com/v1/responses".into(),
            model: "gpt-5.6".into(),
            api_key_env: "OPENAI_API_KEY".into(),
            cache_ttl: ApiCacheTtl::ThirtyMinutes,
        }
    }

    fn anthropic() -> ProviderConfig {
        ProviderConfig::AnthropicApi {
            endpoint: "https://api.anthropic.com/v1/messages".into(),
            model: "claude-sonnet-4-6".into(),
            api_key_env: "ANTHROPIC_API_KEY".into(),
            cache_ttl: ApiCacheTtl::OneHour,
        }
    }

    #[test]
    fn openai_request_has_one_explicit_boundary_before_dynamic_state() {
        let body = build_openai_request(&openai(), &prompt()).expect("OpenAI body");

        assert_eq!(body["prompt_cache_options"]["mode"], "explicit");
        assert_eq!(body["prompt_cache_options"]["ttl"], "30m");
        assert_eq!(
            body["prompt_cache_key"],
            format!("codeunlimited:{}", "ab".repeat(32))
        );
        assert_eq!(body["input"][0]["role"], "developer");
        assert_eq!(body["input"][0]["content"][0]["text"], "stable-contract");
        assert_eq!(
            body["input"][0]["content"][0]["prompt_cache_breakpoint"]["mode"],
            "explicit"
        );
        assert_eq!(body["input"][1]["role"], "user");
        assert_eq!(body["input"][1]["content"][0]["text"], "dynamic-state");
        assert!(body.to_string().contains("json_schema"));
        assert!(!body.to_string().contains("combined-private"));
    }

    #[test]
    fn anthropic_request_caches_only_stable_system_block() {
        let body = build_anthropic_request(&anthropic(), &prompt()).expect("Anthropic body");

        assert_eq!(body["system"][0]["text"], "stable-contract");
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(body["system"][0]["cache_control"]["ttl"], "1h");
        assert_eq!(body["messages"][0]["content"][0]["text"], "dynamic-state");
        assert_eq!(body["output_config"]["format"]["type"], "json_schema");
        assert!(!body.to_string().contains("combined-private"));
    }

    #[test]
    fn api_responses_parse_envelope_and_provider_native_usage() {
        let envelope = serde_json::json!({
            "schema_version": 1,
            "base_revision": 0,
            "outcome": "continue",
            "summary": "bounded",
            "delta": {}
        });
        let openai_response = serde_json::json!({
            "output_text": envelope.to_string(),
            "usage": {
                "input_tokens": 1000,
                "input_tokens_details": {"cached_tokens": 800, "cache_write_tokens": 100},
                "output_tokens": 25
            }
        });
        let parsed = parse_api_response(&openai(), &openai_response).unwrap();
        assert_eq!(parsed.0.base_revision, 0);
        assert_eq!(
            parsed.1.input_token_semantics,
            InputTokenSemantics::TotalIncludesCache
        );
        assert_eq!(parsed.1.uncached_input_tokens, Some(100));

        let anthropic_response = serde_json::json!({
            "content": [{"type": "text", "text": envelope.to_string()}],
            "usage": {
                "input_tokens": 100,
                "cache_read_input_tokens": 800,
                "cache_creation_input_tokens": 100,
                "cache_creation": {"ephemeral_5m_input_tokens": 0, "ephemeral_1h_input_tokens": 100},
                "output_tokens": 25
            }
        });
        let parsed = parse_api_response(&anthropic(), &anthropic_response).unwrap();
        assert_eq!(
            parsed.1.input_token_semantics,
            InputTokenSemantics::UncachedOnly
        );
        assert_eq!(parsed.1.transported_input_tokens(), Some(1000));
        assert_eq!(parsed.1.cache_write_1h_input_tokens, Some(100));
    }

    #[test]
    fn failed_output_keeps_reported_usage() {
        let response = serde_json::json!({"output_text":"not valid JSON", "usage": {
            "input_tokens":100, "input_tokens_details":{"cached_tokens":80}, "output_tokens":3
        }});
        let error = parse_api_response(&openai(), &response).unwrap_err();
        assert_ne!(
            error,
            crate::runtime::provider::ProviderFailure::InvalidOutput,
            "known usage must survive invalid model output"
        );
        let crate::runtime::provider::ProviderFailure::InvalidOutputWithUsage(usage) = error else {
            panic!("missing usage")
        };
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.cache_read_input_tokens, Some(80));
    }

    #[test]
    fn malformed_or_body_only_errors_do_not_leak_content() {
        let private = Value::String("PRIVATE_RESPONSE_BODY".into());
        let error = parse_api_response(&openai(), &private).unwrap_err();
        assert!(!error.to_string().contains("PRIVATE_RESPONSE_BODY"));
    }

    #[test]
    fn api_schema_uses_supported_unions_and_requires_all_object_fields() {
        fn check(schema: &Value) {
            match schema {
                Value::Object(object) => {
                    assert!(!object.contains_key("oneOf"), "use disjoint anyOf variants");
                    if let Some(properties) = object.get("properties").and_then(Value::as_object) {
                        let required = object
                            .get("required")
                            .and_then(Value::as_array)
                            .expect("strict object must require every property");
                        assert_eq!(required.len(), properties.len());
                        for key in properties.keys() {
                            assert!(required.contains(&Value::String(key.clone())));
                        }
                    }
                    for child in object.values() {
                        check(child);
                    }
                }
                Value::Array(values) => {
                    for value in values {
                        check(value);
                    }
                }
                _ => {}
            }
        }
        let body = build_openai_request(&openai(), &prompt()).unwrap();
        check(&body["text"]["format"]["schema"]);
    }

    #[test]
    fn api_never_follows_redirects_with_credentials() {
        let destination = TcpListener::bind("127.0.0.1:0").unwrap();
        destination.set_nonblocking(true).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let redirect = destination.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 8192];
            let _ = stream.read(&mut request).unwrap();
            write!(stream, "HTTP/1.1 302 Found\r\nlocation: http://{redirect}/leak\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").unwrap();
        });
        let key_env = "CODEUNLIMITED_V21_TEST_REDIRECT_KEY";
        std::env::set_var(key_env, "PRIVATE_REDIRECT_KEY");
        let config = ProviderConfig::AnthropicApi {
            endpoint: format!("http://{address}/v1/messages"),
            model: "test-model".into(),
            api_key_env: key_env.into(),
            cache_ttl: ApiCacheTtl::FiveMinutes,
        };
        let result = invoke_api(&config, &prompt(), Duration::from_millis(300));
        std::env::remove_var(key_env);
        server.join().unwrap();
        assert!(result.is_err());
        assert!(
            destination.accept().is_err(),
            "credential-bearing redirect was followed"
        );
    }

    #[test]
    fn loopback_transport_sends_bearer_header_and_bounded_json_body() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let count = stream.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..count]);
                if let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    let header_end = header_end + 4;
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap();
                    if request.len() >= header_end + content_length {
                        break;
                    }
                }
            }
            let envelope = serde_json::json!({
                "schema_version": 1,
                "base_revision": 0,
                "outcome": "continue",
                "summary": "mock",
                "delta": {}
            });
            let response = serde_json::json!({
                "output_text": envelope.to_string(),
                "usage": {
                    "input_tokens": 100,
                    "input_tokens_details": {"cached_tokens": 80, "cache_write_tokens": 10},
                    "output_tokens": 5
                }
            })
            .to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
            request
        });
        let key_env = "CODEUNLIMITED_V21_TEST_OPENAI_KEY";
        std::env::set_var(key_env, "PRIVATE_TEST_KEY");
        let config = ProviderConfig::OpenAiApi {
            endpoint: format!("http://{address}/v1/responses"),
            model: "gpt-5.6".into(),
            api_key_env: key_env.into(),
            cache_ttl: ApiCacheTtl::ThirtyMinutes,
        };

        let result = invoke_api(&config, &prompt(), Duration::from_secs(2)).unwrap();
        std::env::remove_var(key_env);
        assert_eq!(result.0.base_revision, 0);
        assert_eq!(result.1.cache_read_input_tokens, Some(80));
        let request = String::from_utf8(server.join().unwrap()).unwrap();
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer private_test_key"));
        assert!(request.contains("\"prompt_cache_options\""));
    }
}
