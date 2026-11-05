import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import en from "@/i18n/en";

function readRepo(relativePath: string): string {
  return readFileSync(resolve(process.cwd(), relativePath), "utf8");
}

const typesSource = readFileSync(resolve(process.cwd(), "src/types/index.ts"), "utf8");
const tauriSource = readFileSync(resolve(process.cwd(), "src/api/tauri.ts"), "utf8");
const userProfileSource = readFileSync(
  resolve(process.cwd(), "src-tauri/src/state/user_profile.rs"),
  "utf8",
);
// llm_provider is split into submodules; the contract checks read the production files together.
const llmProviderSource = [
  "src-tauri/src/services/llm_provider.rs",
  "src-tauri/src/services/llm_provider/endpoint.rs",
  "src-tauri/src/services/llm_provider/reasoning.rs",
  "src-tauri/src/services/llm_provider/tests.rs",
]
  .map((relativePath) => readRepo(relativePath))
  .join("\n");
const codexOauthSource = readFileSync(
  resolve(process.cwd(), "src-tauri/src/services/codex_oauth_service.rs"),
  "utf8",
);
const commandsModSource = readFileSync(
  resolve(process.cwd(), "src-tauri/src/commands/mod.rs"),
  "utf8",
);
const libSource = readFileSync(resolve(process.cwd(), "src-tauri/src/lib.rs"), "utf8");
const aiPolishSource = readFileSync(
  resolve(process.cwd(), "src-tauri/src/commands/ai_polish.rs"),
  "utf8",
);
const profileCommandSource = readFileSync(
  resolve(process.cwd(), "src-tauri/src/commands/profile.rs"),
  "utf8",
);
const appStateSource = readFileSync(
  resolve(process.cwd(), "src-tauri/src/state/app_state.rs"),
  "utf8",
);
const servicesModSource = readFileSync(
  resolve(process.cwd(), "src-tauri/src/services/mod.rs"),
  "utf8",
);

const GROK_I18N_KEYS = [
  "xaiDesc",
  "xaiAuthModeLabel",
  "xaiAuthModeApiKey",
  "xaiAuthModeOauth",
  "xaiAuthModeApiKeyHint",
  "xaiAuthModeOauthHint",
  "grokBuildOauthLabel",
  "grokBuildOauthHint",
  "grokBuildOauthConnectedHint",
  "grokBuildOauthLogin",
  "grokBuildOauthReauth",
  "grokBuildOauthLogout",
  "grokBuildOauthWorking",
  "grokBuildOauthDeviceCodeLogin",
  "grokBuildOauthDeviceCodeReady",
  "grokBuildOauthDeviceCodeContinue",
  "fillApiKeyOrGrokLogin",
  "apiKeyOrGrokLoginMissing",
] as const;

describe("Grok Build TypeScript IPC types", () => {
  it("declares XaiAuthMode and persists it on LlmProviderConfig", () => {
    expect(typesSource).toContain('export type XaiAuthMode = "api_key" | "oauth"');
    expect(typesSource).toContain("xai_auth_mode?: XaiAuthMode | null");
  });

  it("declares camelCase Grok Build status and device-code challenge types", () => {
    expect(typesSource).toContain("export interface GrokBuildOauthStatus");
    expect(typesSource).toContain("export interface GrokBuildOauthDeviceCodeChallenge");
    expect(typesSource).toMatch(/interface GrokBuildOauthStatus \{[\s\S]*loggedIn: boolean/);
    expect(typesSource).toMatch(
      /interface GrokBuildOauthDeviceCodeChallenge \{[\s\S]*verificationUrl: string;[\s\S]*userCode: string;[\s\S]*deviceCode: string;[\s\S]*intervalSecs: number/,
    );
  });
});

describe("Grok Build i18n keys", () => {
  it("defines every required settings key in English", () => {
    for (const key of GROK_I18N_KEYS) {
      const enValue = (en.settings as Record<string, unknown>)[key];
      expect(enValue, `en.settings.${key}`).toEqual(expect.any(String));
      expect(String(enValue).trim().length, `en.settings.${key} should be non-empty`).toBeGreaterThan(0);
    }
  });
});

describe("Grok Build backend wiring contract", () => {
  it("registers the Grok Build OAuth service, commands, and generate_handler entries", () => {
    expect(servicesModSource).toContain("pub mod grok_build_oauth_service");
    expect(commandsModSource).toContain("pub mod grok_build_oauth");
    expect(libSource).toContain("commands::grok_build_oauth::get_grok_build_oauth_status");
    expect(libSource).toContain("commands::grok_build_oauth::login_grok_build_oauth");
    expect(libSource).toContain("commands::grok_build_oauth::start_grok_build_oauth_device_code");
    expect(libSource).toContain("commands::grok_build_oauth::complete_grok_build_oauth_device_code");
    expect(libSource).toContain("commands::grok_build_oauth::logout_grok_build_oauth");
    expect(libSource).toContain("grok_build_oauth_service");
  });

  it("uses the Grok CLI public-client identity, keyring slots, and session meta file", () => {
    const grokOauthSource = readRepo("src-tauri/src/services/grok_build_oauth_service.rs");
    const grokCommandsSource = readRepo("src-tauri/src/commands/grok_build_oauth.rs");
    expect(grokOauthSource).toContain("b1a00492-073a-47ea-816f-4c329264a828");
    expect(grokOauthSource).toContain("https://auth.x.ai");
    expect(grokOauthSource).toContain("https://auth.x.ai/oauth2/authorize");
    expect(grokOauthSource).toContain("https://auth.x.ai/oauth2/token");
    expect(grokOauthSource).toContain("https://auth.x.ai/oauth2/device/code");
    expect(grokOauthSource).toContain("openid profile email offline_access grok-cli:access api:access");
    expect(grokOauthSource).toContain("http://127.0.0.1:56121/callback");
    expect(grokOauthSource).toContain("KEYRING_SERVICE");
    expect(llmProviderSource).toContain(
      'pub const KEYRING_SERVICE: &str = "speak-flow"',
    );
    expect(grokOauthSource).toContain("grok-build-oauth");
    expect(grokOauthSource).toContain("grok-build-oauth-refresh-token");
    expect(grokOauthSource).toContain("grok_build_oauth_session.json");
    expect(grokCommandsSource).toContain("login_grok_build_oauth");
    expect(grokCommandsSource).toContain("start_grok_build_oauth_device_code");
    expect(grokCommandsSource).toContain("complete_grok_build_oauth_device_code");
    expect(grokCommandsSource).toContain("logout_grok_build_oauth");
    expect(grokCommandsSource).toContain("get_grok_build_oauth_status");
  });

  it("uses Grok CLI identity headers and proxy URLs for OAuth origin auth", () => {
    const grokOauthSource = readRepo("src-tauri/src/services/grok_build_oauth_service.rs");
    expect(grokOauthSource).toContain("https://cli-chat-proxy.grok.com");
    expect(grokOauthSource).toContain("https://api.x.ai");
    expect(grokOauthSource).toContain("xai-grok-cli");
    expect(grokOauthSource).toContain("grok-shell");
    expect(grokOauthSource).toContain("x-grok-client-identifier");
    expect(grokOauthSource).toContain("X-XAI-Token-Auth");
    expect(grokOauthSource).toContain("x-grok-client-version");
    expect(grokOauthSource).toContain("0.2.114");
  });

  it("adds xai as a built-in Responses-API provider", () => {
    expect(userProfileSource).toMatch(
      /#\[serde\(rename_all = "snake_case"\)\]\s*pub enum XaiAuthMode/,
    );
    expect(userProfileSource).toMatch(/enum XaiAuthMode \{[\s\S]*ApiKey[\s\S]*Oauth/);
    expect(userProfileSource).toContain("xai_auth_mode");
    expect(userProfileSource).toMatch(/fn is_builtin_provider[\s\S]*"xai"/);
    expect(llmProviderSource).toContain('"xai"');
    expect(llmProviderSource).toContain("https://api.x.ai");
    expect(llmProviderSource).toContain("grok-4.6");
  });

  it("threads xai_auth_mode through list_ai_models, set_llm_provider_config, and the API-key resolver", () => {
    expect(aiPolishSource.includes("xai_auth_mode")).toBe(true);
    expect(
      aiPolishSource.includes("GROK_BUILD_MODELS_URL")
        || aiPolishSource.includes("cli-chat-proxy.grok.com/v1/models"),
    ).toBe(true);
    expect(
      aiPolishSource.includes("XAI_API_MODELS_URL")
        || aiPolishSource.includes("https://api.x.ai/v1/models"),
    ).toBe(true);
    expect(profileCommandSource.includes("xai_auth_mode")).toBe(true);
    expect(codexOauthSource.includes('"xai"')).toBe(true);
    expect(tauriSource.includes("xaiAuthMode: xaiAuthMode ?? null")).toBe(true);
    expect(appStateSource.includes("grok_build_oauth_session")).toBe(true);
  });
});
