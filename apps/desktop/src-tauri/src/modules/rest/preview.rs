//! The response Preview's document, served from a scheme of its own.
//!
//! # Why this is not `srcdoc`
//!
//! The obvious way to show an HTML response is an `<iframe srcdoc>`, and that is what this was.
//! It cannot work in a packaged build. An `about:srcdoc` document has no response of its own, so
//! it inherits the embedder's policy container — the app's CSP, `script-src 'self'` with no
//! `'unsafe-inline'` — and the page's own inline scripts and remote images are blocked. `devCsp`
//! allows inline script, which is the only reason `dev:app` ever showed the feature working.
//!
//! Nothing about the document can lift an inherited policy: CSP policies **intersect**, so a
//! `<meta http-equiv>` inside the frame only ever tightens. `data:` and `blob:` frames are local
//! schemes and inherit the same way. A document served from a real URL is the one case that
//! carries its own policy container — hence a URI scheme of this module's own, one document per
//! open preview, each served with the policy its two switches earned.
//!
//! The frame is still sandboxed without `allow-same-origin`, so this scheme buys the page nothing
//! but its own CSP: the document stays in an opaque origin and cannot reach the app or its IPC.

use tauri::{Manager, UriSchemeContext};
use uuid::Uuid;

use super::state::{Preview, RestState};

/// The scheme the preview frame loads from. Three places have to agree on it: `frame-src` in
/// `src-tauri/tauri.conf.json`, `PREVIEW_SCHEME` in `src/modules/rest/api.ts`, and here.
pub const SCHEME: &str = "mixlab-preview";

/// How many documents are kept at once. One open pane needs one; the cap is what stops a body
/// from outliving the pane that asked for it when `rest_preview_close` never arrives — a window
/// closed from the OS, a render that threw. Each is at most `MAX_BODY`.
const MAX_KEPT: usize = 8;

/// The policy a preview document is served with, which is what the two switches actually decide.
///
/// `'unsafe-inline'` for style unconditionally: a style attribute is in half the HTML there is,
/// and it reaches nothing. Scripts are the switch that says so, and `'unsafe-eval'` rides with it
/// — a page whose script is allowed to run is not made safer by it failing inside a library.
/// Neither list ever includes the app's own origin: `*` does not match `tauri:`/`ipc:`, and the
/// frame has no `allow-same-origin` to use it with anyway.
pub fn csp(external: bool, scripts: bool) -> String {
    // With external resources off, `data:` is the whole of the network: no image, no stylesheet,
    // no `fetch` back to the server that sent the page. `connect-src` falls back to this too.
    let sources = if external { "* data: blob:" } else { "data:" };
    let script = if scripts {
        format!("{sources} 'unsafe-inline' 'unsafe-eval'")
    } else {
        "'none'".to_string()
    };
    format!("default-src {sources}; style-src {sources} 'unsafe-inline'; script-src {script}")
}

/// Keeps a document until the pane that asked for it closes it, and hands back the id it is
/// served under.
pub fn open(state: &RestState, html: String, external: bool, scripts: bool) -> String {
    let id = Uuid::new_v4().to_string();
    let mut kept = state.previews.lock().unwrap();
    while kept.len() >= MAX_KEPT {
        kept.remove(0);
    }
    kept.push(Preview {
        id: id.clone(),
        body: html.into_bytes(),
        csp: csp(external, scripts),
    });
    id
}

/// Forgets one. Unknown ids are not an error — the cap above may have dropped it already.
pub fn close(state: &RestState, id: &str) {
    state.previews.lock().unwrap().retain(|kept| kept.id != id);
}

/// Serves `<scheme>://localhost/<id>`, or `http://<scheme>.localhost/<id>` on Windows.
///
/// The path is the id as `convertFileSrc` wrote it. That is `encodeURIComponent` of a UUID, which
/// changes nothing about a UUID, so it is compared as it arrives rather than decoded.
pub fn respond<R: tauri::Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let id = request.uri().path().trim_start_matches('/');
    let state = ctx.app_handle().state::<RestState>();
    // Cloned out under the lock, so nothing holds it while the body is handed to the webview.
    let found = state
        .previews
        .lock()
        .unwrap()
        .iter()
        .find(|kept| kept.id == id)
        .map(|kept| (kept.body.clone(), kept.csp.clone()));

    match found {
        Some((body, csp)) => tauri::http::Response::builder()
            .status(tauri::http::StatusCode::OK)
            .header(
                tauri::http::header::CONTENT_TYPE,
                "text/html; charset=utf-8",
            )
            .header("Content-Security-Policy", csp)
            .body(body)
            .unwrap(),
        None => tauri::http::Response::builder()
            .status(tauri::http::StatusCode::NOT_FOUND)
            .header(
                tauri::http::header::CONTENT_TYPE,
                "text/plain; charset=utf-8",
            )
            .body(Vec::new())
            .unwrap(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_switches_off_reaches_nothing() {
        let policy = csp(false, false);
        assert_eq!(
            policy,
            "default-src data:; style-src data: 'unsafe-inline'; script-src 'none'"
        );
    }

    #[test]
    fn scripts_alone_run_but_cannot_call_home() {
        let policy = csp(false, true);
        assert!(policy.contains("script-src data: 'unsafe-inline' 'unsafe-eval'"));
        // No `*` anywhere: a script that runs still has no network to exfiltrate over.
        assert!(!policy.contains('*'));
    }

    #[test]
    fn external_alone_loads_resources_but_runs_nothing() {
        let policy = csp(true, false);
        assert!(policy.starts_with("default-src * data: blob:;"));
        assert!(policy.contains("script-src 'none'"));
    }

    #[test]
    fn both_on_is_the_loosest_it_goes() {
        let policy = csp(true, true);
        assert!(policy.contains("script-src * data: blob: 'unsafe-inline' 'unsafe-eval'"));
    }

    #[test]
    fn inline_style_is_allowed_whatever_the_switches_say() {
        for external in [false, true] {
            for scripts in [false, true] {
                assert!(
                    csp(external, scripts).contains("style-src")
                        && csp(external, scripts).contains("'unsafe-inline'"),
                    "{external} {scripts}"
                );
            }
        }
    }

    #[test]
    fn kept_documents_are_capped() {
        let state = RestState::default();
        let ids: Vec<String> = (0..MAX_KEPT + 3)
            .map(|i| open(&state, format!("<p>{i}</p>"), false, false))
            .collect();
        let kept = state.previews.lock().unwrap();
        assert_eq!(kept.len(), MAX_KEPT);
        // The oldest went, the newest stayed.
        assert!(!kept.iter().any(|p| p.id == ids[0]));
        assert!(kept.iter().any(|p| p.id == ids[ids.len() - 1]));
    }

    #[test]
    fn closing_forgets_the_body_and_unknown_ids_are_fine() {
        let state = RestState::default();
        let id = open(&state, "<p>hi</p>".to_string(), false, false);
        close(&state, "never-opened");
        assert_eq!(state.previews.lock().unwrap().len(), 1);
        close(&state, &id);
        assert!(state.previews.lock().unwrap().is_empty());
    }
}
