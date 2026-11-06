import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import "@/i18n";
import { findCatalogModel } from "@/lib/asrCatalog";
import LanguageSettingsSection from "@/components/settings/LanguageSettingsSection";

function setup(overrides: Partial<Parameters<typeof LanguageSettingsSection>[0]> = {}) {
  const onLanguageChange = vi.fn();
  const onTranslateChange = vi.fn();
  render(
    <LanguageSettingsSection
      model={findCatalogModel("qwen3-asr-0.6b") ?? null}
      language={null}
      translateToEnglish={false}
      onLanguageChange={onLanguageChange}
      onTranslateChange={onTranslateChange}
      {...overrides}
    />,
  );
  return { onLanguageChange, onTranslateChange };
}

describe("LanguageSettingsSection", () => {
  it("offers auto-detect and every language the model lists", () => {
    setup();
    const select = screen.getByRole("combobox", { name: "Language" });
    expect(select).toHaveValue("auto");
    expect(screen.getByRole("option", { name: "Auto-detect" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: /French \(B\)/ })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: /Cantonese/ })).toBeInTheDocument();
  });

  it("saves a fixed language and clears it back to auto", () => {
    const { onLanguageChange } = setup();
    const select = screen.getByRole("combobox", { name: "Language" });
    fireEvent.change(select, { target: { value: "fr" } });
    expect(onLanguageChange).toHaveBeenCalledWith("fr");
    fireEvent.change(select, { target: { value: "auto" } });
    expect(onLanguageChange).toHaveBeenLastCalledWith(null);
  });

  it("shows the fixed language for a single-language model", () => {
    setup({ model: findCatalogModel("parakeet-unified-en-0.6b") ?? null });
    const select = screen.getByRole("combobox", { name: "Language" });
    expect(select).toBeDisabled();
    expect(select).toHaveValue("en");
    expect(screen.getByText("Uses one fixed language: English.")).toBeInTheDocument();
  });

  it("says when the saved language is unavailable and uses the resolved one", () => {
    setup({
      model: findCatalogModel("parakeet-unified-en-0.6b") ?? null,
      language: "ca",
    });
    expect(screen.getByRole("combobox", { name: "Language" })).toHaveValue("en");
    expect(screen.getByText(/Catalan is not available for this model/)).toBeInTheDocument();
    expect(screen.getByRole("option", { name: /Catalan \(unavailable\)/ })).toBeDisabled();
  });

  it("names auto-detect when an unavailable language falls back to detection", () => {
    setup({ language: "gl" });
    expect(screen.getByRole("combobox", { name: "Language" })).toHaveValue("auto");
    expect(
      screen.getByText(/Galician is not available for this model\. Using Auto-detect instead\./),
    ).toBeInTheDocument();
  });

  it("shows the published quality for the selected language", () => {
    setup({ language: "fr" });
    expect(screen.getByText("Uses French for every recording until you change it.")).toBeInTheDocument();
    expect(screen.getByText(/Good accuracy: B, 7\.76% error\./)).toBeInTheDocument();
  });

  it("does not claim a multi-language model only speaks its fallback", () => {
    setup({ model: findCatalogModel("canary-180m-flash") ?? null, language: null });
    expect(screen.getByText("Uses English until you choose another language.")).toBeInTheDocument();
  });

  it("explains that translation needs a source language when detection is off", () => {
    setup({ model: findCatalogModel("canary-180m-flash") ?? null, language: null });
    expect(
      screen.getByText("Choose the spoken language first; this model cannot detect it."),
    ).toBeInTheDocument();
  });

  it("warns about a poor grade instead of presenting it as normal", () => {
    setup({
      model: findCatalogModel("whisper-large-v3") ?? null,
      language: "yo",
    });
    expect(screen.getByText(/Poor accuracy: F, 97.26% error\./)).toBeInTheDocument();
    expect(screen.getByText("Long tail")).toBeInTheDocument();
  });

  it("does not call an English-only Whisper model long tail", () => {
    setup({ model: findCatalogModel("whisper-tiny.en") ?? null, language: null });
    expect(screen.queryByText("Long tail")).toBeNull();
    expect(screen.queryByText(/Broad language coverage/)).toBeNull();
    expect(screen.getByText("Uses one fixed language: English.")).toBeInTheDocument();
  });

  it("offers translate-to-English only when the model supports it", () => {
    setup();
    expect(screen.queryByRole("switch", { name: "Translate to English" })).toBeNull();

    const { onTranslateChange } = setup({
      model: findCatalogModel("canary-180m-flash") ?? null,
      language: "fr",
    });
    const toggle = screen.getByRole("switch", { name: "Translate to English" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    fireEvent.click(toggle);
    expect(onTranslateChange).toHaveBeenCalledWith(true);
  });
});
