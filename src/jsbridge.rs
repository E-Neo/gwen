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

/// Lock a `sldNum` slide-number shape: the `<p:ph type="sldNum">` placeholder
/// is removed (PowerPoint ignores `<a:spLocks>` on placeholders, so a
/// placeholder number stays movable/editable on slides), `<a:spLocks>` is
/// tightened so the resulting plain shape can't be selected, moved, resized, or
/// have its text edited, and the `<a:fld type="slidenum">` is kept so the number
/// still renders and auto-updates per slide. Applied to pptxgenjs's
/// deterministic output; shapes without a slide-number placeholder are returned
/// unchanged.
fn lock_slide_numbers(xml: &str) -> String {
    let mut out = xml.to_string();
    let mut pos = 0;
    while let Some(ph_rel) = out[pos..].find("<p:ph type=\"sldNum\"") {
        let ph_start = pos + ph_rel;
        let Some(ph_end_rel) = out[ph_start..].find("/>") else {
            break;
        };
        let ph_end = ph_start + ph_end_rel + 2;
        // The lock lives in the same shape's `<p:nvSpPr>` as the placeholder.
        let Some(nv_start) = out[..ph_start].rfind("<p:nvSpPr>") else {
            break;
        };
        let Some(nv_rel) = out[nv_start..].find("</p:nvSpPr>") else {
            break;
        };
        let nv_end = nv_start + nv_rel;
        let mut nvsp = out[nv_start..nv_end].to_string();
        // Remove the placeholder first (indices are relative to the original
        // nvSpPr); the lock upgrade after is a pure string match.
        nvsp.replace_range(ph_start - nv_start..ph_end - nv_start, "");
        nvsp = nvsp.replace(
            "<a:spLocks noGrp=\"1\"/>",
            "<a:spLocks noGrp=\"1\" noSelect=\"1\" noMove=\"1\" noResize=\"1\" noTextEdit=\"1\"/>",
        );
        out.replace_range(nv_start..nv_end, &nvsp);
        // Continue past this shape; its `<a:fld type="slidenum">` is kept.
        let after = nv_start + nvsp.len();
        let Some(close_rel) = out[after..].find("</p:sp>") else {
            break;
        };
        pos = after + close_rel + "</p:sp>".len();
    }
    out
}

/// Drop the slide-number shape from the shared slide master part.
///
/// When any master defines a `slide_number`, pptxgenjs copies its props onto
/// the single shared slide master (`masterSlide._slideNumberProps`), so the
/// master XML carries a slide-number shape that PowerPoint renders on EVERY
/// slide, regardless of which layout a slide uses - and, with several masters,
/// with the first defining master's style. The number belongs to the layout
/// and slides of each master that defines it, so this leaked master-level shape
/// is removed (`name="Slide Number Placeholder 0"` is pptxgenjs's fixed shape
/// name). A no-op when the master carries no slide number.
fn strip_master_slide_number(xml: &str) -> String {
    let anchor = "name=\"Slide Number Placeholder 0\"";
    let Some(at) = xml.find(anchor) else {
        return xml.to_string();
    };
    let Some(sp) = xml[..at].rfind("<p:sp>") else {
        return xml.to_string();
    };
    let Some(rel) = xml[sp..].find("</p:sp>") else {
        return xml.to_string();
    };
    let end = sp + rel + "</p:sp>".len();
    format!("{}{}", &xml[..sp], &xml[end..])
}

/// The captured slide part names whose slide numbers should be locked,
/// derived from the spec: a master with `slideNumber.locked` locks every slide
/// that names it (`ppt/slides/slideN.xml`, in spec order). Layouts are not
/// locked, so the number stays editable in Slide Master view.
fn lock_targets(spec_json: &str) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    let Ok(spec) = serde_json::from_str::<serde_json::Value>(spec_json) else {
        return out;
    };
    let Some(masters) = spec.get("masters").and_then(|v| v.as_array()) else {
        return out;
    };
    let locked: Vec<bool> = masters
        .iter()
        .map(|m| {
            m.get("slideNumber")
                .and_then(|v| v.get("locked"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        })
        .collect();
    let Some(sections) = spec.get("sections").and_then(|v| v.as_array()) else {
        return out;
    };
    let mut n = 0usize;
    for section in sections {
        let Some(slides) = section.get("slides").and_then(|v| v.as_array()) else {
            continue;
        };
        for slide in slides {
            n += 1;
            let master = slide.get("master").and_then(|v| v.as_str()).unwrap_or("");
            if let Some(i) = masters
                .iter()
                .position(|m| m.get("name").and_then(|v| v.as_str()) == Some(master))
                && locked[i]
            {
                out.insert(format!("ppt/slides/slide{n}.xml"));
            }
        }
    }
    out
}

/// pptxgenjs's own default theme and shared slide master XML, captured from a
/// throwaway empty deck.
pub struct DefaultsXml {
    pub theme: String,
    pub master: String,
}

/// Evaluate the bundle + bridge, run `function` with one JSON argument, pump
/// the microtask queue until pptxgenjs's part count settles, and return the
/// captured parts.
fn capture(function: &str, argument: &str) -> Result<Vec<CapturedFile>> {
    let runtime = Runtime::new().map_err(|e| miette::miette!("quickjs runtime: {e}"))?;
    let context = Context::full(&runtime).map_err(|e| miette::miette!("quickjs context: {e}"))?;

    context.with(|ctx| -> Result<Vec<CapturedFile>> {
        let eval = |source: &str| {
            ctx.eval::<(), _>(source)
                .map_err(|e| miette::miette!("quickjs eval failed: {e}"))
        };
        eval(SHIMS)?;
        eval(JSZIP)?;
        eval(PPTXGENJS)?;
        eval(BRIDGE)?;

        let kick_off: Function = ctx
            .eval(function)
            .map_err(|e| miette::miette!("{function} not defined: {e}"))?;
        kick_off
            .call::<_, ()>((argument,))
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
    })
}

/// Capture pptxgenjs's default theme/master for the given theme fonts.
pub fn defaults(major_font: &str, minor_font: &str) -> Result<DefaultsXml> {
    let arg = serde_json::json!({
        "majorFont": major_font,
        "minorFont": minor_font,
    })
    .to_string();
    let parts = capture("gwenDefaultsSync", &arg)?;
    let mut theme = None;
    let mut master = None;
    for part in parts {
        if part.dir {
            continue;
        }
        let Some(data) = part.data else {
            continue;
        };
        match part.name.as_str() {
            "ppt/theme/theme1.xml" => theme = Some(data),
            "ppt/slideMasters/slideMaster1.xml" => master = Some(data),
            _ => {}
        }
    }
    let missing = |name: &str| miette::miette!("pptxgenjs did not produce `{name}`");
    Ok(DefaultsXml {
        theme: theme.ok_or_else(|| missing("ppt/theme/theme1.xml"))?,
        master: master.ok_or_else(|| missing("ppt/slideMasters/slideMaster1.xml"))?,
    })
}

/// Render a deck from a spec JSON string (see `render::spec`) and return the
/// `.pptx` bytes. The spec is plain data - it is `JSON.parse`d and passed to
/// pptxgenjs objects; nothing from it is ever evaluated as code.
pub fn render(spec_json: &str) -> Result<Vec<u8>> {
    let parts = capture("gwenRenderSync", spec_json)?;

    let mut entries: Vec<Entry> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let lock_parts = lock_targets(spec_json);
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
            Some(data) => {
                let mut xml = data;
                // The shared master would leak one master's slide number onto
                // every slide; drop that shape (a no-op without one).
                if part.name == "ppt/slideMasters/slideMaster1.xml" {
                    xml = strip_master_slide_number(&xml);
                }
                if lock_parts.contains(&part.name) && xml.contains("<p:ph type=\"sldNum\"") {
                    xml = lock_slide_numbers(&xml);
                }
                xml.into_bytes()
            }
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
            "placeholder removed so spLocks are honored: {out}"
        );
        assert!(
            out.contains("<a:spLocks noGrp=\"1\" noSelect=\"1\" noMove=\"1\" noResize=\"1\" noTextEdit=\"1\"/>"),
            "locks tightened: {out}"
        );
        assert!(out.contains("type=\"slidenum\""), "field kept");
        assert!(out.contains("<p:txBody>"), "shape body kept");
        // A shape with the same lock but no slide number is not touched.
        let plain = "<p:sp><p:nvSpPr><p:cNvPr id=\"2\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr></p:nvPr></p:nvSpPr></p:sp>";
        assert_eq!(lock_slide_numbers(plain), plain);
    }

    #[test]
    fn strip_master_slide_number_removes_only_the_number_shape() {
        let xml = r#"<p:spTree><p:nvGrpSpPr>..</p:nvGrpSpPr><p:grpSpPr>..</p:grpSpPr><p:sp> <p:nvSpPr>  <p:cNvPr id="25" name="Slide Number Placeholder 0"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr>  <p:nvPr><p:ph type="sldNum" sz="quarter" idx="4294967295"/></p:nvPr> </p:nvSpPr> <p:spPr>..</p:spPr><p:txBody><a:p><a:fld type="slidenum"><a:t>1</a:t></a:fld></a:p></p:txBody></p:sp><p:sp>other</p:sp></p:spTree>"#;
        let out = strip_master_slide_number(xml);
        assert!(
            !out.contains("Slide Number Placeholder 0"),
            "leaked number shape removed: {out}"
        );
        assert!(out.contains("<p:sp>other</p:sp>"), "other shapes kept");
        // No number on the master -> unchanged.
        let plain = "<p:sp><p:nvSpPr><p:cNvPr id=\"2\"/></p:nvSpPr></p:sp>";
        assert_eq!(strip_master_slide_number(plain), plain);
    }

    #[test]
    fn lock_targets_picks_locked_masters_and_their_slides() {
        let spec = r#"{
            "masters": [
                {"name": "amaster", "slideNumber": {"x": 0, "y": 0, "w": 1, "h": 1, "locked": true}},
                {"name": "bmaster", "slideNumber": {"x": 1, "y": 1, "w": 1, "h": 1, "locked": false}},
                {"name": "cmaster", "slideNumber": {"x": 2, "y": 2, "w": 1, "h": 1, "locked": true}}
            ],
            "sections": [
                {"title": "a", "slides": [{"master": "amaster"}, {"master": "bmaster"}]},
                {"title": "c", "slides": [{"master": "cmaster"}]}
            ]
        }"#;
        let targets = lock_targets(spec);
        // Only slides of locked masters; layouts stay editable in master view.
        assert!(!targets.contains("ppt/slideLayouts/slideLayout2.xml"));
        assert!(!targets.contains("ppt/slideLayouts/slideLayout3.xml"));
        assert!(!targets.contains("ppt/slideLayouts/slideLayout4.xml"));
        assert!(targets.contains("ppt/slides/slide1.xml"));
        assert!(!targets.contains("ppt/slides/slide2.xml"));
        assert!(targets.contains("ppt/slides/slide3.xml"));
    }
}
