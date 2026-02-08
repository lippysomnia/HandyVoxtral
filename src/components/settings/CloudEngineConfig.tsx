import React, { useRef } from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../ui/SettingContainer";
import { ApiKeyField } from "./PostProcessingSettingsApi/ApiKeyField";
import { Input } from "../ui/Input";
import { useSettingsStore } from "../../stores/settingsStore";
import type { AppSettings } from "../../bindings";

type SettingsKey = keyof AppSettings;

interface CloudEngineConfigProps {
  apiKeySettingKey: SettingsKey;
  apiKeyI18nPrefix: string;
  modelSettingKey?: SettingsKey;
  modelI18nPrefix?: string;
}

export const CloudEngineConfig: React.FC<CloudEngineConfigProps> = ({
  apiKeySettingKey,
  apiKeyI18nPrefix,
  modelSettingKey,
  modelI18nPrefix,
}) => {
  const { t } = useTranslation();
  const { settings, isUpdatingKey } = useSettingsStore();
  // Track the last value persisted to the backend so optimistic local
  // updates in onChange don't trick the blur handler into skipping the save.
  const persistedModel = useRef<string | null>(null);
  if (modelSettingKey && persistedModel.current === null) {
    persistedModel.current = (settings?.[modelSettingKey] as string) ?? "";
  }

  const handleApiKeyChange = async (value: string) => {
    const trimmed = value.trim();
    if (trimmed === ((settings?.[apiKeySettingKey] as string) ?? "")) return;
    await useSettingsStore.getState().updateSetting(apiKeySettingKey, trimmed);
  };

  const handleModelChange = async (
    e:
      | React.FocusEvent<HTMLInputElement>
      | React.KeyboardEvent<HTMLInputElement>,
  ) => {
    if (!modelSettingKey) return;
    const value = (e.target as HTMLInputElement).value.trim();
    if (value === persistedModel.current) return;
    persistedModel.current = value;
    await useSettingsStore.getState().updateSetting(modelSettingKey, value);
  };

  return (
    <div className="space-y-2">
      <SettingContainer
        title={t(`${apiKeyI18nPrefix}.title`)}
        description={t(`${apiKeyI18nPrefix}.description`)}
        descriptionMode="tooltip"
        layout="horizontal"
        grouped
      >
        <ApiKeyField
          value={(settings?.[apiKeySettingKey] as string) ?? ""}
          onBlur={handleApiKeyChange}
          placeholder={t(`${apiKeyI18nPrefix}.placeholder`)}
          disabled={isUpdatingKey(apiKeySettingKey)}
          className="min-w-[280px]"
        />
      </SettingContainer>
      {modelSettingKey && modelI18nPrefix && (
        <SettingContainer
          title={t(`${modelI18nPrefix}.title`)}
          description={t(`${modelI18nPrefix}.description`)}
          descriptionMode="tooltip"
          layout="horizontal"
          grouped
        >
          <Input
            value={(settings?.[modelSettingKey] as string) ?? ""}
            onChange={(e) => {
              useSettingsStore.getState().setSettings({
                ...settings!,
                [modelSettingKey]: e.target.value,
              });
            }}
            onBlur={handleModelChange}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleModelChange(e);
            }}
            placeholder={t(`${modelI18nPrefix}.placeholder`)}
            disabled={isUpdatingKey(modelSettingKey)}
            className="min-w-[280px]"
          />
        </SettingContainer>
      )}
    </div>
  );
};
