#!/usr/bin/env node
// SpeakFlow Discord release poster.
// Reads the webhook from ~/.config/speakflow/discord-release-webhook.json
// (or SPEAKFLOW_DISCORD_WEBHOOK_CONFIG, never printed). Dry-run by default;
// --send actually posts.

import { readFileSync, existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const CONFIG =
  process.env.SPEAKFLOW_DISCORD_WEBHOOK_CONFIG ||
  join(homedir(), ".config", "speakflow", "discord-release-webhook.json");
const COLOR = 4363994;
const FOOTER = "SpeakFlow Automated Release System";

function fail(msg) {
  console.error(`error: ${msg}`);
  process.exit(1);
}

function parseArgs(argv) {
  const out = { send: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--send") out.send = true;
    else if (a === "--dry-run") out.send = false;
    else if (a.startsWith("--")) {
      const key = a.slice(2);
      const val = argv[++i];
      if (val === undefined) fail(`missing value for ${a}`);
      out[key] = val;
    } else fail(`unexpected argument: ${a}`);
  }
  return out;
}

function bullets(raw) {
  return raw
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l.length > 0)
    .map((l) => "- " + l.replace(/^[-*•]\s*/, ""))
    .join("\n");
}

function splitFieldValue(text, limit) {
  const lines = text.split("\n");
  const chunks = [];
  let current = "";
  for (const line of lines) {
    if (line.length > limit) {
      fail(`a bullet exceeds Discord's ${limit} character field limit: ${line.slice(0, 60)}...`);
    }
    const next = current ? `${current}\n${line}` : line;
    if (next.length <= limit) {
      current = next;
      continue;
    }
    chunks.push(current);
    current = line;
  }
  if (current) chunks.push(current);
  return chunks;
}

function humanBytes(n) {
  if (n >= 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  if (n >= 1024) return `${(n / 1024).toFixed(0)} KB`;
  return `${n} B`;
}

const args = parseArgs(process.argv.slice(2));
if (!args.version) fail("--version is required");
if (!/^\d+\.\d+\.\d+$/.test(args.version)) fail("--version must be X.Y.Z");
if (!args.notes) fail("--notes is required");
if (!args.file && !args.link) fail("provide --file <installer> or --link <url>");

if (!existsSync(CONFIG)) fail(`missing webhook config: ${CONFIG}`);
const cfg = JSON.parse(readFileSync(CONFIG, "utf8"));
for (const k of ["webhookId", "webhookToken", "channelId"]) {
  if (!cfg[k]) fail(`webhook config missing ${k}`);
}

if (!existsSync(args.notes)) fail(`notes file not found: ${args.notes}`);
const notesText = readFileSync(args.notes, "utf8");
const whatIsNew = bullets(notesText);
if (!whatIsNew) fail("notes file produced no bullets");

let fileBytes = null;
let fileName = null;
if (args.file) {
  if (!existsSync(args.file)) fail(`installer not found: ${args.file}`);
  fileBytes = readFileSync(args.file);
  fileName = args.file.split(/[\\/]/).pop();
  const limit = 10 * 1024 * 1024;
  if (fileBytes.length > limit) {
    fail(
      `installer is ${humanBytes(fileBytes.length)}, above the 10 MB webhook upload limit. ` +
        `Publish it as a GitHub release asset and re-run with --link instead.`
    );
  }
}

const product = "SpeakFlow";
const version = args.version;
const configuration = args.configuration || "Windows x64";

const fields = [{ name: "Version", value: version, inline: true }];
fields.push({ name: "Configuration", value: configuration, inline: true });
if (fileName) {
  fields.push({ name: "Attached File", value: `${fileName} (${humanBytes(fileBytes.length)})`, inline: true });
} else {
  fields.push({ name: "Download", value: `[${product} ${version}](${args.link})`, inline: true });
}
const notesChunks = splitFieldValue(whatIsNew, 1024);
if (fields.length + notesChunks.length > 25) {
  fail(`release notes need ${notesChunks.length} fields, above Discord's 25 field embed limit`);
}
notesChunks.forEach((chunk, i) => {
  fields.push({
    name: i === 0 ? "What's New" : `What's New (${i + 1}/${notesChunks.length})`,
    value: chunk,
    inline: false,
  });
});

const payload = {
  username: `${product} Release Bot`,
  allowed_mentions: { parse: [] },
  content: fileName
    ? `New Release: **${product} ${version}** is now available! Download the attached installer below.`
    : `New Release: **${product} ${version}** is now available! Use the download link below.`,
  embeds: [
    {
      title: `Release: ${product} ${version}`,
      color: COLOR,
      description: fileName
        ? `Download the installer below, close any running SpeakFlow window, then run the setup file.`
        : `Download the installer from the link below, close any running SpeakFlow window, then run the setup file.`,
      fields,
      footer: { text: FOOTER },
      timestamp: new Date().toISOString(),
    },
  ],
};

if (!args.send) {
  console.log("DRY RUN - nothing sent.");
  console.log(`channel: #${cfg.channelName} (${cfg.channelId})`);
  console.log(`webhook: ${cfg.webhookName} (${cfg.webhookId})`);
  console.log("payload_json:\n" + JSON.stringify(payload, null, 2));
  if (fileName) console.log(`attachment: ${fileName} (${humanBytes(fileBytes.length)})`);
  process.exit(0);
}

const url = `https://discord.com/api/v10/webhooks/${cfg.webhookId}/${cfg.webhookToken}?wait=true`;
const form = new FormData();
form.append("payload_json", JSON.stringify(payload));
if (fileName) {
  form.append("files[0]", new Blob([fileBytes], { type: "application/octet-stream" }), fileName);
}

const res = await fetch(url, { method: "POST", body: form });
const bodyText = await res.text();
if (!res.ok) {
  fail(`Discord returned HTTP ${res.status}: ${bodyText.slice(0, 500)}`);
}
let msg = {};
try {
  msg = JSON.parse(bodyText);
} catch {
  /* ignore */
}
const link =
  msg.id && msg.channel_id
    ? `https://discord.com/channels/${cfg.guildId}/${msg.channel_id}/${msg.id}`
    : null;
console.log(JSON.stringify({ ok: true, messageId: msg.id ?? null, channelId: msg.channel_id ?? null, link }, null, 2));
