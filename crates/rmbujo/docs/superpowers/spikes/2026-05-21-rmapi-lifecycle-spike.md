# Spike: rmapi non-destructive refresh on the Paper Pro Move

**Date:** 2026-05-21
**Conclusion: GO** — `rmapi put --content-only` preserves on-device handwriting and
user-inserted pages while replacing the PDF background, on the official v4 cloud +
Paper Pro Move.

## Goal

Before building the rmapi deploy backend (Phase 2a), prove on the real device that a
regenerated PDF can replace an annotated document's background **without destroying
the user's handwriting or pages they inserted on-device**. This is the premise the
whole regenerate-and-resync model depends on.

## Environment

- rmapi: nixpkgs 0.0.32 + the v4 `rm-filename` patch (`nix/overlays/rmapi.nix`).
- Cloud: official `my.remarkable.com` (v4 sync schema).
- Device: reMarkable Paper Pro Move.
- **Conf path:** `~/.config/rmapi/rmapi.conf` (confirmed). `RMAPI_XDG_HOME` and
  `XDG_CONFIG_HOME` are both unset, so rmapi and rmbujo's `default_conf_path()` both
  resolve to `$HOME/.config/rmapi/rmapi.conf`. The conf holds `devicetoken:` and
  `usertoken:` keys — the format `is_blank_conf()` checks for.

## Working command sequence

```sh
# 1. Pair once (interactive), then PROVE it works — pairing succeeded even when v4
#    was broken, so a real call is the only proof:
rmapi                      # paste code from https://my.remarkable.com/device/desktop/connect
rmapi -ni ls               # lists cloud root, NO "request failed with status 400"  ✅

# 2. Generate a tiny year and upload one month PDF:
cargo run -- new           # year 2026, base ./dantesttmp, daily_pages 1
cd dantesttmp/2026
rmapi -ni mkdir /rmbujo
rmapi -ni put "2026.05 May.pdf" /rmbujo

# 3. On the Move: annotate page 1 + insert a blank page mid-document, sync.

# 4. Regenerate with a genuinely different background (same page count):
#    edit dantesttmp/2026/rmbujo.toml -> spacing_mm = 4.5 (a value different from
#    what is currently on the device), then:
cargo run -- dantesttmp/2026/rmbujo.toml

# 5. Non-destructive refresh:
rmapi -ni put --content-only "2026.05 May.pdf" /rmbujo
#    On the Move: sync, open the page.
```

## Result

- **(a) Handwriting preserved:** ✅ ink stayed on the pages it was written on.
- **(b) Inserted page preserved:** ✅ the page inserted on-device survived the swap,
  in place.
- **(c) Background replaced:** ✅ the dot grid visibly changed once the pushed PDF
  genuinely differed from the bytes already on the device, confirming the device
  **re-renders** the background after a `--content-only` push (no stale-render cache
  problem).

### Gotcha worth recording

`--content-only` replaces the PDF blob with exactly the bytes you hand it. The first
attempt showed "no visible change" simply because the pushed PDF was byte-identical
to what was already on the device (same `spacing_mm`). It was working; there was
nothing to see. **To verify a refresh visually, ensure the regenerated PDF actually
differs** (e.g. `md5sum` before/after, or change a visible parameter). This is a
test-procedure caveat, not a tool limitation.

## Failure mode (page-count change) — not empirically run

Step 6 (refresh with a *different* page count) was not exhaustively executed. By the
reMarkable storage model, the `.content` redirect table maps each device page to a
**PDF page index**; changing the regenerated PDF's page count/order leaves those
indices pointing at the wrong pages, so backgrounds would mis-map onto annotated
pages. rmbujo avoids this **by design**: regeneration for a given config holds page
count and per-index meaning stable (month = `2 + daily_pages`, fixed order). The
deploy backend therefore never needs to handle a page-count change; if a user edits
`daily_pages` between syncs, that is a structural change and a fresh upload, not a
content-only refresh.

## Implications for the implementation (Tasks 4–6)

- `deploy()` = `mkdir` + `put`; `refresh()` = `put --content-only`. Confirmed correct.
- `-ni` works for non-pairing calls; pairing itself is a one-time interactive step
  against my.remarkable.com.
- `default_conf_path()` → `~/.config/rmapi/rmapi.conf` is correct for this machine.
- Hard invariant for the whole project (incl. future ICS): keep page count and
  per-index meaning stable across regenerations so `--content-only` lands correctly.

## Cleanup

The throwaway `2026.05 May` doc was uploaded to the cloud folder `/rmbujo`. Remove
when finished: `rmapi -ni rm /rmbujo/"2026.05 May"`.
