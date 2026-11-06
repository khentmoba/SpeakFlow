import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CATALOG_MODELS } from "@/lib/asrCatalog";
import "@/i18n";
import { ModelBrowser } from "@/components/settings/ModelBrowser";

function setup(overrides: Partial<Parameters<typeof ModelBrowser>[0]> = {}) {
  const onSelect = vi.fn();
  render(
    <ModelBrowser
      engine="qwen3-asr-0.6b"
      onSelect={onSelect}
      disabled={false}
      installed={new Set(["qwen3-asr-0.6b"])}
      {...overrides}
    />,
  );
  return { onSelect };
}

const listbox = () => screen.getByRole("listbox", { name: "Browse models" });

describe("ModelBrowser", () => {
  it("lists catalog models with grades, speed, size and language count", () => {
    setup();
    const row = within(listbox()).getByRole("option", { name: /Parakeet Unified EN 0.6B/ });
    expect(row).toHaveTextContent("Recommended");
    expect(row).toHaveTextContent("A-");
    expect(row).toHaveTextContent("Excellent");
    expect(row).toHaveTextContent("3.99% error");
    expect(row).toHaveTextContent("1.19 s / 30 s");
    expect(row).toHaveTextContent("1 language");
    expect(row).not.toHaveTextContent("1 languages");
    expect(row).toHaveTextContent("731 MB");
    expect(row).toHaveTextContent("nvidia-open-model-license");
  });

  it("shows the catalog's language coverage", () => {
    setup();
    expect(screen.getByText("58 models · 103 languages")).toBeInTheDocument();
    const search = screen.getByRole("searchbox", { name: "Search models" });
    fireEvent.change(search, { target: { value: "moonshine" } });
    expect(screen.getByText("5 models")).toBeInTheDocument();
    fireEvent.change(search, { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: /^French/ }));
    expect(screen.getByText("26 models")).toBeInTheDocument();
  });

  it("keeps the recommended-for-language badge on the real winner during a search", () => {
    setup();
    fireEvent.click(screen.getByRole("button", { name: /^French/ }));
    // The recommended French model is not one of the catalog's recommended
    // models, so it must still be in the folded list.
    const best = within(listbox()).getByRole("option", { name: /Parakeet Ultra/ });
    expect(best).toHaveTextContent("Recommended for French");

    fireEvent.change(screen.getByRole("searchbox", { name: "Search models" }), {
      target: { value: "whisper" },
    });
    const whisper = within(listbox())
      .getAllByRole("option")
      .find((option) => /Whisper Large v3/.test(option.textContent ?? "")
        && !/Turbo/.test(option.textContent ?? ""))!;
    expect(whisper).not.toHaveTextContent("Recommended for French");
  });

  it("recommends Parakeet Unified EN for English", () => {
    setup();
    fireEvent.click(screen.getByRole("button", { name: /^English/ }));
    const row = within(listbox()).getByRole("option", { name: /Parakeet Unified EN 0.6B/ });
    expect(row).toHaveTextContent("Recommended for English");
  });

  it("marks the current engine and offers the others", () => {
    setup();
    fireEvent.click(screen.getByRole("button", { name: /Show all/ }));
    const current = within(listbox()).getByRole("option", { name: /^Qwen3-ASR 0\.6B(?! 1\.7B)/ });
    expect(current).toHaveAttribute("aria-selected", "true");
    expect(current).toHaveTextContent("Selected");
    expect(within(listbox()).getByRole("option", { name: /^Moonshine Tiny/ }))
      .toHaveTextContent("Download");
  });

  it("selects a model by clicking its row", () => {
    const { onSelect } = setup();
    fireEvent.click(screen.getByRole("button", { name: /Show all/ }));
    fireEvent.click(within(listbox()).getByRole("option", { name: /^Moonshine Tiny/ }));
    expect(onSelect).toHaveBeenCalledWith("moonshine-tiny");
  });

  it("filters by language and shows that language's grade", () => {
    setup();
    fireEvent.click(screen.getByRole("button", { name: /French/ }));
    fireEvent.click(screen.getByRole("button", { name: /Show all/ }));
    const options = within(listbox()).getAllByRole("option");
    expect(options.length).toBeGreaterThan(5);
    expect(options.some((option) => option.textContent?.includes("Parakeet Unified"))).toBe(false);
    expect(options[0]).toHaveTextContent("French");
    expect(options[0]).toHaveTextContent("error");
    expect(options.some((option) => option.textContent?.includes("Recommended for French"))).toBe(true);
  });

  it("labels Whisper as long tail and never as recommended", () => {
    setup();
    fireEvent.change(screen.getByRole("searchbox", { name: "Search models" }), {
      target: { value: "whisper-large-v3" },
    });
    const row = within(listbox())
      .getAllByRole("option")
      .find((option) => /Whisper Large v3/.test(option.textContent ?? "")
        && !/Turbo/.test(option.textContent ?? ""))!;
    expect(row).toHaveTextContent("Long tail");
    expect(row).not.toHaveTextContent("Recommended");
    expect(row).toHaveTextContent("100 languages");
  });

  it("does not call an English-only Whisper model long tail", () => {
    setup();
    fireEvent.change(screen.getByRole("searchbox", { name: "Search models" }), {
      target: { value: "whisper-tiny.en" },
    });
    const row = within(listbox()).getByRole("option", { name: /Whisper Tiny English/ });
    expect(row).not.toHaveTextContent("Long tail");
    expect(row).toHaveTextContent("1 language");
  });

  it("marks a poor language as experimental instead of recommending it", () => {
    setup();
    fireEvent.click(screen.getByRole("button", { name: /Yoruba/ }));
    fireEvent.click(screen.getByRole("button", { name: /Show all/ }));
    const options = within(listbox()).getAllByRole("option");
    expect(options[0]).toHaveTextContent("Yoruba");
    expect(options[0]).toHaveTextContent("Poor");
    expect(options[0]).not.toHaveTextContent("Recommended for Yoruba");
  });

  it("keeps the other engines out of a language filter", () => {
    setup();
    expect(within(listbox()).getByRole("option", { name: /GLM-ASR/ })).toBeInTheDocument();
    expect(within(listbox()).getByRole("option", { name: /Confucius4-R2T2 Q8/ })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /^French/ }));
    expect(within(listbox()).queryByRole("option", { name: /GLM-ASR/ })).toBeNull();
  });

  it("searches by name and reports an empty result", () => {
    setup();
    const search = screen.getByRole("searchbox", { name: "Search models" });
    fireEvent.change(search, { target: { value: "moonshine" } });
    const options = within(listbox()).getAllByRole("option");
    expect(options.length).toBeGreaterThan(3);
    expect(options.every((option) => /Moonshine/i.test(option.textContent ?? ""))).toBe(true);

    fireEvent.change(search, { target: { value: "zzzz" } });
    expect(screen.getByText("No model matches this language and search.")).toBeInTheDocument();
  });

  it("shows an installed model as usable instead of downloadable", () => {
    setup({ installed: new Set(["moonshine-tiny"]) });
    fireEvent.click(screen.getByRole("button", { name: /Show all/ }));
    expect(within(listbox()).getByRole("option", { name: /^Moonshine Tiny/ })).toHaveTextContent("Use");
  });

  it("starts short and reveals the rest on request", () => {
    setup({ installed: new Set(["qwen3-asr-0.6b"]) });
    const catalogRows = () =>
      within(screen.getByRole("group", { name: "Models" })).getAllByRole("option");
    // Installed plus a few recommendations, not the whole catalog.
    const foldedCount = catalogRows().length;
    expect(foldedCount).toBeGreaterThanOrEqual(5);
    expect(foldedCount).toBeLessThan(10);
    expect(within(listbox()).getByRole("option", { name: /^Qwen3-ASR 0\.6B/ })).toBeInTheDocument();
    expect(within(listbox()).queryByRole("option", { name: /^Moonshine Tiny/ })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Show all 58 models" }));
    expect(catalogRows().length).toBeGreaterThan(40);

    fireEvent.click(screen.getByRole("button", { name: "Show fewer" }));
    expect(catalogRows().length).toBe(foldedCount);
  });

  it("keeps the selected model visible when it is neither installed nor recommended", () => {
    setup({ engine: "moonshine-tiny", installed: new Set() });
    expect(within(listbox()).getByRole("option", { name: /^Moonshine Tiny/ }))
      .toHaveAttribute("aria-selected", "true");
  });

  it("offers Show fewer only when collapsing hides something", () => {
    // Every model installed: the collapsed list is the whole list.
    setup({ installed: new Set(CATALOG_MODELS.map((model) => model.key)) });
    expect(screen.queryByRole("button", { name: /Show all/ })).toBeNull();
    expect(screen.queryByRole("button", { name: "Show fewer" })).toBeNull();
  });

  it("searches the whole catalog while folded", () => {
    setup();
    fireEvent.change(screen.getByRole("searchbox", { name: "Search models" }), {
      target: { value: "moonshine" },
    });
    // Search bypasses the fold and hides the "Show all" button.
    expect(within(listbox()).getAllByRole("option").length).toBeGreaterThan(3);
    expect(screen.queryByRole("button", { name: /Show all/ })).toBeNull();
  });

  it("disables every row while an engine switch is pending", () => {
    setup({ disabled: true });
    for (const option of within(listbox()).getAllByRole("option")) {
      expect(option).toBeDisabled();
    }
  });
});
