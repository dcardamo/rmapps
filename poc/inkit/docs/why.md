# Why inkapp

## The problem: annotations are trapped

Pen-based document devices — reMarkable, Supernote, Boox — are genuinely good reading
and writing tools. People use them to read papers, mark up contracts, keep journals, take
notes in meetings. The hardware is purpose-built: e-ink with low latency, a pen that
feels like writing.

But every "app" on these devices is a static PDF. You annotate it, and the annotations
sit there. Nothing reacts. If you circle a task on a to-do page, no system notices. If
you highlight a sentence in an article, the highlight is inert. If you check off a habit
tracker, the tracker doesn't update its streak. The document cannot change in response to
what you wrote.

This is not a hardware limitation. The cloud sync infrastructure already moves documents
back and forth between device and cloud. The `.rm` annotation files are already there.
The bottleneck is that nothing on the server side is reading them and doing anything.

## Why these devices deserve a real framework

People are already living in these devices. reMarkable owners average several hours of
daily use. They maintain handwritten journals, manage reading queues, track habits and
projects. The surface is taken seriously.

Web and mobile got rich app ecosystems because there were frameworks that made the hard
parts easy: routing, state management, UI rendering, event handling. Nobody writes raw
HTTP headers to build a web app; they use Rails, Django, Express. Nobody writes raw OpenGL
to draw a button; they use React or SwiftUI.

Ink devices have no equivalent. Every attempt at a "dynamic" experience on a reMarkable
today is custom, one-off, brittle, and re-solves the same problems: how to push a new
PDF without overwriting the user's ink; how to parse the `.rm` format to find what the
user drew; how to map ink strokes to document regions so you know *what* the user was
responding to. The knowledge exists — it lives in individual repositories — but it has
not been crystallized into a reusable framework.

## The thesis

A document processed server-side and regenerated turns a passive device into an
interactive app surface.

The loop is simple: render a document, sync it to the device, let the user read and write,
sync the annotated document back, read the ink, act on it, render the next document. This
is exactly how CGI worked in web 1.0 — the server renders the response to each user
action. The action is just pen strokes instead of form fields, and the response is a PDF
instead of HTML.

inkapp makes this loop a framework, so that building an app for a pen device is as
straightforward as building a web app: define your handlers, describe your document
regions, let the framework handle sync, ink parsing, and rendering.
