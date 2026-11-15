// Renders the real Svelte UI in Chromium with Tauri's IPC bridge stubbed, so
// the design can be looked at instead of guessed at. Tauri isn't running here:
// every invoke is answered from the fixtures below.
//
//   node tools/screenshot.mjs
//
// Starts vite on :1420 if nothing is serving yet and stops it afterwards, so it
// never leaves a server holding the port that `npm run tauri dev` needs. An
// already-running server is used as-is and left alone.
//
// Writes PNGs to src-tauri/target/shots/ (gitignored).
import { chromium } from "playwright";
import { mkdirSync } from "node:fs";
import { spawn } from "node:child_process";
import { connect } from "node:net";

const OUT = "src-tauri/target/shots";
const URL = "http://localhost:1420/";

const hour = 3600;
const day = 24 * hour;
const now = Math.floor(Date.now() / 1000);

// Months of real-looking history for Chapter 3.docx, so the activity spine
// has something to show: a burst today, a named version, a quiet week, a
// cluster of saves, a two-month gap, then an older named version - the
// oldest entry, so the spine's first mark is a heavy one too.
const versions = [
  { id: 60, ts: now - 3 * 60, milestone: null },
  { id: 59, ts: now - 20 * 60, milestone: "sent to supervisor" },
  { id: 58, ts: now - 25 * 60, milestone: null },
  { id: 57, ts: now - 30 * 60, milestone: null },
  { id: 56, ts: now - 35 * 60, milestone: null },
  { id: 55, ts: now - 40 * 60, milestone: null },
  { id: 54, ts: now - 9 * day - 10 * 60, milestone: null },
  { id: 53, ts: now - 9 * day - 20 * 60, milestone: null },
  { id: 52, ts: now - 9 * day - 30 * 60, milestone: null },
  { id: 51, ts: now - 9 * day - 30 * 60 - 65 * day, milestone: "first full draft" },
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

// The sparse case: a file with barely any history, so the spine has to look
// deliberate rather than broken with next to nothing to draw.
const sparseVersions = [
  { id: 2, ts: now - 2 * day, byteSize: 48_000 },
  { id: 1, ts: now - 3 * day - 2 * hour, byteSize: 47_500 },
].map((v) => ({
  id: v.id,
  fileId: 3,
  timestamp: v.ts,
  byteSize: v.byteSize,
  manifestHash: `hash-s${v.id}`,
  partCount: 6,
  milestone: null,
  authorName: "Eric Feng",
  authorEmail: "eric@example.com",
  parentId: v.id === 1 ? null : v.id - 1,
}));

// Just enough history on the deck to drive the compare flow: a named older
// version and today's unnamed one, far enough apart they never collapse into
// one session.
const deckVersions = [
  { id: 102, ts: now - 10 * 60, milestone: null },
  { id: 101, ts: now - 2 * day, milestone: "first full draft" },
].map((v, i) => ({
  id: v.id,
  fileId: 2,
  timestamp: v.ts,
  byteSize: 5_204_000 - i * 1800,
  manifestHash: `hash-${v.id}`,
  partCount: 58,
  milestone: v.milestone,
  authorName: "Eric Feng",
  authorEmail: "eric@example.com",
  parentId: v.id === 101 ? null : v.id - 1,
}));

const TIMELINES = { 1: versions, 2: deckVersions, 3: sparseVersions };

// Dedup payoff per file, independent of the timeline's own length. The sparse
// file has one version's worth of history and nothing shared yet, so stats
// deliberately show storedBytes >= logicalBytes - the "nothing saved yet" case.
const FILE_STATS = {
  1: {
    versionCount: versions.length,
    logicalBytes: versions.reduce((sum, v) => sum + v.byteSize, 0),
    storedBytes: Math.round(versions.reduce((sum, v) => sum + v.byteSize, 0) * 0.22),
  },
  2: {
    versionCount: deckVersions.length,
    logicalBytes: deckVersions.reduce((sum, v) => sum + v.byteSize, 0),
    storedBytes: Math.round(deckVersions.reduce((sum, v) => sum + v.byteSize, 0) * 0.3),
  },
  3: {
    versionCount: sparseVersions.length,
    logicalBytes: sparseVersions.reduce((sum, v) => sum + v.byteSize, 0),
    storedBytes: sparseVersions.reduce((sum, v) => sum + v.byteSize, 0) + 500,
  },
};

// compare_revisions's fixture: a realistic pptx diff. A couple of changed
// slides, one picture added, one removed, and several bookkeeping entries
// (docProps, [Content_Types].xml, _rels) that must never surface as named
// changes - only their count should.
const COMPARE_RESULT = {
  changes: [
    { entryName: "[Content_Types].xml", status: "changed" },
    { entryName: "docProps/app.xml", status: "changed" },
    { entryName: "docProps/core.xml", status: "changed" },
    { entryName: "ppt/_rels/presentation.xml.rels", status: "changed" },
    { entryName: "ppt/media/image11.png", status: "added" },
    { entryName: "ppt/media/image3.png", status: "removed" },
    { entryName: "ppt/slides/_rels/slide4.xml.rels", status: "changed" },
    { entryName: "ppt/slides/slide4.xml", status: "changed" },
    { entryName: "ppt/slides/slide9.xml", status: "changed" },
  ].sort((a, b) => a.entryName.localeCompare(b.entryName)),
  unchangedCount: 49,
};

// compare_part_text's fixture, for clicking "Slide 4" in the changed-parts
// list. Deliberately long: enough paragraphs that the collapsed-run and
// prev/next controls mean something, changes scattered through it rather
// than bunched at the top, and one paragraph long enough to wrap across
// several lines so the gutter's wrap behaviour is visible.
const same = (text) => ({ status: "same", left: text, right: text, leftSpans: [], rightSpans: [] });
const changed = (left, right, word, replacement) => ({
  status: "changed",
  left,
  right,
  leftSpans: [
    { text: left.slice(0, left.indexOf(word)), changed: false },
    { text: word, changed: true },
    { text: left.slice(left.indexOf(word) + word.length), changed: false },
  ],
  rightSpans: [
    { text: right.slice(0, right.indexOf(replacement)), changed: false },
    { text: replacement, changed: true },
    { text: right.slice(right.indexOf(replacement) + replacement.length), changed: false },
  ],
});
const added = (text) => ({ status: "added", left: null, right: text, leftSpans: [], rightSpans: [] });
const removed = (text) => ({ status: "removed", left: text, right: null, leftSpans: [], rightSpans: [] });

const TEXT_COMPARISON = {
  rows: [
    same("Thank you for your continued partnership this quarter."),
    same(
      "Our engineering team shipped four major releases during the quarter, closing out a sustained backlog of customer-requested improvements across the reporting dashboard, the mobile client, and the underlying data pipeline that feeds every downstream integration our partners depend on.",
    ),
    same("Overall growth remained steady across every region we track."),
    changed(
      "Revenue grew by 12% over the prior period.",
      "Revenue grew by 18% over the prior period.",
      "12%",
      "18%",
    ),
    same("Customer churn held flat at just under two percent."),
    same("Support tickets per seat continued trending downward."),
    same("The onboarding team closed out its backlog in record time."),
    same("Marketing spend tracked within two percent of budget."),
    same("Every region met its quarterly hiring plan."),
    same("The platform's uptime held above our internal target."),
    same("Legal finished its review of the updated vendor contracts."),
    same("Finance closed the books three days ahead of schedule."),
    removed("The pilot program with three customers has concluded."),
    same("Looking ahead, three initiatives define the next quarter."),
    same("First, the enterprise tier moves out of private beta."),
    added("We are also launching a new enterprise tier next month."),
    same("Second, the mobile client gets offline support."),
    changed(
      "The team expects a two week rollout window for this change.",
      "The team expects a one week rollout window for this change.",
      "two",
      "one",
    ),
    same("Third, the reporting dashboard adds a cohort view."),
    same("None of this changes the roadmap we shared in January."),
    same("We remain on track for the targets set at the offsite."),
    same("Thank you again for the trust you've placed in the team."),
  ],
  hasText: true,
};

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
  // get_timeline and get_file_stats are answered per-fileId below instead of
  // from this flat table - see the invoke stub in run().
  get_identity: { name: "Eric Feng", email: "eric@example.com" },
  is_autostart_enabled: false,
  set_identity: null,
  set_autostart: null,
  set_milestone: null,
  save_copy: "/Users/eric/Desktop/Chapter 3_rev31.docx",
  compare_revisions: COMPARE_RESULT,
  compare_part_text: TEXT_COMPARISON,
  add_vault: null,
  quit_app: null,
};

const EMPTY = { ...RESPONSES, list_vaults: [], list_tracked_files: [] };

async function shoot(page, name, { width = 1100, height = 760, dark = false } = {}) {
  await page.setViewportSize({ width, height });
  await page.emulateMedia({ colorScheme: dark ? "dark" : "light" });
  await page.waitForTimeout(350);
  await page.screenshot({ path: `${OUT}/${name}.png` });
  console.log(`${OUT}/${name}.png`);
}

const portIsOpen = () =>
  new Promise((resolve) => {
    // vite binds "localhost", which resolves to ::1 before 127.0.0.1 -
    // probing the IPv4 address alone never sees it.
    const socket = connect(1420, "localhost")
      .on("connect", () => (socket.end(), resolve(true)))
      .on("error", () => resolve(false));
  });

/// Starts vite only if nothing is already serving, and shuts down whatever it
/// started. A leftover dev server holds port 1420 and makes the next
/// `npm run tauri dev` fail with "Port 1420 is already in use" - a confusing
/// error a long way from its cause.
const ensureServer = async () => {
  if (await portIsOpen()) return () => {};

  // Spawned directly rather than through `npm run dev`: killing npm leaves the
  // vite process it spawned orphaned and still holding the port.
  const vite = spawn("node_modules/.bin/vite", ["dev"], { stdio: "ignore" });
  for (let i = 0; i < 120; i++) {
    if (await portIsOpen()) break;
    await new Promise((r) => setTimeout(r, 250));
  }
  if (!(await portIsOpen())) {
    vite.kill("SIGKILL");
    throw new Error("vite did not start on :1420");
  }
  return () => vite.kill("SIGKILL");
};

const run = async () => {
  mkdirSync(OUT, { recursive: true });
  const stopServer = await ensureServer();
  process.on("exit", stopServer);
  process.on("SIGINT", () => (stopServer(), process.exit(130)));
  const browser = await chromium.launch();

  const fixtures = [
    ["", RESPONSES, TIMELINES, FILE_STATS],
    ["empty-", EMPTY, {}, {}],
  ];

  for (const [label, table, timelines, fileStats] of fixtures) {
    const page = await browser.newPage();
    await page.addInitScript(
      ({ responses, timelines, fileStats }) => {
        window.__TAURI_INTERNALS__ = {
          invoke: (cmd, args) => {
            if (cmd === "get_timeline") return Promise.resolve(timelines[args?.fileId] ?? []);
            if (cmd === "get_file_stats") {
              return Promise.resolve(
                fileStats[args?.fileId] ?? { versionCount: 0, logicalBytes: 0, storedBytes: 0 },
              );
            }
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
      },
      { responses: table, timelines, fileStats },
    );

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

      // One shot per candidate theme, same data and same selection, so they
      // can be compared without anything else moving.
      for (const variant of ["hardware", "paper", "phosphor"]) {
        await page.evaluate((v) => (document.documentElement.dataset.variant = v), variant);
        await shoot(page, `variant-${variant}`);
      }
      await page.evaluate(() => delete document.documentElement.dataset.variant);

      await shoot(page, "narrow", { width: 760, height: 700 });

      // The sparse case: a file with only two versions, so the spine has to
      // read as deliberate rather than broken with next to nothing to draw.
      await page.getByText("Results.xlsx").first().click().catch(() => {});
      await page.waitForTimeout(200);
      await shoot(page, "sparse-light");
      await shoot(page, "sparse-dark", { dark: true });

      // The compare flow: select a version, start the pick, then pick a
      // second one. Driven on the deck, whose two versions have real
      // slide/picture-shaped parts so the comparison reads true to life.
      // Scoped to the timeline pane (the third pane) - its rows reuse time
      // text ("today HH:MM") that also appears in the Files pane's own rows.
      const timelinePane = page.locator(".panes > section").nth(2);
      await page.emulateMedia({ colorScheme: "light" });
      await page.getByText("Defense deck.pptx").first().click().catch(() => {});
      await page.waitForTimeout(200);
      await timelinePane
        .getByRole("button", { name: /first full draft/ })
        .click()
        .catch(() => {});
      await page.waitForTimeout(150);
      await timelinePane
        .getByRole("button", { name: "Compare…" })
        .click()
        .catch(() => {});
      await page.waitForTimeout(150);
      await shoot(page, "compare-picking");
      await shoot(page, "compare-picking-dark", { dark: true });

      await page.emulateMedia({ colorScheme: "light" });
      await timelinePane
        .getByRole("button", { name: /today/ })
        .first()
        .click()
        .catch(() => {});
      await page.waitForTimeout(250);
      await shoot(page, "compare");
      await shoot(page, "compare-dark", { dark: true });

      // The text-compare flow: click a changed slide's name to open the
      // full-screen side-by-side view. It takes over the whole window, so
      // these shots are full width rather than scoped to the timeline pane.
      await timelinePane
        .getByRole("button", { name: "Slide 4" })
        .click()
        .catch(() => {});
      await page.waitForTimeout(250);
      await shoot(page, "compare-text");
      await shoot(page, "compare-text-dark", { dark: true });

      // prev/next should jump straight to a changed line and scroll it into
      // view. A short viewport forces that to matter - the whole comparison
      // fits in a tall window with nothing to scroll.
      await page.emulateMedia({ colorScheme: "light" });
      await page.setViewportSize({ width: 1100, height: 420 });
      await page.waitForTimeout(100);
      await page.getByRole("button", { name: "Next change" }).click();
      await page.waitForTimeout(150);
      await page.getByRole("button", { name: "Next change" }).click();
      await page.waitForTimeout(400);
      await page.screenshot({ path: `${OUT}/compare-text-navigated.png` });
      console.log(`${OUT}/compare-text-navigated.png`);

      await shoot(page, "compare-text-narrow", { width: 700, height: 900 });

      // Done should hand back exactly the screen left behind: same file,
      // same selection, the changed-parts list still open underneath.
      await page.setViewportSize({ width: 1100, height: 760 });
      await page.getByRole("button", { name: "Done" }).click();
      await page.waitForTimeout(150);
      await shoot(page, "compare-after-done");

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
  stopServer();
};

run();
