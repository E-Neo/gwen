//! Embedded QuickJS runtime that renders decks with the real pptxgenjs bundle.
//!
//! The two vendored scripts are evaluated as plain scripts, not modules. The
//! pptxgenjs bundle is a UMD wrapper: when `module`/`exports` are absent it
//! falls through to the `window||global||self||this` branch, so we alias those
//! to `globalThis` first. `jszip.min.js` is evaluated before it so the bundle's
//! free `JSZip` identifier resolves to the global.
//!
//! pptxgenjs registers every part on an internal JSZip object inside `write()`.
//! JSZip's own zip serialization is unusable under QuickJS — it builds the
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
/// promise yields, so the host only needs to drain microtasks — no timers.
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

/// Render a deck from a spec JSON string (see `render::spec`) and return the
/// `.pptx` bytes. The spec is plain data — it is `JSON.parse`d and passed to
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
        let spec = r#"{"width":10,"height":7.5,"slides":[]}"#;
        let bytes = render(spec).unwrap();
        assert_eq!(&bytes[0..2], b"PK", "output must be a zip");
        assert!(bytes.len() > 100, "deck is unexpectedly tiny");
    }
}
