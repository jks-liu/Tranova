import { Check, Clipboard, Languages } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import { TranslationOptions, type TranslationOptionsValue } from "../components/TranslationOptions";
import type { BootstrapData } from "../types";

export function TranslatePage({ data, onReload }: { data: BootstrapData; onReload: () => Promise<void> }) {
  const { t } = useTranslation();
  const defaultProvider = useMemo(() => data.providers.find((provider) => provider.enabled)?.id || "", [data.providers]);
  const [options, setOptions] = useState<TranslationOptionsValue>({ sourceLanguage: "auto", targetLanguage: "zh", providerId: defaultProvider, promptId: data.prompts[0]?.id || "", glossaryIds: [] });
  const [source, setSource] = useState("");
  const [result, setResult] = useState("");
  const [working, setWorking] = useState(false);
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!options.providerId && defaultProvider) setOptions((current) => ({ ...current, providerId: defaultProvider }));
  }, [defaultProvider, options.providerId]);

  const translate = async () => {
    if (!source.trim() || !options.providerId) return;
    setWorking(true);
    setError("");
    try {
      const response = await api.translate({ text: source, ...options, promptId: options.promptId || undefined });
      setResult(response.translatedText);
      await onReload();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setWorking(false);
    }
  };

  const copy = async () => {
    await navigator.clipboard.writeText(result);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  return (
    <section className="page translate-page">
      <div className="page-heading">
        <div><h1>{t("translate.title")}</h1><p>{t("translate.subtitle")}</p></div>
      </div>
      <TranslationOptions value={options} onChange={setOptions} providers={data.providers} prompts={data.prompts} glossaries={data.glossaries} />
      <div className="editor-grid">
        <section className="editor-panel">
          <header><strong>{t("translate.source")}</strong><span>{t("translate.characters", { count: source.length })}</span></header>
          <textarea value={source} onChange={(event) => setSource(event.target.value)} placeholder={t("translate.placeholder")} autoFocus />
        </section>
        <section className="editor-panel result-panel">
          <header>
            <strong>{t("translate.target")}</strong>
            <button className="icon-button" onClick={copy} disabled={!result} title={t("translate.copy")} aria-label={t("translate.copy")}>
              {copied ? <Check size={18} /> : <Clipboard size={18} />}
            </button>
          </header>
          <div className={`translation-result ${!result ? "empty" : ""}`}>{result || t("translate.target")}</div>
        </section>
      </div>
      {error && <div className="alert error-alert">{error}</div>}
      <div className="action-row">
        <button className="primary-button" onClick={translate} disabled={working || !source.trim() || !options.providerId}>
          <Languages size={18} /> {working ? t("translate.working") : t("translate.action")}
        </button>
      </div>
    </section>
  );
}
