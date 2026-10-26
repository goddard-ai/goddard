// Text-only WebKit check: no window is shown and no images are captured.
import AppKit
import WebKit

final class Probe: NSObject, WKScriptMessageHandler, WKURLSchemeHandler, WKNavigationDelegate {
    var messages: [[String: Any]] = []
    var requests = 0
    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        if let body = message.body as? String, let data = body.data(using: .utf8),
           let value = try? JSONSerialization.jsonObject(with: data) as? [String: Any] { messages.append(value) }
    }
    func webView(_ webView: WKWebView, start urlSchemeTask: WKURLSchemeTask) {
        requests += 1
        urlSchemeTask.didFailWithError(NSError(domain: "WireframeProbe", code: 1))
    }
    func webView(_ webView: WKWebView, stop urlSchemeTask: WKURLSchemeTask) {}
    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        let url = action.request.url?.absoluteString ?? ""
        decisionHandler(url == "about:blank" || url == "about:srcdoc" ? .allow : .cancel)
    }
}

func pump(_ seconds: TimeInterval = 0.1) {
    let until = Date().addingTimeInterval(seconds)
    while Date() < until { _ = RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(0.01)) }
}
func fail(_ message: String) -> Never { fputs("FAIL: \(message)\n", stderr); exit(1) }
let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
let probe = Probe()
let configuration = WKWebViewConfiguration()
configuration.websiteDataStore = .nonPersistent()
configuration.setURLSchemeHandler(probe, forURLScheme: "probe")
configuration.userContentController.add(probe, name: "wireframe")
// An unshown WKWebView suspends animation frames. Drive the controller's
// scheduled layout callbacks with timers while retaining real DOM geometry.
configuration.userContentController.addUserScript(WKUserScript(source: "window.ipc={postMessage:body=>window.webkit.messageHandlers.wireframe.postMessage(body)}; window.requestAnimationFrame=callback=>setTimeout(()=>callback(performance.now()),0)", injectionTime: .atDocumentStart, forMainFrameOnly: true))
let web = WKWebView(frame: NSRect(x: 0, y: 0, width: 800, height: 700), configuration: configuration)
web.navigationDelegate = probe
let html = try String(contentsOfFile: CommandLine.arguments[1], encoding: .utf8)
web.loadHTMLString(html, baseURL: nil)
let deadline = Date().addingTimeInterval(20)
while !probe.messages.contains(where: { $0["type"] as? String == "ready" }) && Date() < deadline { pump() }
guard probe.messages.contains(where: { $0["type"] as? String == "ready" }) else { fail("trusted controller did not become ready") }

func evaluate(_ script: String) -> Any? {
    var done = false
    var result: Any?
    var failure: Error?
    web.evaluateJavaScript(script) { value, error in result = value; failure = error; done = true }
    let deadline = Date().addingTimeInterval(10)
    while !done && Date() < deadline { pump(0.01) }
    guard done else { fail("JavaScript evaluation timed out") }
    if let failure { fail("JavaScript: \(failure.localizedDescription)") }
    return result
}
func check(_ script: String, _ message: String) {
    guard evaluate(script) as? Bool == true else { fail(message) }
}
check("window.authoredScriptRan === undefined", "authored script escaped its sandbox")
check("getComputedStyle(document.querySelector('iframe').contentDocument.body).color === 'rgb(11, 22, 33)'", "active theme did not reach authored content")
_ = evaluate("window.goddardWireframe.setTheme({'--wf-text':'rgb(33, 22, 11)'}); true")
check("getComputedStyle(document.querySelector('iframe').contentDocument.body).color === 'rgb(33, 22, 11)'", "theme change did not update existing content")
guard probe.requests == 0 else { fail("authored content loaded an external resource") }

let thread = "22222222-2222-4222-8222-222222222222"
let message = "33333333-3333-4333-8333-333333333333"
let initial: [String: Any] = ["id": thread, "screen": "tasks", "anchor": "task-sidebar", "label": "Task navigation", "resolved": false, "unread": true,
    "messages": [["id": message, "authorLabel": "Agent", "body": "Keep navigation visible"]]]
func threads(_ value: [[String: Any]], request: String? = nil) {
    let data = try! JSONSerialization.data(withJSONObject: value)
    let json = String(data: data, encoding: .utf8)!
    let key = request.map { "\"\($0)\"" } ?? "null"
    _ = evaluate("window.goddardWireframe.setThreads(\(json), \(key)); true")
    pump(0.15)
}
threads([initial])
check("document.querySelectorAll('.pin').length === 1", "agent comment has no marker")
_ = evaluate("document.querySelector('.pin').click(); true")
check("document.querySelector('dialog').open && document.activeElement === document.querySelector('textarea')", "comment popover did not open and focus its reply field")
check("(() => { const r = document.querySelector('dialog').getBoundingClientRect(); return r.left >= 0 && r.top >= 0 && r.right <= innerWidth && r.bottom <= innerHeight; })()", "comment popover escaped the preview bounds")
guard probe.messages.contains(where: { $0["type"] as? String == "read" }) else { fail("opening a thread did not mark it read") }
var read = initial; read["unread"] = false
threads([read])
check("document.querySelectorAll('.pin').length === 1 && !document.querySelector('.pin').classList.contains('unread')", "reading removed the marker or retained its unread cue")
_ = evaluate("document.querySelector('textarea').value = 'My reply <literal>'; document.querySelector('#save').click(); true")
pump()
guard let reply = probe.messages.last(where: { $0["type"] as? String == "reply" }),
      reply["body"] as? String == "My reply <literal>", let key = reply["key"] as? String else { fail("reply did not reach the host intact") }
_ = evaluate("window.goddardWireframe.setError('Temporary save failure'); true")
check("document.querySelector('textarea').value === 'My reply <literal>' && !document.querySelector('textarea').disabled", "save failure lost the reply draft")
// Saving another draft must not discard the original retry identity: its
// earlier request may have reached the daemon before the connection failed.
_ = evaluate("document.querySelector('#close').click(); document.querySelector('#comment-mode').click(); document.querySelector('#new-comment').click(); document.querySelector('textarea').value = 'Another draft'; document.querySelector('#save').click(); true")
pump()
_ = evaluate("window.goddardWireframe.setError('Temporary save failure'); document.querySelector('#close').click(); document.querySelector('.pin').click(); true")
check("document.querySelector('textarea').value === 'My reply <literal>'", "switching threads lost the reply draft")
_ = evaluate("document.querySelector('#save').click(); true")
pump()
guard probe.messages.last(where: { $0["type"] as? String == "reply" })?["key"] as? String == key else { fail("retry changed the idempotency key") }
var saved = read
saved["messages"] = [["id": message, "authorLabel": "Agent", "body": "Keep navigation visible"], ["id": "44444444-4444-4444-8444-444444444444", "authorLabel": "You", "body": "My reply <literal>"]]
threads([saved], request: key)
check("document.querySelector('#messages').textContent.includes('My reply <literal>') && document.querySelector('textarea').value === ''", "saved reply was not displayed as plain text")
_ = evaluate("document.querySelector('#stage').click(); true")
pump()
guard probe.messages.last?["type"] as? String == "stage" else { fail("explicit feedback staging did not reach the host") }
var orphaned = saved; orphaned["anchor"] = "removed-element"
threads([orphaned])
_ = evaluate("document.querySelector('#comments').click(); true")
check("document.querySelectorAll('.pin').length === 0 && document.querySelector('#threads').textContent.includes('Anchor removed')", "removed anchor lost its discussion or retained an incorrect marker")
threads([saved])
check("document.querySelectorAll('.pin').length === 1", "restoring an anchor did not restore its marker")
print("PASS: WebKit isolation, resource blocking, live theme, persistent markers, reply retry/draft, feedback staging, removed/restored anchors")
configuration.userContentController.removeScriptMessageHandler(forName: "wireframe")
