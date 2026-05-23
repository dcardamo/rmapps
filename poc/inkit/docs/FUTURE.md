# Future directions

Out-there ideas that aren't on the near-term path but are worth not forgetting.
None of this is committed; it's here so the design doesn't accidentally foreclose it.

---

## Shared documents (multi-user collaboration)

**The idea.** Multiple users sign into a cloud-hosted inkapp and connect their
devices. A single document is shared and pushed **read-write** to everyone. Any
user annotates on their own device; the framework merges all contributions and
pushes the merged result back to every device. Think "a Google Doc you annotate by
hand, on whatever pen tablet you happen to own."

**Why it's a smaller leap than it sounds.** Almost every decision made for other
reasons already pushes this toward feasibility:

- **The medium is async, which suits CRDTs.** There's no real-time editing — users
  ink for hours, then sync. That's the *offline-divergence* case CRDTs are actually
  best at. The latency that makes this not-Google-Docs is exactly the concurrent-
  offline-edit scenario the merge machinery is for.
- **You cannot merge ink — you must merge meaning.** The same content paginates
  differently per device (reMarkable vs Supernote, different sizes). So two users'
  strokes live in different coordinate frames; there is no pixel/ink-level union
  across heterogeneous devices. The merge is *forced up* to the content-relative
  semantic layer ("region X highlighted," "checkbox Y toggled") — which is exactly
  where the framework already operates.
- **Ink merges trivially; conflict lives in intent.** Two pen marks don't
  conflict, they coexist (a grow-only set). All real conflict is in *interpreted*
  intent — the app's MVU `Model`, not document state. This is the same
  event-sourcing/CRDT machinery described in `appdx.md` ("State over time"), just
  with more than one author.

**How contributions are represented.** The existing loop is "interpret ink →
render the next document." Collaboration is just "interpret *everyone's* ink →
render the next document *per user*." That yields a clean rule:

> Your own contributions stay as editable ink; everyone else's are baked into your
> rendered background.

Another user's highlight becomes part of the *content* you see on your next sync —
printed and attributed (per-user color; the framework knows whose device each ink
stream came from), never injected into your `.rm` pen layer. You never forge
strokes into someone else's ink layer (that would misattribute on the next
readback). It reuses the renderer wholesale.

**CRDT, but not the distributed kind.** There's a *mandatory central coordinator* —
the render server is in every round-trip; this is not P2P. So CRDT's headline
feature (merge with no central authority) is mostly wasted. What's wanted is
**deterministic server-side merge with CRDT-flavored types** — OR-set for
highlights, LWW-register for a field, a counter for a tally — resolved by the
framework. The data-type discipline, without the distributed machinery.

**The hard parts** (where the design effort would actually go):

- **Conflict UX on a write-only async surface.** Two users set the same field; you
  can't pop a dialog on e-ink. Resolution becomes *another loop iteration*: the
  next render shows "A wrote X, B wrote Y — circle one." On-brand, but a conflict
  costs a round-trip (hours), so bias merge types toward additive/no-conflict
  wherever possible.
- **N-way version skew.** The version-marker staleness check goes from 2-party to
  N-party — each user may be on a different document version at sync time.
- **Latency suits some use cases and ruins others.** Shared reading group, family
  chore board, a couple's agenda/grocery list — great. Anything time-pressured —
  bad. This is "shared slow surfaces," not co-editing.

**Status:** future direction, not near-term. Captured because the current
architecture (content-relative regions, per-device render, MVU with messages-as-
events, encrypt-everything, per-user app state, a coordinator in every round-trip)
already leans this way and shouldn't be designed in a way that blocks it.
