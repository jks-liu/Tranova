import { Clock3, History as HistoryIcon, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import type { BootstrapData, HistoryEntry } from "../types";

export function HistoryPage({ data, onReload }: { data: BootstrapData; onReload: () => Promise<void> }) {
  const { t, i18n } = useTranslation();

  const remove = async (id: string) => {
    await api.deleteHistory(id);
    await onReload();
  };

  const clear = async () => {
    if (data.history.length === 0) return;
    await api.clearHistory();
    await onReload();
  };

  return (
    <section className="page history-page">
      <div className="page-heading">
        <div><h1>{t("history.title")}</h1><p>{t("history.subtitle")}</p></div>
        <button className="secondary-button" onClick={clear} disabled={data.history.length === 0}>
          <Trash2 size={17} /> {t("history.clear")}
        </button>
      </div>
      {data.history.length === 0 ? (
        <div className="empty-state"><HistoryIcon size={28} />{t("history.empty")}</div>
      ) : (
        <div className="item-list history-list">
          {data.history.map((entry) => <HistoryItem key={entry.id} entry={entry} locale={i18n.language} onRemove={remove} />)}
        </div>
      )}
    </section>
  );
}

function HistoryItem({ entry, locale, onRemove }: { entry: HistoryEntry; locale: string; onRemove: (id: string) => Promise<void> }) {
  const { t } = useTranslation();
  const date = Number(entry.createdAt);
  const timestamp = Number.isFinite(date) && date > 0
    ? new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" }).format(new Date(date * 1000))
    : entry.createdAt;
  const isFile = entry.kind === "file";

  return (
    <article className="list-item history-item">
      <div className="list-icon dark"><Clock3 size={19} /></div>
      <div className="list-content history-content">
        {isFile && <strong className="truncate">{entry.filename || t("history.file")}</strong>}
        <span>{entry.sourceLanguage} → {entry.targetLanguage} · {timestamp}</span>
        {!isFile && <div className="history-preview"><span>{entry.sourceText}</span><span>{entry.translatedText}</span></div>}
      </div>
      <button className="icon-button danger" onClick={() => onRemove(entry.id)} title={t("history.remove")} aria-label={t("history.remove")}>
        <Trash2 size={17} />
      </button>
    </article>
  );
}
