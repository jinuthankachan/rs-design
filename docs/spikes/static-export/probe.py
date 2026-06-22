#!/usr/bin/env python3
"""CP1-Task1 spike: load the Next.js static export (apps/web/out/) in a real
WebKitGTK 4.1 webview — the *same* engine Tauri v2 uses on Linux — and verify
render, hydration, and client-side routing.

Strategy
--------
1. Serve `out/` over loopback HTTP with the same semantics axum will use in CP2:
     - `trailingSlash: true` directories -> their index.html
     - SPA fallback: unknown non-asset paths -> out/index.html (the [[...slug]] route)
     - `/api/*` and `/artifacts/*` -> benign 200 stubs (no daemon in this spike)
   HTTP (not file://) is mandatory because the export references assets at
   absolute `/_next/...` paths that only resolve from an origin root.
2. Inject a document-start user script that captures console.* + window.onerror
   (WebKitGTK 4.1 has no public console signal; interception is the faithful way
   to see React hydration warnings, which go to console.error).
3. Load `/`, wait for hydration to settle, then run JS probes for:
     - render (title, body content, app root populated)
     - hydration (React commit markers, no hydration-mismatch errors)
     - client routing (history.pushState to a sub-route without a full reload)
4. Snapshot the rendered document to PNG.

Exit code 0 = all hard checks passed. Results are printed as a JSON block.
"""
import json
import os
import sys
import threading
import http.server
import socketserver
from functools import partial

import gi
gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import Gtk, WebKit2, GLib  # noqa: E402

OUT_DIR = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "out")
# Optional deep-link to hard-load (tests SPA fallback for a non-"/" entry point).
START_PATH = sys.argv[2] if len(sys.argv) > 2 else "/"
SHOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "render.png")
# Route to client-navigate to during the routing probe (must differ from START_PATH).
ROUTE_TARGET = "/settings" if START_PATH.startswith("/projects") else "/projects"

# ----------------------------------------------------------------------------
# Static server: mimic the CP2 axum serving contract for out/
# ----------------------------------------------------------------------------
class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *a, **k):
        super().__init__(*a, directory=OUT_DIR, **k)

    def log_message(self, *a):
        pass  # quiet

    def _send_json(self, obj, code=200):
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        path = self.path.split("?", 1)[0].split("#", 1)[0]
        # Daemon stubs — no daemon in this spike; return benign empties so data
        # fetches don't mask real render/hydration problems with network noise.
        if path.startswith("/api/") or path.startswith("/artifacts/") or path.startswith("/frames/"):
            return self._send_json([] if path.rstrip("/").endswith("s") else {})
        fs = self.translate_path(self.path)
        # trailingSlash dirs -> index.html
        if os.path.isdir(fs):
            idx = os.path.join(fs, "index.html")
            if os.path.exists(idx):
                return super().do_GET()
        if os.path.exists(fs) and not os.path.isdir(fs):
            return super().do_GET()
        # asset miss -> real 404 (so we notice broken asset refs)
        if path.startswith("/_next/") or "." in os.path.basename(path):
            self.send_error(404)
            return
        # SPA fallback -> index.html (the [[...slug]] catch-all route)
        self.path = "/index.html"
        return super().do_GET()

    def do_POST(self):
        return self._send_json({})


def start_server():
    httpd = socketserver.TCPServer(("127.0.0.1", 0), Handler)
    httpd.allow_reuse_address = True
    port = httpd.server_address[1]
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    return httpd, port


# ----------------------------------------------------------------------------
# Probe sequence in the real webview
# ----------------------------------------------------------------------------
CAPTURE_SCRIPT = r"""
(function(){
  window.__probe = { console: [], errors: [], reloads: (window.__probe && window.__probe.reloads||0) };
  ['log','info','warn','error'].forEach(function(k){
    var o = console[k].bind(console);
    console[k] = function(){
      try { window.__probe.console.push(k+': '+Array.prototype.map.call(arguments,String).join(' ')); } catch(e){}
      return o.apply(console, arguments);
    };
  });
  window.addEventListener('error', function(e){
    window.__probe.errors.push(String(e.message||e.error||e));
  });
  window.addEventListener('unhandledrejection', function(e){
    window.__probe.errors.push('promise: '+String(e.reason));
  });
})();
"""

RESULTS = {}
STEP = {"i": 0}


def js(view, script, cb):
    view.run_javascript(script, None, cb, None)


def js_value(view, result):
    try:
        jsres = view.run_javascript_finish(result)
        return jsres.get_js_value().to_string()
    except Exception as e:  # noqa
        return "JS_ERROR: " + str(e)


def snapshot_then_quit(view, loop, httpd):
    # Preferred path (WebKit get_snapshot) returns a cairo.Surface, which needs
    # the python3-gi-cairo foreign-struct bridge (absent here). Fall back to a
    # GdkPixbuf window grab, which saves PNG without touching pycairo.
    try:
        from gi.repository import Gdk
        win = view.get_toplevel()
        gdkwin = win.get_window()
        alloc = view.get_allocation()
        pb = Gdk.pixbuf_get_from_window(gdkwin, 0, 0, alloc.width, alloc.height)
        if pb is not None:
            pb.savev(SHOT, "png", [], [])
            RESULTS["screenshot"] = SHOT
        else:
            RESULTS["screenshot_error"] = "pixbuf_get_from_window returned None (offscreen/unmapped)"
    except Exception as e:  # noqa
        RESULTS["screenshot_error"] = str(e)
    finish(loop, httpd)


def finish(loop, httpd):
    print("PROBE_RESULTS_JSON_BEGIN")
    print(json.dumps(RESULTS, indent=2))
    print("PROBE_RESULTS_JSON_END")
    httpd.shutdown()
    loop.quit()


def run_probes(view, loop, httpd):
    # Probe 1: render snapshot of the DOM
    render_js = r"""
    JSON.stringify({
      title: document.title,
      url: location.href,
      bodyChildren: document.body ? document.body.children.length : 0,
      bodyTextLen: document.body ? document.body.innerText.trim().length : 0,
      rootHtmlLen: (document.querySelector('#__next, [data-nextjs-router], main, body>div')||{innerHTML:''}).innerHTML.length,
      hasNextData: typeof window.__next_f !== 'undefined' || typeof window.__NEXT_DATA__ !== 'undefined',
      reactRoot: !!document.querySelector('[data-reactroot], #__next, main'),
      stylesheets: document.styleSheets.length,
      scripts: document.scripts.length
    });
    """

    def after_render(v, res, _):
        RESULTS["render"] = json.loads(js_value(v, res))
        # Probe 2: hydration — React replaces server HTML & attaches listeners.
        # Check next router announcer + that an interactive handler exists.
        hyd_js = r"""
        JSON.stringify({
          nextRouterAnnouncer: !!document.querySelector('next-route-announcer, [id*="route-announcer"]'),
          buttons: document.querySelectorAll('button, a[href], [role="button"]').length,
          // React 18/19 marks hydrated containers; presence of __reactContainer key on a DOM node:
          reactFiber: (function(){
            var el = document.querySelector('main, #__next, body>div');
            if(!el) return false;
            return Object.keys(el).some(function(k){return k.indexOf('__react')===0;});
          })(),
          hydrationErrors: window.__probe.console.filter(function(m){
            return /hydrat|did not match|server.*client|Minified React error #4|#418|#423|#425/i.test(m);
          })
        });
        """
        js(v, hyd_js, after_hyd)

    def after_hyd(v, res, _):
        RESULTS["hydration"] = json.loads(js_value(v, res))
        # Probe 3: client-side routing via history API (no full reload).
        # window.__probe survives only if there was NO document reload.
        route_js = r"""
        (function(){
          window.__probe.routeMark = 'set-' + Date.now();
          try {
            history.pushState({}, '', '__ROUTE_TARGET__');
            window.dispatchEvent(new PopStateEvent('popstate'));
          } catch(e){ return JSON.stringify({error:String(e)}); }
          return JSON.stringify({ pushed:true, pathname: location.pathname });
        })();
        """.replace("__ROUTE_TARGET__", ROUTE_TARGET)
        js(v, route_js, after_route_push)

    def after_route_push(v, res, _):
        RESULTS["routing_push"] = json.loads(js_value(v, res))
        # Give the SPA a tick to react, then verify no reload happened and the
        # body still has content at the new path.
        def check(*_a):
            verify_js = r"""
            JSON.stringify({
              markSurvived: (window.__probe && window.__probe.routeMark) ? window.__probe.routeMark.indexOf('set-')===0 : false,
              pathname: location.pathname,
              bodyTextLen: document.body.innerText.trim().length,
              consoleErrors: window.__probe.console.filter(function(m){return m.indexOf('error:')===0;}),
              uncaught: window.__probe.errors
            });
            """
            js(v, verify_js, after_verify)
            return False
        GLib.timeout_add(800, check)

    def after_verify(v, res, _):
        RESULTS["routing_verify"] = json.loads(js_value(v, res))
        # Compute pass/fail verdict
        r = RESULTS["render"]; h = RESULTS["hydration"]; rv = RESULTS["routing_verify"]
        checks = {
            "render_nonempty": r["bodyTextLen"] > 20 and r["bodyChildren"] > 0,
            "assets_loaded": r["stylesheets"] > 0 and r["scripts"] > 0,
            "hydrated": (h["buttons"] > 0 and h["reactFiber"]) ,
            "no_hydration_errors": len(h["hydrationErrors"]) == 0,
            "client_routing": rv["markSurvived"] and rv["pathname"] == ROUTE_TARGET,
            "no_uncaught": len(rv["uncaught"]) == 0,
        }
        RESULTS["checks"] = checks
        RESULTS["pass"] = all(checks.values())
        snapshot_then_quit(v, loop, httpd)

    js(view, render_js, after_render)


def main():
    httpd, port = start_server()
    url = f"http://127.0.0.1:{port}/"
    RESULTS["url"] = url
    RESULTS["out_dir"] = OUT_DIR
    RESULTS["engine"] = WebKit2.get_major_version.__self__ if False else f"WebKitGTK {WebKit2.MAJOR_VERSION}.{WebKit2.MINOR_VERSION}.{WebKit2.MICRO_VERSION}"

    win = Gtk.Window()
    win.set_default_size(1280, 860)
    ucm = WebKit2.UserContentManager()
    ucm.add_script(WebKit2.UserScript.new(
        CAPTURE_SCRIPT,
        WebKit2.UserContentInjectedFrames.TOP_FRAME,
        WebKit2.UserScriptInjectionTime.START,
        None, None,
    ))
    view = WebKit2.WebView.new_with_user_content_manager(ucm)
    settings = view.get_settings()
    settings.set_enable_developer_extras(True)
    settings.set_enable_write_console_messages_to_stdout(True)
    win.add(view)
    win.show_all()

    loop = GLib.MainLoop()

    def on_load(v, event):
        if event == WebKit2.LoadEvent.FINISHED:
            # settle: allow React hydration + first data tick
            GLib.timeout_add(1500, lambda: (run_probes(v, loop, httpd), False)[1])

    view.connect("load-changed", on_load)

    # Definitional evidence for the static-vs-standalone decision: which routes
    # actually exist as HTML on disk. Everything else only works via SPA fallback.
    disk_routes = sorted(
        d for d in os.listdir(OUT_DIR)
        if os.path.isdir(os.path.join(OUT_DIR, d))
        and os.path.exists(os.path.join(OUT_DIR, d, "index.html"))
    )
    RESULTS["html_routes_on_disk"] = ["/"] + ["/" + d for d in disk_routes]
    RESULTS["start_path"] = START_PATH

    def watchdog():
        RESULTS["timeout"] = True
        finish(loop, httpd)
        return False
    GLib.timeout_add(25000, watchdog)

    view.load_uri(url.rstrip("/") + START_PATH)
    loop.run()
    sys.exit(0 if RESULTS.get("pass") else 1)


if __name__ == "__main__":
    main()
