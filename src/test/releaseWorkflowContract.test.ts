import {
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { describe, expect, it } from "vitest";

function read(path: string): string {
  return readFileSync(path, "utf8");
}

describe("release workflow contract", () => {
  it("does not package the application in ordinary CI", () => {
    const workflow = read(".github/workflows/ci.yml");

    expect(workflow).not.toContain("pnpm tauri build");
    expect(workflow).not.toContain("gh release create");
  });

  it("makes Tauri distribution builds verify the engine archive first", () => {
    const tauriConfig = JSON.parse(read("src-tauri/tauri.conf.json"));
    const packageJson = JSON.parse(read("package.json"));

    expect(tauriConfig.build.beforeBuildCommand).toBe(
      "pnpm build:distribution",
    );
    expect(packageJson.scripts["build:distribution"]).toContain(
      "verify_engine_archive.mjs",
    );
  });

  it("rejects missing, empty, non-XZ and truncated engine archives before packaging", () => {
    const directory = mkdtempSync(join(tmpdir(), "speak-flow-engine-check-"));
    const archive = join(directory, "engine.tar.xz");
    const verify = () =>
      spawnSync(
        process.execPath,
        ["scripts/verify_engine_archive.mjs", archive],
        { encoding: "utf8" },
      );

    try {
      expect(verify().status).not.toBe(0);

      writeFileSync(archive, Buffer.alloc(0));
      expect(verify().status).not.toBe(0);

      writeFileSync(archive, "not an xz archive");
      expect(verify().status).not.toBe(0);

      writeFileSync(
        archive,
        Buffer.from([0xfd, 0x37, 0x7a, 0x58, 0x5a, 0x00]),
      );
      expect(verify().status).not.toBe(0);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  }, 30000);

  it("splits long Discord release notes on bullet boundaries without truncating bullets", () => {
    const directory = mkdtempSync(join(tmpdir(), "speak-flow-discord-check-"));
    const configPath = join(directory, "webhook.json");
    const notesPath = join(directory, "notes.md");

    try {
      writeFileSync(
        configPath,
        JSON.stringify({
          webhookId: "123",
          webhookToken: "secret",
          channelId: "456",
          channelName: "speakflow-releases",
          webhookName: "SpeakFlow Release Bot",
          guildId: "789",
        }),
      );
      const lines = Array.from(
        { length: 18 },
        (_, i) => `Shipped release improvement number ${i + 1} with full verification across local and cloud pipelines.`,
      );
      writeFileSync(notesPath, lines.join("\n"));

      const run = spawnSync(
        process.execPath,
        [
          "scripts/post_release_discord.mjs",
          "--version",
          "1.2.0",
          "--notes",
          notesPath,
          "--link",
          "https://github.com/khentmoba/SpeakFlow/releases/download/1.2.0/SpeakFlow_1.2.0_x64-setup.exe",
        ],
        {
          encoding: "utf8",
          env: { ...process.env, SPEAKFLOW_DISCORD_WEBHOOK_CONFIG: configPath },
        },
      );

      expect(run.status, run.stderr).toBe(0);
      const jsonStart = run.stdout.indexOf("payload_json:\n") + "payload_json:\n".length;
      const payload = JSON.parse(run.stdout.slice(jsonStart)) as {
        embeds: Array<{ fields: Array<{ name: string; value: string }> }>;
      };
      const noteFields = payload.embeds[0].fields.filter((f) => f.name.startsWith("What's New"));
      expect(noteFields.length).toBe(2);
      expect(noteFields[0].name).toBe("What's New");
      expect(noteFields[1].name).toBe("What's New (2/2)");
      for (const field of noteFields) {
        expect(field.value.length).toBeLessThanOrEqual(1024);
      }
      const reconstructed = noteFields.map((f) => f.value).join("\n").split("\n");
      expect(reconstructed).toEqual(lines.map((l) => `- ${l}`));
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });
});
