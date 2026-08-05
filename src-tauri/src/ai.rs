use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use reqwest::{
    multipart::{Form, Part},
    Client, Proxy,
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
    Request(#[from] reqwest::Error),
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

pub async fn test_provider(store: &AppStore, id: &str) -> Result<String, AiError> {
    let data = store.snapshot();
    let provider = data
        .providers
        .iter()
        .find(|provider| provider.id == id)
        .cloned()
        .ok_or_else(|| AiError::Message("Provider was not found".to_string()))?;
    let request = TranslateRequest {
        text: "Reply with OK only.".to_string(),
        source_language: "English".to_string(),
        target_language: "English".to_string(),
        provider_id: id.to_string(),
        prompt_id: None,
        glossary_ids: Vec::new(),
    };
    translate_with_data(&data, &provider, &request).await?;
    Ok(format!("{} responded successfully", provider.name))
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
    let client = build_client(&data.settings.proxy_url)?;
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
    let response = call.send().await?;
    let status = response.status();
    let body: Value = response.json().await?;
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
    let (system, user) = build_messages(data, request);
    let client = build_client(&data.settings.proxy_url)?;
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

fn build_client(proxy_url: &str) -> Result<Client, AiError> {
    let mut builder = Client::builder().timeout(Duration::from_secs(180));
    if !proxy_url.trim().is_empty() {
        let proxy = Proxy::all(proxy_url.trim())
            .map_err(|error| AiError::Message(format!("Invalid proxy URL: {error}")))?;
        builder = builder.proxy(proxy);
    }
    builder.build().map_err(AiError::Request)
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
    let mut call = client.post(endpoint).json(&json!({
        "model": provider.model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ],
        "temperature": 0.2,
        "stream": false
    }));
    if !provider.api_key.trim().is_empty() {
        call = call.bearer_auth(provider.api_key.trim());
    }
    let response = call.send().await?;
    let status = response.status();
    let body: Value = response.json().await?;
    if !status.is_success() {
        return Err(AiError::Message(provider_error(&body, status.as_u16())));
    }
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
    let response = client
        .post(endpoint)
        .json(&json!({
            "model": provider.model,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user }
            ],
            "stream": false,
            "options": { "temperature": 0.2 }
        }))
        .send()
        .await?;
    let status = response.status();
    let body: Value = response.json().await?;
    if !status.is_success() {
        return Err(AiError::Message(provider_error(&body, status.as_u16())));
    }
    body.pointer("/message/content")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            AiError::Message("Ollama response did not contain message.content".to_string())
        })
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
    use super::{build_messages, image_format_matches, image_output_format};
    use crate::models::{
        AppData, Glossary, GlossaryEntry, PromptTemplate, Provider, ProviderKind, TranslateRequest,
    };

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
