#import "/inkapp/region.typ": region

// Checkbox render half. The framework's #region (from the prelude) wraps the
// tappable affordance only — a fixed 14x14 box — so ink hit-testing matches the
// box, not the label. The label is placed beside it, outside the region.
#let checkbox(name, label) = [
  #region(name, box(width: 14pt, height: 14pt, stroke: 0.5pt))#h(4pt)#text[#label]
]
