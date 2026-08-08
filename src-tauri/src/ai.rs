use std::{error::Error as StdError, time::Duration};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use reqwest::{
    header::{HeaderMap, CONTENT_TYPE},
    multipart::{Form, Part},
    Client, Proxy, StatusCode,
};
use serde_json::{json, Value};
use thiserror::Error;

use crate::{
    models::{AppData, Provider, ProviderKind, TranslateRequest, TranslationResult},
    store::AppStore,
};

#[derive(Debug, Error)]
pub enum AiError {
    #[error("{0}")]
    Message(String),
    #[error("Network request failed: {0}")]
    Request(String),
}

const AI_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_REQUEST_ATTEMPTS: usize = 3;

struct ProviderResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

pub async fn translate(
    store: &AppStore,
    request: &TranslateRequest,
) -> Result<TranslationResult, AiError> {
    if request.text.trim().is_empty() {
        return Err(AiError::Message("Text to translate is empty".to_string()));
    }
    if request.source_language.trim().is_empty() || request.target_language.trim().is_empty() {
        return Err(AiError::Message(
            "Source and target languages are required".to_string(),
        ));
    }
    let data = store.snapshot();
    let provider = data
        .providers
        .iter()
        .find(|provider| provider.id == request.provider_id)
        .filter(|provider| provider.enabled)
        .cloned()
        .ok_or_else(|| {
            AiError::Message("Selected AI provider does not exist or is disabled".to_string())
        })?;
    let translated_text = translate_with_data(&data, &provider, request).await?;
    Ok(TranslationResult {
        translated_text,
        provider: provider.name,
        model: provider.model,
    })
}

pub async fn translate_batch(
    store: &AppStore,
    request: &TranslateRequest,
    texts: &[String],
) -> Result<Vec<String>, AiError> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    if request.source_language.trim().is_empty() || request.target_language.trim().is_empty() {
        return Err(AiError::Message(
            "Source and target languages are required".to_string(),
        ));
    }
    let data = store.snapshot();
    let provider = data
        .providers
        .iter()
        .find(|provider| provider.id == request.provider_id)
        .filter(|provider| provider.enabled)
        .cloned()
        .ok_or_else(|| {
            AiError::Message("Selected AI provider does not exist or is disabled".to_string())
        })?;
    let mut batch_request = request.clone();
    batch_request.text = serde_json::to_string(texts).map_err(|error| {
        AiError::Message(format!("Unable to encode translation batch: {error}"))
    })?;
    let instruction = format!(
        "The user text is a JSON array containing {} independent segments. Translate every segment in order. Return ONLY a valid JSON array of strings with exactly {} items. Do not add markdown, commentary, labels or code fences.",
        texts.len(),
        texts.len()
    );
    let response =
        translate_with_data_instruction(&data, &provider, &batch_request, Some(&instruction))
            .await?;
    parse_batch_response(&response, texts.len())
}

pub async fn translate_image(
    store: &AppStore,
    request: &TranslateRequest,
    filename: &str,
    image: Vec<u8>,
) -> Result<Vec<u8>, AiError> {
    let data = store.snapshot();
    let provider = data
        .providers
        .iter()
        .find(|provider| provider.id == request.provider_id)
        .filter(|provider| provider.enabled && provider.supports_images)
        .cloned()
        .ok_or_else(|| {
            AiError::Message("Selected provider does not support image input/output".to_string())
        })?;
    let client = build_client(&data.settings.proxy_url, data.settings.ai_timeout_seconds)?;
    let endpoint = if provider.base_url.ends_with("/images/edits") {
        provider.base_url.clone()
    } else {
        format!("{}/images/edits", provider.base_url.trim_end_matches('/'))
    };
    let prompt = format!(
        "Translate every readable text element in this image from {} to {}. Preserve all non-text pixels, composition, layout, visual style, font scale and colors. Replace the original text with the translation and return the edited image only.",
        request.source_language, request.target_language
    );
    let mime = mime_guess::from_path(filename)
        .first_or_octet_stream()
        .to_string();
    let output_format = image_output_format(filename)?;
    let image_part = Part::bytes(image)
        .file_name(filename.to_string())
        .mime_str(&mime)
        .map_err(|error| AiError::Message(format!("Unsupported image MIME type: {error}")))?;
    let form = Form::new()
        .text("model", provider.model.clone())
        .text("prompt", prompt)
        .text("output_format", output_format)
        .part("image", image_part);
    let mut call = client.post(endpoint).multipart(form);
    if !provider.api_key.trim().is_empty() {
        call = call.bearer_auth(provider.api_key.trim());
    }
    let response = call
        .send()
        .await
        .map_err(|error| request_error("image provider request", error))?;
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .map_err(|error| request_error("reading image provider response", error))?;
    if !status.is_success() {
        return Err(AiError::Message(provider_error(&body, status.as_u16())));
    }
    let encoded = body
        .pointer("/data/0/b64_json")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AiError::Message("Image provider response did not contain data[0].b64_json".to_string())
        })?;
    let decoded = BASE64.decode(encoded).map_err(|error| {
        AiError::Message(format!(
            "Image provider returned invalid base64 data: {error}"
        ))
    })?;
    if !image_format_matches(filename, &decoded) {
        return Err(AiError::Message(format!(
            "Image provider did not return the requested {output_format} format"
        )));
    }
    Ok(decoded)
}

fn image_output_format(filename: &str) -> Result<&'static str, AiError> {
    match filename
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Ok("png"),
        "jpg" | "jpeg" => Ok("jpeg"),
        "webp" => Ok("webp"),
        _ => Err(AiError::Message("Unsupported image format".to_string())),
    }
}

fn image_format_matches(filename: &str, bytes: &[u8]) -> bool {
    match filename
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "jpg" | "jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        _ => false,
    }
}

async fn translate_with_data(
    data: &AppData,
    provider: &Provider,
    request: &TranslateRequest,
) -> Result<String, AiError> {
    translate_with_data_instruction(data, provider, request, None).await
}

async fn translate_with_data_instruction(
    data: &AppData,
    provider: &Provider,
    request: &TranslateRequest,
    extra_system_instruction: Option<&str>,
) -> Result<String, AiError> {
    let (mut system, user) = build_messages(data, request);
    if let Some(instruction) = extra_system_instruction {
        system.push_str("\n\n");
        system.push_str(instruction);
    }
    let client = build_client(&data.settings.proxy_url, data.settings.ai_timeout_seconds)?;
    let response = match provider.kind {
        ProviderKind::Ollama => call_ollama(&client, provider, &system, &user).await?,
        _ => call_openai_compatible(&client, provider, &system, &user).await?,
    };
    let response = response.trim();
    if response.is_empty() {
        return Err(AiError::Message(
            "The AI provider returned an empty translation".to_string(),
        ));
    }
    Ok(response.to_string())
}

fn parse_batch_response(response: &str, expected: usize) -> Result<Vec<String>, AiError> {
    let cleaned = response
        .trim()
        .strip_prefix("```json")
        .or_else(|| response.trim().strip_prefix("```JSON"))
        .or_else(|| response.trim().strip_prefix("```"))
        .unwrap_or(response.trim())
        .strip_suffix("```")
        .unwrap_or_else(|| {
            response
                .trim()
                .strip_prefix("```json")
                .or_else(|| response.trim().strip_prefix("```JSON"))
                .or_else(|| response.trim().strip_prefix("```"))
                .unwrap_or(response.trim())
        })
        .trim();
    let value = serde_json::from_str::<Value>(cleaned)
        .or_else(|_| {
            let start = cleaned.find('[').ok_or_else(|| {
                serde_json::Error::io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "translation batch is not a JSON array",
                ))
            })?;
            let end = cleaned.rfind(']').ok_or_else(|| {
                serde_json::Error::io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "translation batch is not a JSON array",
                ))
            })?;
            serde_json::from_str(&cleaned[start..=end])
        })
        .map_err(|error| {
            AiError::Message(format!("AI returned an invalid translation batch: {error}"))
        })?;
    let items = value
        .as_array()
        .or_else(|| value.get("translations").and_then(Value::as_array))
        .ok_or_else(|| {
            AiError::Message("AI returned a translation batch that is not an array".to_string())
        })?;
    if items.len() != expected {
        return Err(AiError::Message(format!(
            "AI returned {} translations for {} input segments",
            items.len(),
            expected
        )));
    }
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            item.as_str().map(str::to_string).ok_or_else(|| {
                AiError::Message(format!(
                    "AI translation batch item {} is not text",
                    index + 1
                ))
            })
        })
        .collect()
}

fn build_client(proxy_url: &str, timeout_seconds: u64) -> Result<Client, AiError> {
    let mut builder = Client::builder();
    if !proxy_url.trim().is_empty() {
        let proxy = Proxy::all(proxy_url.trim())
            .map_err(|error| AiError::Message(format!("Invalid proxy URL: {error}")))?;
        builder = builder.proxy(proxy);
    }
    builder
        .connect_timeout(AI_CONNECT_TIMEOUT)
        .timeout(Duration::from_secs(timeout_seconds.max(1)))
        .build()
        .map_err(|error| request_error("building HTTP client", error))
}

fn build_messages(data: &AppData, request: &TranslateRequest) -> (String, String) {
    let glossary = data
        .glossaries
        .iter()
        .filter(|glossary| request.glossary_ids.contains(&glossary.id))
        .flat_map(|glossary| glossary.entries.iter())
        .map(
            |entry| match entry.note.as_deref().filter(|note| !note.trim().is_empty()) {
                Some(note) => format!("- {} => {} ({note})", entry.source, entry.target),
                None => format!("- {} => {}", entry.source, entry.target),
            },
        )
        .collect::<Vec<_>>()
        .join("\n");

    let template = request.prompt_id.as_ref()
        .and_then(|id| data.prompts.iter().find(|prompt| &prompt.id == id))
        .map(|prompt| prompt.content.as_str())
        .unwrap_or("Translate from {{source_language}} to {{target_language}}. Preserve the original meaning, formatting and markup. Return only the translation.\n\n{{glossary}}");
    let render = |input: &str| {
        input
            .replace("{{source_language}}", &request.source_language)
            .replace("{{target_language}}", &request.target_language)
            .replace(
                "{{glossary}}",
                if glossary.is_empty() {
                    "No special terminology."
                } else {
                    &glossary
                },
            )
    };
    let rendered = render(template);
    let has_text_slot = rendered.contains("{{text}}");
    let system = if has_text_slot {
        rendered.replace("{{text}}", "")
    } else {
        rendered
    };
    let user = if has_text_slot {
        render(template).replace("{{text}}", &request.text)
    } else {
        format!("Text to translate:\n{}", request.text)
    };
    (system, user)
}

async fn call_openai_compatible(
    client: &Client,
    provider: &Provider,
    system: &str,
    user: &str,
) -> Result<String, AiError> {
    let endpoint = if provider.base_url.ends_with("/chat/completions") {
        provider.base_url.clone()
    } else {
        format!(
            "{}/chat/completions",
            provider.base_url.trim_end_matches('/')
        )
    };
    let body = json!({
        "model": provider.model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ],
        "temperature": 0.2,
        "stream": false
    });
    let response = send_json_request(
        client,
        &endpoint,
        &body,
        (!provider.api_key.trim().is_empty()).then_some(provider.api_key.trim()),
        "OpenAI-compatible request",
    )
    .await?;
    let body = parse_provider_json(response, "OpenAI-compatible provider")?;
    content_from_openai_response(&body).ok_or_else(|| {
        AiError::Message("Provider response did not contain choices[0].message.content".to_string())
    })
}

async fn call_ollama(
    client: &Client,
    provider: &Provider,
    system: &str,
    user: &str,
) -> Result<String, AiError> {
    let endpoint = if provider.base_url.ends_with("/api/chat") {
        provider.base_url.clone()
    } else {
        format!("{}/api/chat", provider.base_url.trim_end_matches('/'))
    };
    let body = json!({
        "model": provider.model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ],
        "stream": false,
        "options": { "temperature": 0.2 }
    });
    let response = send_json_request(client, &endpoint, &body, None, "Ollama request").await?;
    let body = parse_provider_json(response, "Ollama")?;
    body.pointer("/message/content")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            AiError::Message("Ollama response did not contain message.content".to_string())
        })
}

async fn send_json_request(
    client: &Client,
    endpoint: &str,
    body: &Value,
    api_key: Option<&str>,
    operation: &str,
) -> Result<ProviderResponse, AiError> {
    let mut last_error = None;
    for attempt in 1..=MAX_REQUEST_ATTEMPTS {
        match send_json_attempt(client, endpoint, body, api_key).await {
            Ok(response) => return Ok(response),
            Err(error) if retryable_request_error(&error) && attempt < MAX_REQUEST_ATTEMPTS => {
                last_error = Some(error);
                tokio::time::sleep(Duration::from_millis(250 * attempt as u64)).await;
            }
            Err(error) => {
                let attempts = if last_error.is_some() {
                    format!(" after {attempt} attempts")
                } else {
                    String::new()
                };
                return Err(request_error(&format!("{operation}{attempts}"), error));
            }
        }
    }

    Err(request_error(
        &format!("{operation} after {MAX_REQUEST_ATTEMPTS} attempts"),
        last_error.expect("a request attempt must fail before reaching this point"),
    ))
}

async fn send_json_attempt(
    client: &Client,
    endpoint: &str,
    body: &Value,
    api_key: Option<&str>,
) -> Result<ProviderResponse, reqwest::Error> {
    let mut call = client.post(endpoint).json(body);
    if let Some(api_key) = api_key {
        call = call.bearer_auth(api_key);
    }
    let response = call.send().await?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.text().await?;
    Ok(ProviderResponse {
        status,
        headers,
        body,
    })
}

fn retryable_request_error(error: &reqwest::Error) -> bool {
    !error.is_timeout()
        && (error.is_connect() || error.is_request() || error.is_body() || error.is_decode())
}

fn request_error(operation: &str, error: reqwest::Error) -> AiError {
    let category = if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_body() || error.is_decode() {
        "response body"
    } else if error.is_request() {
        "request"
    } else {
        "unknown"
    };
    let mut details = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        details.push_str("; cause: ");
        details.push_str(&cause.to_string());
        source = cause.source();
    }
    let hint = if error.is_timeout() {
        "; the provider may have accepted the request but did not finish within the client timeout"
    } else {
        ""
    };
    AiError::Request(format!("{operation} [{category}]: {details}{hint}"))
}

fn parse_provider_json(response: ProviderResponse, provider_name: &str) -> Result<Value, AiError> {
    let status = response.status;
    let content_type = response
        .headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("unknown");
    if !status.is_success() {
        if let Ok(body) = serde_json::from_str::<Value>(&response.body) {
            return Err(AiError::Message(provider_error(&body, status.as_u16())));
        }
        return Err(AiError::Message(format!(
            "{provider_name} returned HTTP {status} (Content-Type: {content_type}): {}",
            response_excerpt(&response.body)
        )));
    }
    serde_json::from_str(&response.body).map_err(|error| {
        AiError::Message(format!(
            "{provider_name} returned invalid JSON (HTTP {status}, Content-Type: {content_type}): {error}; response body: {}",
            response_excerpt(&response.body)
        ))
    })
}

fn response_excerpt(body: &str) -> String {
    const MAX_RESPONSE_EXCERPT_CHARS: usize = 2_000;
    let excerpt = body
        .chars()
        .take(MAX_RESPONSE_EXCERPT_CHARS)
        .collect::<String>();
    if body.chars().count() > MAX_RESPONSE_EXCERPT_CHARS {
        format!("{excerpt}...")
    } else {
        excerpt
    }
}

fn content_from_openai_response(body: &Value) -> Option<String> {
    let content = body.pointer("/choices/0/message/content")?;
    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }
    content.as_array().map(|items| {
        items
            .iter()
            .filter_map(|item| {
                item.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| item.get("content").and_then(Value::as_str))
            })
            .collect::<String>()
    })
}

fn provider_error(body: &Value, status: u16) -> String {
    body.pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| body.get("error").and_then(Value::as_str))
        .or_else(|| body.get("message").and_then(Value::as_str))
        .map(|message| format!("Provider returned HTTP {status}: {message}"))
        .unwrap_or_else(|| format!("Provider returned HTTP {status}"))
}

#[cfg(test)]
mod tests {
    use super::{
        build_messages, image_format_matches, image_output_format, parse_batch_response,
        parse_provider_json, ProviderResponse,
    };
    use crate::models::{
        AppData, Glossary, GlossaryEntry, PromptTemplate, Provider, ProviderKind, TranslateRequest,
    };
    use reqwest::{header::HeaderMap, StatusCode};

    #[test]
    fn prompt_includes_every_selected_glossary() {
        let mut data = AppData::default();
        data.prompts.push(PromptTemplate {
            id: "custom".to_string(),
            name: "Custom".to_string(),
            content: "From {{source_language}} to {{target_language}}\n{{glossary}}".to_string(),
            updated_at: "now".to_string(),
        });
        data.glossaries = vec![
            glossary("one", "Tranova", "特译"),
            glossary("two", "studio", "工作室"),
            glossary("unused", "ignore", "忽略"),
        ];
        let request = TranslateRequest {
            text: "Tranova studio".to_string(),
            source_language: "English".to_string(),
            target_language: "Chinese".to_string(),
            provider_id: "provider".to_string(),
            prompt_id: Some("custom".to_string()),
            glossary_ids: vec!["one".to_string(), "two".to_string()],
        };

        let (system, user) = build_messages(&data, &request);
        assert!(system.contains("Tranova => 特译"));
        assert!(system.contains("studio => 工作室"));
        assert!(!system.contains("ignore"));
        assert!(user.contains("Tranova studio"));
    }

    #[test]
    fn image_output_keeps_the_document_media_type() {
        assert_eq!(
            image_output_format("word/media/image1.jpeg").unwrap(),
            "jpeg"
        );
        assert!(image_format_matches("image.png", b"\x89PNG\r\n\x1a\nrest"));
        assert!(image_format_matches("image.jpg", &[0xff, 0xd8, 0xff, 0xdb]));
        assert!(image_format_matches("image.webp", b"RIFF1234WEBPdata"));
        assert!(!image_format_matches("image.jpg", b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn batch_response_requires_the_expected_number_of_strings() {
        assert_eq!(
            parse_batch_response(r#"["one","two"]"#, 2).unwrap(),
            vec!["one", "two"]
        );
        assert!(parse_batch_response(r#"["one"]"#, 2).is_err());
    }

    #[test]
    fn provider_http_errors_keep_the_raw_response_context() {
        let error = parse_provider_json(
            ProviderResponse {
                status: StatusCode::BAD_GATEWAY,
                headers: HeaderMap::new(),
                body: "upstream model failed".to_string(),
            },
            "Ollama",
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("HTTP 502"));
        assert!(error.contains("upstream model failed"));
    }

    #[test]
    fn provider_json_errors_keep_content_type_and_body_context() {
        let error = parse_provider_json(
            ProviderResponse {
                status: StatusCode::OK,
                headers: HeaderMap::from_iter([(
                    reqwest::header::CONTENT_TYPE,
                    "text/plain".parse().unwrap(),
                )]),
                body: "not json".to_string(),
            },
            "OpenAI-compatible provider",
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("Content-Type: text/plain"));
        assert!(error.contains("not json"));
    }

    #[tokio::test]
    async fn openai_compatible_provider_response_is_parsed() {
        let (base_url, server) = mock_server(
            "/v1/chat/completions",
            serde_json::json!({ "choices": [{ "message": { "content": "你好" } }] }),
        )
        .await;
        let provider = provider(ProviderKind::Openai, format!("{base_url}/v1"));
        let response = super::translate_with_data(&AppData::default(), &provider, &request())
            .await
            .unwrap();
        server.abort();
        assert_eq!(response, "你好");
    }

    #[tokio::test]
    async fn ollama_provider_response_is_parsed() {
        let (base_url, server) = mock_server(
            "/api/chat",
            serde_json::json!({ "message": { "content": "本地译文" } }),
        )
        .await;
        let provider = provider(ProviderKind::Ollama, base_url);
        let response = super::translate_with_data(&AppData::default(), &provider, &request())
            .await
            .unwrap();
        server.abort();
        assert_eq!(response, "本地译文");
    }

    async fn mock_server(
        path: &'static str,
        response: serde_json::Value,
    ) -> (String, tokio::task::JoinHandle<()>) {
        use axum::{routing::post, Json, Router};
        let app = Router::new().route(path, post(move || async move { Json(response) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{address}"), server)
    }

    fn provider(kind: ProviderKind, base_url: String) -> Provider {
        Provider {
            id: "provider".to_string(),
            name: "Test provider".to_string(),
            kind,
            base_url,
            model: "test-model".to_string(),
            api_key: String::new(),
            enabled: true,
            supports_images: false,
        }
    }

    fn request() -> TranslateRequest {
        TranslateRequest {
            text: "Hello".to_string(),
            source_language: "English".to_string(),
            target_language: "Chinese".to_string(),
            provider_id: "provider".to_string(),
            prompt_id: None,
            glossary_ids: Vec::new(),
        }
    }

    fn glossary(id: &str, source: &str, target: &str) -> Glossary {
        Glossary {
            id: id.to_string(),
            name: id.to_string(),
            source_language: "en".to_string(),
            target_language: "zh".to_string(),
            entries: vec![GlossaryEntry {
                source: source.to_string(),
                target: target.to_string(),
                note: None,
            }],
            updated_at: "now".to_string(),
        }
    }
}
