# Tracer's look

Mac OS 8 Platinum and Windows 95, built properly for a modern screen.

## Why this, for this product

Tracer is a utility that sits quietly and keeps your work safe. The era of
System 7 and Windows 95 is when desktop software still looked like a *tool* —
beveled, physical, legible, with no gradient washes or persuasion in it. A
control panel didn't try to delight you; it told you its state and let you
change it.

That is exactly the right register for software whose job is "your deck is
safe." The retro styling isn't nostalgia for its own sake: chiseled bevels
encode affordance better than flat design does, and this app's whole UI is
lists and state.

"Modernized" means: real spacing, legible type sizes, dark mode, keyboard
focus, no 8px-tall click targets. It does not mean rounded corners or shadows.

## Color

Platinum greys, warmed slightly so it doesn't read as dead pixels.

| Token | Light | Role |
|---|---|---|
| `--chrome` | `#d6d2ca` | Window background, the dominant surface |
| `--face` | `#cac5bc` | Raised controls: buttons, title bar |
| `--well` | `#f2f0ec` | Sunken content areas: lists, text fields |
| `--light` | `#ffffff` | Top-left bevel |
| `--shade` | `#817d75` | Bottom-right bevel |
| `--face-light` | `#e4e0d8` | Inner step of a raised bevel |
| `--face-dark` | `#a6a199` | Inner step of a sunken bevel |
| `--edge` | `#2b2925` | Hard outer border, near-black but warm |
| `--ink` | `#17150f` | Text |
| `--select` | `#000080` | Selection fill, white text on it |
| `--live` | `#007a3d` | Watching. The only green in the app |
| `--warn` | `#8a5a00` | Not watched. The only amber |

Dark mode has no historical basis, so it is an honest reinterpretation: chrome
`#39352f`, well `#211f1b`, bevels inverted (light becomes `#56514a`, shade
becomes `#15130f`), ink `#e8e4da`. Selection stays navy — it still reads.

## Type

**Geneva** for everything, with **Monaco** only where digits must align.

Both ship with macOS, so there is no font to download, no network call at
runtime, and no new dependency. Geneva *is* the classic Mac UI face — using the
real thing rather than a lookalike is the whole point.

```
--ui:   Geneva, "MS Sans Serif", Verdana, sans-serif
--data: Monaco, "Courier New", monospace
```

Scale, deliberately small-stepped the way system UIs were:

| Use | Size | Weight |
|---|---|---|
| Window title | 13px | bold |
| Pane headers | 11px | bold |
| Body, list rows | 13px | normal |
| Secondary (timestamps, counts) | 11px | normal |

No all-caps labels. Sentence case everywhere.

## Layout

The window is a classic document window: a title bar, a menu strip, then
sunken panes.

```
┌─────────────────────────────────────────────────────────────┐
│ ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓  Tracer  ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓ │  pinstripe title bar
├─────────────────────────────────────────────────────────────┤
│ ● Watching 1 folder              [ Settings ] [ Quit Tracer ]│  status strip
├───────────────┬──────────────────┬──────────────────────────┤
│ Folders       │ Files            │ Chapter 3.docx           │  pane headers
│┌─────────────┐│┌────────────────┐│┌────────────────────────┐│
││ Thesis      ││││Chapter 3.docx │││ ★ sent to supervisor   ││  sunken wells
││  ● Watching ││││Defense deck   │││   today 12:20  3.6 MB  ││
││ Client work ││││Results.xlsx   │││ ▸ 12:11–12:18, 4 saves ││
││  ▲ Not watch││││               │││   today 12:28  3.6 MB  ││
│└─────────────┘│└────────────────┘│└────────────────────────┘│
└───────────────┴──────────────────┴──────────────────────────┘
```

Left-aligned throughout. Lists are flush; no card gutters between rows.

**Settings moves into a dialog.** Name, email and launch-at-login are set once
and never looked at again — they do not deserve a third of the window. A modal
styled as a classic dialog box (raised panel, hard border, OK button) is both
the right information architecture and the most era-appropriate element in the
whole design.

## Principles

1. **Bevels carry meaning, not decoration.** Raised = you can press it. Sunken
   = content lives here. Never bevel something inert. Two pixels deep, not one:
   a 1px bevel reads as a hairline and the whole interface looks flat, which is
   why there are inner `--face-light` / `--face-dark` steps between the bright
   edge and the shadow.
2. **Spend the boldness on the title bar.** The pinstriped bar is the one
   memorable flourish. Everything below it stays quiet.
3. **Selection is a navy block with white text**, not a pale tint. Era-correct,
   and far clearer about what is selected.
4. **One green, one amber, one navy.** Color only ever means watching, not
   watched, or selected. Nothing is colored to look nice.
5. **No rounded corners, no shadows, no gradients** except the title bar's
   pinstripes. Authentic, and it keeps the eye on the content.
6. **Rows are flush and dense**, like a Finder list. A timeline of 200 versions
   has to stay scannable.
7. Modern floor, unannounced: visible keyboard focus, `prefers-reduced-motion`
   respected, hit targets at least 24px, contrast that passes.
