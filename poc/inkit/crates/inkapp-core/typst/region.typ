// Framework prelude: region(name, body) emits <region>-labelled metadata for the
// laid-out body, then places the body. recover_regions queries the <region> label
// and downcasts the MetadataElem, so the label MUST attach to the metadata element.
// here().position() gives a 1-based page; we store 0-based. Lengths are divided by
// 1pt to unitless floats. measure(body) gives the body's own size.
#let region(name, body) = box[
  #context [
    #metadata((
      name: name,
      page: here().position().page - 1,
      x: here().position().x / 1pt,
      y: here().position().y / 1pt,
      w: measure(body).width / 1pt,
      h: measure(body).height / 1pt,
    )) <region>
  ]
  #body
]
