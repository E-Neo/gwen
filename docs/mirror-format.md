# The Gwen Markdown Mirror — format v2

`gwen new <project> --pptx <template.pptx>` turns a `.pptx` into a *project
directory* whose only source of truth is a plain-Markdown mirror. `gwen build
<project>` regenerates the deck from that mirror, writing the output to
`target/<name>.pptx`. The template is consulted exactly once, at `new` time, to
produce the initial mirror; after that the build reads only the Markdown and
the `src/media/` files.

This document is the specification. Every generated mirror also starts with a
legend comment summarizing the same grammar.

## Project layout

```
deck/
  config.toml            [presentation] name (defaults to the template's file
                         stem)
  src/
    PRESENTATION.md      the index: presentation geometry, theme, core
                         properties, and one `<!-- slide -->` marker per slide
                         plus one `<!-- master -->` marker per master
    masters/master1.md   one slide master per file; its `<!-- layout -->`
                         markers reference its layout files
    layouts/layout1.md   one slide layout per file
    slides/slide1.md     one slide per file; `### Notes` holds its notes
    media/               picture/chart image files, keyed by filename
```

The project holds no captured parts. The structural XML (theme `fmtScheme`,
master `clrMap`, presentation default text style, content types, relationship
files) is regenerated from standard Office defaults on build; anything the
mirror does not model is regenerated to a sensible default.

## The index (`PRESENTATION.md`)

A YAML front matter holds the presentation geometry, theme colors/fonts and
core properties:

```yaml
---
pptx:
  slide_width: 9144000
  slide_height: 6858000
core_properties:
  title: "My Deck"
  author: ""
  created: "2013-01-27T09:14:16Z"
theme:
  colors:
    dk1: ""
    accent1: "4F81BD"
  fonts:
    major: "Calibri"
    minor: "Calibri"
---
```

The body lists masters and slides as HTML comment markers. Order matters — the
slides are matched by index.

```html
<!-- master name="Office Theme" src="masters/master1.md" -->
<!-- slide name="" src="slides/slide1.md" -->
<!-- slide name="" src="slides/slide2.md" -->
```

Each slide file starts with its own front matter (`name` and `layout`), followed
by a legend, its shapes and its notes. Each master file starts with its own
front matter, a `<style>` block, its shapes, and one `<!-- layout
src="layouts/layout1.md" -->` marker per layout. Layouts are numbered globally
across all masters, in order — `src="layouts/layoutN.md"` is the layout's file
path; the master/layout indices in the original deck are internal and never
written into the mirror.

## Shape markers

Each shape is an HTML comment followed by its content:

```html
<!-- shape type="textbox" auto-shape="rect" class="textbox-1"
            name="TextBox 1" left="914400" top="914400"
            width="3657600" height="914400" rotation="-20"
            grid="1828800,1828800" crop-left="0.1" -->
```

A picture shape references its media file: the `image=` attribute names a file
in `src/media/`, and the markdown image is written below the marker.

```html
<!-- shape type="picture" class="pic-1" name="Picture 1" image="image1.png"
            left="914400" top="914400" width="3657600" height="2743200" -->
![Picture 1](media/image1.png)
```

Attributes:

| Attribute | Meaning |
| --- | --- |
| `type` | The shape identity: `textbox`, `placeholder`, `picture`, `table`, `group`, `chart`, `autoshape`, `line`, `freeform`, ... `type` is identity — it is *not* styling. |
| `auto-shape` | For `type="autoshape"`: the geometry preset, e.g. `roundRect`, `ellipse`, `chevron`. |
| `class` | The shape's styling class in the `<style>` block: its fill/outline, its effects, its frame (`a:bodyPr`) properties, and its default paragraph style, folded into one rule. |
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
  `white-space`, `margin-top/right/bottom/left`, `box-shadow`;
* `--pptx-*` namespaced properties for the rest — `--pptx-vertical-anchor`,
  `--pptx-auto-size`, `--pptx-level`, `--pptx-space-before`,
  `--pptx-space-after`, `--pptx-outline-cap/compound/dash`, `--pptx-glow`,
  `--pptx-soft-edge`, `--pptx-reflection`;
* colors use `TYPE(VALUE)` tokens: `RGB(FF0000)`, `SCHEME(tx1)`;
* fills use CSS gradient functions — `linear-gradient(135deg, RGB(FF0000)
  0%, RGB(0000FF) 100%)` for linear gradients (degrees, clockwise) and
  `radial-gradient(RGB(FFFF00) 0%, RGB(00FF00) 100%)` for radial.

### Fills and effects

A shape's fill can be solid, gradient or none:

```css
.rect-1 {
  fill: RGB(C0504D);
}
.rect-2 {
  fill: linear-gradient(135deg, RGB(FF0000) 0%, RGB(0000FF) 100%);
}
.rect-3 {
  fill: radial-gradient(RGB(FFFF00) 0%, RGB(00FF00) 100%);
}
.rect-4 {
  fill: none;
}
```

Effects use `box-shadow` and the `--pptx-*` properties:

```css
.shadowed {
  box-shadow: 63500 38100 90deg RGB(000000) 40%;
}
.inset-shadowed {
  box-shadow: inset 63500 38100 45deg RGB(000000) 60%;
}
.glowing {
  --pptx-glow: 127000 RGB(FFFF00) 50%;
}
.soft-edged {
  --pptx-soft-edge: 50800;
}
.reflected {
  --pptx-reflection: 63500 0 100 100 0;
}
```

`box-shadow` is `[inset ]<blurRad> <dist> <dir>deg <COLOR> <alpha>%` — EMU
distances, direction in degrees, alpha as a percentage. The reflection tokens
are `blurRad startPos endPos startAlpha endAlpha` (positions and alphas as
percentages). When an effect is absent from the original shape it is simply not
declared; the default is no shadow/glow/soft edge/reflection.

### One class per shape

For a shape with a text frame, the shape class folds **fill/outline + effects +
frame properties + default paragraph style** into a single rule. On parse, one
class feeds all of the extractors back. Two shapes with the same styling share
one class; geometry and identity never enter the class.

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
* **add a slide** — write a new `<!-- slide src="slides/slideN.md" -->` marker
  in the right position in the index and a new file `src/slides/slideN.md`; a
  fresh slide part is created with the slide layout copied from the nearest
  slide;
* **delete a slide** — remove its marker from the index and its mirror file;
  the slide part, its relationships, notes slide and content-type override are
  removed too;
* **reorder a slide** — move its marker in the index; slides are rebuilt from
  the mirror in index order, so the part name and layout of a slide follow its
  position;
* **edit styling** — edit the referenced class in the `<style>` block;
* **read-only fields** (`shape_id`, placeholder flags, slide layouts, table
  row heights, cell paragraph styles, chart data) are never emitted and are
  regenerated on build;
* **removing a required field** (for example a theme color) is an error.

### Fidelity notes

Gradient fills, shadows, glows, soft edges and reflections are preserved to the
extent the CSS grammar above expresses them. Structural XML that the mirror
never writes (theme `fmtScheme`, master `clrMap`, presentation default text
style, content types, rels) is regenerated from standard Office defaults, so
customizations in those parts do not survive a rebuild.

## Units

All geometry is stored as EMU. Accepted input units:

| Unit | EMU per unit |
| --- | --- |
| `in` | 914400 |
| `cm` | 360000 |
| `mm` | 36000 |
| `pt` | 12700 |
| `px` | 9525 (96 dpi) |