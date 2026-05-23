# Inspiration

## Web 1.0 and CGI — the core analogy

The mental model for inkapp is web 1.0, specifically the CGI era. In that model, the
server renders every response from scratch. The user submits a form; the server runs a
program (the CGI handler), produces HTML, and sends it back. There is no client-side
state, no JavaScript, no persistent connection. The document — the HTML page — is the
interface. When the user acts on it, the server produces the next document.

inkapp is structurally identical, with a substitution table:

| Web 1.0 / CGI       | inkapp                                   |
|---------------------|------------------------------------------|
| HTML page           | PDF document                             |
| Form fields         | Pen strokes in named regions             |
| HTTP POST           | Sync (device → cloud)                    |
| CGI handler program | inkapp handler (Rust crate)              |
| HTTP response       | Re-rendered PDF (sync to device)         |
| URL                 | Document UUID + manifest version         |
| Session cookie      | Manifest embedded in PDF                 |

The CGI model was criticised for being stateless and server-heavy, but those properties
are advantages here. Pen devices are not running JavaScript. The loop is naturally async
— the user may take hours or days between writing and syncing. The server does not need
an open socket. Generating a new document per interaction is cheap because Typst renders
fast, and the document is only a few hundred kilobytes.

The analogy also suggests what inkapp is *not*: it is not a real-time system. It is not
a continuous ink stream. It does not try to interpret pen strokes as they happen. It
reads completed annotations after the user has finished writing and synced. This matches
how people actually use these devices.

## TUI and web frameworks — the right abstraction level

TUI frameworks (ncurses, ratatui, bubbletea) and web frameworks (Rails, Django, Express,
Axum) solved the same underlying problem: how do you make a hard surface easy to build
for? Terminals and HTTP sockets are not friendly primitives. These frameworks raised the
abstraction level: define routes, describe components, let the framework handle the
protocol.

Pen devices are in the same position terminals were in the 1980s. The protocol (cloud
sync, `.rm` format, bundle structure) is not friendly. Every developer who tries to build
something dynamic on a reMarkable today has to reverse-engineer the sync format, learn
the `.rm` binary structure, solve the ink-to-region mapping problem, and re-implement
annotation-preserving PDF updates — every time, from scratch.

inkapp aims to be the ratatui of ink devices: a framework that abstracts the device
surface so that the developer writes handlers, not sync code.

## Typst — the render foundation

Typst is a document typesetting system, implemented in Rust, designed as a library as
well as a CLI tool. inkapp uses it as a library crate — compiled into the framework, not
shelled out to.

Three properties make Typst the right render engine for inkapp:

**1. Layout introspection.** When Typst compiles a document, it produces a tree of laid-out
frames with precise element positions in final document coordinates. This is what makes
the region manifest possible: inkapp asks Typst where each labelled element ended up on
the page, then records those bounding boxes in the manifest. No other pure-Rust renderer
exposes this level of layout detail.

**2. Pure Rust, no heavy runtime.** Typst has no dependency on Chromium, a browser engine,
or any other heavy runtime. The entire render pipeline — including font shaping and PDF
generation — runs in-process. A handler that renders a new document is just a Rust binary
calling library functions; it does not shell out, spawn a browser, or need a display
server.

**3. Purpose-built typesetting quality.** Typst produces high-quality output: proper
kerning, ligatures, OpenType features, clean vector PDF. Ink devices have high-resolution
e-ink displays and users who care about the quality of what they read. The render engine
needs to match the hardware.

Typst's primary weakness is that it uses its own markup language rather than HTML/CSS.
Pulling existing HTML content (e.g. a web article body) into a Typst document requires
conversion. The framework documents the limits of this conversion path; see the Typst
spike findings in `docs/superpowers/spikes/`.
