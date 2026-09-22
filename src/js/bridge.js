// gwen -> pptxgenjs bridge. Rendered inside the embedded QuickJS runtime by
// src/jsbridge.rs. `gwenRender(specJson)` builds a deck with pptxgenjs using
// only its public API (defineLayout/defineSlideMaster/addSection/addSlide/
// addText/addShape/addImage/addNotes/write) and lets `write()` register every
// part on its internal JSZip object. JSZip's async generation is unreliable
// under QuickJS, so instead of using its output we intercept
// `JSZip.prototype.file`/`folder` and hand the captured part list back to the
// host, which packages it into a zip itself.

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

  // Drop the spec-only keys so the object is a plain pptxgenjs options map.
  function optOf(sh) {
    var out = {};
    for (var k in sh) {
      if (sh.hasOwnProperty(k)) {
        var v = sh[k];
        if (v === null || v === undefined) {
          continue;
        }
        switch (k) {
          case "kind":
          case "runs":
          case "type":
            continue;
        }
        out[k] = v;
      }
    }
    return out;
  }

  function renderSlide(p, s, sectionTitle) {
    var opts = {};
    if (s.master) {
      opts.masterName = s.master;
    }
    if (sectionTitle) {
      opts.sectionTitle = sectionTitle;
    }
    var slide = p.addSlide(opts);
    if (s.background) {
      slide.background = s.background;
    }
    if (s.notes) {
      slide.addNotes(s.notes);
    }
    for (var hi = 0; hi < s.shapes.length; hi++) {
      var sh = s.shapes[hi];
      if (sh.kind === "text") {
        // pptxgenjs's text model: addText takes a flat run list; paragraph
        // boundaries are `breakLine` run options and soft breaks are
        // `softBreakBefore` (pptxgenjs renders both, not us).
        var runs = [];
        for (var ri = 0; ri < sh.runs.length; ri++) {
          var r = sh.runs[ri];
          var ro = {};
          for (var rk in r) {
            if (rk === "text") {
              continue;
            }
            if (r[rk] === null || r[rk] === undefined || r[rk] === false) {
              continue;
            }
            ro[rk] = r[rk];
          }
          runs.push({ text: r.text, options: ro });
        }
        slide.addText(runs, optOf(sh));
      } else if (sh.kind === "shape") {
        slide.addShape(sh.type, optOf(sh));
      } else if (sh.kind === "image") {
        slide.addImage(optOf(sh));
      }
    }
  }

  function gwenRender(specJson) {
    var spec = JSON.parse(specJson);
    var p = new PptxGenJS();
    p.defineLayout({ name: "GWEN", width: spec.width, height: spec.height });
    p.layout = "GWEN";
    p.theme = { headFontFace: spec.majorFont, bodyFontFace: spec.minorFont };

    for (var mi = 0; mi < spec.masters.length; mi++) {
      var m = spec.masters[mi];
      var def = { title: m.name };
      if (m.background) {
        def.background = m.background;
      }
      if (m.margin) {
        def.margin = m.margin;
      }
      if (m.slideNumber) {
        def.slideNumber = m.slideNumber;
      }
      def.objects = [];
      for (var oi = 0; oi < m.objects.length; oi++) {
        var o = m.objects[oi];
        var obj = {};
        var combined = { x: o.x, y: o.y, w: o.w, h: o.h };
        for (var k in o.options) {
          if (o.options.hasOwnProperty(k)) {
            combined[k] = o.options[k];
          }
        }
        if (o.data) {
          combined.data = o.data;
        }
        if (o.type === "text" || o.type === "placeholder") {
          // defineSlideMaster text/placeholder objects are `{ text, options }`.
          obj[o.type] = { text: o.text || "", options: combined };
        } else {
          obj[o.type] = combined;
        }
        def.objects.push(obj);
      }
      p.defineSlideMaster(def);
    }

    for (var si = 0; si < spec.sections.length; si++) {
      var sec = spec.sections[si];
      p.addSection({ title: sec.title });
      for (var li = 0; li < sec.slides.length; li++) {
        renderSlide(p, sec.slides[li], sec.title);
      }
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