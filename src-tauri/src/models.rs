use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Openai,
    Deepseek,
    Doubao,
    LlamaCpp,
    LmStudio,
    Ollama,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub supports_images: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlossaryEntry {
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Glossary {
    pub id: String,
    pub name: String,
    pub source_language: String,
    pub target_language: String,
    #[serde(default)]
    pub entries: Vec<GlossaryEntry>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptTemplate {
    pub id: String,
    pub name: String,
    pub content: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub proxy_url: String,
    #[serde(default = "default_web_host")]
    pub web_host: String,
    #[serde(default = "default_web_port")]
    pub web_port: u16,
    #[serde(default = "default_chunk_chars")]
    pub max_chunk_chars: usize,
    #[serde(default = "default_concurrent_ai")]
    pub max_concurrent_ai: usize,
    #[serde(default = "default_ai_timeout_seconds")]
    pub ai_timeout_seconds: u64,
}

fn default_language() -> String {
    "en".to_string()
}
fn default_web_host() -> String {
    "127.0.0.1".to_string()
}
fn default_web_port() -> u16 {
    48731
}
fn default_chunk_chars() -> usize {
    6000
}
fn default_concurrent_ai() -> usize {
    3
}
fn default_ai_timeout_seconds() -> u64 {
    180
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            language: default_language(),
            proxy_url: String::new(),
            web_host: default_web_host(),
            web_port: default_web_port(),
            max_chunk_chars: default_chunk_chars(),
            max_concurrent_ai: default_concurrent_ai(),
            ai_timeout_seconds: default_ai_timeout_seconds(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppData {
    #[serde(default)]
    pub providers: Vec<Provider>,
    #[serde(default = "default_prompts")]
    pub prompts: Vec<PromptTemplate>,
    #[serde(default)]
    pub glossaries: Vec<Glossary>,
    #[serde(default)]
    pub settings: AppSettings,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
}

fn default_prompts() -> Vec<PromptTemplate> {
    vec![PromptTemplate {
        id: "faithful-translation".to_string(),
        name: "Faithful translation".to_string(),
        content: "Translate faithfully from {{source_language}} to {{target_language}}. Preserve the original meaning, tone, formatting, markup and line breaks. Use the terminology below when relevant. Return only the translated text.\n\n{{glossary}}".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
    }]
}

impl Default for AppData {
    fn default() -> Self {
        Self {
            providers: Vec::new(),
            prompts: default_prompts(),
            glossaries: Vec::new(),
            settings: AppSettings::default(),
            history: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub kind: String,
    pub source_language: String,
    pub target_language: String,
    #[serde(default)]
    pub source_text: String,
    #[serde(default)]
    pub translated_text: String,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslateOptions {
    pub source_language: String,
    pub target_language: String,
    pub provider_id: String,
    #[serde(default)]
    pub prompt_id: Option<String>,
    #[serde(default)]
    pub glossary_ids: Vec<String>,
    #[serde(default)]
    pub output_mode: FileOutputMode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FileOutputMode {
    #[default]
    Translated,
    Bilingual,
}

impl TranslateOptions {
    pub fn into_request(self) -> TranslateRequest {
        TranslateRequest {
            text: String::new(),
            source_language: self.source_language,
            target_language: self.target_language,
            provider_id: self.provider_id,
            prompt_id: self.prompt_id,
            glossary_ids: self.glossary_ids,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AppSettings, TranslateOptions};

    #[test]
    fn settings_default_timeout_is_180_seconds() {
        assert_eq!(AppSettings::default().ai_timeout_seconds, 180);
        let settings: AppSettings = serde_json::from_str(
            r#"{"language":"en","proxyUrl":"","webHost":"127.0.0.1","webPort":48731,"maxChunkChars":6000,"maxConcurrentAi":3}"#,
        )
        .unwrap();
        assert_eq!(settings.ai_timeout_seconds, 180);
    }

    #[test]
    fn file_translation_options_do_not_require_text() {
        let options: TranslateOptions = serde_json::from_str(
            r#"{"sourceLanguage":"en","targetLanguage":"zh","providerId":"provider","glossaryIds":[]}"#,
        )
        .unwrap();
        let request = options.into_request();
        assert!(request.text.is_empty());
        assert_eq!(request.target_language, "zh");
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslateRequest {
    pub text: String,
    pub source_language: String,
    pub target_language: String,
    pub provider_id: String,
    #[serde(default)]
    pub prompt_id: Option<String>,
    #[serde(default)]
    pub glossary_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationResult {
    pub translated_text: String,
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct FileProgress {
    pub stage: String,
    pub total_segments: usize,
    pub translated_segments: usize,
    pub skipped_segments: usize,
    pub total_batches: usize,
    pub completed_batches: usize,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileJobState {
    Queued,
    Processing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileJobStatus {
    pub id: String,
    pub filename: String,
    pub output_mode: FileOutputMode,
    pub state: FileJobState,
    pub stage: String,
    pub total_segments: usize,
    pub translated_segments: usize,
    pub skipped_segments: usize,
    pub total_batches: usize,
    pub completed_batches: usize,
    pub result_filename: String,
    pub media_type: String,
    pub error: Option<String>,
    pub created_at: String,
}
