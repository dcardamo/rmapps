//! `Article` — a Capture-mode component for real (HTML) articles. It renders
//! structured, sanitized Typst with per-token highlight regions and decodes
//! highlighter ink into coalesced span strings, each mapped to an app message.

use inkapp_core::component::{Component, RenderCx};
use inkapp_core::components::highlighted_token_indices;
use inkapp_core::ink::RegionInk;
use inkapp_core::manifest::Manifest;

use crate::convert::{convert, Converted};

/// A highlightable article. `M` is the app message; `on_highlight` builds one
/// message per coalesced highlighted span (the appdx-sanctioned escape hatch for
/// a reusable content component whose message depends on what was decoded).
pub struct Article<M> {
    converted: Converted,
    on_highlight: Box<dyn Fn(&str) -> M>,
}

impl<M> Article<M> {
    /// Convert `html` once (rendering tokens in `highlights` pre-marked).
    pub fn new(
        html: &str,
        highlights: &[String],
        on_highlight: impl Fn(&str) -> M + 'static,
    ) -> Self {
        Self {
            converted: convert(html, highlights),
            on_highlight: Box::new(on_highlight),
        }
    }

    /// `(key, url)` for every referenced image — the only seam with the image
    /// worktree (which fetches and serves `/assets/{key}.png`).
    pub fn images(&self) -> &[(String, String)] {
        &self.converted.images
    }

    /// Highlighted spans, in document order, with index-adjacent tokens that
    /// share a block coalesced into one space-joined string.
    pub fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<String> {
        let tokens = &self.converted.tokens;
        let hits = highlighted_token_indices(tokens.len(), ink, manifest);

        let mut spans: Vec<String> = Vec::new();
        let mut run: Vec<usize> = Vec::new();
        let flush = |run: &mut Vec<usize>, spans: &mut Vec<String>| {
            if !run.is_empty() {
                let s = run
                    .iter()
                    .map(|&i| tokens[i].text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                spans.push(s);
                run.clear();
            }
        };
        for &i in &hits {
            let contiguous = run
                .last()
                .is_some_and(|&p| i == p + 1 && tokens[i].block == tokens[p].block);
            if !contiguous {
                flush(&mut run, &mut spans);
            }
            run.push(i);
        }
        flush(&mut run, &mut spans);
        spans
    }
}

impl<M> Component for Article<M> {
    type Msg = M;

    fn render(&self, _cx: &mut RenderCx) -> String {
        self.converted.typst.clone()
    }

    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<M> {
        self.read(ink, manifest)
            .iter()
            .map(|s| (self.on_highlight)(s))
            .collect()
    }

    /// The image URLs this article references, so the framework's asset pipeline
    /// fetches and serves them at `/assets/{asset_key(url)}.png` (the same key the
    /// rendered `#image` paths use). This is the discovery half of the image seam.
    fn image_urls(&self) -> Vec<String> {
        self.converted
            .images
            .iter()
            .map(|(_key, url)| url.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkapp_core::geometry::{PdfPoint, PdfRect};
    use inkapp_core::ink::Stroke;
    use inkapp_core::manifest::{Manifest, Region};

    // Build a manifest with one rect per token index, and highlighter ink over
    // the chosen indices, so `read` can be exercised without compiling Typst.
    fn manifest_for(n: usize) -> Manifest {
        Manifest {
            regions: (0..n)
                .map(|i| Region {
                    name: format!("tok-{i}"),
                    page: 0,
                    rect: PdfRect {
                        x0: i as f64 * 10.0,
                        y0: 0.0,
                        x1: i as f64 * 10.0 + 8.0,
                        y1: 10.0,
                    },
                })
                .collect(),
            ..Default::default()
        }
    }

    fn swipe(indices: &[usize], m: &Manifest) -> Vec<RegionInk> {
        indices
            .iter()
            .map(|&i| {
                let r = m
                    .regions
                    .iter()
                    .find(|r| r.name == format!("tok-{i}"))
                    .unwrap()
                    .rect;
                RegionInk {
                    region: format!("tok-{i}"),
                    strokes: vec![Stroke {
                        points: vec![
                            PdfPoint {
                                x: r.x0 + 1.0,
                                y: 5.0,
                            },
                            PdfPoint {
                                x: r.x1 - 1.0,
                                y: 5.0,
                            },
                        ],
                        highlighter: true,
                    }],
                }
            })
            .collect()
    }

    #[test]
    fn contiguous_tokens_coalesce_across_inline_styling() {
        let a = Article::new("<p>very <strong>important</strong> note</p>", &[], |s| {
            s.to_string()
        });
        let m = manifest_for(a.converted.tokens.len());
        let got = a.read(&swipe(&[1, 2], &m), &m);
        assert_eq!(got, vec!["important note".to_string()]);
    }

    #[test]
    fn gap_splits_into_separate_spans() {
        let a = Article::new("<p>a b c</p>", &[], |s| s.to_string());
        let m = manifest_for(a.converted.tokens.len());
        let got = a.read(&swipe(&[0, 2], &m), &m);
        assert_eq!(got, vec!["a".to_string(), "c".to_string()]);
    }

    #[test]
    fn block_boundary_prevents_merge() {
        let a = Article::new("<p>a</p><p>b</p>", &[], |s| s.to_string());
        let m = manifest_for(a.converted.tokens.len());
        let got = a.read(&swipe(&[0, 1], &m), &m);
        assert_eq!(got, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn decode_maps_spans_through_on_highlight() {
        #[derive(Debug, PartialEq)]
        struct Hi(String);
        let a = Article::new("<p>a b</p>", &[], |s| Hi(s.to_string()));
        let m = manifest_for(a.converted.tokens.len());
        assert_eq!(
            a.decode(&swipe(&[0, 1], &m), &m),
            vec![Hi("a b".to_string())]
        );
    }

    #[test]
    fn images_seam_is_exposed() {
        let url = "https://example.com/x.png";
        let a: Article<String> = Article::new(&format!("<p><img src=\"{url}\"></p>"), &[], |s| {
            s.to_string()
        });
        assert_eq!(a.images(), &[(crate::image_key(url), url.to_string())]);
    }

    #[test]
    fn image_urls_drives_the_fetch_pipeline() {
        // The Component::image_urls hook is how the framework discovers which
        // images to fetch; it must return every referenced URL (deduped).
        use inkapp_core::component::Component;
        let a: Article<String> = Article::new(
            "<p><img src=\"https://example.com/a.png\"><img src=\"https://example.com/b.png\"></p>",
            &[],
            |s| s.to_string(),
        );
        assert_eq!(
            a.image_urls(),
            vec![
                "https://example.com/a.png".to_string(),
                "https://example.com/b.png".to_string()
            ]
        );
    }

    #[test]
    fn rendered_typst_recovers_one_region_per_token() {
        use inkapp_core::component::RenderCx;
        use inkapp_core::manifest::recover_regions;
        use inkapp_core::render::compile_to_document;

        let a: Article<String> = Article::new(
            "<h2>Title</h2><p>the <strong>quick</strong> fox</p><ul><li>one</li></ul>",
            &[],
            |s| s.to_string(),
        );
        let body = a.render(&mut RenderCx::new(0));
        let src = format!("#set page(width: 400pt, height: 600pt, margin: 16pt)\n{body}");
        let doc = compile_to_document(&src).expect("structured article compiles");
        let m = recover_regions(&doc).unwrap();
        let toks = m
            .regions
            .iter()
            .filter(|r| r.name.starts_with("tok-"))
            .count();
        assert_eq!(
            toks,
            a.converted.tokens.len(),
            "every token recovers as a region through headings/lists/bold"
        );
    }
}
