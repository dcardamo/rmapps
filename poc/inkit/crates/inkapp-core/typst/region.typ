// Framework prelude. region(name, body) emits <region>-labelled metadata for the
// laid-out body. By default the body is wrapped in a `box` (atomic — never breaks,
// one rect). With `breakable: true` the body flows (it may break across pages) and
// two markers (flow-start / flow-end) bound it; Rust recover_regions reconstructs
// the per-frame rects. The <region> label MUST attach to the metadata element, and
// here().position() gives a 1-based page we store 0-based; lengths are /1pt floats.
// For breakable regions the flow-start and flow-end markers MUST share the same
// `name` — the Rust side pairs them by name to reconstruct per-frame rects.
#let region(name, body, breakable: false) = {
  if not breakable {
    box[
      #context [
        #metadata((
          name: name,
          role: "box",
          page: here().position().page - 1,
          x: here().position().x / 1pt,
          y: here().position().y / 1pt,
          w: measure(body).width / 1pt,
          h: measure(body).height / 1pt,
        )) <region>
      ]
      #body
    ]
  } else {
    // Use layout() to capture the enclosing container's inner width (the actual
    // content-column width), which is what a full-column flowing body occupies.
    // measure(body) in an unconstrained context returns the intrinsic width of
    // the body, not the column width, so it gives wrong-width recovery rects.
    layout(size => {
      context [
        #metadata((
          name: name,
          role: "flow-start",
          page: here().position().page - 1,
          x: here().position().x / 1pt,
          y: here().position().y / 1pt,
          w: size.width / 1pt,
        )) <region>
      ]
      body
      context [
        #metadata((
          name: name,
          role: "flow-end",
          page: here().position().page - 1,
          x: here().position().x / 1pt,
          y: here().position().y / 1pt,
        )) <region>
      ]
    })
  }
}
