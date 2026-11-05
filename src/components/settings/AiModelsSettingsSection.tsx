import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Sparkles, RefreshCw, LogIn, LogOut, KeyRound, ExternalLink } from "lucide-react";
import { toast } from "sonner";
import SecretInput from "@/components/SecretInput";
import {
  completeClaudeOauth,
  completeOpenaiCodexOauthDeviceCode,
  getAiPolishApiKey,
  getAiPolishConfig,
  getClaudeOauthStatus,
  getOpenaiCodexOauthStatus,
  loginOpenaiCodexOauth,
  logoutClaudeOauth,
  logoutOpenaiCodexOauth,
  saveAiPolishApiKey,
  setAiPolishConfig,
  setLlmProviderConfig,
  setOpenaiFastMode,
  startClaudeOauth,
  startOpenaiCodexOauthDeviceCode,
  syncClaudeOauth,
  syncOpenaiCodexOauth,
} from "@/api/tauri";
import { findLlmPreset } from "@/lib/llmModelOptions";
import type {
  ClaudeAuthMode,
  ClaudeOauthStatus,
  OpenaiAuthMode,
  OpenaiCodexOauthDeviceCodeChallenge,
  OpenaiCodexOauthStatus,
  UserProfile,
} from "@/types";

export interface AiModelsSettingsSectionProps {
  profile: UserProfile | null;
  allProviderOptions: Array<{
    key: string;
    label: string;
    desc: string;
    baseUrl: string;
    isCustom: boolean;
  }>;
  onRefreshProfile: () => Promise<UserProfile | undefined> | Promise<void> | void;
}

export default function AiModelsSettingsSection({
  profile,
  allProviderOptions,
  onRefreshProfile,
}: AiModelsSettingsSectionProps) {
  const { t } = useTranslation();

  const activeProvider = profile?.llm_provider.active || "cerebras";
  const activePreset = allProviderOptions.find((option) => option.key === activeProvider);
  const preset = findLlmPreset(activeProvider);
  // A custom provider keeps its model on the provider entry, not on the
  // top-level custom_model field.
  const customProviderModel = profile?.llm_provider.custom_providers?.find(
    (provider) => provider.id === activeProvider,
  )?.model;
  const savedModel =
    customProviderModel?.trim() || profile?.llm_provider.custom_model || preset.defaultModel;

  const [aiPolishEnabled, setAiPolishEnabled] = useState(false);
  const [modelDraft, setModelDraft] = useState(savedModel);

  const [openaiAuthMode, setOpenaiAuthMode] = useState<OpenaiAuthMode>(
    profile?.llm_provider.openai_auth_mode || "oauth",
  );
  const [claudeAuthMode, setClaudeAuthMode] = useState<ClaudeAuthMode>(
    profile?.llm_provider.claude_auth_mode || "oauth",
  );
  const [openaiFastMode, setOpenaiFastModeState] = useState(
    profile?.llm_provider.openai_fast_mode ?? false,
  );

  const [apiKey, setApiKey] = useState("");
  const [openaiStatus, setOpenaiStatus] = useState<OpenaiCodexOauthStatus | null>(null);
  const [claudeStatus, setClaudeStatus] = useState<ClaudeOauthStatus | null>(null);

  const [oauthWorking, setOauthWorking] = useState(false);
  const [deviceCodeChallenge, setDeviceCodeChallenge] =
    useState<OpenaiCodexOauthDeviceCodeChallenge | null>(null);
  const [deviceCodeSubmitting, setDeviceCodeSubmitting] = useState(false);
  const [claudeCode, setClaudeCode] = useState("");
  const [claudeSignInStarted, setClaudeSignInStarted] = useState(false);

  const loadKey = useCallback(async (provider: string) => {
    try {
      setApiKey((await getAiPolishApiKey(provider)) || "");
    } catch {
      setApiKey("");
    }
  }, []);

  const refreshStatuses = useCallback(async () => {
    const [openai, claude] = await Promise.allSettled([
      getOpenaiCodexOauthStatus(),
      getClaudeOauthStatus(),
    ]);
    if (openai.status === "fulfilled") setOpenaiStatus(openai.value);
    if (claude.status === "fulfilled") setClaudeStatus(claude.value);
  }, []);

  useEffect(() => {
    void loadKey(activeProvider);
  }, [activeProvider, loadKey]);

  useEffect(() => {
    void refreshStatuses();
  }, [refreshStatuses]);

  useEffect(() => {
    let cancelled = false;
    getAiPolishConfig()
      .then((config) => {
        if (!cancelled) setAiPolishEnabled(config.enabled);
      })
      .catch(() => {
        /* leave the toggle off when the state cannot be read */
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    setModelDraft(savedModel);
  }, [savedModel]);

  useEffect(() => {
    if (profile?.llm_provider.openai_auth_mode) {
      setOpenaiAuthMode(profile.llm_provider.openai_auth_mode);
    }
    if (profile?.llm_provider.claude_auth_mode) {
      setClaudeAuthMode(profile.llm_provider.claude_auth_mode);
    }
    if (profile?.llm_provider.openai_fast_mode !== undefined) {
      setOpenaiFastModeState(profile.llm_provider.openai_fast_mode);
    }
  }, [profile]);

  const handleAiPolishToggle = async (enabled: boolean) => {
    setAiPolishEnabled(enabled);
    try {
      await setAiPolishConfig(enabled, undefined, activeProvider);
      toast.success(enabled ? t("toast.aiPolishEnabled") : t("toast.aiPolishDisabled"));
    } catch (err) {
      setAiPolishEnabled(!enabled);
      toast.error(err instanceof Error ? err.message : t("toast.settingsSaveFailed"));
    }
  };

  const handleProviderChange = async (provider: string) => {
    try {
      // Clearing the model lets the new provider fall back to its own default
      // instead of carrying the previous provider's model name.
      await setLlmProviderConfig(provider);
      await onRefreshProfile();
      await loadKey(provider);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t("toast.settingsSaveFailed"));
    }
  };

  const commitModel = async () => {
    const next = modelDraft.trim();
    if (next === savedModel) return;
    try {
      await setLlmProviderConfig(activeProvider, undefined, next || undefined);
      await onRefreshProfile();
    } catch (err) {
      setModelDraft(savedModel);
      toast.error(err instanceof Error ? err.message : t("toast.settingsSaveFailed"));
    }
  };

  // Typing stays local; the keyring is written once the field loses focus.
  const handleApiKeyChange = (key: string) => setApiKey(key);

  const commitApiKey = async () => {
    try {
      await saveAiPolishApiKey(apiKey, activeProvider);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t("toast.settingsSaveFailed"));
    }
  };

  const handleOpenaiAuthModeSelect = async (mode: OpenaiAuthMode) => {
    setOpenaiAuthMode(mode);
    try {
      await setLlmProviderConfig(
        activeProvider,
        undefined,
        savedModel,
        undefined,
        undefined,
        undefined,
        undefined,
        undefined,
        mode,
      );
      await onRefreshProfile();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t("toast.settingsSaveFailed"));
    }
  };

  const handleClaudeAuthModeSelect = async (mode: ClaudeAuthMode) => {
    setClaudeAuthMode(mode);
    try {
      await setLlmProviderConfig(
        activeProvider,
        undefined,
        savedModel,
        undefined,
        undefined,
        undefined,
        undefined,
        undefined,
        undefined,
        undefined,
        mode,
      );
      await onRefreshProfile();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t("toast.settingsSaveFailed"));
    }
  };

  const handleOpenaiFastModeToggle = async (enabled: boolean) => {
    setOpenaiFastModeState(enabled);
    try {
      await setOpenaiFastMode(enabled);
      await onRefreshProfile();
    } catch (err) {
      setOpenaiFastModeState(!enabled);
      toast.error(err instanceof Error ? err.message : t("toast.settingsSaveFailed"));
    }
  };

  const handleChatgptLogin = async (newAccount = false) => {
    setOauthWorking(true);
    try {
      setOpenaiStatus(await loginOpenaiCodexOauth(undefined, newAccount));
      toast.success(t("toast.codexOauthLoginSuccess"));
      await onRefreshProfile();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t("toast.codexOauthLoginFailed"));
    } finally {
      setOauthWorking(false);
    }
  };

  const handleChatgptDeviceCodeStart = async () => {
    setOauthWorking(true);
    try {
      const challenge = await startOpenaiCodexOauthDeviceCode();
      setDeviceCodeChallenge(challenge);
      window.open(challenge.verificationUrl, "_blank");
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t("toast.codexOauthLoginFailed"));
    } finally {
      setOauthWorking(false);
    }
  };

  const handleChatgptDeviceCodeComplete = async () => {
    if (!deviceCodeChallenge) return;
    setDeviceCodeSubmitting(true);
    try {
      setOpenaiStatus(await completeOpenaiCodexOauthDeviceCode(deviceCodeChallenge));
      setDeviceCodeChallenge(null);
      toast.success(t("toast.codexOauthLoginSuccess"));
      await onRefreshProfile();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t("toast.codexOauthLoginFailed"));
    } finally {
      setDeviceCodeSubmitting(false);
    }
  };

  const handleChatgptLogout = async () => {
    setOauthWorking(true);
    try {
      await logoutOpenaiCodexOauth();
      setOpenaiStatus({ loggedIn: false });
      toast.success(t("toast.codexOauthLogoutSuccess"));
      await onRefreshProfile();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t("toast.codexOauthLogoutFailed"));
    } finally {
      setOauthWorking(false);
    }
  };

  const handleChatgptSync = async () => {
    setOauthWorking(true);
    try {
      setOpenaiStatus(await syncOpenaiCodexOauth());
      toast.success(t("toast.syncOauthSuccess"));
      await onRefreshProfile();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t("toast.syncOauthFailed"));
    } finally {
      setOauthWorking(false);
    }
  };

  const handleClaudeStart = async () => {
    setOauthWorking(true);
    try {
      const start = await startClaudeOauth();
      setClaudeSignInStarted(true);
      setClaudeCode("");
      window.open(start.authorizeUrl, "_blank");
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t("toast.claudeOauthLoginFailed"));
    } finally {
      setOauthWorking(false);
    }
  };

  const handleClaudeComplete = async () => {
    if (!claudeCode.trim()) return;
    setOauthWorking(true);
    try {
      setClaudeStatus(await completeClaudeOauth(claudeCode));
      setClaudeSignInStarted(false);
      setClaudeCode("");
      toast.success(t("toast.claudeOauthLoginSuccess"));
      await onRefreshProfile();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t("toast.claudeOauthLoginFailed"));
    } finally {
      setOauthWorking(false);
    }
  };

  const handleClaudeLogout = async () => {
    setOauthWorking(true);
    try {
      await logoutClaudeOauth();
      setClaudeStatus({ loggedIn: false });
      setClaudeSignInStarted(false);
      toast.success(t("toast.claudeOauthLogoutSuccess"));
      await onRefreshProfile();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t("toast.claudeOauthLogoutFailed"));
    } finally {
      setOauthWorking(false);
    }
  };

  const handleClaudeSync = async () => {
    setOauthWorking(true);
    try {
      setClaudeStatus(await syncClaudeOauth());
      toast.success(t("toast.syncOauthSuccess"));
      await onRefreshProfile();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t("toast.syncOauthFailed"));
    } finally {
      setOauthWorking(false);
    }
  };

  return (
    <section className="settings-card" data-nav-id="ai-models">
      <div className="settings-section-header">
        <Sparkles size={15} className="icon-accent" />
        <h2 className="settings-section-title">{t("settings.aiPolishSection")}</h2>
        <div className="settings-row" style={{ marginLeft: "auto", gap: 8, flex: "0 0 auto" }}>
          <span className="settings-option-desc">{t("settings.aiPolishEnabled")}</span>
          <button
            role="switch"
            aria-checked={aiPolishEnabled}
            aria-label={t("settings.aiPolishEnabled")}
            onClick={() => void handleAiPolishToggle(!aiPolishEnabled)}
            className="toggle-switch"
          >
            <div className="toggle-knob" />
          </button>
        </div>
      </div>

      <div className="settings-column" style={{ gap: 12 }}>
        <p className="settings-hint">{t("settings.aiPolishEnabledDesc")}</p>

        <div className="settings-column" style={{ gap: 6 }}>
          <span className="settings-option-desc">{t("settings.aiProvider")}</span>
          <select
            className="settings-input"
            value={activeProvider}
            aria-label={t("settings.aiProvider")}
            onChange={(e) => void handleProviderChange(e.target.value)}
          >
            {allProviderOptions.map((option) => (
              <option key={option.key} value={option.key}>
                {option.label} — {option.desc}
              </option>
            ))}
          </select>
        </div>

        <div className="settings-column" style={{ gap: 6 }}>
          <span className="settings-option-desc">{t("settings.modelLabel")}</span>
          <div style={{ display: "flex", gap: 8 }}>
            {preset.models.length > 0 && (
              <select
                className="settings-input"
                style={{ flex: 1 }}
                aria-label={t("settings.modelPresetLabel")}
                value={preset.models.includes(modelDraft) ? modelDraft : ""}
                onChange={(e) => {
                  if (!e.target.value) return;
                  setModelDraft(e.target.value);
                  void setLlmProviderConfig(activeProvider, undefined, e.target.value)
                    .then(() => {
                      void onRefreshProfile();
                    })
                    .catch((err: unknown) =>
                      toast.error(
                        err instanceof Error ? err.message : t("toast.settingsSaveFailed"),
                      ),
                    );
                }}
              >
                <option value="">{t("settings.canInputModelName")}</option>
                {preset.models.map((model) => (
                  <option key={model} value={model}>
                    {model}
                  </option>
                ))}
              </select>
            )}
            <input
              type="text"
              className="settings-input"
              style={{ flex: 1 }}
              aria-label={t("settings.modelLabel")}
              placeholder={activePreset?.isCustom ? t("settings.modelNamePlaceholder") : preset.defaultModel}
              value={modelDraft}
              onChange={(e) => setModelDraft(e.target.value)}
              onBlur={() => void commitModel()}
              onKeyDown={(e) => {
                if (e.key === "Enter") e.currentTarget.blur();
              }}
            />
          </div>
        </div>

        {activeProvider === "openai" && (
          <div className="settings-column" style={{ gap: 10, marginTop: 4 }}>
            <div className="settings-row" style={{ alignItems: "center", justifyContent: "space-between" }}>
              <span className="settings-option-desc">{t("settings.openaiAuthModeLabel")}</span>
              <div className="openai-auth-mode-toggle">
                <button
                  type="button"
                  className="openai-auth-mode-toggle-btn"
                  data-active={openaiAuthMode === "api_key"}
                  onClick={() => void handleOpenaiAuthModeSelect("api_key")}
                >
                  <KeyRound size={12} style={{ display: "inline", marginRight: 4 }} />
                  {t("settings.openaiAuthModeApiKey")}
                </button>
                <button
                  type="button"
                  className="openai-auth-mode-toggle-btn"
                  data-active={openaiAuthMode === "oauth"}
                  onClick={() => void handleOpenaiAuthModeSelect("oauth")}
                >
                  <LogIn size={12} style={{ display: "inline", marginRight: 4 }} />
                  {t("settings.openaiAuthModeOauth")}
                </button>
              </div>
            </div>

            {openaiAuthMode === "api_key" ? (
              <div className="settings-column" style={{ gap: 6 }}>
                <SecretInput
                  value={apiKey}
                  placeholder="sk-proj-..."
                  ariaLabel={t("settings.apiKey")}
                  ariaLabelShow={t("settings.showApiKey")}
                  ariaLabelHide={t("settings.hideApiKey")}
                  onChange={handleApiKeyChange}
                  onBlur={() => void commitApiKey()}
                />
                <span className="settings-hint">{t("settings.openaiAuthModeApiKeyHint")}</span>
              </div>
            ) : (
              <div className="oauth-card">
                <div className="oauth-card-head">
                  <span
                    className="oauth-status-dot"
                    data-connected={Boolean(openaiStatus?.loggedIn)}
                  />
                  <strong className="oauth-card-title">
                    {openaiStatus?.loggedIn
                      ? `${openaiStatus.email || "ChatGPT"} (${openaiStatus.planType || "Plan"})`
                      : t("settings.codexOauthLabel")}
                  </strong>
                  {openaiStatus?.loggedIn && openaiStatus.accountId && (
                    <span className="oauth-card-meta">
                      {t("settings.codexOauthAccountSummary", {
                        accountId: openaiStatus.accountId.slice(0, 8),
                      })}
                    </span>
                  )}
                </div>

                <p className="settings-hint" style={{ margin: 0 }}>
                  {openaiStatus?.loggedIn
                    ? t("settings.codexOauthApiKeyFallbackHint")
                    : t("settings.codexOauthHint")}
                </p>

                {openaiStatus?.loggedIn ? (
                  <div className="settings-column" style={{ gap: 8 }}>
                    <div className="settings-row" style={{ justifyContent: "space-between" }}>
                      <div>
                        <strong style={{ fontSize: 12, color: "var(--color-text-primary)" }}>
                          {t("settings.fastModeLabel")}
                        </strong>
                        <p className="settings-hint" style={{ margin: "2px 0 0" }}>
                          {t("settings.fastModeHint")}
                        </p>
                      </div>
                      <button
                        role="switch"
                        aria-checked={openaiFastMode}
                        aria-label={t("settings.fastModeLabel")}
                        onClick={() => void handleOpenaiFastModeToggle(!openaiFastMode)}
                        className="toggle-switch"
                      >
                        <div className="toggle-knob" />
                      </button>
                    </div>

                    <div style={{ display: "flex", gap: 8 }}>
                      <button
                        type="button"
                        className="btn-ghost"
                        disabled={oauthWorking}
                        onClick={() => void handleChatgptLogin(true)}
                        style={{ fontSize: 12, padding: "6px 12px" }}
                      >
                        {t("settings.codexOauthReauth")}
                      </button>
                      <button
                        type="button"
                        className="btn-ghost"
                        disabled={oauthWorking}
                        onClick={() => void handleChatgptLogout()}
                        style={{ fontSize: 12, padding: "6px 12px", color: "var(--color-error)" }}
                      >
                        <LogOut size={12} style={{ display: "inline", marginRight: 4 }} />
                        {t("settings.codexOauthLogout")}
                      </button>
                    </div>
                  </div>
                ) : (
                  <div className="settings-column" style={{ gap: 8 }}>
                    <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
                      <button
                        type="button"
                        className="theme-btn"
                        disabled={oauthWorking}
                        onClick={() => void handleChatgptLogin()}
                        style={{ fontSize: 12, padding: "7px 14px" }}
                      >
                        <LogIn size={13} style={{ display: "inline", marginRight: 6 }} />
                        {t("settings.codexOauthLogin")}
                      </button>
                      <button
                        type="button"
                        className="btn-ghost"
                        disabled={oauthWorking}
                        onClick={() => void handleChatgptDeviceCodeStart()}
                        style={{ fontSize: 12, padding: "7px 12px" }}
                      >
                        {t("settings.codexOauthDeviceCodeLogin")}
                      </button>
                      <button
                        type="button"
                        className="btn-ghost"
                        disabled={oauthWorking}
                        onClick={() => void handleChatgptSync()}
                        style={{ fontSize: 12, padding: "7px 12px" }}
                      >
                        <RefreshCw size={12} style={{ display: "inline", marginRight: 4 }} />
                        {t("settings.syncCodexOauth")}
                      </button>
                    </div>

                    {deviceCodeChallenge && (
                      <div className="oauth-device-code">
                        <span style={{ fontSize: 12, color: "var(--color-text-secondary)" }}>
                          {t("settings.codexOauthDeviceCodeReady")}
                        </span>
                        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                          <span className="oauth-device-code-value">
                            {deviceCodeChallenge.userCode}
                          </span>
                          <a
                            href={deviceCodeChallenge.verificationUrl}
                            target="_blank"
                            rel="noreferrer"
                            className="theme-btn theme-btn-xs"
                            style={{ textDecoration: "none" }}
                          >
                            <ExternalLink size={12} style={{ marginRight: 4 }} />
                            {t("settings.oauthOpenPage")}
                          </a>
                          <button
                            type="button"
                            className="theme-btn theme-btn-xs"
                            disabled={deviceCodeSubmitting}
                            onClick={() => void handleChatgptDeviceCodeComplete()}
                          >
                            {deviceCodeSubmitting
                              ? t("settings.codexOauthWorking")
                              : t("settings.codexOauthDeviceCodeContinue")}
                          </button>
                        </div>
                      </div>
                    )}
                  </div>
                )}
              </div>
            )}
          </div>
        )}

        {activeProvider === "claude" && (
          <div className="settings-column" style={{ gap: 10, marginTop: 4 }}>
            <div className="settings-row" style={{ alignItems: "center", justifyContent: "space-between" }}>
              <span className="settings-option-desc">{t("settings.claudeAuthModeLabel")}</span>
              <div className="openai-auth-mode-toggle">
                <button
                  type="button"
                  className="openai-auth-mode-toggle-btn"
                  data-active={claudeAuthMode === "api_key"}
                  onClick={() => void handleClaudeAuthModeSelect("api_key")}
                >
                  <KeyRound size={12} style={{ display: "inline", marginRight: 4 }} />
                  {t("settings.claudeAuthModeApiKey")}
                </button>
                <button
                  type="button"
                  className="openai-auth-mode-toggle-btn"
                  data-active={claudeAuthMode === "oauth"}
                  onClick={() => void handleClaudeAuthModeSelect("oauth")}
                >
                  <LogIn size={12} style={{ display: "inline", marginRight: 4 }} />
                  {t("settings.claudeAuthModeOauth")}
                </button>
              </div>
            </div>

            {claudeAuthMode === "api_key" ? (
              <div className="settings-column" style={{ gap: 6 }}>
                <SecretInput
                  value={apiKey}
                  placeholder="sk-ant-api03-..."
                  ariaLabel={t("settings.apiKey")}
                  ariaLabelShow={t("settings.showApiKey")}
                  ariaLabelHide={t("settings.hideApiKey")}
                  onChange={handleApiKeyChange}
                  onBlur={() => void commitApiKey()}
                />
                <span className="settings-hint">{t("settings.claudeAuthModeApiKeyHint")}</span>
              </div>
            ) : (
              <div className="oauth-card">
                <div className="oauth-card-head">
                  <span
                    className="oauth-status-dot"
                    data-connected={Boolean(claudeStatus?.loggedIn)}
                  />
                  <strong className="oauth-card-title">
                    {claudeStatus?.loggedIn
                      ? `${claudeStatus.email || "Claude"}${claudeStatus.planType ? ` (${claudeStatus.planType})` : ""}`
                      : t("settings.claudeOauthLabel")}
                  </strong>
                  {claudeStatus?.organizationName && (
                    <span className="oauth-card-meta">{claudeStatus.organizationName}</span>
                  )}
                </div>

                <p className="settings-hint" style={{ margin: 0 }}>
                  {t("settings.claudeOauthHint")}
                </p>

                {claudeStatus?.loggedIn ? (
                  <div style={{ display: "flex", gap: 8 }}>
                    <button
                      type="button"
                      className="btn-ghost"
                      disabled={oauthWorking}
                      onClick={() => void handleClaudeLogout()}
                      style={{ fontSize: 12, padding: "6px 12px", color: "var(--color-error)" }}
                    >
                      <LogOut size={12} style={{ display: "inline", marginRight: 4 }} />
                      {t("settings.claudeOauthLogout")}
                    </button>
                  </div>
                ) : (
                  <div className="settings-column" style={{ gap: 8 }}>
                    <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
                      <button
                        type="button"
                        className="theme-btn"
                        disabled={oauthWorking}
                        onClick={() => void handleClaudeStart()}
                        style={{ fontSize: 12, padding: "7px 14px" }}
                      >
                        <LogIn size={13} style={{ display: "inline", marginRight: 6 }} />
                        {claudeSignInStarted
                          ? t("settings.claudeOauthRestart")
                          : t("settings.claudeOauthLogin")}
                      </button>
                      <button
                        type="button"
                        className="btn-ghost"
                        disabled={oauthWorking}
                        onClick={() => void handleClaudeSync()}
                        style={{ fontSize: 12, padding: "7px 12px" }}
                      >
                        <RefreshCw size={12} style={{ display: "inline", marginRight: 4 }} />
                        {t("settings.claudeOauthSync")}
                      </button>
                    </div>

                    {claudeSignInStarted && (
                      <div className="settings-column" style={{ gap: 6 }}>
                        <span className="settings-hint">{t("settings.claudeOauthPasteHint")}</span>
                        <div style={{ display: "flex", gap: 8 }}>
                          <input
                            type="text"
                            className="settings-input"
                            style={{ flex: 1 }}
                            aria-label={t("settings.claudeOauthCodeLabel")}
                            placeholder={t("settings.claudeOauthCodePlaceholder")}
                            value={claudeCode}
                            onChange={(e) => setClaudeCode(e.target.value)}
                            onKeyDown={(e) => {
                              if (e.key === "Enter") void handleClaudeComplete();
                            }}
                          />
                          <button
                            type="button"
                            className="theme-btn"
                            disabled={oauthWorking || !claudeCode.trim()}
                            onClick={() => void handleClaudeComplete()}
                          >
                            {t("settings.claudeOauthContinue")}
                          </button>
                        </div>
                      </div>
                    )}
                  </div>
                )}
              </div>
            )}
          </div>
        )}

        {!["openai", "claude"].includes(activeProvider) && (
          <div className="settings-column" style={{ gap: 6 }}>
            <span className="settings-option-desc">{t("settings.apiKey")}</span>
            <SecretInput
              value={apiKey}
              placeholder={t("settings.apiKeyPlaceholder")}
              ariaLabel={t("settings.apiKey")}
              ariaLabelShow={t("settings.showApiKey")}
              ariaLabelHide={t("settings.hideApiKey")}
              onChange={handleApiKeyChange}
              onBlur={() => void commitApiKey()}
            />
          </div>
        )}
      </div>
    </section>
  );
}
