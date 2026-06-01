# rmbujo

**A dot-grid bullet-journal generator for reMarkable e-ink tablets.**

`rmbujo` builds a full year of clean, typeset PDF notebooks — a future log, twelve
monthly notebooks, a reference key, and collection templates — sized exactly for the
[reMarkable Paper Pro](https://remarkable.com) and Paper Pro Move. It overlays your
real calendar (any ICS / webcal feed) onto each month, then syncs everything to the
device **without ever touching your handwriting**.

It's written in Rust and renders HTML/CSS straight to PDF with
[fulgur](https://crates.io/crates/fulgur) (Blitz + krilla) — there is no headless
browser involved, so output is fast and byte-for-byte deterministic.

> **This is a library crate.** `rmbujo` is the generation engine inside the
> [`rmapps`](../../README.md) workspace; the user-facing command is `rmapps bujo`.
> Configuration lives in the unified `~/.config/rmapps/config.toml` under a
> `[bujo]` section. Everything below describes the engine and that command.

<p align="center">
  <img src="docs/images/cover.png" width="240" alt="Future Log cover">
  &nbsp;&nbsp;
  <img src="docs/images/monthly_view.png" width="240" alt="May 2026 month index with calendar badges">
  &nbsp;&nbsp;
  <img src="docs/images/day_events.png" width="240" alt="A single day's calendar detail">
</p>

---

## Why this exists

Stock reMarkable templates are fine, but a bullet journal wants a few things they
don't give you:

- **One notebook per month**, each a tidy index of numbered days, with a tappable
  link from any day to its own detail page.
- **Your calendar, on the page.** Holidays, work meetings, appointments — pulled from
  ICS feeds and printed right next to the day, with a colored swatch per calendar.
- **Handwriting that survives regeneration.** When your calendar changes, rmbujo
  refreshes only the printed background via reMarkable's *content-only* sync. Anything
  you've written by hand stays exactly where it is.
- **A dot grid that matches the device.** Spacing lines up with reMarkable's built-in
  *Dots Small* template, so pages you add on the tablet blend right in.

If you keep an analog-style journal on a reMarkable but want it pre-loaded with the
year's structure and your calendar, that's what this is for.

## What it produces

A single flat folder per year, one PDF per notebook:

| File                          | What it is                                                        |
| ----------------------------- | ----------------------------------------------------------------- |
| `2026 Future Log.pdf`         | Year-at-a-glance spread, one block per month                      |
| `2026.01 January.pdf` … `.12` | One notebook per month: day index + a detail page for every day   |
| `2026 Collection Template.pdf`| Blank dot-grid + task pages for ad-hoc collections                 |
| `2026 Reference.pdf`          | The bullet-journal key (task, event, migrated, note, …)           |

### A look inside

<table>
<tr>
<td align="center"><img src="docs/images/monthly_view.png" width="200"><br><sub><b>Monthly index</b> — every day, with a badge counting that day's events.</sub></td>
<td align="center"><img src="docs/images/day_events.png" width="200"><br><sub><b>Day detail</b> — full agenda with times, location, notes, and a color per calendar.</sub></td>
<td align="center"><img src="docs/images/future_log.png" width="200"><br><sub><b>Future log</b> — the year at a glance.</sub></td>
</tr>
<tr>
<td align="center"><img src="docs/images/daily_page.png" width="200"><br><sub><b>Daily writing page</b> — dot grid with the date and a tap-back link.</sub></td>
<td align="center"><img src="docs/images/reference.png" width="200"><br><sub><b>Reference key</b> — the bullet-journal notation legend.</sub></td>
<td align="center"><img src="docs/images/tasks.png" width="200"><br><sub><b>Collection template</b> — blank pages for lists and trackers.</sub></td>
</tr>
</table>

> The calendar entries shown above (Victoria Day, Dentist, Lunch with Sam, …) are
> fictional sample data, not a real calendar.

## Install

Build the `rmapps` binary from the workspace root — one build gives you every
subcommand, `bujo` included:

```sh
cargo build --release        # binary at ./target/release/rmapps
```

On Nix, a dev shell for this crate is available for working on the engine
itself (`nix develop ./crates/rmbujo`), but a plain `cargo build` from the repo
root builds the whole workspace.

## Quick start

Pair the machine once (native — no rmapi), then add a `[bujo]` section to
`~/.config/rmapps/config.toml` and generate the year:

```sh
rmapps auth login            # paste the 8-char code from my.remarkable.com
rmapps bujo                  # generate the whole year and deploy it
```

Set `deploy.backend = "none"` in the `[bujo]` section to generate the PDFs
without uploading.

## Usage

```sh
# Generate the whole year and deploy it (reuses cached calendar feeds — fast & offline).
rmapps bujo

# Same, but re-fetch every ICS feed first.
rmapps bujo --refresh-feeds

# Refresh just one month (upsert), leaving every other month — and your ink — alone.
rmapps bujo --only-month 5

# Generate this month and later only; earlier months are kept on-device.
rmapps bujo --from-month 5

# Regenerate a single month and upsert it to a specific target folder.
rmapps bujo --month 5 --target /
```

## Calendar feeds (ICS)

Add feeds under the `[bujo]` section of `~/.config/rmapps/config.toml`. Each
feed's events are overlaid on the monthly notebooks:

```toml
[bujo]
year = 2026
timezone = "America/Toronto"   # IANA timezone — used for all event rendering

  [[bujo.ics]]
  name = "Holidays"
  url  = "https://example.com/holidays.ics"   # color omitted → auto-assigned

  [[bujo.ics]]
  name  = "Work"
  url   = "webcal://example.com/work.ics"      # webcal:// is accepted (treated as https)
  color = "primary"                            # optional override
```

- **Colors are automatic.** Omit `color` and each feed gets a distinct shade from a
  10-color palette, in order — so multiple calendars stay readable with zero setup.
  To pin one, set `color` to `primary` (indigo), `accent` (tomato), `rust`, `muted`,
  or `cal1`…`cal10`. An unknown name is rejected with the list of valid options.
- **Full detail.** Events render with their start time on the day index; the detail
  page shows the start–end range, location, notes, and attendees when the feed
  provides them.
- **Robust timezones.** IANA names, fixed offsets (`GMT+0200`), and Windows/Outlook
  names (`Eastern Standard Time`) are all handled and converted to your configured
  `timezone`.
- **Cached & reproducible.** Fetched feeds are cached under `<year>/.ics-cache/`. A
  plain regenerate reuses the snapshot — fast, deterministic, and offline. Pass
  `--refresh-feeds` to force a re-fetch.

## Syncing to the reMarkable

Set `deploy.base_folder = "/rmbujo"` in the `[bujo]` section (leave
`deploy.backend` at its default to upload, or set it to `"none"` to generate
only). Cloud sync is native — a pure-Rust client, no external `rmapi` tool. Pair
once with `rmapps auth login` and paste a code from
<https://my.remarkable.com/device/desktop/connect>. After that:

- `rmapps bujo` uploads the year's PDFs to `<base_folder>/<year>` (e.g. `/rmbujo/2026`).
- It re-syncs each notebook with a **content-only** cloud update, which
  **replaces each PDF's printed background without touching your handwriting**.

**Device sync rule:** always sync the device *before* running `rmapps bujo`, then
sync again after. This guarantees handwriting you added on the device reaches the
cloud before the content-only push, so nothing is lost.

## Adding pages on the device

To insert an extra page directly on the reMarkable, tap **+** and choose the built-in
**Dots Small** template — its dot grid matches rmbujo's spacing exactly. No sideloaded
template is needed.

## Supported devices

| Device                      | `device` key      |
| --------------------------- | ----------------- |
| reMarkable Paper Pro Move   | `paper-pro-move`  |
| reMarkable Paper Pro        | `paper-pro`       |

Paper Pro Move is the primary, most-tested target.

## Development

```sh
make test             # full suite in the Nix shell
make update-goldens   # regenerate visual-regression golden images
make clippy           # lints (warnings are errors)
make build            # nix build (compiles + checks this crate)
make hooks            # enable the tracked pre-commit hook (cargo fmt --check)
```

The README screenshots are regenerated from fictional sample data with:

```sh
nix develop -c cargo run --example screenshots   # writes docs/images/*.png
```

## License

[MIT](LICENSE) © Dan Cardamore
