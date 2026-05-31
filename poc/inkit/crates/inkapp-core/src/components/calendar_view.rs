//! `CalendarView` — the mode axis made into a component. One list of events whose
//! interaction is governed by its `mode` field:
//! - `ReadOnly` renders inert rows and decodes nothing (Display behavior);
//! - `Editable` renders a per-event cancel affordance in its own region and
//!   decodes a mark into one app message per cancelled event (Control behavior).
//!
//! Reusable across apps: it carries the message *factory* `on_cancel` (a
//! non-capturing `fn(&str) -> M`, uid -> Msg) rather than a stored closure,
//! matching `Checkbox`'s value-message pattern. `view`/`update` chooses the mode
//! from the backing connector's capability — the component never sees a connector.

use crate::calendar::EventRow;
use crate::component::Component;
use crate::component::RenderCx;
use crate::components::esc_typst_str;
use crate::ink::RegionInk;
use crate::manifest::Manifest;
use crate::mode::Mode;

/// A calendar/agenda component. `M` is the app message type.
pub struct CalendarView<M> {
    events: Vec<EventRow>,
    mode: Mode,
    /// Only set in `Editable`; maps a cancelled event's uid to an app message.
    on_cancel: Option<fn(&str) -> M>,
}

impl<M> CalendarView<M> {
    /// A read-only agenda: inert rows, decodes nothing (Display behavior).
    pub fn read_only(events: Vec<EventRow>) -> Self {
        Self {
            events,
            mode: Mode::ReadOnly,
            on_cancel: None,
        }
    }

    /// An editable calendar: each event gets a cancel affordance; a mark decodes
    /// to `on_cancel(uid)` (Control behavior).
    ///
    /// Editable regions are named by position (`evt-0`, `evt-1`, …) within this
    /// instance, mirroring `HighlightableText`'s `tok-<i>`. Two *editable*
    /// `CalendarView`s in one document would therefore mint colliding region names;
    /// today nothing does that (the agenda app pairs one read-only with one
    /// editable). A second editable instance per document would need an
    /// instance-level name prefix (as `Checkbox` takes a caller-supplied name).
    pub fn editable(events: Vec<EventRow>, on_cancel: fn(&str) -> M) -> Self {
        Self {
            events,
            mode: Mode::Editable,
            on_cancel: Some(on_cancel),
        }
    }
}

impl<M> Component for CalendarView<M> {
    type Msg = M;

    fn render(&self, _cx: &mut RenderCx) -> String {
        let mut s = String::new();
        for (i, ev) in self.events.iter().enumerate() {
            let label = esc_typst_str(&format!("{} — {}", ev.summary, ev.start));
            match self.mode {
                Mode::ReadOnly => {
                    // Inert row; a cancelled event is struck through. No region:
                    // there is nothing to decode, so nothing to attribute ink to.
                    if ev.cancelled {
                        s.push_str(&format!("#strike[#text[#\"{label}\"]]\n\n"));
                    } else {
                        s.push_str(&format!("#text[#\"{label}\"]\n\n"));
                    }
                }
                Mode::Editable => {
                    // A per-event region `evt-<i>` recovered from layout (the
                    // in-flow `here().position()` pattern shared with Checkbox /
                    // HighlightableText), a cancel-box affordance, and the label.
                    // The affordance is shown for every event, including an
                    // already-`cancelled` one: re-marking just re-emits the same
                    // cancel, which the connector handles idempotently.
                    s.push_str(&format!(
                        "#box[#context [#metadata((name: \"evt-{i}\", \
                           page: here().position().page - 1, x: here().position().x / 1pt, \
                           y: here().position().y / 1pt, w: 14, h: 14)) <region>]\
                         #rect(width: 14pt, height: 14pt, stroke: 0.5pt)] #text[#\"{label}\"]\n\n"
                    ));
                }
            }
        }
        s
    }

    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<M> {
        // ReadOnly discards all ink: branching the same `mode` that render used is
        // what guarantees a no-affordance render can't decode an affordance.
        let (Mode::Editable, Some(on_cancel)) = (self.mode, self.on_cancel) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (i, ev) in self.events.iter().enumerate() {
            let name = format!("evt-{i}");
            let Some(region) = manifest.regions.iter().find(|r| r.name == name) else {
                continue;
            };
            let marked = ink
                .iter()
                .filter(|ri| ri.region == name)
                .flat_map(|ri| &ri.strokes)
                .any(|stroke| stroke.points.iter().any(|p| region.rect.contains(p.x, p.y)));
            if marked {
                out.push(on_cancel(&ev.uid));
            }
        }
        out
    }
}
