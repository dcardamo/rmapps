#import "/inkapp/region.typ": region

// Heading render half. Theme tones (heading/muted) are passed as luma integers
// so the Rust caller controls colour from `Theme`. Named `heading-block` to
// avoid collision with Typst's built-in `heading` keyword.
#let heading-block(title, byline: none, meta: none, subtitle: none, heading-tone: 26, muted-tone: 110) = {
  block(below: 6pt, text(weight: "bold", size: 18pt, fill: luma(heading-tone), title))
  if byline != none {
    block(below: 2pt, text(size: 10pt, weight: "medium", fill: luma(muted-tone), byline))
  }
  if meta != none {
    block(below: 6pt, text(size: 9pt, fill: luma(muted-tone), meta))
  }
  if subtitle != none {
    block(below: 6pt, text(size: 10pt, style: "italic", fill: luma(muted-tone), subtitle))
  }
  v(2pt)
}
