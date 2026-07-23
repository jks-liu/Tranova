import type {
  AppSettings,
  BootstrapData,
  FileJobResult,
  Glossary,
  PromptTemplate,
  Provider,
  TranslateRequest,
  TranslationResult,
} from "./types";

let apiBase = "";

export async function initializeApi() {
  if (import.meta.env.DEV || !("__TAURI_INTERNALS__" in window)) return;
  const { invoke } = await import("@tauri-apps/api/core");
  apiBase = await invoke<string>("server_url");
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${apiBase}/api${path}`, {
    ...init,
    headers: {
      ...(init?.body instanceof FormData ? {} : { "Content-Type": "application/json" }),
      ...init?.headers,
    },
  });

  if (!response.ok) {
    const body = await response.json().catch(() => null) as { error?: string } | null;
    throw new Error(body?.error || `${response.status} ${response.statusText}`);
  }
  if (response.status === 204) return undefined as T;
  return response.json() as Promise<T>;
}

export const api = {
  bootstrap: () => request<BootstrapData>("/bootstrap"),
  translate: (payload: TranslateRequest) =>
    request<TranslationResult>("/translate", { method: "POST", body: JSON.stringify(payload) }),
  translateFile: async (file: File, options: Omit<TranslateRequest, "text">) => {
    const form = new FormData();
    form.append("file", file);
    form.append("options", JSON.stringify(options));
    return request<FileJobResult>("/translate-file", { method: "POST", body: form });
  },
  saveProvider: (provider: Provider) =>
    request<Provider>("/providers", { method: "PUT", body: JSON.stringify(provider) }),
  deleteProvider: (id: string) => request<void>(`/providers/${id}`, { method: "DELETE" }),
  testProvider: (id: string) => request<{ message: string }>(`/providers/${id}/test`, { method: "POST" }),
  saveGlossary: (glossary: Glossary) =>
    request<Glossary>("/glossaries", { method: "PUT", body: JSON.stringify(glossary) }),
  deleteGlossary: (id: string) => request<void>(`/glossaries/${id}`, { method: "DELETE" }),
  savePrompt: (prompt: PromptTemplate) =>
    request<PromptTemplate>("/prompts", { method: "PUT", body: JSON.stringify(prompt) }),
  deletePrompt: (id: string) => request<void>(`/prompts/${id}`, { method: "DELETE" }),
  saveSettings: (settings: AppSettings) =>
    request<AppSettings>("/settings", { method: "PUT", body: JSON.stringify(settings) }),
  importData: (kind: "glossaries" | "prompts", data: unknown) =>
    request<BootstrapData>(`/import/${kind}`, { method: "POST", body: JSON.stringify(data) }),
};
