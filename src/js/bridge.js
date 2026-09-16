// gwen -> pptxgenjs bridge. Rendered inside the embedded QuickJS runtime by
// src/jsbridge.rs. `gwenRender(specJson)` builds a deck with pptxgenjs and
// lets `write()` register every part on its internal JSZip object. JSZip's
// async generation is unreliable under QuickJS, so instead of using its output
// we intercept `JSZip.prototype.file`/`folder` and hand the captured part list
// back to the host, which packages it into a zip itself.

(function () {
  "use strict";

  var __gwenFiles = [];

  // Intercept every part as pptxgenjs registers it, keeping the `base64`
  // marker JSZip uses for media payloads.
  (function () {
    if (typeof JSZip === "undefined") {
      return;
    }
    var origFile = JSZip.prototype.file;
    var origFolder = JSZip.prototype.folder;
    JSZip.prototype.file = function (name, data, opts) {
      if (typeof name === "string" && data !== undefined && data !== null) {
        __gwenFiles.push({
          name: name,
          data: data,
          dir: false,
          base64: !!(opts && opts.base64),
        });
      }
      return origFile.apply(this, arguments);
    };
    JSZip.prototype.folder = function (name) {
      if (typeof name === "string") {
        __gwenFiles.push({ name: name, data: null, dir: true });
      }
      return origFolder.apply(this, arguments);
    };
  })();

  function gwenRender(specJson) {
    var spec = JSON.parse(specJson);
    var p = new PptxGenJS();
    p.defineLayout({ name: "GWEN", width: spec.width, height: spec.height });
    p.layout = "GWEN";

    for (var si = 0; si < spec.slides.length; si++) {
      var s = spec.slides[si];
      var slide = p.addSlide();
      if (s.background) {
        slide.background = { color: s.background };
      }
      for (var hi = 0; hi < s.shapes.length; hi++) {
        var sh = s.shapes[hi];
        if (sh.kind === "shape") {
          var so = { x: sh.x, y: sh.y, w: sh.w, h: sh.h };
          if (sh.fill) so.fill = { color: sh.fill };
          if (sh.line) so.line = { color: sh.line.color, width: sh.line.width };
          slide.addShape(sh.preset, so);
        } else if (sh.kind === "text") {
          var paras = [];
          for (var pi = 0; pi < sh.paragraphs.length; pi++) {
            var para = sh.paragraphs[pi];
            var po = { breakLine: false };
            if (para.bullet) po.bullet = { code: "2022" };
            if (para.level > 0) po.indentLevel = para.level;
            paras.push({ text: para.text, options: po });
          }
          var to = { x: sh.x, y: sh.y, w: sh.w, h: sh.h, isTextBox: true };
          if (sh.fontSize) to.fontSize = sh.fontSize;
          if (sh.color) to.color = sh.color;
          if (sh.bold) to.bold = sh.bold;
          if (sh.italic) to.italic = sh.italic;
          if (sh.fontFace) to.fontFace = sh.fontFace;
          if (sh.align) to.align = sh.align;
          if (sh.anchor) to.valign = sh.anchor;
          to.wrap = sh.wrap !== false;
          if (sh.fill) to.fill = { color: sh.fill };
          slide.addText(paras, to);
        } else if (sh.kind === "picture") {
          slide.addImage({
            x: sh.x, y: sh.y, w: sh.w, h: sh.h,
            data: sh.dataUri, sizing: { type: "contain" },
          });
        }
      }
      if (s.notes) { slide.addNotes(s.notes); }
    }

    // Let write() register every part on its JSZip instance (which we
    // intercept); its async tail is not used.
    p.write({ outputType: "base64", compression: "DEFLATE" });
  }

  globalThis.__gwenFileCount = function () { return __gwenFiles.length; };
  globalThis.__gwenFilesJson = function () { return JSON.stringify(__gwenFiles); };
  globalThis.gwenRenderSync = function (specJson) {
    globalThis.__gwenError = null;
    try { gwenRender(specJson); }
    catch (e) { globalThis.__gwenError = String(e && e.message !== undefined ? e.message : e); }
  };
})();
