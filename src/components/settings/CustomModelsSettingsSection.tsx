import { useCallback, useEffect, useState } from "react";
import { Boxes, LoaderCircle, Plus, RefreshCw, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

import {
  addCustomModel,
  inspectGgufModel,
  listCustomModels,
  pickGgufFile,
  removeCustomModel,
  scanHfCacheModels,
  setEngine,
} from "@/api/tauri";
import { formatFileSize } from "@/lib/behaviorConfig";
import type { CustomModelEntry, DiscoveredHfModel } from "@/types";
import { SettingsSectionHeader } from "./SettingsControls";

export default function CustomModelsSettingsSection() {
  const { t } = useTranslation();
  const [models, setModels] = useState<CustomModelEntry[]>([]);
  const [discovered, setDiscovered] = useState<DiscoveredHfModel[] | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      setModels(await listCustomModels());
    } catch {
      toast.error(t("settings.customModelsAddFailed"));
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const handleAdd = async () => {
    setBusy(true);
    try {
      const path = await pickGgufFile();
      if (!path) return;
      const inspection = await inspectGgufModel(path);
      if (!inspection.valid || !inspection.supportedByTranscribeCpp) {
        toast.error(t("settings.customModelsInvalid"));
        return;
      }
      await addCustomModel({ path, name: inspection.modelName, source: "local_gguf" });
      await reload();
    } catch {
      toast.error(t("settings.customModelsAddFailed"));
    } finally {
      setBusy(false);
    }
  };

  const handleScan = async () => {
    setBusy(true);
    try {
      const found = await scanHfCacheModels();
      setDiscovered(found);
      if (found.length === 0) toast.info(t("settings.customModelsScanEmpty"));
    } catch {
      toast.error(t("settings.customModelsScanFailed"));
    } finally {
      setBusy(false);
    }
  };

  const handleRemove = async (id: string) => {
    setBusy(true);
    try {
      await removeCustomModel(id);
      await reload();
    } catch {
      toast.error(t("settings.customModelsRemoveFailed"));
    } finally {
      setBusy(false);
    }
  };

  const handleUse = async (model: CustomModelEntry) => {
    setBusy(true);
    try {
      await setEngine(model.engineKey);
      toast.success(t("settings.customModelsActivated", { name: model.name }));
    } catch {
      toast.error(t("settings.customModelsActivateFailed"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="settings-card" data-nav-id="custom-models">
      <SettingsSectionHeader
        icon={<Boxes size={15} className="icon-accent" />}
        title={t("settings.customModels")}
        trailing={(
          <div className="settings-header-actions">
            <button type="button" className="btn-ghost btn-ghost-sm" disabled={busy} onClick={() => { void handleScan(); }}>
              <RefreshCw size={12} aria-hidden="true" /> {t("settings.customModelsScan")}
            </button>
            <button type="button" className="test-btn" disabled={busy} onClick={() => { void handleAdd(); }}>
              <Plus size={12} aria-hidden="true" /> {t("settings.customModelsAdd")}
            </button>
          </div>
        )}
      />
      <div className="settings-column">
        <p className="settings-hint settings-hint-flush">{t("settings.customModelsHint")}</p>

        {loading ? (
          <p className="settings-hint">
            <LoaderCircle size={12} className="animate-spin" aria-hidden="true" /> {t("common.loading")}
          </p>
        ) : models.length === 0 ? (
          <div className="app-rules-empty">{t("settings.customModelsEmpty")}</div>
        ) : (
          models.map((model) => (
            <div className="app-rule-card" key={model.id}>
              <div className="app-rule-summary">
                <strong>{model.name}</strong>
                <span>
                  {model.architecture ?? model.source}
                  {" · "}
                  {formatFileSize(model.fileSize)}
                  {model.repoId ? ` · ${model.repoId}` : ""}
                </span>
              </div>
              <div className="app-rule-actions">
                <button
                  type="button"
                  className="btn-ghost btn-ghost-sm"
                  disabled={busy}
                  onClick={() => { void handleUse(model); }}
                >
                  {t("settings.customModelsActivate")}
                </button>
                <button
                  type="button"
                  className="btn-ghost app-rule-icon-button app-rule-delete"
                  aria-label={t("settings.customModelsRemove")}
                  disabled={busy}
                  onClick={() => { void handleRemove(model.id); }}
                >
                  <Trash2 size={13} />
                </button>
              </div>
            </div>
          ))
        )}

        {discovered !== null && discovered.length > 0 && (
          <div className="settings-column">
            <span className="settings-option-label">{t("settings.customModelsScan")}</span>
            {discovered.map((model) => (
              <div className="app-rule-card" key={model.path}>
                <div className="app-rule-summary">
                  <strong>{model.modelName ?? model.filename}</strong>
                  <span>
                    {model.repoId}
                    {" · "}
                    {formatFileSize(model.fileSize)}
                    {model.architecture ? ` · ${model.architecture}` : ""}
                  </span>
                </div>
                <div className="app-rule-actions">
                  {model.alreadyRegistered ? (
                    <span className="settings-option-desc">{t("settings.customModelsRegistered")}</span>
                  ) : (
                    <button
                      type="button"
                      className="btn-ghost btn-ghost-sm"
                      disabled={busy}
                      onClick={() => {
                        setBusy(true);
                        void addCustomModel({
                          path: model.path,
                          name: model.modelName ?? model.filename,
                          source: "hf_cache",
                          repoId: model.repoId,
                        })
                          .then(reload)
                          .catch(() => toast.error(t("settings.customModelsAddFailed")))
                          .finally(() => setBusy(false));
                      }}
                    >
                      {t("settings.customModelsUse")}
                    </button>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </section>
  );
}
