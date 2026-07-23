import { Globe2, Save } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import type { AppSettings, BootstrapData } from "../types";

export function SettingsPage({ data, onReload }: { data: BootstrapData; onReload: () => Promise<void> }) {
  const { t, i18n } = useTranslation();
  const [settings, setSettings] = useState<AppSettings>({ ...data.settings });
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");

  const save = async () => {
    setMessage(""); setError("");
    try {
      await api.saveSettings(settings);
      localStorage.setItem("tranova-language", settings.language);
      await i18n.changeLanguage(settings.language);
      setMessage(t("status.saved"));
      await onReload();
    } catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
  };

  return (
    <section className="page settings-page">
      <div className="page-heading"><div><h1>{t("settings.title")}</h1><p>{t("settings.subtitle")}</p></div></div>
      {message && <div className="alert success-alert">{message}</div>}
      {error && <div className="alert error-alert">{error}</div>}
      <div className="settings-section">
        <h2>{t("settings.language")}</h2>
        <label className="setting-row"><div><strong>{t("settings.language")}</strong></div><select value={settings.language} onChange={(event) => setSettings({ ...settings, language: event.target.value as AppSettings["language"] })}><option value="en">English</option><option value="zh-CN">简体中文</option></select></label>
      </div>
      <div className="settings-section">
        <h2>{t("settings.proxy")}</h2>
        <label className="setting-row"><div><strong>{t("settings.proxy")}</strong><span>{t("settings.proxyHint")}</span></div><input value={settings.proxyUrl} onChange={(event) => setSettings({ ...settings, proxyUrl: event.target.value })} placeholder="socks5h://127.0.0.1:1080" /></label>
      </div>
      <div className="settings-section">
        <h2><Globe2 size={18} /> {t("settings.webAccess")}</h2>
        <label className="setting-row"><div><strong>{t("settings.address")}</strong><span>{t("settings.webHint")}</span></div><input value={settings.webHost} onChange={(event) => setSettings({ ...settings, webHost: event.target.value })} /></label>
        <label className="setting-row"><div><strong>{t("settings.port")}</strong></div><input type="number" min={1024} max={65535} value={settings.webPort} onChange={(event) => setSettings({ ...settings, webPort: Number(event.target.value) })} /></label>
        <div className="server-url">{data.serverUrl}</div>
      </div>
      <div className="settings-section">
        <h2>{t("settings.chunk")}</h2>
        <label className="setting-row"><div><strong>{t("settings.chunk")}</strong></div><input type="number" min={500} max={50000} step={500} value={settings.maxChunkChars} onChange={(event) => setSettings({ ...settings, maxChunkChars: Number(event.target.value) })} /></label>
      </div>
      <div className="action-row"><button className="primary-button" onClick={save}><Save size={18} /> {t("common.save")}</button></div>
    </section>
  );
}
