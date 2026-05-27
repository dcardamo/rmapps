//! `Stack<M>` — a passthrough container that renders its children in order, with
//! no page break, no state, no region. Useful for composing multiple components
//! into one slot (e.g. a `Document::page_header` that wants both a `NavBand`
//! above an `ActionBand`). Section adds a state update + pagebreak; Stack adds
//! neither. Generic over `M = ()` so it slots cleanly under any app message.

use std::marker::PhantomData;

use crate::component::{Component, RenderCx};
use crate::ink::RegionInk;
use crate::manifest::Manifest;

pub struct Stack<M = ()> {
    body: Vec<Box<dyn Component<Msg = M>>>,
    _msg: PhantomData<fn() -> M>,
}

impl<M> Stack<M> {
    pub fn new(body: Vec<Box<dyn Component<Msg = M>>>) -> Self {
        Self {
            body,
            _msg: PhantomData,
        }
    }
}

impl<M> Component for Stack<M> {
    type Msg = M;

    fn render(&self, cx: &mut RenderCx) -> String {
        let mut out = String::new();
        for c in &self.body {
            out.push_str(&c.render(cx));
        }
        out
    }

    fn typst_sources(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for c in &self.body {
            out.extend(c.typst_sources());
        }
        out
    }

    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<M> {
        let mut out = Vec::new();
        for c in &self.body {
            out.extend(c.decode(ink, manifest));
        }
        out
    }

    fn image_urls(&self) -> Vec<String> {
        let mut out = Vec::new();
        for c in &self.body {
            out.extend(c.image_urls());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::notice::Notice;

    #[test]
    fn render_concatenates_children() {
        let s: Stack<()> = Stack::new(vec![
            Box::new(Notice::line("first")),
            Box::new(Notice::line("second")),
        ]);
        let out = s.render(&mut RenderCx::new(0));
        let p1 = out.find("first").expect("first present");
        let p2 = out.find("second").expect("second present");
        assert!(p1 < p2, "children render in declaration order: {out}");
    }

    #[test]
    fn decode_delegates_to_children() {
        let s: Stack<()> = Stack::new(vec![Box::new(Notice::line("x"))]);
        let manifest = Manifest::default();
        let msgs = s.decode(&[], &manifest);
        assert!(msgs.is_empty());
    }
}
