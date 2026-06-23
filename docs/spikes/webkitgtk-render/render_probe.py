#!/usr/bin/env python3
"""CP1-Task5 spike: render a set of *showcase artifacts* in the real WebKitGTK 4.1
engine Tauri v2 uses on Linux, and triage engine-fidelity issues.

For each artifact it:
  1. serves the artifact's own directory over loopback (so relative assets resolve);
  2. injects a document-start capture script (console.* + window error events +
     a capturing resource-error listener for failed <img>/<link>/<script>);
  3. after load + settle, probes:
       - layout sanity (scroll dimensions, body computed background — blank-render guard)
       - CSS.supports() for the modern features the corpus uses
       - WebGL context availability (webgl2 / webgl)
       - font readiness (document.fonts count/status + resolved body font-family)
       - console errors + uncaught errors
  4. tallies external (non-loopback) resource requests + their failures via WebKit's
     resource-load signals (the offline-packaging risk, recorded separately);
  5. screenshots each artifact (GdkPixbuf window grab).

Run under a SANITIZED env on the snap-sandboxed shell (see static-export.md env note):
  env -i HOME=$HOME PATH=/usr/local/bin:/usr/bin:/bin DISPLAY=$DISPLAY \
      XAUTHORITY=$XAUTHORITY XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR GDK_BACKEND=x11 \
      XDG_DATA_DIRS=/usr/local/share:/usr/share \
    python3 render_probe.py <vendor-root>

Prints a JSON block between RENDER_RESULTS_JSON_BEGIN/END. Exit 0 iff every artifact
cleared the hard checks (render-nonblank, no-uncaught-JS, css-features-supported).
"""
import json, os, sys, threading, http.server, socketserver

import gi
gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import Gtk, WebKit2, GLib  # noqa: E402

VENDOR = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else ".")
OUTDIR = os.path.dirname(os.path.abspath(__file__))

# (slug, relpath-from-vendor, what-it-stresses)
ARTIFACTS = [
    ("stripe-ds",      "design-systems/stripe/components.html",                              "design-system baseline: color-mix + grid"),
    ("zhangzara-coral","design-templates/html-ppt-zhangzara-coral/example.html",             "WebGL/Three.js hero + canvas"),
    ("social-glass",   "design-templates/social-media-dashboard/example.html",               "backdrop-filter glassmorphism"),
    ("8bit-orbit",     "design-templates/html-ppt-zhangzara-8-bit-orbit/example.html",        "clip-path + mask-image + mix-blend-mode"),
    ("crypto-dash",    "design-templates/live-artifact/examples/crypto-dashboard.html",       "Google Fonts + heavy dashboard"),
    ("matrix-canvas",  "design-templates/social-media-matrix-tracker-template/example.html",  "canvas 2D charts"),
]

CAPTURE = r"""
(function(){
  window.__p = { console: [], errors: [], resErrors: [] };
  ['log','info','warn','error'].forEach(function(k){
    var o=console[k].bind(console);
    console[k]=function(){ try{window.__p.console.push(k+': '+Array.prototype.map.call(arguments,String).join(' '));}catch(e){} return o.apply(console,arguments);};
  });
  window.addEventListener('error', function(e){
    if (e && e.target && (e.target.tagName)) {
      window.__p.resErrors.push((e.target.tagName)+': '+(e.target.src||e.target.href||''));
    } else { window.__p.errors.push(String(e.message||e.error||e)); }
  }, true);
  window.addEventListener('unhandledrejection', function(e){ window.__p.errors.push('promise: '+String(e.reason)); });
})();
"""

PROBE = r"""
JSON.stringify((function(){
  var b=document.body, de=document.documentElement;
  function supp(x){ try{return CSS.supports(x);}catch(e){return 'err';} }
  var cv=document.createElement('canvas'); var gl=null;
  try{ gl = cv.getContext('webgl2') || cv.getContext('webgl'); }catch(e){}
  var bodyCS = b? getComputedStyle(b):{};
  return {
    title: document.title,
    scrollW: de.scrollWidth, scrollH: de.scrollHeight,
    bodyChildren: b? b.children.length:0,
    bodyTextLen: b? b.innerText.trim().length:0,
    bodyBg: bodyCS.backgroundColor||'', bodyColor: bodyCS.color||'',
    bodyFont: (bodyCS.fontFamily||'').slice(0,80),
    fontsCount: (document.fonts&&document.fonts.size)||0,
    fontsStatus: (document.fonts&&document.fonts.status)||'n/a',
    css: {
      colorMix: supp('color: color-mix(in srgb, red, blue)'),
      backdrop: supp('backdrop-filter: blur(4px)'),
      clipPath: supp('clip-path: inset(0)'),
      maskImage: supp('mask-image: linear-gradient(#000,#0000)'),
      mixBlend: supp('mix-blend-mode: multiply'),
      aspect: supp('aspect-ratio: 1/1'),
      has: supp('selector(:has(*))'),
      container: supp('container-type: inline-size'),
      conic: supp('background: conic-gradient(red,blue)'),
      nesting: supp('selector(& > *)')
    },
    webgl: !!gl, webglKind: gl? (gl.constructor && gl.constructor.name):'none',
    canvasCount: document.querySelectorAll('canvas').length,
    consoleErrors: window.__p.console.filter(function(m){return m.indexOf('error:')===0;}),
    resErrors: window.__p.resErrors,
    uncaught: window.__p.errors
  };
})());
"""

RESULTS = {"engine": f"WebKitGTK {WebKit2.MAJOR_VERSION}.{WebKit2.MINOR_VERSION}.{WebKit2.MICRO_VERSION}", "artifacts": []}


def make_handler(root):
    class H(http.server.SimpleHTTPRequestHandler):
        def __init__(self, *a, **k): super().__init__(*a, directory=root, **k)
        def log_message(self, *a): pass
    return H


def serve(root):
    httpd = socketserver.TCPServer(("127.0.0.1", 0), make_handler(root))
    httpd.allow_reuse_address = True
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    return httpd, httpd.server_address[1]


class Runner:
    def __init__(self, view, loop):
        self.view, self.loop = view, loop
        self.i = -1
        self.httpd = None
        self.ext = []      # external resource requests for current artifact
        self.extFail = []

    def on_resource(self, view, resource, request):
        uri = request.get_uri() if request else (resource.get_uri() if resource else "")
        if uri and not uri.startswith("http://127.0.0.1"):
            self.ext.append(uri)
            resource.connect("failed", lambda r, e, u=uri: self.extFail.append(u))

    def next(self, *_):
        if self.httpd: self.httpd.shutdown(); self.httpd = None
        self.i += 1
        if self.i >= len(ARTIFACTS):
            finish(self.loop)
            return
        slug, rel, stress = ARTIFACTS[self.i]
        path = os.path.join(VENDOR, rel)
        root = os.path.dirname(path)
        self.ext, self.extFail = [], []
        self.httpd, port = serve(root)
        self.cur = {"slug": slug, "rel": rel, "stresses": stress}
        self.view.load_uri(f"http://127.0.0.1:{port}/{os.path.basename(path)}")

    def probe(self):
        def after(v, res, _):
            try:
                val = v.run_javascript_finish(res).get_js_value().to_string()
                data = json.loads(val)
            except Exception as e:
                data = {"probe_error": str(e)}
            data["externalRequests"] = sorted(set(
                __import__("urllib.parse", fromlist=["urlparse"]).urlparse(u).netloc for u in self.ext))
            data["externalFailures"] = sorted(set(
                __import__("urllib.parse", fromlist=["urlparse"]).urlparse(u).netloc for u in self.extFail))
            # hard checks (engine fidelity, network-independent)
            data["checks"] = {
                "render_nonblank": data.get("scrollH", 0) > 200 and data.get("bodyTextLen", 0) > 30,
                "no_uncaught_js": len(data.get("uncaught", [])) == 0,
                "css_modern_ok": all(data.get("css", {}).get(k) is True for k in
                                     ["colorMix", "clipPath", "aspect", "conic", "mixBlend"]),
            }
            data["pass"] = all(data["checks"].values())
            self.cur.update(data)
            suffix = "-offline" if OFFLINE else ""
            shot = os.path.join(OUTDIR, f"render-{self.cur['slug']}{suffix}.png")
            grab(self.view, shot)
            self.cur["screenshot"] = os.path.basename(shot)
            RESULTS["artifacts"].append(self.cur)
            self.next()
        self.view.run_javascript(PROBE, None, after, None)


def grab(view, path):
    try:
        from gi.repository import Gdk
        gw = view.get_toplevel().get_window()
        a = view.get_allocation()
        pb = Gdk.pixbuf_get_from_window(gw, 0, 0, a.width, a.height)
        if pb: pb.savev(path, "png", [], [])
    except Exception as e:
        RESULTS.setdefault("grab_errors", []).append(str(e))


def finish(loop):
    n = len(RESULTS["artifacts"]); ok = sum(1 for a in RESULTS["artifacts"] if a.get("pass"))
    RESULTS["summary"] = {"total": n, "passed_hard_checks": ok}
    print("RENDER_RESULTS_JSON_BEGIN")
    print(json.dumps(RESULTS, indent=2))
    print("RENDER_RESULTS_JSON_END")
    loop.quit()


OFFLINE = "--offline" in sys.argv


def main():
    win = Gtk.Window(); win.set_default_size(1366, 900)
    if OFFLINE:
        # Simulate the packaged app with no internet: route all non-loopback
        # traffic to a dead proxy, but exempt 127.0.0.1 so the artifact's own
        # server still serves. Characterizes the offline font/CDN degradation.
        ctx = WebKit2.WebContext.get_default()
        proxy = WebKit2.NetworkProxySettings.new("http://127.0.0.1:1", ["127.0.0.1", "localhost"])
        ctx.set_network_proxy_settings(WebKit2.NetworkProxyMode.CUSTOM, proxy)
        RESULTS["mode"] = "offline"
    else:
        RESULTS["mode"] = "online"
    ucm = WebKit2.UserContentManager()
    ucm.add_script(WebKit2.UserScript.new(
        CAPTURE, WebKit2.UserContentInjectedFrames.ALL_FRAMES,
        WebKit2.UserScriptInjectionTime.START, None, None))
    view = WebKit2.WebView.new_with_user_content_manager(ucm)
    s = view.get_settings()
    s.set_enable_developer_extras(True)
    s.set_enable_write_console_messages_to_stdout(True)
    s.set_enable_webgl(True)
    s.set_hardware_acceleration_policy(WebKit2.HardwareAccelerationPolicy.ALWAYS)
    win.add(view); win.show_all()

    loop = GLib.MainLoop()
    runner = Runner(view, loop)
    view.connect("resource-load-started", runner.on_resource)

    def on_load(v, ev):
        if ev == WebKit2.LoadEvent.FINISHED:
            GLib.timeout_add(2200, lambda: (runner.probe(), False)[1])
    view.connect("load-changed", on_load)

    def watchdog():
        RESULTS["timeout_at_index"] = runner.i
        finish(loop); return False
    GLib.timeout_add(90000, watchdog)

    runner.next()
    loop.run()
    sys.exit(0 if RESULTS.get("summary", {}).get("passed_hard_checks") == len(ARTIFACTS) else 1)


if __name__ == "__main__":
    main()
