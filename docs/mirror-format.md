# The Gwen Markdown Mirror — format v2

`gwen new <project> --pptx <template.pptx>` turns a `.pptx` into a *project
directory* holding a plain-Markdown *mirror* plus every part the mirror cannot
express. `gwen build <project>` regenerates the deck from the mirror and the
captured parts, writing the output to `target/<name>.pptx`.

This document is the specification. Every generated mirror also starts with a
legend comment summarizing the same grammar.

## Project layout

```
deck/
  config.toml            [presentation] name (defaults to the template's file
                         stem) plus the front matter values
  PRESENTATION.md        the index: presentation name, slide geometry, theme,
                         core properties, and one `<!-- slide -->` marker per
                         slide, one `<!-- master -->` marker per master
  src/
    masters/master1.md   one slide master per file; `<!-- layout -->` markers
                         reference its layout files
    layouts/layout1.md   one slide layout per file
    slides/slide1.md     one slide per file; `### Notes` holds its notes
    parts/               every part the mirror cannot express, preserved
                         byte-for-byte (plus _rels/, _fragments/,
                         _content-types.toml)
    media/               picture/chart image files, keyed by filename
```

## The index (`PRESENTATION.md`)

A YAML front matter holds the presentation name, slide geometry and the theme
reference:

```yaml
---
name: "My Deck"
slide_width: 9144000
slide_height: 6858000
theme: "ppt/theme/theme1.xml"
---
```

The body lists masters and slides as HTML comment markers. Order matters — the
slides are matched by index.

```html
<!-- master name="Office Theme" uri="ppt/slideMasters/slideMaster1.xml"
              src="masters/master1.md" -->
<!-- slide uri="ppt/slides/slide1.xml" src="slides/slide1.md" -->
<!-- slide uri="ppt/slides/slide2.xml" src="slides/slide2.md" -->
```

Each slide file starts with its own front matter (`uri`, `name`, `master`,
`layout`) followed by a legend and its shapes. Each master file starts with its
own front matter and a `<style>` block, its shapes, and one `<!-- layout
src="layouts/layout1.md" -->` marker per layout.

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
| `id` | The shape's unique id; leave it out on new shapes to auto-assign. |
| `placeholder`, `ph-type`, `ph-idx`, `ph-sz` | Placeholder identity. |
| `image` | The media filename for `type="picture"` (relative to `src/media`). |
| `row-heights`, `merge` | Table row heights and merged cells. |
| `chart-type` | The chart geometry for `type="chart"` (e.g. `c:barChart`). |
| `ch-off-x/y`, `ch-ext-cx/cy` | A group's child coordinate system. |

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

Links are native Markdown:

```html
[See the docs](https://example.com)
[Next slide](slide://2)
```

## Editing rules

* **delete a shape** — remove its marker and content;
* **add a shape** — copy an existing marker block and edit it, or write a new
  one; `type="textbox"` needs no class;
* **add a slide** — write a new `<!-- slide uri=... src=... -->` marker in the
  right position in the index and a new file `src/slides/slideN.md`; a fresh
  slide part is created with the slide layout copied from the nearest slide;
* **delete a slide** — remove its marker from the index and its mirror file;
  the slide part, its relationships, notes slide and content-type override are
  removed too;
* **reorder a slide** — move its marker in the index; slides are matched by
  their shape signature (shape types and names in order), so a slide whose
  signature changed while staying at the same index is *regenerated in place* —
  its part name, slide layout and notes survive. Slides matched by position
  after an insert/delete are left byte-for-byte intact;
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