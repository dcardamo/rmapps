#set page(width: 200pt, height: 300pt, margin: 0pt)

// region(name, w, h): emit metadata with actual top-left position (via here().position())
// then draw a stroked box of the given size.
// We pass explicit w/h rather than using measure() inside context because
// measure() returns the intrinsic size of the inner box, which needs a context
// anyway. Passing w/h directly is simpler and unambiguous.
#let region(name, w, h) = context {
  let pos = here().position()
  [#metadata((
    name: name,
    page: pos.page,
    x: pos.x.pt(),
    y: pos.y.pt(),
    w: w.pt(),
    h: h.pt(),
  )) <region>]
  box(stroke: 1pt, width: w, height: h)
}

#place(top + left, dx: 20pt, dy: 40pt, region("a", 60pt, 24pt))
#place(top + left, dx: 100pt, dy: 200pt, region("b", 50pt, 30pt))
