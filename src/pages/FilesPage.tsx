import { Download, FileText, RefreshCw, UploadCloud } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import { TranslationOptions, type TranslationOptionsValue } from "../components/TranslationOptions";
import { downloadBlob } from "../utils";
import type { BootstrapData, FileJobStatus, FileOutputMode } from "../types";

export function FilesPage({ data, onReload }: { data: BootstrapData; onReload: () => Promise<void> }) {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement>(null);
  const defaultProvider = useMemo(() => data.providers.find((provider) => provider.enabled)?.id || "", [data.providers]);
  const [options, setOptions] = useState<TranslationOptionsValue>(() => readOptions(defaultProvider, data.prompts[0]?.id || ""));
  const [outputMode, setOutputMode] = useState<FileOutputMode>(() => readOutputMode());
  const [file, setFile] = useState<File | null>(null);
  const [jobs, setJobs] = useState<FileJobStatus[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [downloadingId, setDownloadingId] = useState("");
  const [error, setError] = useState("");
  const knownStates = useRef(new Map<string, FileJobStatus["state"]>());

  const choose = useCallback((next: File | undefined) => {
    if (next) {
      setFile(next);
      setError("");
    }
  }, []);

  const refreshJobs = useCallback(async () => {
    try {
      const next = await api.fileJobs();
      const firstLoad = knownStates.current.size === 0;
      const completedSinceLastRefresh = next.some((job) => job.state === "completed" && knownStates.current.has(job.id) && knownStates.current.get(job.id) !== "completed");
      for (const job of next) knownStates.current.set(job.id, job.state);
      setJobs(next);
      if (completedSinceLastRefresh || (firstLoad && next.some((job) => job.state === "completed"))) void onReload();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }, [onReload]);

  useEffect(() => {
    if (!options.providerId && defaultProvider) setOptions((current) => ({ ...current, providerId: defaultProvider }));
    if (options.promptId && !data.prompts.some((prompt) => prompt.id === options.promptId)) {
      setOptions((current) => ({ ...current, promptId: data.prompts[0]?.id || "" }));
    }
  }, [data.prompts, defaultProvider, options.providerId, options.promptId]);

  useEffect(() => {
    localStorage.setItem("tranova-file-options", JSON.stringify(options));
  }, [options]);

  useEffect(() => {
    localStorage.setItem("tranova-file-output-mode", outputMode);
  }, [outputMode]);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/webview")
      .then(({ getCurrentWebview }) => getCurrentWebview().onDragDropEvent(async (event) => {
        if (event.payload.type !== "drop" || !event.payload.paths[0]) return;
        try {
          const dropped = await api.readDroppedFile(event.payload.paths[0]);
          if (disposed) return;
          const bytes = Uint8Array.from(atob(dropped.contentBase64), (character) => character.charCodeAt(0));
          choose(new File([bytes], dropped.name, { type: mimeTypeForName(dropped.name) }));
        } catch (reason) {
          if (!disposed) setError(reason instanceof Error ? reason.message : String(reason));
        }
      }))
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch((reason) => {
        if (!disposed) setError(reason instanceof Error ? reason.message : String(reason));
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [choose]);

  useEffect(() => {
    let disposed = false;
    const load = async () => {
      try {
        const next = await api.fileJobs();
        if (disposed) return;
        const firstLoad = knownStates.current.size === 0;
        const completedSinceLastRefresh = next.some((job) => job.state === "completed" && knownStates.current.has(job.id) && knownStates.current.get(job.id) !== "completed");
        for (const job of next) knownStates.current.set(job.id, job.state);
        setJobs(next);
        if (completedSinceLastRefresh || (firstLoad && next.some((job) => job.state === "completed"))) void onReload();
      } catch (reason) {
        if (!disposed) setError(reason instanceof Error ? reason.message : String(reason));
      }
    };
    void load();
    const timer = window.setInterval(() => void load(), 900);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, [onReload]);

  const translate = async () => {
    if (!file || !options.providerId) return;
    setSubmitting(true);
    setError("");
    try {
      const job = await api.translateFile(file, {
        ...options,
        promptId: options.promptId || undefined,
        outputMode,
      });
      knownStates.current.set(job.id, job.state);
      setJobs((current) => [job, ...current.filter((item) => item.id !== job.id)]);
      setFile(null);
      if (inputRef.current) inputRef.current.value = "";
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setSubmitting(false);
    }
  };

  const downloadJob = async (job: FileJobStatus) => {
    if (job.state !== "completed") return;
    setDownloadingId(job.id);
    setError("");
    try {
      downloadBlob(job.resultFilename, await api.downloadFileJob(job.id));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setDownloadingId("");
    }
  };

  const changeOptions = (next: TranslationOptionsValue) => {
    setOptions(next);
    localStorage.setItem("tranova-file-options", JSON.stringify(next));
  };

  return (
    <section className="page files-page">
      <div className="page-heading"><div><h1>{t("files.title")}</h1><p>{t("files.subtitle")}</p></div></div>
      <TranslationOptions value={options} onChange={changeOptions} providers={data.providers} prompts={data.prompts} glossaries={data.glossaries} />
      <div className="file-output-options">
        <label>
          <span>{t("files.mode")}</span>
          <select value={outputMode} onChange={(event) => setOutputMode(event.target.value as FileOutputMode)}>
            <option value="translated">{t("files.translatedOnly")}</option>
            <option value="bilingual">{t("files.bilingual")}</option>
          </select>
        </label>
      </div>
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
      <div className="action-row">
        <button className="primary-button" onClick={translate} disabled={!file || submitting || !options.providerId}>
          <FileText size={18} /> {submitting ? t("files.working") : t("files.action")}
        </button>
        <button className="icon-text-button" onClick={() => void refreshJobs()} title={t("files.refresh")} aria-label={t("files.refresh")}>
          <RefreshCw size={17} /> {t("files.refresh")}
        </button>
      </div>
      <section className="file-jobs-section">
        <header className="section-heading"><h2>{t("files.jobs")}</h2></header>
        {jobs.length === 0 ? <div className="empty-state">{t("files.noJobs")}</div> : (
          <div className="file-job-list">
            {jobs.map((job) => {
              const percent = progressPercent(job);
              return (
                <article className={`file-job ${job.state}`} key={job.id}>
                  <div className="file-job-heading">
                    <div className="file-job-name"><strong>{job.filename}</strong><span>{job.outputMode === "bilingual" ? t("files.bilingual") : t("files.translatedOnly")}</span></div>
                    <span className={`badge ${job.state === "completed" ? "enabled" : ""}`}>{t(`files.${job.state}`)}</span>
                  </div>
                  <div className="progress-track" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent}>
                    <div className="progress-fill" style={{ width: `${percent}%` }} />
                  </div>
                  <div className="file-job-meta">
                    <span>{t(`files.${job.stage}`)}</span>
                    <span>{t("files.segments", { done: job.translatedSegments, total: job.totalSegments })}</span>
                    <span>{t("files.batches", { done: job.completedBatches, total: job.totalBatches })}</span>
                    {job.skippedSegments > 0 && <span>{t("files.skipped", { count: job.skippedSegments })}</span>}
                  </div>
                  {job.error && <div className="file-job-error">{job.error}</div>}
                  {job.state === "completed" && (
                    <div className="file-job-actions">
                      <span className="muted">{job.resultFilename}</span>
                      <button className="secondary-button" onClick={() => void downloadJob(job)} disabled={downloadingId === job.id}>
                        <Download size={18} /> {t("files.download")}
                      </button>
                    </div>
                  )}
                </article>
              );
            })}
          </div>
        )}
      </section>
    </section>
  );
}

function progressPercent(job: FileJobStatus) {
  if (job.state === "completed") return 100;
  if (job.totalSegments > 0) return Math.min(99, Math.round((job.translatedSegments / job.totalSegments) * 100));
  return job.state === "processing" ? 4 : 0;
}

function readOptions(defaultProvider: string, defaultPrompt: string): TranslationOptionsValue {
  const fallback: TranslationOptionsValue = {
    sourceLanguage: "auto",
    targetLanguage: "zh",
    providerId: defaultProvider,
    promptId: defaultPrompt,
    glossaryIds: [],
  };
  try {
    const stored = JSON.parse(localStorage.getItem("tranova-file-options") || "null") as Partial<TranslationOptionsValue> | null;
    if (!stored || typeof stored !== "object") return fallback;
    return {
      ...fallback,
      ...stored,
      glossaryIds: Array.isArray(stored.glossaryIds) ? stored.glossaryIds : [],
    };
  } catch {
    return fallback;
  }
}

function readOutputMode(): FileOutputMode {
  return localStorage.getItem("tranova-file-output-mode") === "bilingual" ? "bilingual" : "translated";
}

function mimeTypeForName(filename: string) {
  const extension = filename.split(".").pop()?.toLowerCase();
  const types: Record<string, string> = {
    docx: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    pptx: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    xlsx: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    txt: "text/plain",
    md: "text/markdown",
    markdown: "text/markdown",
    html: "text/html",
    htm: "text/html",
    csv: "text/csv",
    json: "application/json",
  };
  return (extension && types[extension]) || "application/octet-stream";
}
