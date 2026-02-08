import React from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "../ui/SettingContainer";
import { ApiKeyField } from "./PostProcessingSettingsApi/ApiKeyField";
import { useSettingsStore } from "../../stores/settingsStore";

interface MistralApiKeyProps {
  grouped?: boolean;
}

export const MistralApiKey: React.FC<MistralApiKeyProps> = ({
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { settings, isUpdatingKey } = useSettingsStore();

  const handleApiKeyChange = async (value: string) => {
    const trimmed = value.trim();
    if (trimmed === (settings?.mistral_api_key ?? "")) return;
    await useSettingsStore
      .getState()
      .updateSetting("mistral_api_key", trimmed);
  };

  return (
    <SettingContainer
      title={t("settings.general.mistralApiKey.title")}
      description={t("settings.general.mistralApiKey.description")}
      descriptionMode="tooltip"
      layout="horizontal"
      grouped={grouped}
    >
      <ApiKeyField
        value={settings?.mistral_api_key ?? ""}
        onBlur={handleApiKeyChange}
        placeholder={t("settings.general.mistralApiKey.placeholder")}
        disabled={isUpdatingKey("mistral_api_key")}
        className="min-w-[280px]"
      />
    </SettingContainer>
  );
};
