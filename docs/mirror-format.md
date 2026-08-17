# The Gwen Markdown Mirror — format v2

`gwen markdown` turns a `.pptx` into a plain-Markdown *mirror*; `gwen build`
applies an edited mirror to the original deck, writing only the parts you
changed (everything else stays byte-for-byte intact, including unmodeled XML
such as shadows, gradients and effects).

This document is the specification. Every generated mirror also starts with a
legend comment summarizing the same grammar.

## Document structure

A mirror is, in order:

1. a YAML **front matter** (`---` delimited) with `pptx.slide_width`,
   `pptx.slide_height`, `theme`, and `core_properties`;
2. the **legend** comment (a single `<!-- ... -->` block; ignored by the
   parser);
3. a `<style>` block defining every class referenced below;
4. `# Master N` sections for each slide master's shapes;
5. `## Slide N` sections — one per slide — and their shapes;
6. optional `### Notes` sections inside a slide.

### Slide headings

A slide is introduced by a level-2 heading. The heading is a **structural
anchor**: its text is ignored, so `## Slide 1`, `## Cover`, and `## Anything`
all mean "the next slide". Add a slide by writing a new `## ` section; delete
one by removing its section. Order matters — slides are matched by index.

`# Master N` headings and `### Notes` headings are likewise structural.

## Shape markers

Each shape is an HTML comment followed by its content:

```html
<!-- shape type="textbox" auto-shape="rect" class="textbox-1"
            name="TextBox 1" left="914400" top="914400"
            width="3657600" height="914400" rotation="-20"
            grid="1828800,1828800" crop-left="0.1" -->
```

Attributes:

| Attribute | Meaning |
| --- | --- |
| `type` | The shape identity: `textbox`, `placeholder`, `picture`, `table`, `group`, `chart`, `autoshape`, `line`, `freeform`, ... `type` is identity — it is *not* styling. |
| `auto-shape` | For `type="autoshape"`: the geometry preset, e.g. `roundRect`, `ellipse`, `chevron`. |
| `class` | The shape's styling class in the `<style>` block: its fill/outline, its frame (`a:bodyPr`) properties, and its default paragraph style, folded into one rule. |
| `name` | The shape name. |
| `left`, `top`, `width`, `height` | Geometry in EMU (`914400` EMU = 1 inch). On input, units are also accepted: `1in`, `3cm`, `25mm`, `72pt`, `96px`. |
| `rotation` | Degrees, clockwise positive. |
| `grid` | Comma-separated table column widths in EMU. |
| `crop-left/top/right/bottom` | Picture crop, fraction 0.0–1.0 of the cropped amount. |

Geometry, identity and grid are read from the marker's attributes; everything
else about the shape's look lives in its `class`.

## The `<style>` block

Classes are deduplicated, content-addressed rules referenced by `class=` in
shape markers, `<!-- paragraph -->` markers and inline `<span class="...">`.
The declaration grammar is a small superset of CSS:

* standard properties where they map 1:1 onto the OOXML model —
  `fill`, `outline-width`, `text-align`, `line-height`, `font-size`,
  `font-family`, `font-weight`, `font-style`, `text-decoration`, `color`,
  `white-space`, `margin-top/right/bottom/left`;
* `--pptx-*` namespaced properties for the rest — `--pptx-vertical-anchor`,
  `--pptx-auto-size`, `--pptx-level`, `--pptx-space-before`,
  `--pptx-space-after`, `--pptx-outline-cap/compound/dash`;
* colors use `TYPE(VALUE)` tokens: `RGB(FF0000)`, `SCHEME(tx1)`,
  `SOLID:FF00FF` for background fills.

### One class per shape

For a shape with a text frame, the shape class folds **fill/outline + frame
properties + default paragraph style** into a single rule. On parse, one class
feeds all three extractors back. Two shapes with the same styling share one
class; geometry and identity never enter the class.

The text frame itself is implicit: it exists exactly when the shape has body
paragraphs. Remove every paragraph of a shape and the frame is deleted with
it; the class's frame properties become inert until a paragraph returns.

## Paragraphs

A paragraph's own style marker is emitted **only when it deviates from the
shape's default**:

```html
<!-- paragraph class="para-3" -->
An explicitly styled paragraph
```

Unmarked paragraphs inherit the shape default (or a plain level-0 paragraph
when there is none). Because the projected snapshot stores only explicit
properties, a paragraph is marked exactly when it carries explicit style — the
common inherited case stays clean.

## Background

```html
<!-- background fill="SOLID:FF00FF" -->
```

placed inside a slide section sets the slide background. `fill="NONE"` (or an
empty value) clears it.

## Inline formatting

Runs render with native Markdown emphasis when possible (`**bold**`,
`*italic*`, `***bold italic***`); anything else uses a style-block class:

```html
Text with <span class="run-1">custom</span> formatting
```

## Editing rules

* **delete a shape** — remove its marker and content;
* **add a shape** — copy an existing marker block and edit it, or write a new
  one; `type="textbox"` needs no class;
* **add a slide** — write a new `## ` section in the right position; it is
  inserted there with a fresh slide part and a slide layout copied from the
  nearest slide;
* **delete a slide** — remove its whole `## ` section; the slide part, its
  relationships, notes slide and content-type override are removed too;
* **reorder a slide** — move its `## ` block; slides are matched by their
  shape signature (shape types and names in order), so a slide whose signature
  changed while staying at the same index is *regenerated in place* — its part
  name, slide layout and notes survive. Slides matched by position after an
  insert/delete are left byte-for-byte intact;
* **edit styling** — edit the referenced class in the `<style>` block;
* **read-only fields** (`shape_id`, placeholder flags, slide layouts, table
  row heights, cell paragraph styles, chart data) are never emitted and are
  preserved from the original deck on build;
* **removing a required field** (for example a theme color) is an error.

### Fidelity notes for slide-level edits

Adding or removing a shape changes a slide's signature, so that slide is
rebuilt from the mirror snapshot (in place). Rebuilt slides keep their part
name, layout and notes, but unmodeled XML on that slide — shadows, gradients,
effects — is lost, and placeholder shapes on it are regenerated as plain
shapes. Two slides with identical shape signatures are indistinguishable to
the matcher; deleting or reordering among them keeps the leading one. These
are the documented costs of slide-level editing; editing a slide's *content*
(geometry, text, styling) without changing its shape list is always lossless.

## Units

All geometry is stored as EMU. Accepted input units:

| Unit | EMU per unit |
| --- | --- |
| `in` | 914400 |
| `cm` | 360000 |
| `mm` | 36000 |
| `pt` | 12700 |
| `px` | 9525 (96 dpi) |
