import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, describe, it, expect, vi, beforeEach } from "vitest";
import AiModelsSettingsSection from "./AiModelsSettingsSection";
import * as tauri from "@/api/tauri";
import type { UserProfile } from "@/types";

vi.mock("@/api/tauri", () => ({
  getAiPolishApiKey: vi.fn(),
  getAiPolishConfig: vi.fn(),
  saveAiPolishApiKey: vi.fn(),
  setAiPolishConfig: vi.fn(),
  setLlmProviderConfig: vi.fn(),
  setOpenaiFastMode: vi.fn(),
  getOpenaiCodexOauthStatus: vi.fn(),
  loginOpenaiCodexOauth: vi.fn(),
  logoutOpenaiCodexOauth: vi.fn(),
  syncOpenaiCodexOauth: vi.fn(),
  startOpenaiCodexOauthDeviceCode: vi.fn(),
  completeOpenaiCodexOauthDeviceCode: vi.fn(),
  getClaudeOauthStatus: vi.fn(),
  startClaudeOauth: vi.fn(),
  completeClaudeOauth: vi.fn(),
  logoutClaudeOauth: vi.fn(),
  syncClaudeOauth: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const mockProviders = [
  { key: "openai", label: "OpenAI", desc: "OpenAI models", baseUrl: "https://api.openai.com", isCustom: false },
  { key: "claude", label: "Anthropic Claude", desc: "Claude models", baseUrl: "https://api.anthropic.com", isCustom: false },
  { key: "cerebras", label: "Cerebras", desc: "Cerebras models", baseUrl: "https://api.cerebras.ai", isCustom: false },
];

const createMockProfile = (active = "openai"): UserProfile => ({
  hot_words: [],
  correction_patterns: [],
  vocab_frequency: {},
  total_transcriptions: 0,
  last_updated: Date.now(),
  llm_provider: {
    active,
    reasoning_mode: "provider_default",
    assistant_use_separate_model: false,
    selection_use_separate_model: false,
    custom_providers: [],
    validation_use_separate_model: false,
    screen_vision_enabled: false,
    openai_auth_mode: "oauth",
    openai_fast_mode: false,
    claude_auth_mode: "oauth",
  },
});

describe("AiModelsSettingsSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.spyOn(window, "open").mockImplementation(() => null);
    vi.mocked(tauri.getAiPolishApiKey).mockResolvedValue("");
    vi.mocked(tauri.getAiPolishConfig).mockResolvedValue({ enabled: true, provider: "openai" });
    vi.mocked(tauri.getOpenaiCodexOauthStatus).mockResolvedValue({ loggedIn: false });
    vi.mocked(tauri.loginOpenaiCodexOauth).mockResolvedValue({ loggedIn: true });
    vi.mocked(tauri.logoutOpenaiCodexOauth).mockResolvedValue(undefined);
    vi.mocked(tauri.syncOpenaiCodexOauth).mockResolvedValue({ loggedIn: true });
    vi.mocked(tauri.getClaudeOauthStatus).mockResolvedValue({ loggedIn: false });
    vi.mocked(tauri.startClaudeOauth).mockResolvedValue({
      authorizeUrl: "https://claude.com/cai/oauth/authorize?x=1",
      state: "st",
    });
    vi.mocked(tauri.completeClaudeOauth).mockResolvedValue({ loggedIn: true });
    vi.mocked(tauri.logoutClaudeOauth).mockResolvedValue(undefined);
    vi.mocked(tauri.syncClaudeOauth).mockResolvedValue({ loggedIn: true });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders the polish toggle, provider picker and model field", async () => {
    render(
      <AiModelsSettingsSection
        profile={createMockProfile("openai")}
        allProviderOptions={mockProviders}
        onRefreshProfile={vi.fn()}
      />,
    );

    expect(screen.getByText("settings.aiPolishSection")).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "settings.aiProvider" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "settings.modelLabel" })).toBeInTheDocument();
  });

  it("reads the persisted polish state instead of assuming it is off", async () => {
    render(
      <AiModelsSettingsSection
        profile={createMockProfile("openai")}
        allProviderOptions={mockProviders}
        onRefreshProfile={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(screen.getByRole("switch", { name: "settings.aiPolishEnabled" })).toHaveAttribute(
        "aria-checked",
        "true",
      );
    });
  });

  it("commits the model field on blur, not on every keystroke", async () => {
    render(
      <AiModelsSettingsSection
        profile={createMockProfile("openai")}
        allProviderOptions={mockProviders}
        onRefreshProfile={vi.fn()}
      />,
    );

    const field = screen.getByRole("textbox", { name: "settings.modelLabel" });
    fireEvent.change(field, { target: { value: "g" } });
    fireEvent.change(field, { target: { value: "gp" } });
    fireEvent.change(field, { target: { value: "gpt-6.1-sol" } });
    expect(tauri.setLlmProviderConfig).not.toHaveBeenCalled();

    fireEvent.blur(field);
    await waitFor(() => {
      expect(tauri.setLlmProviderConfig).toHaveBeenCalledTimes(1);
    });
    expect(tauri.setLlmProviderConfig).toHaveBeenCalledWith("openai", undefined, "gpt-6.1-sol");
  });

  it("does not write a model when the field is left unchanged", async () => {
    render(
      <AiModelsSettingsSection
        profile={createMockProfile("openai")}
        allProviderOptions={mockProviders}
        onRefreshProfile={vi.fn()}
      />,
    );

    const field = screen.getByRole("textbox", { name: "settings.modelLabel" });
    fireEvent.blur(field);
    await waitFor(() => expect(tauri.getAiPolishConfig).toHaveBeenCalled());
    expect(tauri.setLlmProviderConfig).not.toHaveBeenCalled();
  });

  it("signs in to ChatGPT and shows the connected account", async () => {
    vi.mocked(tauri.getOpenaiCodexOauthStatus).mockResolvedValue({
      loggedIn: true,
      email: "test@example.com",
      planType: "Plus",
    });

    render(
      <AiModelsSettingsSection
        profile={createMockProfile("openai")}
        allProviderOptions={mockProviders}
        onRefreshProfile={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText("test@example.com (Plus)")).toBeInTheDocument();
      expect(screen.getByRole("switch", { name: "settings.fastModeLabel" })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("switch", { name: "settings.fastModeLabel" }));
    await waitFor(() => expect(tauri.setOpenaiFastMode).toHaveBeenCalledWith(true));

    fireEvent.click(screen.getByText("settings.codexOauthLogout"));
    await waitFor(() => expect(tauri.logoutOpenaiCodexOauth).toHaveBeenCalled());
  });

  it("starts the ChatGPT sign-in when logged out", async () => {
    render(
      <AiModelsSettingsSection
        profile={createMockProfile("openai")}
        allProviderOptions={mockProviders}
        onRefreshProfile={vi.fn()}
      />,
    );

    const button = await screen.findByText("settings.codexOauthLogin");
    fireEvent.click(button);
    await waitFor(() => expect(tauri.loginOpenaiCodexOauth).toHaveBeenCalled());
  });

  it("runs the two-step Claude sign-in and exchanges the pasted code", async () => {
    render(
      <AiModelsSettingsSection
        profile={createMockProfile("claude")}
        allProviderOptions={mockProviders}
        onRefreshProfile={vi.fn()}
      />,
    );

    fireEvent.click(await screen.findByText("settings.claudeOauthLogin"));
    await waitFor(() => expect(tauri.startClaudeOauth).toHaveBeenCalled());

    const codeField = await screen.findByRole("textbox", {
      name: "settings.claudeOauthCodeLabel",
    });
    fireEvent.change(codeField, { target: { value: "abc123#st" } });
    fireEvent.click(screen.getByText("settings.claudeOauthContinue"));

    await waitFor(() => expect(tauri.completeClaudeOauth).toHaveBeenCalledWith("abc123#st"));
  });

  it("adopts an existing Claude sign-in from this machine", async () => {
    render(
      <AiModelsSettingsSection
        profile={createMockProfile("claude")}
        allProviderOptions={mockProviders}
        onRefreshProfile={vi.fn()}
      />,
    );

    fireEvent.click(await screen.findByText("settings.claudeOauthSync"));
    await waitFor(() => expect(tauri.syncClaudeOauth).toHaveBeenCalled());
  });

  it("shows a custom provider's own model instead of the OpenAI default", async () => {
    const profile = createMockProfile("my-gateway");
    profile.llm_provider.custom_providers = [
      {
        id: "my-gateway",
        name: "My Gateway",
        base_url: "https://gateway.example/v1",
        model: "house-model-7b",
        api_format: "openai_compat",
      },
    ];

    render(
      <AiModelsSettingsSection
        profile={profile}
        allProviderOptions={[
          ...mockProviders,
          {
            key: "my-gateway",
            label: "My Gateway",
            desc: "openai_compat",
            baseUrl: "https://gateway.example/v1",
            isCustom: true,
          },
        ]}
        onRefreshProfile={vi.fn()}
      />,
    );

    const field = screen.getByRole("textbox", { name: "settings.modelLabel" });
    expect(field).toHaveValue("house-model-7b");
  });

  it("offers an API key field for providers without a sign-in", async () => {
    render(
      <AiModelsSettingsSection
        profile={createMockProfile("cerebras")}
        allProviderOptions={mockProviders}
        onRefreshProfile={vi.fn()}
      />,
    );

    await waitFor(() => expect(tauri.getAiPolishApiKey).toHaveBeenCalledWith("cerebras"));
    expect(screen.queryByText("settings.codexOauthLogin")).not.toBeInTheDocument();
    expect(screen.queryByText("settings.claudeOauthLogin")).not.toBeInTheDocument();
  });
});
