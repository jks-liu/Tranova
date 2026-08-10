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
  maxChunkSegments: number;
  maxConcurrentAi: number;
  maxBatchRetries: number;
  aiTimeoutSeconds: number;
  downloadLocation: DownloadLocation;
  customDownloadDirectory: string;
  autoDownloadFiles: boolean;
  askDownloadLocation: boolean;
  lastDownloadDirectory: string;
}

export type DownloadLocation = "source" | "downloads" | "custom";

export interface BootstrapData {
  version: string;
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

export type FileOutputMode = "translated" | "bilingual";

export type FileJobState = "queued" | "processing" | "completed" | "failed";

export interface FileJobStatus {
  id: string;
  filename: string;
  outputMode: FileOutputMode;
  state: FileJobState;
  stage: string;
  totalSegments: number;
  translatedSegments: number;
  failedSegments: number;
  skippedSegments: number;
  totalBatches: number;
  completedBatches: number;
  failedBatches: FileBatchFailure[];
  streamingBatch?: number;
  streamingSegments: number;
  streamingBatchSegments: number;
  streamingText?: string;
  resultFilename: string;
  mediaType: string;
  error?: string;
  createdAt: string;
  sourcePath?: string;
  downloadedPath?: string;
}

export interface FileBatchFailure {
  id: number;
  segmentCount: number;
  attempts: number;
  error: string;
}

export interface TranslateFileOptions extends Omit<TranslateRequest, "text"> {
  outputMode: FileOutputMode;
}
