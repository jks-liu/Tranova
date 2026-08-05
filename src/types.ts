export type ProviderKind =
  | "openai"
  | "deepseek"
  | "doubao"
  | "llama_cpp"
  | "lm_studio"
  | "ollama";

export interface Provider {
  id: string;
  name: string;
  kind: ProviderKind;
  baseUrl: string;
  model: string;
  apiKey: string;
  enabled: boolean;
  supportsImages: boolean;
}

export interface GlossaryEntry {
  source: string;
  target: string;
  note?: string;
}

export interface Glossary {
  id: string;
  name: string;
  sourceLanguage: string;
  targetLanguage: string;
  entries: GlossaryEntry[];
  updatedAt: string;
}

export interface PromptTemplate {
  id: string;
  name: string;
  content: string;
  updatedAt: string;
}

export interface AppSettings {
  language: "en" | "zh-CN";
  proxyUrl: string;
  webHost: string;
  webPort: number;
  maxChunkChars: number;
}

export interface BootstrapData {
  providers: Provider[];
  glossaries: Glossary[];
  prompts: PromptTemplate[];
  settings: AppSettings;
  history: HistoryEntry[];
  serverUrl: string;
}

export interface HistoryEntry {
  id: string;
  kind: "text" | "file" | string;
  sourceLanguage: string;
  targetLanguage: string;
  sourceText: string;
  translatedText: string;
  filename?: string;
  provider: string;
  model: string;
  createdAt: string;
}

export interface TranslateRequest {
  text: string;
  sourceLanguage: string;
  targetLanguage: string;
  providerId: string;
  promptId?: string;
  glossaryIds: string[];
}

export interface TranslationResult {
  translatedText: string;
  provider: string;
  model: string;
}

export interface FileJobResult {
  filename: string;
  mediaType: string;
  contentBase64: string;
  translatedSegments: number;
  skippedSegments: number;
}
