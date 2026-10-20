//! Embedded QuickJS runtime that renders decks with the real pptxgenjs bundle.
//!
//! The two vendored scripts are evaluated as plain scripts, not modules. The
//! pptxgenjs bundle is a UMD wrapper: when `module`/`exports` are absent it
//! falls through to the `window||global||self||this` branch, so we alias those
//! to `globalThis` first. `jszip.min.js` is evaluated before it so the bundle's
//! free `JSZip` identifier resolves to the global.
//!
//! pptxgenjs registers every part on an internal JSZip object inside `write()`.
//! JSZip's own zip serialization is unusable under QuickJS - it builds the
//! archive from JavaScript strings, and QuickJS's UTF-8 string internals break
//! JSZip's byte-offset accounting (the output is a corrupt archive). So the
//! bridge intercepts `file`/`folder`, captures the parts, and the host packages
//! them into a zip itself (`crate::zip`). The parts are still pptxgenjs's own.

use rquickjs::{Context, Function, Runtime};

use crate::error::Result;
use crate::zip::{self, Entry};
use base64::Engine;

const JSZIP: &str = include_str!("js/jszip.min.js");
const PPTXGENJS: &str = include_str!("js/pptxgen.bundle.js");
const BRIDGE: &str = include_str!("js/bridge.js");

/// Host shims: globals the UMD bundle and pptxgenjs expect that QuickJS does
/// not provide by default. `process` is intentionally left undefined so the
/// bundle's `typeof process` feature-detection takes the browser path.
///
/// pptxgenjs registers parts in the synchronous prefix of `write()` and its
/// promise yields, so the host only needs to drain microtasks - no timers.
const SHIMS: &str = r#"
if (typeof globalThis !== "undefined") {
  globalThis.window = globalThis;
  globalThis.global = globalThis;
  globalThis.self = globalThis;
}
if (typeof console === "undefined") {
  globalThis.console = { log: function(){}, warn: function(){}, error: function(){} };
}
globalThis.__gwenError = null;

// Freeze Date so the deck's embedded timestamps (core.xml, app.xml) are
// byte-reproducible across builds.
(function () {
  var NativeDate = Date;
  var frozen = new NativeDate("2020-01-01T00:00:00Z");
  globalThis.Date = class extends NativeDate {
    constructor(...a) {
      if (a.length === 0) {
        super(frozen.getTime());
      } else {
        super(...a);
      }
    }
    static now() {
      return frozen.getTime();
    }
  };
})();

// pptxgenjs gives every section a random UUID. A frozen `Math.random` made
// those reproducible but identical across sections (duplicate `<p14:section
// id>`), which breaks PowerPoint's slide sorter for decks with more than one
// section. Use a deterministic LCG instead: same seed per build (byte-identical
// output) but a distinct value on every call (unique section ids).
(function () {
  var state = 0x9e3779b9;
  Math.random = function () {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 4294967296;
  };
})();
"#;

/// One part captured from pptxgenjs's JSZip object.
#[derive(serde::Deserialize)]
struct CapturedFile {
    name: String,
    /// `null` for directory entries.
    data: Option<String>,
    dir: bool,
    #[serde(default)]
    base64: bool,
}

/// Turn a `sldNum` placeholder into a locked, non-placeholder slide-number
/// field. The `<p:ph>` is dropped so PowerPoint's per-slide "Insert -> Slide
/// Number" toggle can't hide the number, `<a:spLocks>` is tightened so the
/// shape can't be selected, moved, resized, or its text edited, and the
/// `<a:fld type="slidenum">` is preserved so the number still updates per
/// slide. Applied to pptxgenjs's deterministic output; slides without a
/// slide-number placeholder are returned unchanged.
fn lock_slide_numbers(xml: &str) -> String {
    let mut out = xml.to_string();
    while let Some(ph_start) = out.find("<p:ph type=\"sldNum\"") {
        let Some(rel) = out[ph_start..].find("/>") else {
            break;
        };
        let ph_end = ph_start + rel + 2;
        // The lock and placeholder live in the shape's `<p:nvSpPr>`.
        let Some(nv_start) = out[..ph_start].rfind("<p:nvSpPr>") else {
            break;
        };
        let Some(nv_rel) = out[nv_start..].find("</p:nvSpPr>") else {
            break;
        };
        let nv_end = nv_start + nv_rel;
        let mut nvsp = out[nv_start..nv_end].to_string();
        // Remove the placeholder first (indices were computed on the original
        // string); the lock upgrade after is a pure string match.
        nvsp.replace_range(ph_start - nv_start..ph_end - nv_start, "");
        nvsp = nvsp.replace(
            "<a:spLocks noGrp=\"1\"/>",
            "<a:spLocks noGrp=\"1\" noSelect=\"1\" noMove=\"1\" noResize=\"1\" noTextEdit=\"1\"/>",
        );
        out.replace_range(nv_start..nv_end, &nvsp);
    }
    out
}

/// Render a deck from a spec JSON string (see `render::spec`) and return the
/// `.pptx` bytes. The spec is plain data - it is `JSON.parse`d and passed to
/// pptxgenjs objects; nothing from it is ever evaluated as code.
pub fn render(spec_json: &str) -> Result<Vec<u8>> {
    let runtime = Runtime::new().map_err(|e| miette::miette!("quickjs runtime: {e}"))?;
    let context = Context::full(&runtime).map_err(|e| miette::miette!("quickjs context: {e}"))?;

    let parts = context.with(|ctx| -> Result<Vec<CapturedFile>> {
        let eval = |source: &str| {
            ctx.eval::<(), _>(source)
                .map_err(|e| miette::miette!("quickjs eval failed: {e}"))
        };
        eval(SHIMS)?;
        eval(JSZIP)?;
        eval(PPTXGENJS)?;
        eval(BRIDGE)?;

        let kick_off: Function = ctx
            .eval("gwenRenderSync")
            .map_err(|e| miette::miette!("gwenRenderSync not defined: {e}"))?;
        kick_off
            .call::<_, ()>((spec_json,))
            .map_err(|e| miette::miette!("pptxgenjs threw: {e}"))?;

        // Drive the microtask queue: most parts are registered synchronously
        // inside `write()`, but media and some relationship parts land after
        // its promise yields. Stop once the part count is stable.
        let mut last_count = -1i32;
        let mut stable = 0;
        for _ in 0..500 {
            let error: Option<String> = ctx
                .eval("__gwenError")
                .map_err(|e| miette::miette!("reading error failed: {e}"))?;
            if let Some(message) = error {
                return Err(miette::miette!("pptxgenjs failed: {message}"));
            }
            let count: i32 = ctx
                .eval("__gwenFileCount()")
                .map_err(|e| miette::miette!("reading part count failed: {e}"))?;
            while ctx.execute_pending_job() {}
            if count == last_count {
                stable += 1;
                if stable >= 20 {
                    break;
                }
            } else {
                stable = 0;
                last_count = count;
            }
        }
        if last_count <= 0 {
            return Err(miette::miette!(
                "pptxgenjs registered no parts (count={last_count})"
            ));
        }

        let json: String = ctx
            .eval("__gwenFilesJson()")
            .map_err(|e| miette::miette!("reading parts failed: {e}"))?;
        serde_json::from_str(&json).map_err(|e| miette::miette!("parts JSON invalid: {e}"))
    })?;

    let mut entries: Vec<Entry> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let lock_numbers = serde_json::from_str::<serde_json::Value>(spec_json)
        .ok()
        .and_then(|v| v.get("lockSlideNumbers").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    for part in parts {
        if !seen.insert(part.name.clone()) {
            continue;
        }
        if part.dir {
            entries.push(Entry {
                name: part.name,
                data: Vec::new(),
                dir: true,
            });
            continue;
        }
        let data = match part.data {
            Some(data) if part.base64 => base64::engine::general_purpose::STANDARD
                .decode(data.trim())
                .map_err(|e| miette::miette!("part `{}` has invalid base64: {e}", part.name))?,
            Some(data) if lock_numbers && data.contains("<p:ph type=\"sldNum\"") => {
                lock_slide_numbers(&data).into_bytes()
            }
            Some(data) => data.into_bytes(),
            None => Vec::new(),
        };
        entries.push(Entry {
            name: part.name,
            data,
            dir: false,
        });
    }

    Ok(zip::write(&entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pptxgenjs_is_available() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        context
            .with(|ctx| -> rquickjs::Result<()> {
                for source in [SHIMS, JSZIP, PPTXGENJS, BRIDGE] {
                    ctx.eval::<(), _>(source)?;
                }
                let present: String = ctx.eval("typeof PptxGenJS === 'function' ? 'yes' : 'no'")?;
                assert_eq!(present, "yes");
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn empty_deck_renders_a_zip() {
        let spec = r#"{"width":10,"height":7.5,"majorFont":"Calibri","minorFont":"Calibri","masters":[],"sections":[]}"#;
        let bytes = render(spec).unwrap();
        assert_eq!(&bytes[0..2], b"PK", "output must be a zip");
        assert!(bytes.len() > 100, "deck is unexpectedly tiny");
    }

    #[test]
    fn lock_slide_numbers_removes_placeholder_and_locks() {
        let xml = r#"<p:sp> <p:nvSpPr>  <p:cNvPr id="25" name="Slide Number Placeholder 0"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr>  <p:nvPr><p:ph type="sldNum" sz="quarter" idx="4294967295"/></p:nvPr> </p:nvSpPr> <p:spPr><a:xfrm>..</a:xfrm></p:spPr><p:txBody><a:p><a:fld id="{ABC}" type="slidenum"><a:t>1</a:t></a:fld></a:p></p:txBody></p:sp>"#;
        let out = lock_slide_numbers(xml);
        assert!(
            !out.contains("<p:ph type=\"sldNum\""),
            "placeholder removed"
        );
        assert!(
            out.contains("<a:spLocks noGrp=\"1\" noSelect=\"1\" noMove=\"1\" noResize=\"1\" noTextEdit=\"1\"/>"),
            "locks tightened: {out}"
        );
        assert!(out.contains("type=\"slidenum\""), "field kept");
        assert!(out.contains("<p:txBody>"), "shape body kept");
        // Unrelated XML (no slide number) is untouched.
        let plain = "<p:sp><p:nvSpPr><p:cNvPr id=\"2\"/></p:nvSpPr></p:sp>";
        assert_eq!(lock_slide_numbers(plain), plain);
    }
}
