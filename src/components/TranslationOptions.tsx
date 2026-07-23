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
      <label>
        <span>{t("translate.sourceLanguage")}</span>
        <select value={value.sourceLanguage} onChange={(event) => update({ sourceLanguage: event.target.value })}>
          <option value="auto">{t("languages.auto")}</option>
          {LANGUAGES.map((language) => <option key={language} value={language}>{t(`languages.${language}`)}</option>)}
        </select>
      </label>
      <label>
        <span>{t("translate.targetLanguage")}</span>
        <select value={value.targetLanguage} onChange={(event) => update({ targetLanguage: event.target.value })}>
          {LANGUAGES.map((language) => <option key={language} value={language}>{t(`languages.${language}`)}</option>)}
        </select>
      </label>
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
