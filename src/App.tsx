import { RefreshCw, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, initializeApi } from "./api";
import { Layout, type View } from "./components/Layout";
import { FilesPage } from "./pages/FilesPage";
import { GlossariesPage } from "./pages/GlossariesPage";
import { PromptsPage } from "./pages/PromptsPage";
import { ProvidersPage } from "./pages/ProvidersPage";
import { SettingsPage } from "./pages/SettingsPage";
import { TranslatePage } from "./pages/TranslatePage";
import type { BootstrapData } from "./types";

export default function App() {
  const { t, i18n } = useTranslation();
  const [view, setView] = useState<View>("translate");
  const [data, setData] = useState<BootstrapData | null>(null);
  const [error, setError] = useState("");

  const load = useCallback(async () => {
    setError("");
    try {
      await initializeApi();
      let next: BootstrapData | null = null;
      let lastError: unknown;
      for (let attempt = 0; attempt < 8 && !next; attempt += 1) {
        try {
          next = await api.bootstrap();
        } catch (reason) {
          lastError = reason;
          await new Promise((resolve) => window.setTimeout(resolve, 150));
        }
      }
      if (!next) throw lastError || new Error(t("status.backendUnavailable"));
      setData(next);
      if (!localStorage.getItem("tranova-language")) await i18n.changeLanguage(next.settings.language);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }, [i18n, t]);

  useEffect(() => { void load(); }, [load]);

  if (!data) {
    return (
      <div className="startup-state">
        {error ? <TriangleAlert size={28} /> : <div className="spinner" />}
        <strong>{error ? t("status.backendUnavailable") : "Tranova"}</strong>
        {error && <><span>{error}</span><button className="secondary-button" onClick={load}><RefreshCw size={17} /> {t("common.retry")}</button></>}
      </div>
    );
  }

  const pages: Record<View, React.ReactNode> = {
    translate: <TranslatePage data={data} />,
    files: <FilesPage data={data} />,
    glossaries: <GlossariesPage data={data} onReload={load} />,
    prompts: <PromptsPage data={data} onReload={load} />,
    providers: <ProvidersPage data={data} onReload={load} />,
    settings: <SettingsPage data={data} onReload={load} />,
  };

  return <Layout view={view} onViewChange={setView} connected={true}>{pages[view]}</Layout>;
}
