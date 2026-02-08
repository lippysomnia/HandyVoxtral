import React, { useRef } from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../ui/SettingContainer";
import { ApiKeyField } from "./PostProcessingSettingsApi/ApiKeyField";
import { Input } from "../ui/Input";
import { useSettingsStore } from "../../stores/settingsStore";

export const OpenRouterConfig: React.FC = () => {
  const { t } = useTranslation();
  const { settings, isUpdatingKey } = useSettingsStore();
  // Track the last value persisted to the backend so optimistic local
  // updates don't trick the blur handler into skipping the save.
  const persistedModel = useRef(settings?.openrouter_model ?? "");

  const handleApiKeyChange = async (value: string) => {
    const trimmed = value.trim();
    if (trimmed === (settings?.openrouter_api_key ?? "")) return;
    await useSettingsStore
      .getState()
      .updateSetting("openrouter_api_key", trimmed);
  };

  const handleModelChange = async (
    e:
      | React.FocusEvent<HTMLInputElement>
      | React.KeyboardEvent<HTMLInputElement>,
  ) => {
    const value = (e.target as HTMLInputElement).value.trim();
    if (value === persistedModel.current) return;
    persistedModel.current = value;
    await useSettingsStore.getState().updateSetting("openrouter_model", value);
  };

  return (
    <div className="space-y-2">
      <SettingContainer
        title={t("settings.general.openrouterApiKey.title")}
        description={t("settings.general.openrouterApiKey.description")}
        descriptionMode="tooltip"
        layout="horizontal"
        grouped
      >
        <ApiKeyField
          value={settings?.openrouter_api_key ?? ""}
          onBlur={handleApiKeyChange}
          placeholder={t("settings.general.openrouterApiKey.placeholder")}
          disabled={isUpdatingKey("openrouter_api_key")}
          className="min-w-[280px]"
        />
      </SettingContainer>
      <SettingContainer
        title={t("settings.general.openrouterModel.title")}
        description={t("settings.general.openrouterModel.description")}
        descriptionMode="tooltip"
        layout="horizontal"
        grouped
      >
        <Input
          value={settings?.openrouter_model ?? ""}
          onChange={(e) => {
            useSettingsStore.getState().setSettings({
              ...settings!,
              openrouter_model: e.target.value,
            });
          }}
          onBlur={handleModelChange}
          onKeyDown={(e) => {
            if (e.key === "Enter") handleModelChange(e);
          }}
          placeholder={t("settings.general.openrouterModel.placeholder")}
          disabled={isUpdatingKey("openrouter_model")}
          className="min-w-[280px]"
        />
      </SettingContainer>
    </div>
  );
};
