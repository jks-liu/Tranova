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

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            language: default_language(),
            proxy_url: String::new(),
            web_host: default_web_host(),
            web_port: default_web_port(),
            max_chunk_chars: default_chunk_chars(),
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
        }
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileJobResult {
    pub filename: String,
    pub media_type: String,
    pub content_base64: String,
    pub translated_segments: usize,
    pub skipped_segments: usize,
}
