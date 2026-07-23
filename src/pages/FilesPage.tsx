import { Download, FileText, UploadCloud } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import { TranslationOptions, type TranslationOptionsValue } from "../components/TranslationOptions";
import type { BootstrapData, FileJobResult } from "../types";

function downloadResult(result: FileJobResult) {
  const bytes = Uint8Array.from(atob(result.contentBase64), (char) => char.charCodeAt(0));
  const blob = new Blob([bytes], { type: result.mediaType });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = result.filename;
  anchor.click();
  URL.revokeObjectURL(url);
}

export function FilesPage({ data }: { data: BootstrapData }) {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement>(null);
  const defaultProvider = useMemo(() => data.providers.find((provider) => provider.enabled)?.id || "", [data.providers]);
  const [options, setOptions] = useState<TranslationOptionsValue>({ sourceLanguage: "auto", targetLanguage: "zh", providerId: defaultProvider, promptId: data.prompts[0]?.id || "", glossaryIds: [] });
  const [file, setFile] = useState<File | null>(null);
  const [result, setResult] = useState<FileJobResult | null>(null);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState("");

  const choose = (next: File | undefined) => {
    if (next) {
      setFile(next);
      setResult(null);
      setError("");
    }
  };

  const translate = async () => {
    if (!file || !options.providerId) return;
    setWorking(true);
    setError("");
    try {
      setResult(await api.translateFile(file, { ...options, promptId: options.promptId || undefined }));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setWorking(false);
    }
  };

  return (
    <section className="page">
      <div className="page-heading"><div><h1>{t("files.title")}</h1><p>{t("files.subtitle")}</p></div></div>
      <TranslationOptions value={options} onChange={setOptions} providers={data.providers} prompts={data.prompts} glossaries={data.glossaries} />
      <button
        className={`drop-zone ${file ? "has-file" : ""}`}
        type="button"
        onClick={() => inputRef.current?.click()}
        onDragOver={(event) => event.preventDefault()}
        onDrop={(event) => { event.preventDefault(); choose(event.dataTransfer.files[0]); }}
      >
        {file ? <FileText size={34} /> : <UploadCloud size={34} />}
        <strong>{file?.name || t("files.drop")}</strong>
        <span>{file ? `${(file.size / 1024).toFixed(1)} KB` : t("files.supported")}</span>
      </button>
      <input ref={inputRef} className="visually-hidden" type="file" accept=".docx,.pptx,.xlsx,.txt,.md,.markdown,.html,.htm,.csv,.json,.srt,.vtt,.png,.jpg,.jpeg,.webp" onChange={(event) => choose(event.target.files?.[0])} />
      {error && <div className="alert error-alert">{error}</div>}
      {result && (
        <div className="file-result">
          <div><strong>{result.filename}</strong><span>{t("files.segments", { count: result.translatedSegments })}</span></div>
          <button className="secondary-button" onClick={() => downloadResult(result)}><Download size={18} /> {t("files.download")}</button>
        </div>
      )}
      <div className="action-row">
        <button className="primary-button" onClick={translate} disabled={!file || working || !options.providerId}>
          <FileText size={18} /> {working ? t("files.working") : t("files.action")}
        </button>
      </div>
    </section>
  );
}
