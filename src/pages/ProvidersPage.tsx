import { Bot, CheckCircle2, Pencil, Plus, Trash2, Wifi } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import { Modal } from "../components/Modal";
import type { BootstrapData, Provider, ProviderKind } from "../types";

const PROVIDER_DEFAULTS: Record<ProviderKind, { name: string; baseUrl: string; model: string }> = {
  openai: { name: "OpenAI", baseUrl: "https://api.openai.com/v1", model: "gpt-4.1-mini" },
  deepseek: { name: "DeepSeek", baseUrl: "https://api.deepseek.com", model: "deepseek-chat" },
  doubao: { name: "Doubao", baseUrl: "https://ark.cn-beijing.volces.com/api/v3", model: "" },
  llama_cpp: { name: "llama.cpp", baseUrl: "http://127.0.0.1:8080/v1", model: "local-model" },
  lm_studio: { name: "LM Studio", baseUrl: "http://127.0.0.1:1234/v1", model: "local-model" },
  ollama: { name: "Ollama", baseUrl: "http://127.0.0.1:11434", model: "qwen3:8b" },
};

function blankProvider(): Provider {
  const defaults = PROVIDER_DEFAULTS.openai;
  return { id: crypto.randomUUID(), kind: "openai", ...defaults, apiKey: "", enabled: true, supportsImages: false };
}

export function ProvidersPage({ data, onReload }: { data: BootstrapData; onReload: () => Promise<void> }) {
  const { t } = useTranslation();
  const [editing, setEditing] = useState<Provider | null>(null);
  const [testing, setTesting] = useState<string | null>(null);
  const [testMessage, setTestMessage] = useState("");
  const [error, setError] = useState("");

  const changeKind = (kind: ProviderKind) => {
    if (!editing) return;
    setEditing({ ...editing, kind, ...PROVIDER_DEFAULTS[kind] });
  };

  const save = async () => {
    if (!editing) return;
    try { await api.saveProvider(editing); setEditing(null); await onReload(); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
  };

  const test = async (id: string) => {
    setTesting(id); setError(""); setTestMessage("");
    try { const response = await api.testProvider(id); setTestMessage(response.message || t("provider.connected")); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setTesting(null); }
  };

  return (
    <section className="page">
      <div className="page-heading">
        <div><h1>{t("provider.title")}</h1><p>{t("provider.subtitle")}</p></div>
        <button className="primary-button" onClick={() => setEditing(blankProvider())}><Plus size={18} /> {t("provider.new")}</button>
      </div>
      {error && <div className="alert error-alert">{error}</div>}
      {testMessage && <div className="alert success-alert"><CheckCircle2 size={18} /> {testMessage}</div>}
      <div className="item-list">
        {data.providers.map((provider) => (
          <article className="list-item provider-item" key={provider.id}>
            <div className="list-icon dark"><Bot size={19} /></div>
            <div className="list-content"><strong>{provider.name}</strong><span>{provider.kind.replace("_", " ")} · {provider.model || t("common.noModel")}</span></div>
            <span className={`badge ${provider.enabled ? "enabled" : ""}`}>{provider.enabled ? t("common.enabled") : t("common.disabled")}</span>
            <div className="list-actions">
              <button className="icon-text-button compact" onClick={() => test(provider.id)} disabled={testing === provider.id}><Wifi size={16} /> {testing === provider.id ? t("common.testing") : t("common.test")}</button>
              <button className="icon-button" onClick={() => setEditing({ ...provider })}><Pencil size={17} /></button>
              <button className="icon-button danger" onClick={async () => { await api.deleteProvider(provider.id); await onReload(); }}><Trash2 size={17} /></button>
            </div>
          </article>
        ))}
      </div>
      {editing && (
        <Modal title={editing.name || t("provider.new")} onClose={() => setEditing(null)} footer={<><button className="secondary-button" onClick={() => setEditing(null)}>{t("common.cancel")}</button><button className="primary-button" onClick={save} disabled={!editing.name || !editing.baseUrl || !editing.model}>{t("common.save")}</button></>}>
          <div className="form-grid two-columns">
            <label><span>{t("provider.type")}</span><select value={editing.kind} onChange={(event) => changeKind(event.target.value as ProviderKind)}>{Object.keys(PROVIDER_DEFAULTS).map((kind) => <option value={kind} key={kind}>{kind.replace("_", " ")}</option>)}</select></label>
            <label><span>{t("common.name")}</span><input value={editing.name} onChange={(event) => setEditing({ ...editing, name: event.target.value })} /></label>
            <label className="full"><span>{t("provider.baseUrl")}</span><input value={editing.baseUrl} onChange={(event) => setEditing({ ...editing, baseUrl: event.target.value })} /></label>
            <label><span>{t("provider.model")}</span><input value={editing.model} onChange={(event) => setEditing({ ...editing, model: event.target.value })} /></label>
            <label><span>{t("provider.apiKey")}</span><input type="password" value={editing.apiKey} onChange={(event) => setEditing({ ...editing, apiKey: event.target.value })} placeholder={t("provider.apiKeyHint")} /></label>
            <label className="switch-row"><input type="checkbox" checked={editing.enabled} onChange={(event) => setEditing({ ...editing, enabled: event.target.checked })} /><span>{t("common.enabled")}</span></label>
            <label className="switch-row"><input type="checkbox" checked={editing.supportsImages} onChange={(event) => setEditing({ ...editing, supportsImages: event.target.checked })} /><span>{t("provider.images")}</span></label>
          </div>
        </Modal>
      )}
    </section>
  );
}
