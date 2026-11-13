# Tracer

Version history for people who will never open a terminal.

## The problem

A deck goes through twenty rounds. It lives in an email thread and a shared
folder as `Pitch_v3_JOHN_EDITS_final2.pptx`. Nobody knows which one is current,
what changed between two of them, or how to get back the version from before
the supervisor's feedback.

Git solves this for programmers. It doesn't work here: it can't dedupe inside a
`.pptx`, it can't diff one, and it needs a terminal.

## What Tracer is

A desktop app that watches a folder. Every time someone saves a tracked file, it
records a version automatically. The user gets a timeline, can name the versions
that matter, can see what changed between any two, and can restore any of them.

No servers. No accounts. No command line. Open source, forever.

Sharing works through a folder the team already syncs — OneDrive, SharePoint,
Dropbox. Tracer never runs a service of its own.

## How storage works

Office files (`.pptx`, `.docx`, `.xlsx`) are zip archives. Tracer unzips each
save into its parts, hashes each part, and stores only parts it has never seen.

A 40MB deck is roughly 39MB of images and 1MB of XML. The images almost never
change, so they're stored once across hundreds of versions. Fixing a typo costs
a few KB, not 40MB.

A version is just a list of `(part name, hash)`. Restoring means re-zipping
those parts.

This is git's object model, applied one level deeper — inside the archive
instead of treating it as one opaque blob. That difference is the whole product.

## The `.tracer` folder

One hidden folder per tracked folder, holding the history for everything in it:

```
Team Pitch/                    ← synced by OneDrive
  Pitch.pptx                   ← the real files, untouched
  Budget.xlsx
  .tracer/
    objects/
      ab/cdef1234...           ← content-addressed parts
      c3/11aa22bb...
    meta/
      eric.jsonl               ← one journal per device
      supervisor.jsonl
```

Hidden by the leading dot on macOS and Linux; set the hidden attribute on
Windows. Users never open it.

**Why a folder and not one packed file.** A single file on OneDrive means every
save re-uploads the whole archive, and two people saving at once produces a
silent conflict copy (`Pitch-DESKTOP-K7F2.tracer`) with no way to reconcile.
A folder of small files syncs incrementally, and because every object is named
after the hash of its own contents, two people writing the same object write
byte-identical bytes. Conflict is impossible by construction.

The packed single file exists only as an export, for emailing to someone outside
the shared folder. Import unzips it back. **Folder is how it lives; zipped file
is how it travels.**

### Why journals instead of a shared database

Objects can't conflict. The *list of versions* could, since it's something
people append to.

So each device writes only its own append-only journal — `eric.jsonl`,
`supervisor.jsonl`, one line per version. No two writers ever touch the same
file. On open, Tracer reads every journal and merges them into one timeline.

This makes SQLite a **local derived cache**, not the source of truth. It stays
in the OS app-data directory and is never synced — a SQLite file in a synced
folder gets written mid-sync and corrupts. Delete it and it rebuilds from the
journals.

| Lives in `.tracer/` (synced) | Lives in app-data (local) |
|---|---|
| `objects/` — the actual content | `tracer.db` — index for fast queries |
| `meta/*.jsonl` — version records | Export cache, render cache |

## Collaboration model

Two people edit the same deck, so one of them has to lose. There is no sane
three-way merge of DrawingML, and pretending otherwise is a dead end.

Tracer branches and does not merge. Your line of versions and your supervisor's
are separate lineages. You look at them side by side and a human picks. That
isn't a limitation to apologize for — it *is* the review feature.

Consequence: versions form a graph, not a list, and timestamps from different
machines can't order them (clock skew, offline edits). So each version needs a
parent pointer and an author, and its id must be a content hash rather than an
autoincrementing number, or ids collide across machines.

## Which file types

The split isn't popularity, it's structure.

| Tier | Formats | Dedupe | Diff |
|---|---|---|---|
| Zip container | pptx, docx, xlsx | per part | per part, plus extracted text |
| Plain text | md, txt, csv, json | whole file | real line diff |
| Blob | pdf, png, psd, mp4 | whole file | changed / not changed |

Only the first tier gets part-level dedupe, but all three work — a blob is just
a version with exactly one part. The schema already handles this: `entry_name`
and `compression_method` are plain columns, so a PDF is one row.

Ship with the three Office formats as the default, because "version control for
the files you email around" is a sharper pitch than "another backup tool." Keep
the tier list in config so widening it is one line.

The real limit isn't format, it's noise — skip dotfiles, skip anything over
~100MB, or a vault pointed at `node_modules` eats the disk.

**Known weak spot:** `.xlsx` keeps each worksheet as one XML part, so a 60MB
spreadsheet re-stores ~60MB when one cell changes. Dedupe doesn't help there.

## Diffing

Two levels, ship the first.

**Which parts changed** is free — compare two versions' part lists. `slide4.xml`
differs, `slide7.xml` doesn't. That's the file-tree sidebar of a PR, as one
query, with no new storage.

**What changed inside** comes from pulling the text runs out of the XML
(`<a:t>` for slides, paragraphs for Word, cell values for Excel) and word-diffing
them. Never raw-diff the XML — it reshuffles on every save and shows 400 changed
lines for a one-word edit.

**Visual diff** — rendering slides to images via headless LibreOffice — is the
version that truly feels like a PR, but it's an external dependency and seconds
per render. Defer it, gate it on LibreOffice being installed.

## The UI

The audience is consultants, founders, researchers and students. Assume no git,
no terminal, no concept of a commit.

Screens:

1. **Folders** — pick a folder to watch. Empty state explains it in one line.
2. **Files** — what's being tracked, when each last changed.
3. **Timeline** — versions newest-first, with size and author. Name any version
   ("sent to supervisor"). Named ones are visually pinned.
4. **Compare** — pick two versions, see changed slides/pages listed, click one
   for side-by-side text.
5. **Restore** — puts a copy on the Desktop, read-only, never overwrites the
   live file.

Vocabulary, enforced everywhere:

| Never say | Say |
|---|---|
| commit, revision | version |
| repository, vault | folder |
| checkout, restore HEAD | save a copy |
| diff | compare |
| branch | their copy / your copy |

New files are tracked **silently**, not with a prompt. Prompting on every new
file is 30 modals when someone drops in a folder, and it defeats the point —
if it asks, it's git with extra clicks. Show "3 new files tracked" passively
with an undo. Opt-out beats opt-in when a wrong guess costs one click.

## Current state

| Piece | Status |
|---|---|
| `db.rs` | Done — full schema and query API |
| `lib.rs` | Done — 6 commands, shared connection, event to UI |
| `watcher.rs` | Stub — filters events correctly, then drops them |
| `engine.rs` | Missing |
| Frontend | Still the `greet` scaffold |

`cargo check` currently fails with 4 errors, all of the form "lib.rs calls
something not written yet": missing `engine` module, missing `watcher::spawn`
and `watcher::WatchHandle`, missing `db::get_revision_target`. Those signatures
are the spec for the work below.

## Build order

Single-player first. Sync is worthless without a working timeline.

1. **`engine.rs`** — `snapshot(path, objects_root) -> Vec<PartRecord>` and
   `restore(parts, objects_root, dest)`. Hash each part's *uncompressed* bytes,
   or the same slide saved by two Office versions dedupes as two objects.
   Shard objects as `objects/ab/cdef...` by hash prefix from the start.
2. **`db::get_revision_target`** — one query, unblocks export.
3. **`watcher.rs`** — real `spawn`/`WatchHandle`. Needs a ~5s debounce (Office
   writes a temp file then renames, so one save fires several events) and the
   `latest_manifest_hash` check so an unchanged save makes no version.
4. **Frontend** — folders, files, timeline, name, restore. Listen for
   `revision-created`.
5. **Compare** — part-list diff, then extracted-text diff.
6. **Schema change for multiplayer** — content-hash version ids, parent pointer,
   author. Cheap now, painful once there's real data.
7. **`.tracer` folders and journals** — move objects out of app-data, write and
   merge journals, rebuild the DB cache from them.
8. **Export / import** — zip and unzip a `.tracer`, for the email path.
9. **Pruning** — keep 30 days plus every named version, sweep unreferenced
   objects. This is the answer to unbounded growth.

## Decided

- **No servers, ever.** Sync rides on a folder the team already has. Keeps the
  project free to run, and keeps someone's unpublished thesis off a stranger's
  infrastructure.
- **Open source**, permissive license.
- **Hashes name the files.** That single choice is what makes sync conflict-free,
  dedupe free, and merging two diverged copies a matter of concatenating journals.
- **Never touch the live file.** Restores go to the Desktop, read-only.

## Open questions

- **The supervisor needs Tracer installed**, or the handoff dies. Mitigation:
  an export drops the reconstructed `.pptx` next to the `.tracer` file, so they
  can ignore the history and still open the deck.
- **OneDrive's own sync writes trigger the watcher.** When their version lands on
  your disk, your watcher fires. The `latest_manifest_hash` check stops a
  duplicate version, but author attribution must be resolved before that, or
  their work gets recorded under your name.
- **Files On-Demand** means a synced file may be a placeholder, not local bytes.
  Reading an object can block on a download or fail offline.
- **Windows path length** with sharded hash directories under SharePoint —
  fails at sync time, not write time, and silently. Needs a real test.
- **No server also means no backup.** Tracer protects against bad edits, not a
  deleted folder. Say so plainly in the UI rather than letting people assume.
