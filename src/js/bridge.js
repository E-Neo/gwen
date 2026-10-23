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
          case "preview":
            continue;
        }
        out[k] = v;
      }
    }
    return out;
  }

  // SVG previews rasterized by gwen (Rust), keyed by the SVG data URI. The
  // QuickJS runtime has no DOM, so pptxgenjs's `new Image` + canvas preview
  // rasterizer can't run; feed it just enough fake DOM that it "succeeds" and
  // returns the real preview PNG (or a 1x1 placeholder when none is given).
  var __gwenSvgPreviews = {};
  var __gwenLastPreview = null;
  var __gwenPlaceholderPng =
    "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=";

  function __gwenFakeImage() {
    this.width = 1;
    this.height = 1;
    this.onload = null;
    this.onerror = null;
    var self = this;
    Object.defineProperty(this, "src", {
      set: function (v) {
        self._src = v;
        __gwenLastPreview = __gwenSvgPreviews[v] || __gwenPlaceholderPng;
        if (self.onload) {
          self.onload();
        }
      },
    });
  }
  globalThis.Image = __gwenFakeImage;
  // The fake canvas never reports `onreadystatechange`, so JSZip's microtask
  // polyfill (initialized before this runs) keeps its setTimeout branch.
  globalThis.document = {
    createElement: function () {
      return {
        width: 0,
        height: 0,
        getContext: function () {
          return { drawImage: function () {} };
        },
        toDataURL: function () {
          return __gwenLastPreview;
        },
      };
    },
  };

  // pptxgenjs's text model: addText / master text takes a flat run list;
  // paragraph boundaries are `breakLine` run options and soft breaks are
  // `softBreakBefore` (pptxgenjs renders both, not us).
  function buildRuns(runList) {
    var runs = [];
    for (var ri = 0; ri < runList.length; ri++) {
      var r = runList[ri];
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
    return runs;
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
        var runs = buildRuns(sh.runs);
        slide.addText(runs, optOf(sh));
      } else if (sh.kind === "shape") {
        slide.addShape(sh.type, optOf(sh));
      } else if (sh.kind === "image") {
        if (sh.preview && sh.data) {
          __gwenSvgPreviews[sh.data] = sh.preview;
        }
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
        if (o.preview && o.data) {
          __gwenSvgPreviews[o.data] = o.preview;
        }
        if (o.type === "text" || o.type === "placeholder") {
          // Master text/placeholder objects accept the same flat run list as
          // slides (markdown + `<span>` spans); plain string as a fallback.
          var text = o.runs ? buildRuns(o.runs) : (o.text || "");
          obj[o.type] = { text: text, options: combined };
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

  // Capture pptxgenjs's own default theme and shared slide master, so gwen's
  // preview inherits exactly what PowerPoint would (palette, title/body text
  // styles, placeholder defaults) from the vendored bundle instead of a
  // hand-copied table. Fonts match the deck's [theme] so `+mj-lt`/`+mn-lt`
  // resolve the same way.
  globalThis.gwenDefaultsSync = function (fontsJson) {
    globalThis.__gwenError = null;
    try {
      __gwenFiles.length = 0;
      var fonts = JSON.parse(fontsJson || "{}");
      var p = new PptxGenJS();
      p.theme = {
        headFontFace: fonts.majorFont || "Calibri",
        bodyFontFace: fonts.minorFont || "Calibri",
      };
      p.write({ outputType: "base64", compression: "DEFLATE" });
    } catch (e) {
      globalThis.__gwenError = String(e && e.message !== undefined ? e.message : e);
    }
  };
})();