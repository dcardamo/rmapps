# Glossary

**app**
An inkapp application: a handler (or set of handlers) that implement a complete user-facing
experience on a pen device, such as a journal, a reading tracker, or a habit log. Apps are
device-agnostic — the same app runs on reMarkable, Supernote, or any device the framework
supports. App crates never carry a device name.

**handler**
The user-supplied Rust code that implements one step of the loop: it receives the readback
result (structured ink attributed to named regions) and produces the next document or takes
external actions. The handler is the app-specific logic; everything else — sync, ink
parsing, region mapping, rendering — is supplied by the framework. The name comes from the
CGI mental model (see [inspiration.md](inspiration.md)).

**document / bundle**
A document as the reMarkable cloud sees it: not a single file but a bundle of files keyed
by a UUID, including the PDF background, per-page `.rm` annotation files, and a `.content`
index. inkapp treats the whole bundle as the unit of a single interaction cycle. See
[remarkable-pdf-mechanics.md](remarkable-pdf-mechanics.md) §1 for the bundle structure.

**manifest**
A structured record embedded in the PDF that describes the document's layout: the set of
named page regions and their bounding boxes in PDF-point coordinates, plus a version marker.
The manifest travels with the document so the handler can interpret ink without an out-of-band
state store. Secrets are never placed in the manifest.

**region**
A labelled rectangle on a document page, defined by the app developer and recorded in the
manifest. Regions give ink meaning: a stroke that falls within the region named `done` on a
task page means the user checked that task off. The framework maps pen strokes to regions
during readback using the bounding boxes recovered from Typst's laid-out frames.

**readback**
The step in the loop where the framework reads the annotated `.rm` files (via the `rmfiles`
crate), parses the ink strokes, and maps them to manifest regions. The output of readback is
a structured description of what the user did, expressed in terms of region names and ink
content — the input to the handler.

**sync**
The act of transferring the document bundle between the device and the cloud. inkapp's loop
has two sync directions: pushing a newly rendered PDF to the device (a content-only PDF-blob
swap for reMarkable, via the native `rm-cloud` client, which preserves existing ink), and the
device syncing its annotations back to
the cloud for the framework to read. The framework abstracts the device-specific sync transport
behind a trait. See [remarkable-pdf-mechanics.md](remarkable-pdf-mechanics.md) for sync
invariants.

**device**
The physical pen-based document device (reMarkable, Supernote, Boox, etc.) that the user
reads and writes on. Device-specific concerns — the `.rm` annotation format, the reMarkable
cloud transport (`rm-cloud`), page dimensions, toolbar offsets — are abstracted behind device traits in
the framework. Infrastructure crates that are inherently device-specific (like `rmfiles`, the
`.rm` parser) may carry a device name; app crates must not.
