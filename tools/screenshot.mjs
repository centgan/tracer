// Renders the real Svelte UI in Chromium with Tauri's IPC bridge stubbed, so
// the design can be looked at instead of guessed at. Tauri isn't running here:
// every invoke is answered from the fixtures below.
//
//   npm run dev          # in another terminal, serves on :1420
//   node tools/screenshot.mjs
//
// Writes PNGs to src-tauri/target/shots/ (gitignored).
import { chromium } from "playwright";
import { mkdirSync } from "node:fs";

const OUT = "src-tauri/target/shots";
const URL = "http://localhost:1420/";

const hour = 3600;
const now = Math.floor(Date.now() / 1000);

// A plausible afternoon: a named version, a burst of saves that should collapse
// into one session, and an older solo save.
const versions = [
  { id: 31, ts: now - 400, milestone: null },
  { id: 30, ts: now - 900, milestone: "sent to supervisor" },
  { id: 29, ts: now - 1000, milestone: null },
  { id: 28, ts: now - 1120, milestone: null },
  { id: 27, ts: now - 1260, milestone: null },
  { id: 26, ts: now - 1400, milestone: null },
  { id: 25, ts: now - 6 * hour, milestone: "first full draft" },
  { id: 24, ts: now - 30 * hour, milestone: null },
].map((v, i) => ({
  id: v.id,
  fileId: 1,
  timestamp: v.ts,
  byteSize: 3_812_000 - i * 1100,
  manifestHash: `hash-${v.id}`,
  partCount: 44,
  milestone: v.milestone,
  authorName: "Eric Feng",
  authorEmail: "eric@example.com",
  parentId: v.id - 1,
}));

const RESPONSES = {
  list_vaults: [
    { id: 1, rootPath: "/Users/eric/Documents/Thesis", isActive: true, hasTracerFolder: true },
    { id: 2, rootPath: "/Users/eric/Documents/Client work", isActive: true, hasTracerFolder: false },
  ],
  list_tracked_files: [
    { id: 1, vaultId: 1, relativePath: "Chapter 3.docx", extension: "docx" },
    { id: 2, vaultId: 1, relativePath: "Defense deck.pptx", extension: "pptx" },
    { id: 3, vaultId: 1, relativePath: "Results.xlsx", extension: "xlsx" },
  ],
  get_timeline: versions,
  get_identity: { name: "Eric Feng", email: "eric@example.com" },
  is_autostart_enabled: false,
  set_identity: null,
  set_autostart: null,
  set_milestone: null,
  save_copy: "/Users/eric/Desktop/Chapter 3_rev31.docx",
  add_vault: null,
  quit_app: null,
};

const EMPTY = { ...RESPONSES, list_vaults: [], list_tracked_files: [], get_timeline: [] };

async function shoot(page, name, { width = 1100, height = 760, dark = false } = {}) {
  await page.setViewportSize({ width, height });
  await page.emulateMedia({ colorScheme: dark ? "dark" : "light" });
  await page.waitForTimeout(350);
  await page.screenshot({ path: `${OUT}/${name}.png` });
  console.log(`${OUT}/${name}.png`);
}

const run = async () => {
  mkdirSync(OUT, { recursive: true });
  const browser = await chromium.launch();

  for (const [label, table] of [["", RESPONSES], ["empty-", EMPTY]]) {
    const page = await browser.newPage();
    await page.addInitScript((responses) => {
      window.__TAURI_INTERNALS__ = {
        invoke: (cmd) => {
          if (cmd in responses) return Promise.resolve(responses[cmd]);
          // Tauri's own plugin IPC (event listen/unlisten) has no fixture and
          // no bearing on how the page looks.
          if (cmd.startsWith("plugin:")) return Promise.resolve(0);
          return Promise.reject(`no fixture for ${cmd}`);
        },
        transformCallback: (cb) => cb,
        unregisterCallback: () => {},
        convertFileSrc: (p) => p,
      };
    }, table);

    const errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    await page.goto(URL, { waitUntil: "networkidle" });

    if (label === "") {
      // Open the first folder and file so the timeline is actually populated.
      await page.getByText("Thesis").first().click().catch(() => {});
      await page.waitForTimeout(200);
      await page.getByText("Chapter 3.docx").first().click().catch(() => {});
      await page.waitForTimeout(200);
    }

    await shoot(page, `${label}light`);
    await shoot(page, `${label}dark`, { dark: true });

    if (label === "") {
      // Select a named version in the timeline so the selection highlight
      // and the bottom action strip's enabled state are both visible.
      await page
        .getByRole("button", { name: /sent to supervisor/ })
        .click()
        .catch(() => {});
      await page.waitForTimeout(200);
      await shoot(page, "timeline-selected");

      await shoot(page, "narrow", { width: 760, height: 700 });
      await page.emulateMedia({ colorScheme: "light" });
      await page.getByRole("button", { name: /settings/i }).click();
      await page.waitForTimeout(300);
      await shoot(page, "settings");
      await shoot(page, "settings-dark", { dark: true });
      await page.keyboard.press("Escape");
    }

    if (errors.length) console.error(`page errors (${label || "main"}):`, errors.slice(0, 4));
    await page.close();
  }

  await browser.close();
};

run();
