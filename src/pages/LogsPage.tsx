import { Bug, Clock3, MessageSquareText } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import type { AiConversationLog, BootstrapData, LogsData, SystemLogEntry } from "../types";

export function LogsPage({ data }: { data: BootstrapData }) {
  const { t, i18n } = useTranslation();
  const [logs, setLogs] = useState<LogsData>(data.logs);

  useEffect(() => setLogs(data.logs), [data.logs]);
  useEffect(() => {
    if (!data.settings.loggingEnabled) return;
    const refresh = () => void api.logs().then(setLogs).catch(() => undefined);
    const timer = window.setInterval(refresh, 1_500);
    return () => window.clearInterval(timer);
  }, [data.settings.loggingEnabled]);

  return (
    <section className="page logs-page">
      <div className="page-heading"><div><h1>{t("logs.title")}</h1><p>{t("logs.subtitle")}</p></div></div>
      <section className="logs-section">
        <header className="section-heading"><h2><Bug size={18} /> {t("logs.system")}</h2></header>
        {logs.system.length === 0 ? <div className="empty-state">{t("logs.empty")}</div> : <div className="log-list">{logs.system.map((entry) => <SystemLogRow key={entry.id} entry={entry} locale={i18n.language} />)}</div>}
      </section>
      <section className="logs-section">
        <header className="section-heading"><h2><MessageSquareText size={18} /> {t("logs.conversations")}</h2></header>
        {logs.conversations.length === 0 ? <div className="empty-state">{t("logs.empty")}</div> : <div className="conversation-list">{logs.conversations.map((entry) => <ConversationRow key={entry.id} entry={entry} locale={i18n.language} />)}</div>}
      </section>
    </section>
  );
}

function timestamp(value: string, locale: string) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return value;
  return new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "medium" }).format(new Date(parsed));
}

function SystemLogRow({ entry, locale }: { entry: SystemLogEntry; locale: string }) {
  return <article className={`system-log-row log-${entry.level}`}><span className="log-time"><Clock3 size={14} /> {timestamp(entry.timestamp, locale)}</span><strong>{entry.scope}</strong><span className="log-message">{entry.message}</span><span className="badge">{entry.level}</span></article>;
}

function ConversationRow({ entry, locale }: { entry: AiConversationLog; locale: string }) {
  const { t } = useTranslation();
  return <article className={`conversation-row ${entry.success ? "success" : "failure"}`}><div className="conversation-meta"><strong>{entry.operation}</strong><span>{timestamp(entry.timestamp, locale)}</span><span>{entry.provider} · {entry.model}</span><span>{entry.durationMs} ms</span><span className={`badge ${entry.success ? "enabled" : ""}`}>{entry.success ? t("logs.success") : t("logs.failure")}</span></div><div className="conversation-columns"><div><strong>{t("logs.request")}</strong><pre>{entry.request}</pre></div><div><strong>{t("logs.response")}</strong><pre>{entry.error || entry.response}</pre></div></div></article>;
}
