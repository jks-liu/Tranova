import { ArrowLeftRight } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { Glossary, PromptTemplate, Provider } from "../types";

const LANGUAGES = ["en", "zh", "ja", "ko", "fr", "de", "es", "ru"];

export interface TranslationOptionsValue {
  sourceLanguage: string;
  targetLanguage: string;
  providerId: string;
  promptId: string;
  glossaryIds: string[];
}

interface TranslationOptionsProps {
  value: TranslationOptionsValue;
  onChange: (next: TranslationOptionsValue) => void;
  providers: Provider[];
  prompts: PromptTemplate[];
  glossaries: Glossary[];
}

export function TranslationOptions({ value, onChange, providers, prompts, glossaries }: TranslationOptionsProps) {
  const { t } = useTranslation();
  const update = (patch: Partial<TranslationOptionsValue>) => onChange({ ...value, ...patch });

  const toggleGlossary = (id: string) => {
    const glossaryIds = value.glossaryIds.includes(id)
      ? value.glossaryIds.filter((item) => item !== id)
      : [...value.glossaryIds, id];
    update({ glossaryIds });
  };

  return (
    <div className="translation-options">
      <div className="language-pair">
        <LanguageInput
          label={t("translate.sourceLanguage")}
          value={value.sourceLanguage}
          listId="tranova-source-languages"
          allowAuto
          onChange={(sourceLanguage) => update({ sourceLanguage })}
        />
        <button
          className="icon-button language-swap"
          type="button"
          onClick={() => update({ sourceLanguage: value.targetLanguage, targetLanguage: value.sourceLanguage })}
          disabled={value.sourceLanguage === "auto"}
          title={t("translate.swapLanguages")}
          aria-label={t("translate.swapLanguages")}
        >
          <ArrowLeftRight size={18} />
        </button>
        <LanguageInput
          label={t("translate.targetLanguage")}
          value={value.targetLanguage}
          listId="tranova-target-languages"
          onChange={(targetLanguage) => update({ targetLanguage })}
        />
      </div>
      <label>
        <span>{t("translate.provider")}</span>
        <select value={value.providerId} onChange={(event) => update({ providerId: event.target.value })}>
          {providers.filter((provider) => provider.enabled).map((provider) => <option key={provider.id} value={provider.id}>{provider.name}</option>)}
        </select>
      </label>
      <label>
        <span>{t("translate.prompt")}</span>
        <select value={value.promptId} onChange={(event) => update({ promptId: event.target.value })}>
          <option value="">{t("common.none")}</option>
          {prompts.map((prompt) => <option key={prompt.id} value={prompt.id}>{prompt.name}</option>)}
        </select>
      </label>
      <fieldset className="glossary-picker">
        <legend>{t("translate.glossaries")}</legend>
        <div>
          {glossaries.length === 0 && <span className="muted">{t("common.none")}</span>}
          {glossaries.map((glossary) => (
            <label key={glossary.id} className="check-row">
              <input type="checkbox" checked={value.glossaryIds.includes(glossary.id)} onChange={() => toggleGlossary(glossary.id)} />
              <span>{glossary.name}</span>
            </label>
          ))}
        </div>
      </fieldset>
    </div>
  );
}

function LanguageInput({
  label,
  value,
  listId,
  allowAuto = false,
  onChange,
}: {
  label: string;
  value: string;
  listId: string;
  allowAuto?: boolean;
  onChange: (value: string) => void;
}) {
  const { t } = useTranslation();
  return (
    <label className="language-input">
      <span>{label}</span>
      <input
        list={listId}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={t("languages.customPlaceholder")}
        spellCheck={false}
      />
      <datalist id={listId}>
        {allowAuto && <option value="auto" label={t("languages.auto")} />}
        {LANGUAGES.map((language) => <option key={language} value={language} label={t(`languages.${language}`)} />)}
      </datalist>
    </label>
  );
}
