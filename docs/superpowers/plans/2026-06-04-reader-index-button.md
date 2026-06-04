# Reader "Index" Button Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an "Index" button to the Reader article nav bar that jumps to the index page listing the current article (Feed and Library).

**Architecture:** All changes live in `crates/rmreader/src/render/typst_doc.rs`. Each index row gains a queryable `idx-<anchor>` label; `nav-bar()` gains a 4th cell, `Index`, that resolves the current article's row page via `query(label("idx-" + sid))` and links to that absolute page. The article nav bar becomes `< Prev | Index | Home | Next >`. `index-nav()` and `Home` are unchanged.

**Tech Stack:** Rust, Typst (inlined source string), lopdf (test-side PDF inspection).

**User Verification:** NO — no user verification required.

---

### Task 1: Add Index button to the article nav bar

**Goal:** Each index row is labelled `idx-<anchor>`; the article nav bar shows `< Prev | Index | Home | Next >` where Index links to the index page that lists the current article.

**Files:**
- Modify: `crates/rmreader/src/render/typst_doc.rs` (the `nav-bar()` helper in the preamble `format!`, and the row emission in `build_index`)
- Test: `crates/rmreader/tests/render.rs`

**Acceptance Criteria:**
- [ ] Each index row carries a `idx-<anchor>` Typst label.
- [ ] `nav-bar()` renders four cells: `< Prev`, `Index`, `Home`, `Next >`.
- [ ] The `Index` cell links to the absolute page of `label("idx-" + sid)`.
- [ ] A first-article page carries a Link whose GoTo destination resolves to the index page (page 0).
- [ ] All pre-existing `tests/render.rs` assertions still pass (page_range, action rects, mark_all_read, clean text layer, determinism).

**Verify:** `cargo test -p rmreader --test render` → all tests pass.

**Steps:**

- [ ] **Step 1: Write the failing test**

Add to `crates/rmreader/tests/render.rs`. First add a helper that resolves the GoTo destination page index for each Link annotation on a page, then the test.

```rust
/// 0-based destination page indices of GoTo links on a single 0-based page.
/// Typst emits internal links as /A << /S /GoTo /D [<page ref> /XYZ ...] >>.
fn link_dest_pages(pdf: &[u8], page_index: usize) -> Vec<usize> {
    let doc = lopdf::Document::load_mem(pdf).unwrap();
    let pages: Vec<_> = doc.get_pages().into_values().collect();
    // Map each page object id to its 0-based index.
    let page_idx = |target: lopdf::ObjectId| pages.iter().position(|&p| p == target);
    let pid = pages[page_index];
    let mut out = Vec::new();
    if let Ok(annots) = doc
        .get_dictionary(pid)
        .and_then(|p| p.get(b"Annots"))
        .and_then(|a| a.as_array())
    {
        for a in annots {
            let Ok(ad) = a.as_reference().and_then(|id| doc.get_dictionary(id)) else { continue };
            // Destination array can hang off /Dest directly or /A /D.
            let dest = ad
                .get(b"Dest")
                .and_then(|d| d.as_array())
                .or_else(|_| {
                    ad.get(b"A")
                        .and_then(|a| a.as_reference().and_then(|id| doc.get_dictionary(id)))
                        .or_else(|_| ad.get(b"A").and_then(|a| a.as_dict()))
                        .and_then(|act| act.get(b"D").and_then(|d| d.as_array()))
                });
            if let Ok(arr) = dest {
                if let Some(first) = arr.first() {
                    if let Ok(r) = first.as_reference() {
                        if let Some(ix) = page_idx(r) {
                            out.push(ix);
                        }
                    }
                }
            }
        }
    }
    out
}

#[test]
fn article_page_has_index_button_linking_to_index_page() {
    let device = get_device("paper-pro-move").unwrap();
    let theme = load_theme("reader").unwrap();
    let (rows, articles) = sample();
    let r = render_collection(&device, &theme, "Feed", &rows, &articles, &[]).unwrap();
    // Page 0 is the index; page 1 is the first article. Its nav bar's Index cell
    // must link back to the index page (page 0).
    let dests = link_dest_pages(&r.pdf, 1);
    assert!(
        dests.contains(&0),
        "first article page should carry an Index link to the index page (page 0), got dests {dests:?}"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rmreader --test render article_page_has_index_button_linking_to_index_page`
Expected: FAIL — no link on the article page targets page 0 yet (only Prev/Home/Next exist, none of which is a page-target link to the index page; Home targets the `index-home` label/anchor on page 0 — see note below).

Note: `Home` links to the `<index-home>` label which also resolves to page 0. If the test passes already because Home's destination resolves to page 0, tighten the assertion to require **two** distinct links to page 0 on the article page (Index + Home):

```rust
    let to_index = dests.iter().filter(|&&d| d == 0).count();
    assert!(to_index >= 2, "expected Index + Home both targeting page 0, got {to_index} (dests {dests:?})");
```

Use this tightened form as the committed test so it genuinely exercises the new Index cell.

- [ ] **Step 3: Add the `idx-<anchor>` label to each index row**

In `build_index` (`crates/rmreader/src/render/typst_doc.rs`), the row is emitted by a `format!` that produces `#link(label("art-{anchor}"))[ … ]`. Append a label after the link, mirroring the `#label("art-" + id)` pattern used on the article headline (line ~246). Change the trailing `]]\n` of the row format string so the row ends with the link followed by `#label("idx-{anchor}")`.

Current (lines ~320-330):

```rust
        s.push_str(&format!(
            "#link(label(\"art-{anchor}\"))[#block(below: 0pt, inset: (y: 4pt), \
             stroke: (bottom: 0.5pt + rule-col))[#grid(columns: (14pt, 1fr, auto), \
             column-gutter: 8pt, align: (left + top, left + top, right + top),\n\
             text(font: \"Lora\", weight: \"semibold\", size: 9pt, fill: accent, \"{num}\"),\n\
             text(font: \"Lora\", size: 9.5pt, fill: ink, [{title_line}]),\n\
             text(font: \"Hanken Grotesk\", size: 7.5pt, fill: muted, \"{rt}\"))]]\n",
            anchor = esc_str(&r.anchor),
            num = esc_str(&r.num),
            rt = esc_str(&r.reading_time),
        ));
```

Replace the closing `…))]]\n"` with `…))]] #label(\"idx-{anchor}\")\n"` (a space then the label, so it attaches to the preceding `#link` element):

```rust
        s.push_str(&format!(
            "#link(label(\"art-{anchor}\"))[#block(below: 0pt, inset: (y: 4pt), \
             stroke: (bottom: 0.5pt + rule-col))[#grid(columns: (14pt, 1fr, auto), \
             column-gutter: 8pt, align: (left + top, left + top, right + top),\n\
             text(font: \"Lora\", weight: \"semibold\", size: 9pt, fill: accent, \"{num}\"),\n\
             text(font: \"Lora\", size: 9.5pt, fill: ink, [{title_line}]),\n\
             text(font: \"Hanken Grotesk\", size: 7.5pt, fill: muted, \"{rt}\"))]] #label(\"idx-{anchor}\")\n",
            anchor = esc_str(&r.anchor),
            num = esc_str(&r.num),
            rt = esc_str(&r.reading_time),
        ));
```

- [ ] **Step 4: Add the Index cell to `nav-bar()`**

In the preamble `format!` string, replace the `nav-bar()` helper (lines ~144-166) with the version below. It adds `idx-page` resolution and a `page-cell` helper (links to an absolute page, like `index-nav`'s cell), and makes the grid 4 columns with `Index` between `Prev` and `Home`. All literal Typst braces stay doubled (`{{`/`}}`) because this is inside `format!`.

```rust
// Indigo filled nav bar: < Prev | Index | Home | Next >. Inert cells are dimmed.
// Index links to the absolute page of this article's index row (label idx-<sid>).
#let nav-bar() = context {{
  let sid = section-state.at(here())
  let cur = if sid == "" {{ none }} else {{ order.position(s => s == sid) }}
  let prev = if cur == none or cur == 0 {{ none }} else {{ order.at(cur - 1) }}
  let next = if cur == none or cur + 1 >= order.len() {{ none }} else {{ order.at(cur + 1) }}
  let idx-page = if sid == "" {{ none }} else {{
    let m = query(label("idx-" + sid))
    if m.len() == 0 {{ none }} else {{ m.first().location().page() }}
  }}
  let cell(txt, target) = align(center + horizon, if target == none {{
    text(font: "Hanken Grotesk", size: 8pt, weight: "semibold", tracking: 0.04em,
      fill: navfg.transparentize(55%), txt)
  }} else {{
    link(label("art-" + target),
      text(font: "Hanken Grotesk", size: 8pt, weight: "semibold", tracking: 0.04em,
        fill: navfg, txt))
  }})
  let page-cell(txt, target) = align(center + horizon, if target == none {{
    text(font: "Hanken Grotesk", size: 8pt, weight: "semibold", tracking: 0.04em,
      fill: navfg.transparentize(55%), txt)
  }} else {{
    link((page: target, x: 0pt, y: 0pt),
      text(font: "Hanken Grotesk", size: 8pt, weight: "semibold", tracking: 0.04em,
        fill: navfg, txt))
  }})
  let home = align(center + horizon, link(<index-home>,
    text(font: "Hanken Grotesk", size: 8pt, weight: "semibold", tracking: 0.04em,
      fill: navfg, "Home")))
  // align(horizon, …) centres the row vertically in the fixed-height bar so the
  // labels aren't clipped at the bottom edge.
  block(width: 100%, height: 24pt, fill: navbg, inset: (x: 10pt),
    align(horizon, grid(columns: (1fr, 1fr, 1fr, 1fr),
      cell([< Prev], prev), page-cell([Index], idx-page), home, cell([Next >], next))))
}}
```

- [ ] **Step 5: Run the full render test suite**

Run: `cargo test -p rmreader --test render`
Expected: PASS — the new test plus all pre-existing tests (`renders_internal_links_and_pages`, `text_layer_is_clean_no_actualtext_duplication`, `render_is_deterministic`, `feed_index_emits_mark_all_read_region`, `library_index_has_no_mark_all_read_region`, `inline_code_followed_by_dot_field_compiles`, `index_page_has_nav_bar_links`).

If `index_page_has_nav_bar_links` or the link-count assertion in `renders_internal_links_and_pages` shifts, re-read the counts — adding the Index link increases per-article-page link counts but those assertions use `>=`, so they should still hold. Do not weaken any assertion to pass.

- [ ] **Step 6: Commit**

```bash
git add crates/rmreader/src/render/typst_doc.rs crates/rmreader/tests/render.rs
git commit -m "feat(rmreader): add Index button to article nav bar

Jumps to the index page listing the current article (Feed + Library);
each index row gains an idx-<anchor> label the nav bar resolves to a
page link. Bar is now < Prev | Index | Home | Next >."
```

```json:metadata
{"files": ["crates/rmreader/src/render/typst_doc.rs", "crates/rmreader/tests/render.rs"], "verifyCommand": "cargo test -p rmreader --test render", "acceptanceCriteria": ["index rows labelled idx-<anchor>", "nav-bar renders Prev|Index|Home|Next", "Index links to the index row's absolute page", "first-article page has a link to page 0", "all prior render tests still pass"], "requiresUserVerification": false}
```
