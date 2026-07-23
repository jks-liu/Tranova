import type { ReactNode } from "react";
import { Bot, FileText, Languages, Library, Menu, Settings, Sparkles, X } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

export type View = "translate" | "files" | "glossaries" | "prompts" | "providers" | "settings";

interface LayoutProps {
  view: View;
  onViewChange: (view: View) => void;
  children: ReactNode;
  connected: boolean;
}

export function Layout({ view, onViewChange, children, connected }: LayoutProps) {
  const { t } = useTranslation();
  const [mobileOpen, setMobileOpen] = useState(false);
  const items: Array<{ id: View; icon: typeof Languages; label: string }> = [
    { id: "translate", icon: Languages, label: t("nav.translate") },
    { id: "files", icon: FileText, label: t("nav.files") },
    { id: "glossaries", icon: Library, label: t("nav.glossaries") },
    { id: "prompts", icon: Sparkles, label: t("nav.prompts") },
    { id: "providers", icon: Bot, label: t("nav.providers") },
    { id: "settings", icon: Settings, label: t("nav.settings") },
  ];

  const select = (next: View) => {
    onViewChange(next);
    setMobileOpen(false);
  };

  return (
    <div className="app-shell">
      <aside className={`sidebar ${mobileOpen ? "sidebar-open" : ""}`}>
        <div className="brand-row">
          <div className="brand-mark">T</div>
          <div>
            <strong>Tranova</strong>
            <span>{t("common.productSubtitle")}</span>
          </div>
          <button className="icon-button mobile-close" onClick={() => setMobileOpen(false)} aria-label={t("common.closeMenu")}>
            <X size={19} />
          </button>
        </div>
        <nav className="sidebar-nav">
          {items.map(({ id, icon: Icon, label }) => (
            <button key={id} className={view === id ? "active" : ""} onClick={() => select(id)}>
              <Icon size={18} />
              <span>{label}</span>
            </button>
          ))}
        </nav>
        <div className="connection-state">
          <span className={connected ? "status-dot online" : "status-dot"} />
          {connected ? t("common.ready") : t("common.error")}
        </div>
      </aside>
      {mobileOpen && <div className="mobile-overlay" onClick={() => setMobileOpen(false)} />}
      <div className="main-column">
        <header className="mobile-header">
          <button className="icon-button" onClick={() => setMobileOpen(true)} aria-label={t("common.openMenu")}><Menu size={20} /></button>
          <strong>Tranova</strong>
          <span className={connected ? "status-dot online" : "status-dot"} />
        </header>
        <main>{children}</main>
      </div>
    </div>
  );
}
