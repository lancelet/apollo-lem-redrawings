#!/usr/bin/env python3
"""Drive headless Chrome over the DevTools protocol with the standard library
only: load the inspector, optionally hover and run the benchmark, print the
page's readouts and console, and save a screenshot.

    python3 headless.py URL OUT.png [--bench] [--hover X,Y] [--wait SECONDS] [--scheme dark|light]
                        [--script "wheel:X,Y,DY[,ctrl|meta];hover:X,Y[,alt][,shift];key:0;dblclick:X,Y;click:X,Y;drag:X0,Y0,X1,Y1;eval:JS;shot:NAME;wait:S"]

`--scheme` emulates the system's prefers-color-scheme. `--size WxH` sets the
viewport (default 1600x900), `--dpr N` the device pixel ratio (default 1),
`--touch` emulates a phone (mobile viewport, touch input, a coarse pointer).
`--base URL` makes the run print every request that is not under URL (and
the count and bytes of those that are), for the static site's check.

Touch steps (they need `--touch`): `tap:X,Y`, `swipe:X0,Y0,X1,Y1`,
`pinch:X,Y,D0,D1` (two fingers D0 apart spreading to D1 about X,Y) and
`flick:X0,Y0,X1,Y1` (a fast swipe that returns at once after the lift, to watch momentum), `hold:X,Y,MS` (a finger down and still); `viewport:W,H` resizes the viewport\n(a rotation).

`--script` runs after the load, step by step: `wheel` sends a wheel event at
a page point (`ctrl` makes it a pinch), `hover` moves the pointer (`alt`
holds Alt), `key` presses a key, `dblclick` double-clicks, `click` clicks,
`drag` presses at one point and releases at another, `eval` runs JavaScript
and prints its value (a button is `document.getElementById('mode-overlay').click()`;
`;` cannot appear in it), `shot` saves OUT's directory/NAME.png, `wait`
sleeps. Each `hover` prints the readout.
"""
import base64, http.client, json, os, socket, struct, subprocess, sys, tempfile, time

CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
PORT = 9333


class Ws:
    def __init__(self, host, port, path):
        self.s = socket.create_connection((host, port))
        key = base64.b64encode(os.urandom(16)).decode()
        self.s.sendall(
            f"GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\n"
            f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n".encode()
        )
        buf = b""
        while b"\r\n\r\n" not in buf:
            buf += self.s.recv(1)
        self.buf = b""

    def send(self, obj):
        data = json.dumps(obj).encode()
        head = bytes([0x81])
        n = len(data)
        mask = os.urandom(4)
        if n < 126:
            head += bytes([0x80 | n])
        elif n < 65536:
            head += bytes([0x80 | 126]) + struct.pack(">H", n)
        else:
            head += bytes([0x80 | 127]) + struct.pack(">Q", n)
        self.s.sendall(head + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(data)))

    def _read(self, n):
        while len(self.buf) < n:
            chunk = self.s.recv(65536)
            if not chunk:
                raise EOFError
            self.buf += chunk
        out, self.buf = self.buf[:n], self.buf[n:]
        return out

    def recv(self):
        msg = b""
        while True:
            b0, b1 = self._read(2)
            n = b1 & 0x7F
            if n == 126:
                n = struct.unpack(">H", self._read(2))[0]
            elif n == 127:
                n = struct.unpack(">Q", self._read(8))[0]
            msg += self._read(n)
            if b0 & 0x80:
                return json.loads(msg)


def main():
    args = sys.argv[1:]
    url, out = args[0], args[1]
    bench = "--bench" in args
    hover = args[args.index("--hover") + 1] if "--hover" in args else None
    wait = float(args[args.index("--wait") + 1]) if "--wait" in args else 20.0
    profile = tempfile.mkdtemp()
    chrome = subprocess.Popen(
        [CHROME, "--headless=new", "--enable-unsafe-webgpu", "--use-angle=metal",
         f"--remote-debugging-port={PORT}", f"--user-data-dir={profile}",
         "--window-size=1600,900", "--no-first-run", "about:blank"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(50):
            try:
                c = http.client.HTTPConnection("127.0.0.1", PORT)
                c.request("GET", "/json")
                pages = [p for p in json.loads(c.getresponse().read()) if p["type"] == "page"]
                if pages:
                    break
            except OSError:
                pass
            time.sleep(0.3)
        ws = Ws("127.0.0.1", PORT, pages[0]["webSocketDebuggerUrl"].split(str(PORT))[1])
        ids = iter(range(1, 10**6))
        log = []; requests = []; sizes = []

        def call(method, **params):
            i = next(ids)
            ws.send({"id": i, "method": method, "params": params})
            while True:
                m = ws.recv()
                if m.get("id") == i:
                    return m.get("result", m.get("error"))
                note(m)

        def note(m):
            if m.get("method") == "Runtime.consoleAPICalled":
                log.append("console." + m["params"]["type"] + ": " + " ".join(str(a.get("value", a.get("description"))) for a in m["params"]["args"]))
            elif m.get("method") == "Runtime.exceptionThrown":
                log.append("exception: " + json.dumps(m["params"]["exceptionDetails"].get("exception", {}).get("description", "")))
            elif m.get("method") == "Network.requestWillBeSent":
                requests.append(m["params"]["request"]["url"])
            elif m.get("method") == "Network.loadingFinished":
                sizes.append(m["params"].get("encodedDataLength", 0))
            elif m.get("method") == "Log.entryAdded":
                log.append("log: " + m["params"]["entry"]["text"])

        def js(expr):
            r = call("Runtime.evaluate", expression=expr, returnByValue=True, awaitPromise=True)
            return r.get("result", {}).get("value")

        call("Runtime.enable"); call("Log.enable"); call("Page.enable")
        size = args[args.index("--size") + 1] if "--size" in args else "1600x900"
        vw, vh = [int(v) for v in size.split("x")]
        dpr = float(args[args.index("--dpr") + 1]) if "--dpr" in args else 1.0
        touch = "--touch" in args
        call("Emulation.setDeviceMetricsOverride", width=vw, height=vh, deviceScaleFactor=dpr, mobile=touch)
        if touch:
            call("Emulation.setTouchEmulationEnabled", enabled=True, maxTouchPoints=5)
            call("Emulation.setEmitTouchEventsForMouse", enabled=False)
        if "--base" in args:
            call("Network.enable")
        if "--scheme" in args:
            call("Emulation.setEmulatedMedia", features=[
                {"name": "prefers-color-scheme", "value": args[args.index("--scheme") + 1]}])
        call("Page.navigate", url=url + ("&bench=1" if bench and "?" in url else "?bench=1" if bench else ""))
        t0 = time.time()
        read = "JSON.stringify({status:document.querySelector('.status .hint:nth-of-type(3)')?.textContent, fail:document.getElementById('fail')?.textContent, bench:document.getElementById('bench')?.textContent, fps:document.getElementById('fps')?.textContent, hover:document.getElementById('hover')?.textContent})"
        while time.time() - t0 < wait:
            ws.s.settimeout(0.5)
            try:
                note(ws.recv())
            except (socket.timeout, TimeoutError):
                pass
            ws.s.settimeout(None)
            st = json.loads(js(read) or "{}")
            if st.get("fail") or (bench and st.get("bench", "").startswith("bench ")) or (not bench and time.time() - t0 > 6):
                break
        if hover:
            x, y = [float(v) for v in hover.split(",")]
            for t in ("mouseMoved",):
                call("Input.dispatchMouseEvent", type=t, x=x, y=y)
            time.sleep(1.5)
            st = json.loads(js(read) or "{}")
        if "--script" in args:
            outdir = os.path.dirname(os.path.abspath(out))
            for step in args[args.index("--script") + 1].split(";"):
                op, _, arg = step.partition(":")
                f = arg.split(",")
                if op == "wheel":
                    mods = 2 if "ctrl" in f else 4 if "meta" in f else 0
                    call("Input.dispatchMouseEvent", type="mouseMoved", x=float(f[0]), y=float(f[1]))
                    call("Input.dispatchMouseEvent", type="mouseWheel", x=float(f[0]), y=float(f[1]),
                         deltaX=0, deltaY=float(f[2]), modifiers=mods)
                    time.sleep(0.4)
                elif op == "hover":
                    call("Input.dispatchMouseEvent", type="mouseMoved", x=float(f[0]), y=float(f[1]),
                         modifiers=(1 if "alt" in f else 0) | (8 if "shift" in f else 0))
                    time.sleep(0.8)
                    st = json.loads(js(read) or "{}")
                    print("hover", arg, "->", st.get("hover"))
                elif op == "key":
                    call("Input.dispatchKeyEvent", type="keyDown", key=f[0], text=f[0])
                    call("Input.dispatchKeyEvent", type="keyUp", key=f[0])
                    time.sleep(0.4)
                elif op == "dblclick":
                    for n in (1, 2):
                        for t in ("mousePressed", "mouseReleased"):
                            call("Input.dispatchMouseEvent", type=t, x=float(f[0]), y=float(f[1]),
                                 button="left", clickCount=n)
                    time.sleep(0.4)
                elif op == "click":
                    for t in ("mousePressed", "mouseReleased"):
                        call("Input.dispatchMouseEvent", type=t, x=float(f[0]), y=float(f[1]),
                             button="left", clickCount=1)
                    time.sleep(0.5)
                elif op == "drag":
                    x0, y0, x1, y1 = [float(v) for v in f[:4]]
                    call("Input.dispatchMouseEvent", type="mouseMoved", x=x0, y=y0)
                    call("Input.dispatchMouseEvent", type="mousePressed", x=x0, y=y0, button="left", clickCount=1)
                    for k in range(1, 9):
                        call("Input.dispatchMouseEvent", type="mouseMoved", x=x0 + (x1 - x0) * k / 8,
                             y=y0 + (y1 - y0) * k / 8, button="left", buttons=1)
                    call("Input.dispatchMouseEvent", type="mouseReleased", x=x1, y=y1, button="left", clickCount=1)
                    time.sleep(0.5)
                elif op == "tap":
                    pt = [{"x": float(f[0]), "y": float(f[1]), "id": 1}]
                    call("Input.dispatchTouchEvent", type="touchStart", touchPoints=pt)
                    time.sleep(0.06)
                    call("Input.dispatchTouchEvent", type="touchEnd", touchPoints=[])
                    time.sleep(0.6)
                elif op == "swipe":
                    x0, y0, x1, y1 = [float(v) for v in f[:4]]
                    call("Input.dispatchTouchEvent", type="touchStart", touchPoints=[{"x": x0, "y": y0, "id": 1}])
                    for k in range(1, 9):
                        call("Input.dispatchTouchEvent", type="touchMove", touchPoints=[
                            {"x": x0 + (x1 - x0) * k / 8, "y": y0 + (y1 - y0) * k / 8, "id": 1}])
                        time.sleep(0.02)
                    call("Input.dispatchTouchEvent", type="touchEnd", touchPoints=[])
                    time.sleep(0.6)
                elif op == "flick":
                    # A quick swipe that returns at once after the lift, to watch the momentum.
                    x0, y0, x1, y1 = [float(v) for v in f[:4]]
                    call("Input.dispatchTouchEvent", type="touchStart", touchPoints=[{"x": x0, "y": y0, "id": 1}])
                    for k in range(1, 9):
                        call("Input.dispatchTouchEvent", type="touchMove", touchPoints=[
                            {"x": x0 + (x1 - x0) * k / 8, "y": y0 + (y1 - y0) * k / 8, "id": 1}])
                        time.sleep(0.01)
                    call("Input.dispatchTouchEvent", type="touchEnd", touchPoints=[])
                elif op == "pinch":
                    cx, cy, d0, d1 = [float(v) for v in f[:4]]
                    two = lambda d: [{"x": cx - d / 2, "y": cy, "id": 1}, {"x": cx + d / 2, "y": cy, "id": 2}]
                    call("Input.dispatchTouchEvent", type="touchStart", touchPoints=two(d0))
                    for k in range(1, 9):
                        call("Input.dispatchTouchEvent", type="touchMove", touchPoints=two(d0 + (d1 - d0) * k / 8))
                        time.sleep(0.02)
                    call("Input.dispatchTouchEvent", type="touchEnd", touchPoints=[])
                    time.sleep(0.6)
                elif op == "hold":
                    pt = [{"x": float(f[0]), "y": float(f[1]), "id": 1}]
                    call("Input.dispatchTouchEvent", type="touchStart", touchPoints=pt)
                    time.sleep(float(f[2]) / 1000.0)
                    call("Input.dispatchTouchEvent", type="touchEnd", touchPoints=[])
                    time.sleep(0.6)
                elif op == "viewport":
                    # A rotation: the viewport changes size, as the phone turns.
                    call("Emulation.setDeviceMetricsOverride", width=int(f[0]), height=int(f[1]),
                         deviceScaleFactor=dpr, mobile=touch)
                    time.sleep(1.0)
                elif op == "shot":
                    shot = call("Page.captureScreenshot", format="png")
                    open(os.path.join(outdir, arg + ".png"), "wb").write(base64.b64decode(shot["data"]))
                elif op == "wait":
                    time.sleep(float(arg))
                elif op == "eval":
                    print("eval:", js(arg))
            st = json.loads(js(read) or "{}")
        print("readout:", json.dumps(st, indent=1))
        shot = call("Page.captureScreenshot", format="png")
        open(out, "wb").write(base64.b64decode(shot["data"]))
        for l in log[-15:]:
            print(l)
        if "--base" in args:
            base = args[args.index("--base") + 1]
            inside = [u for u in requests if u.startswith(base)]
            outside = [u for u in requests if not u.startswith(base) and not u.startswith("data:") and not u.startswith("blob:")]
            print(f"requests: {len(inside)} under {base} ({sum(sizes)} bytes on the wire, all), {len(outside)} outside")
            for u in outside:
                print("OUTSIDE", u)
    finally:
        chrome.terminate()


main()
