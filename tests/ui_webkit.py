#!/usr/bin/env python3
"""Drive the shipped drop panel in WebKitGTK and check every control."""

import json
import os
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
PAGE = os.path.join(ROOT, "ui", "index.html")
BINARY = os.path.join(ROOT, "target", "debug", "markitdown-desktop")
MARKDOWN = "| name | city |\n| --- | --- |\n| Ada | London |"

BRIDGE = r"""
(function () {
  var pending = {};
  var next = 1;
  var listeners = {};
  window.__markitdownReply = function (id, ok, value) {
    var slot = pending[id];
    if (!slot) return;
    delete pending[id];
    if (ok) slot.resolve(value);
    else slot.reject(new Error(typeof value === "string" ? value : "invoke failed"));
  };
  window.__markitdownEmit = function (name, event) {
    var fn = listeners[name];
    if (fn) fn(event);
  };
  window.__TAURI__ = {
    core: {
      invoke: function (command, args) {
        var id = next++;
        return new Promise(function (resolve, reject) {
          pending[id] = { resolve: resolve, reject: reject };
          window.webkit.messageHandlers.markitdown.postMessage(JSON.stringify({
            id: id,
            command: command,
            args: args
          }));
        });
      }
    },
    event: {
      listen: function (name, fn) {
        listeners[name] = fn;
        return Promise.resolve(function () { delete listeners[name]; });
      }
    }
  };
  try {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: function (text) {
          window.__copied = text;
          return Promise.resolve();
        }
      }
    });
  } catch (err) {
    window.__clipboardStubFailed = String(err);
  }
})();
"""


class Fail(Exception):
    pass


def run_smoke(source, destination, replace):
    """Call the real desktop command, then stop the window it opens."""
    if not os.path.isfile(BINARY):
        raise Fail("desktop binary is missing at " + BINARY)
    descriptor, result_path = tempfile.mkstemp(prefix="markitdown-smoke-", suffix=".json")
    os.close(descriptor)
    os.remove(result_path)
    env = os.environ.copy()
    env["MARKITDOWN_SMOKE_SOURCE"] = source
    env["MARKITDOWN_SMOKE_FILE"] = result_path
    env.pop("MARKITDOWN_SMOKE_DEST", None)
    env.pop("MARKITDOWN_SMOKE_REPLACE", None)
    if destination is not None:
        env["MARKITDOWN_SMOKE_DEST"] = destination
    if replace:
        env["MARKITDOWN_SMOKE_REPLACE"] = "1"
    proc = subprocess.Popen(
        [BINARY],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        env=env,
        start_new_session=True,
    )
    data = b""
    deadline = time.time() + 45
    try:
        while time.time() < deadline:
            if os.path.isfile(result_path):
                with open(result_path, encoding="utf-8") as handle:
                    text = handle.read()
                if text:
                    try:
                        return json.loads(text)
                    except json.JSONDecodeError:
                        pass
            ready, _, _ = select.select([proc.stderr], [], [], 0.2)
            if ready:
                chunk = os.read(proc.stderr.fileno(), 8192)
                if chunk:
                    data += chunk
                elif proc.poll() is not None:
                    break
            elif proc.poll() is not None:
                break
        raise Fail(
            "desktop command did not report a result:\n" + data.decode("utf-8", "replace")[-2000:]
        )
    finally:
        try:
            os.killpg(proc.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            proc.wait(timeout=3)
        for path in (result_path, result_path + ".partial"):
            if os.path.exists(path):
                os.remove(path)


def main():
    if not os.environ.get("DISPLAY") and not os.environ.get("WAYLAND_DISPLAY"):
        print("webkit skipped: no display", file=sys.stderr)
        return 77
    try:
        import gi

        gi.require_version("Gtk", "3.0")
        gi.require_version("WebKit2", "4.1")
        from gi.repository import GLib, Gtk, WebKit2
    except Exception as err:
        print(f"webkit skipped: {err}", file=sys.stderr)
        return 77

    Gtk.init([])
    view = WebKit2.WebView()
    settings = view.get_settings()
    settings.set_enable_javascript(True)
    settings.set_allow_file_access_from_file_urls(True)
    settings.set_enable_write_console_messages_to_stdout(True)

    manager = view.get_user_content_manager()
    manager.add_script(
        WebKit2.UserScript(
            BRIDGE,
            WebKit2.UserContentInjectedFrames.TOP_FRAME,
            WebKit2.UserScriptInjectionTime.START,
            None,
            None,
        )
    )
    try:
        manager.register_script_message_handler("markitdown")
    except TypeError:
        manager.register_script_message_handler("markitdown", None)

    runner = Runner(view, GLib)
    manager.connect("script-message-received::markitdown", runner.on_message)

    window = Gtk.Window(title="MarkItDown UI test")
    window.set_default_size(880, 720)
    window.add(view)
    window.show_all()

    def loaded(_view, event):
        if event == WebKit2.LoadEvent.FINISHED and not runner.started:
            runner.started = True
            GLib.idle_add(runner.start)

    view.connect("load-changed", loaded)
    view.load_uri("file://" + PAGE)
    GLib.timeout_add(180000, runner.timed_out)
    Gtk.main()
    window.destroy()
    if runner.error:
        print(runner.error, file=sys.stderr)
        return 1
    print("webkit-ui ok")
    for line in runner.log:
        print(line)
    return 0


class Runner:
    def __init__(self, view, glib):
        self.view = view
        self.glib = glib
        self.calls = []
        self.queue = []
        self.waiting = []
        self.live = False
        self.live_dir = None
        self.started = False
        self.error = None
        self.log = []
        self.steps = []

    def on_message(self, _manager, value):
        try:
            raw = value.get_js_value().to_string()
        except Exception:
            raw = value.to_json(0) if hasattr(value, "to_json") else str(value)
        message = json.loads(raw)
        self.calls.append(message)
        if self.live and message.get("command") == "save_conversion":
            args = message.get("args") or {}
            try:
                result = run_smoke(
                    args.get("source") or "",
                    args.get("destination"),
                    bool(args.get("replaceExisting")),
                )
            except Exception as err:
                self.fail("live save failed: %s" % err)
                return
            self.answer(message["id"], result)
            return
        if self.queue:
            reply = self.queue.pop(0)
            self.answer(message["id"], reply)
        else:
            self.waiting.append(message)

    def answer(self, ident, reply):
        if isinstance(reply, dict) and reply.get("__reject"):
            script = "window.__markitdownReply(%d, false, %s)" % (
                ident,
                json.dumps(reply["__reject"]),
            )
        else:
            script = "window.__markitdownReply(%d, true, %s)" % (ident, json.dumps(reply))
        self.eval(script, lambda _value: None)

    def arm(self, reply):
        self.queue.append(reply)

    def eval(self, script, done):
        def finish(webview, result, _data):
            if self.error:
                return
            try:
                value = webview.evaluate_javascript_finish(result)
            except Exception as err:
                self.fail(f"javascript failed: {err}\n{script}")
                return
            text = None
            if value is not None:
                try:
                    text = value.to_string()
                except Exception:
                    text = str(value)
            done(text)

        self.view.evaluate_javascript(script, -1, None, None, None, finish, None)

    def value(self, expression, done):
        self.eval(
            "(function(){ return JSON.stringify(%s); })()" % expression,
            lambda text: done(json.loads(text) if text is not None else None),
        )

    def fail(self, message):
        self.error = message
        print(message, file=sys.stderr)
        self.glib.idle_add(self.stop)

    def stop(self):
        from gi.repository import Gtk

        if self.live_dir and not self.error:
            shutil.rmtree(self.live_dir, ignore_errors=True)
        Gtk.main_quit()
        return False

    def timed_out(self):
        if not self.error:
            self.fail("timed out. calls=%s log=%s" % (self.calls[-3:], self.log))
        return False

    def start(self):
        self.steps = [
            self.step_installed,
            self.step_convert_without_source,
            self.step_drag_highlight,
            self.step_browser_drop_beside,
            self.step_copy,
            self.step_explicit_empty_does_not_save,
            self.step_suggestion_then_convert,
            self.step_whitespace_is_empty,
            self.step_trimmed_path_is_what_gets_saved,
            self.step_save_rejection,
            self.step_keeps_a_chosen_path,
            self.step_confirmation_then_replace,
            self.step_edited_path_is_not_an_automatic_replace,
            self.step_failure_hides_preview,
            self.step_unexpected_response,
            self.step_native_multi_drop,
            self.step_native_one_clears_notice,
            self.step_one_drop_is_saved_once_when_both_events_fire,
            self.step_native_empty,
            self.step_choose_file,
            self.step_choose_cancel,
            self.step_choose_error,
            self.step_browse_output,
            self.step_browse_from_beside_uses_the_chosen_path,
            self.step_keyboard_opens_dialog,
            self.step_drop_without_path,
            self.step_pathless_drop_does_not_erase_a_newer_save,
            self.step_file_url,
            self.step_windows_file_url,
            self.step_multi_browser_files,
            self.step_stale_response_is_ignored,
            self.step_mode_switch_ignores_inflight,
            self.step_tar_suggestion,
            self.step_live_beside_then_explicit_then_replace,
        ]
        self.next()
        return False

    def next(self):
        if self.error:
            return
        if not self.steps:
            self.stop()
            return
        step = self.steps.pop(0)
        self.log.append(step.__name__)
        step()

    def later(self, fn):
        self.glib.timeout_add(30, lambda: fn() or False)

    def wait_until(self, expression, then, tries=40):
        def poll(left):
            if self.error:
                return
            self.value(
                expression,
                lambda ok: then() if ok else (self.fail(f"timed out waiting for {expression}. last={self.snapshot()}") if left <= 1 else self.glib.timeout_add(25, lambda: poll(left - 1) or False)),
            )

        poll(tries)

    def snapshot(self):
        return "calls=%d queue=%d waiting=%d log=%s" % (
            len(self.calls),
            len(self.queue),
            len(self.waiting),
            self.log[-4:],
        )

    def run_js(self, script, then):
        self.eval(script, lambda _text: then())

    def text_is(self, element_id, expected, then):
        self.value(
            "document.getElementById(%s).textContent" % json.dumps(element_id),
            lambda got: then() if got == expected else self.fail(f"#{element_id} text {got!r} != {expected!r}"),
        )

    def step_installed(self):
        self.value(
            "({ready: window.__markitdownDropReady === true, installed: document.getElementById('drop-target').getAttribute('data-installed'), visible: document.getElementById('output-row').classList.contains('visible'), bridge: !!(window.__TAURI__ && window.__TAURI__.core)})",
            lambda got: self.next()
            if got == {"ready": True, "installed": "1", "visible": False, "bridge": True}
            else self.fail(f"install state {got}"),
        )

    def step_convert_without_source(self):
        before = len(self.calls)
        self.run_js(
            "document.getElementById('convert').click()",
            lambda: self.later(
                lambda: self.text_is(
                    "status",
                    "Choose a file first.",
                    lambda: self.next()
                    if len(self.calls) == before
                    else self.fail("convert without a source invoked a command"),
                )
            ),
        )

    def step_drag_highlight(self):
        script = """
          (function () {
            var drop = document.getElementById('drop-target');
            var over = new Event('dragover', {cancelable: true});
            drop.dispatchEvent(over);
            var on = drop.classList.contains('dragover');
            var leave = new Event('dragleave');
            drop.dispatchEvent(leave);
            return on && !drop.classList.contains('dragover');
          })()
        """
        self.eval(script, lambda text: self.next() if text == "true" else self.fail(f"drag highlight {text}"))

    def written(self, path):
        return {"status": "written", "path": path, "markdown": MARKDOWN}

    def step_browser_drop_beside(self):
        self.arm(self.written("/data/notes.md"))
        script = """
          (function () {
            var drop = document.getElementById('drop-target');
            var event = new Event('drop', {cancelable: true});
            Object.defineProperty(event, 'dataTransfer', {value: {
              files: [{path: '/data/notes.csv'}],
              getData: function () { return ''; }
            }});
            drop.dispatchEvent(event);
          })()
        """
        self.run_js(script, lambda: self.wait_saved("/data/notes.md", "/data/notes.csv", None))

    def wait_saved(self, path, source, destination):
        def check():
            save = [call for call in self.calls if call["args"] and call["command"] == "save_conversion"][-1]
            args = save["args"]
            if args.get("source") != source or args.get("destination") != destination or args.get("replaceExisting") is not False:
                self.fail(f"save args {args}")
                return
            self.value(
                "({status: document.getElementById('status').textContent, kind: document.getElementById('status').className, preview: document.getElementById('preview').textContent, hidden: document.getElementById('preview').hidden, copy: document.getElementById('copy').hidden, written: document.getElementById('written-path').textContent, confirm: document.getElementById('confirm-replace').hidden})",
                lambda got: self.next()
                if got["status"] == "Saved."
                and got["kind"] == "ok"
                and got["preview"] == MARKDOWN
                and got["hidden"] is False
                and got["copy"] is False
                and got["written"] == "Wrote " + path
                and got["confirm"] is True
                else self.fail(f"saved panel {got}"),
            )

        self.wait_until("document.getElementById('status').textContent === 'Saved.'", check)

    def step_copy(self):
        self.run_js(
            "document.getElementById('copy').click()",
            lambda: self.wait_until(
                "document.getElementById('status').textContent === 'Copied Markdown.'",
                lambda: self.value(
                    "window.__copied",
                    lambda copied: self.next() if copied == MARKDOWN else self.fail(f"copied {copied!r}"),
                ),
            ),
        )

    def step_explicit_empty_does_not_save(self):
        before = len(self.calls)
        script = """
          (function () {
            var explicit = document.querySelector('input[value="explicit"]');
            explicit.checked = true;
            explicit.dispatchEvent(new Event('change'));
            document.getElementById('output-path').value = '';
            document.getElementById('convert').click();
          })()
        """
        self.run_js(
            script,
            lambda: self.later(
                lambda: self.value(
                    "({row: document.getElementById('output-row').classList.contains('visible'), status: document.getElementById('status').textContent, preview: document.getElementById('preview').hidden, copy: document.getElementById('copy').hidden, value: document.getElementById('output-path').value})",
                    lambda got: self.next()
                    if len(self.calls) == before
                    and got["row"] is True
                    and got["status"] == "Choose an output path before saving."
                    and got["preview"] is True
                    and got["copy"] is True
                    and got["value"] == "/data/notes.md"
                    else self.fail(f"empty explicit {got} calls {len(self.calls)} before {before}"),
                )
            ),
        )

    def step_suggestion_then_convert(self):
        self.arm(self.written("/data/notes.md"))
        self.run_js(
            "document.getElementById('convert').click()",
            lambda: self.wait_saved("/data/notes.md", "/data/notes.csv", "/data/notes.md"),
        )

    def step_trimmed_path_is_what_gets_saved(self):
        self.arm(self.written("/data/padded.md"))
        script = """
          (function () {
            document.getElementById('output-path').value = '  /data/padded.md  ';
            document.getElementById('convert').click();
          })()
        """
        self.run_js(
            script,
            lambda: self.wait_saved("/data/padded.md", "/data/notes.csv", "/data/padded.md"),
        )

    def step_save_rejection(self):
        self.arm({"__reject": "The converter stopped."})
        self.run_js(
            "document.getElementById('convert').click()",
            lambda: self.wait_until(
                "document.getElementById('status').textContent === 'The converter stopped.' && document.getElementById('preview').hidden && document.getElementById('copy').hidden",
                self.next,
            ),
        )

    def step_whitespace_is_empty(self):
        before = len(self.calls)
        script = """
          (function () {
            document.getElementById('output-path').value = '   ';
            document.getElementById('convert').click();
          })()
        """
        self.run_js(
            script,
            lambda: self.later(
                lambda: self.text_is(
                    "status",
                    "Choose an output path before saving.",
                    lambda: self.next()
                    if len(self.calls) == before
                    else self.fail("whitespace path was saved"),
                )
            ),
        )

    def step_keeps_a_chosen_path(self):
        self.arm(self.written("/keep/mine.md"))
        script = """
          (function () {
            document.getElementById('output-path').value = '/keep/mine.md';
            var event = new Event('drop', {cancelable: true});
            Object.defineProperty(event, 'dataTransfer', {value: {
              files: [{path: '/other/archive.tar.gz'}],
              getData: function () { return ''; }
            }});
            document.getElementById('drop-target').dispatchEvent(event);
          })()
        """
        self.run_js(script, lambda: self.wait_saved("/keep/mine.md", "/other/archive.tar.gz", "/keep/mine.md"))

    def step_confirmation_then_replace(self):
        self.arm(
            {
                "status": "needsConfirmation",
                "path": "/keep/mine.md",
                "message": "Destination already exists: /keep/mine.md. Confirm replacement to overwrite it.",
                "markdown": MARKDOWN,
            }
        )
        self.run_js(
            "document.getElementById('convert').click()",
            lambda: self.wait_until(
                "!document.getElementById('confirm-replace').hidden && document.getElementById('preview').textContent.indexOf('Ada') !== -1 && document.getElementById('written-path').textContent === '' && !document.getElementById('copy').hidden",
                self.replace_existing,
            ),
        )

    def replace_existing(self):
        self.text_is(
            "status",
            "Destination already exists: /keep/mine.md. Confirm replacement to overwrite it.",
            self.copy_confirmed,
        )

    def copy_confirmed(self):
        self.run_js(
            "window.__copied = ''; document.getElementById('copy').click()",
            lambda: self.wait_until(
                "window.__copied === %s" % json.dumps(MARKDOWN),
                self.click_replace,
            ),
        )

    def click_replace(self):
        before = [call for call in self.calls if call["command"] == "save_conversion"]
        self.arm(self.written("/keep/mine.md"))
        self.run_js(
            "document.getElementById('confirm-replace').click()",
            lambda: self.wait_until(
                "document.getElementById('status').textContent === 'Saved.'",
                lambda: self.assert_replace(before),
            ),
        )

    def assert_replace(self, before):
        saves = [call for call in self.calls if call["command"] == "save_conversion"]
        latest = saves[-1]["args"]
        if len(saves) != len(before) + 1 or latest.get("replaceExisting") is not True or latest.get("destination") != "/keep/mine.md":
            self.fail(f"replace args {latest}")
            return
        self.value(
            "document.getElementById('confirm-replace').hidden && !document.getElementById('copy').hidden",
            lambda ok: self.next() if ok else self.fail("replace did not restore the preview controls"),
        )

    def step_edited_path_is_not_an_automatic_replace(self):
        self.arm(
            {
                "status": "needsConfirmation",
                "path": "/keep/mine.md",
                "message": "Destination already exists: /keep/mine.md. Confirm replacement to overwrite it.",
                "markdown": MARKDOWN,
            }
        )
        self.run_js(
            "document.getElementById('convert').click()",
            lambda: self.wait_until(
                "!document.getElementById('confirm-replace').hidden",
                self.retarget_replace,
            ),
        )

    def retarget_replace(self):
        self.arm(self.written("/keep/other.md"))
        script = """
          (function () {
            document.getElementById('output-path').value = '/keep/other.md';
            document.getElementById('confirm-replace').click();
          })()
        """
        self.run_js(
            script,
            lambda: self.wait_saved("/keep/other.md", "/other/archive.tar.gz", "/keep/other.md"),
        )

    def step_failure_hides_preview(self):
        self.arm({"status": "failed", "message": "Unsupported format: blob.bin"})
        self.run_js(
            "document.getElementById('convert').click()",
            lambda: self.wait_until(
                "document.getElementById('status').textContent === 'Unsupported format: blob.bin'",
                lambda: self.value(
                    "document.getElementById('preview').hidden && document.getElementById('copy').hidden && document.getElementById('confirm-replace').hidden && document.getElementById('status').className === 'error'",
                    lambda ok: self.next() if ok else self.fail("failure left a success control visible"),
                ),
            ),
        )

    def step_unexpected_response(self):
        self.arm({"status": "nope"})
        self.run_js(
            "document.getElementById('convert').click()",
            lambda: self.wait_until(
                "document.getElementById('status').textContent === 'Unexpected response from the converter.'",
                self.next,
            ),
        )

    def step_native_multi_drop(self):
        self.arm(self.written("/data/first.md"))
        script = """
          window.__markitdownEmit('tauri://drag-drop', {payload: {paths: ['/data/first.csv', '/data/second.csv']}});
        """
        self.run_js(
            script,
            lambda: self.wait_until(
                "document.getElementById('notice').textContent === 'Using the first file. Drop one file at a time.' && document.getElementById('status').textContent === 'Saved.'",
                self.assert_first_only,
            ),
        )

    def assert_first_only(self):
        latest = [call for call in self.calls if call["command"] == "save_conversion"][-1]["args"]
        if latest.get("source") != "/data/first.csv":
            self.fail(f"multi drop used {latest}")
            return
        self.next()

    def step_native_one_clears_notice(self):
        self.arm(self.written("/data/only.md"))
        self.run_js(
            "window.__markitdownEmit('tauri://drag-drop', {payload: {paths: ['/data/only.csv']}});",
            lambda: self.wait_until(
                "document.getElementById('notice').textContent === '' && document.getElementById('status').textContent === 'Saved.'",
                self.next,
            ),
        )

    def step_one_drop_is_saved_once_when_both_events_fire(self):
        before = len([call for call in self.calls if call["command"] == "save_conversion"])
        self.arm(self.written("/keep/other.md"))
        script = """
          (function () {
            var event = new Event('drop', {cancelable: true});
            Object.defineProperty(event, 'dataTransfer', {value: {
              files: [{path: '/data/once.csv'}],
              getData: function () { return ''; }
            }});
            document.getElementById('drop-target').dispatchEvent(event);
            window.__markitdownEmit('tauri://drag-drop', {payload: {paths: ['/data/once.csv']}});
          })()
        """
        self.run_js(script, lambda: self.wait_until(
            "document.getElementById('status').textContent === 'Saved.'",
            lambda: self.assert_one_drop(before),
        ))

    def assert_one_drop(self, before):
        saves = [call for call in self.calls if call["command"] == "save_conversion"]
        if len(saves) != before + 1 or saves[-1]["args"].get("source") != "/data/once.csv":
            self.fail("one file drop started %d saves: %s" % (len(saves) - before, saves[-1]["args"] if saves else None))
            return
        self.next()

    def step_native_empty(self):
        before = len(self.calls)
        self.run_js(
            "window.__markitdownEmit('tauri://drag-drop', {payload: {paths: []}});",
            lambda: self.later(
                lambda: self.text_is(
                    "status",
                    "Drop a file to convert.",
                    lambda: self.next() if len(self.calls) == before else self.fail("empty drop invoked a command"),
                )
            ),
        )

    def step_choose_file(self):
        self.arm("/data/picked.csv")
        self.arm(self.written("/keep/mine.md"))
        self.run_js(
            "document.getElementById('choose-file').click()",
            lambda: self.wait_until(
                "document.getElementById('status').textContent === 'Saved.' && document.getElementById('source-label').textContent === 'Source: /data/picked.csv'",
                self.assert_open_dialog,
            ),
        )

    def assert_open_dialog(self):
        opens = [call for call in self.calls if call["command"] == "plugin:dialog|open"]
        if not opens or opens[-1]["args"]["options"] != {
            "multiple": False,
            "directory": False,
            "title": "Choose a file to convert",
        }:
            self.fail(f"open dialog {opens[-1]['args'] if opens else None}")
            return
        self.next()

    def step_choose_cancel(self):
        before = len([call for call in self.calls if call["command"] == "save_conversion"])
        self.arm(None)
        self.run_js(
            "document.getElementById('choose-file').click()",
            lambda: self.later(
                lambda: self.next()
                if len([call for call in self.calls if call["command"] == "save_conversion"]) == before
                else self.fail("cancel still saved")
            ),
        )

    def step_choose_error(self):
        self.arm({"__reject": "The dialog could not open."})
        self.run_js(
            "document.getElementById('drop-target').click()",
            lambda: self.wait_until(
                "document.getElementById('status').textContent === 'The dialog could not open.'",
                self.next,
            ),
        )

    def step_browse_output(self):
        self.arm("/data/out/chosen.md")
        self.arm(self.written("/data/out/chosen.md"))
        self.run_js(
            "document.getElementById('choose-output').click()",
            lambda: self.wait_until(
                "document.getElementById('output-path').value === '/data/out/chosen.md' && document.getElementById('status').textContent === 'Saved.'",
                self.assert_save_dialog,
            ),
        )

    def assert_save_dialog(self):
        saves = [call for call in self.calls if call["command"] == "plugin:dialog|save"]
        options = saves[-1]["args"]["options"]
        if options.get("filters") != [{"name": "Markdown", "extensions": ["md"]}]:
            self.fail(f"save dialog {options}")
            return
        latest = [call for call in self.calls if call["command"] == "save_conversion"][-1]["args"]
        if latest.get("destination") != "/data/out/chosen.md" or latest.get("source") != "/data/picked.csv":
            self.fail(f"browse save used {latest}")
            return
        self.next()

    def step_keyboard_opens_dialog(self):
        before = len([call for call in self.calls if call["command"] == "plugin:dialog|open"])
        self.arm(None)
        script = """
          (function () {
            var event = new KeyboardEvent('keydown', {key: 'Enter', cancelable: true, bubbles: true});
            document.getElementById('drop-target').dispatchEvent(event);
          })()
        """
        self.run_js(
            script,
            lambda: self.later(
                lambda: self.next()
                if len([call for call in self.calls if call["command"] == "plugin:dialog|open"]) == before + 1
                else self.fail("Enter did not open the file dialog")
            ),
        )

    def step_drop_without_path(self):
        before = len([call for call in self.calls if call["command"] == "save_conversion"])
        script = """
          (function () {
            var event = new Event('drop', {cancelable: true});
            Object.defineProperty(event, 'dataTransfer', {value: {
              files: [{}],
              getData: function () { return ''; }
            }});
            document.getElementById('drop-target').dispatchEvent(event);
          })()
        """
        self.run_js(
            script,
            lambda: self.wait_until(
                "document.getElementById('status').textContent === 'That drop did not include a file path. Use Choose file.'",
                lambda: self.next()
                if len([call for call in self.calls if call["command"] == "save_conversion"]) == before
                else self.fail("pathless drop saved"),
            ),
        )

    def step_pathless_drop_does_not_erase_a_newer_save(self):
        self.arm(self.written("/data/from-beside.md"))
        script = """
          (function () {
            var event = new Event('drop', {cancelable: true});
            Object.defineProperty(event, 'dataTransfer', {value: {
              files: [{}],
              getData: function () { return ''; }
            }});
            document.getElementById('drop-target').dispatchEvent(event);
            document.getElementById('convert').click();
          })()
        """
        self.run_js(
            script,
            lambda: self.wait_until(
                "document.getElementById('status').textContent === 'Saved.'",
                lambda: self.glib.timeout_add(150, lambda: self.saved_stuck() or False),
            ),
        )

    def saved_stuck(self):
        self.text_is("status", "Saved.", self.next)

    def step_file_url(self):
        self.arm(self.written("/tmp/my notes.md"))
        script = r"""
          (function () {
            var event = new Event('drop', {cancelable: true});
            Object.defineProperty(event, 'dataTransfer', {value: {
              files: [],
              getData: function () { return '# comment\r\nfile:///tmp/my%20notes.csv\r\n'; }
            }});
            document.getElementById('drop-target').dispatchEvent(event);
          })()
        """
        self.run_js(script, lambda: self.wait_saved("/tmp/my notes.md", "/tmp/my notes.csv", "/data/from-beside.md"))

    def step_windows_file_url(self):
        # The output field still holds the path chosen in the browse step.
        self.arm(self.written("C:/Users/Ada/notes.md"))
        script = r"""
          (function () {
            document.getElementById('output-path').value = '';
            var beside = document.querySelector('input[value="beside"]');
            beside.checked = true;
            beside.dispatchEvent(new Event('change'));
            var event = new Event('drop', {cancelable: true});
            Object.defineProperty(event, 'dataTransfer', {value: {
              files: [],
              getData: function () { return 'file:///C:/Users/Ada/notes.csv'; }
            }});
            document.getElementById('drop-target').dispatchEvent(event);
          })()
        """
        self.run_js(script, lambda: self.wait_saved("C:/Users/Ada/notes.md", "C:/Users/Ada/notes.csv", None))

    def step_multi_browser_files(self):
        self.arm(self.written("/data/a.md"))
        script = """
          (function () {
            var event = new Event('drop', {cancelable: true});
            Object.defineProperty(event, 'dataTransfer', {value: {
              files: [{path: '/data/a.csv'}, {path: '/data/b.csv'}],
              getData: function () { return ''; }
            }});
            document.getElementById('drop-target').dispatchEvent(event);
          })()
        """
        self.run_js(
            script,
            lambda: self.wait_until(
                "document.getElementById('notice').textContent === 'Using the first file. Drop one file at a time.' && document.getElementById('status').textContent === 'Saved.'",
                lambda: self.assert_source("/data/a.csv"),
            ),
        )

    def assert_source(self, source):
        latest = [call for call in self.calls if call["command"] == "save_conversion"][-1]["args"]
        if latest.get("source") != source:
            self.fail(f"expected source {source}, got {latest}")
            return
        self.next()

    def step_stale_response_is_ignored(self):
        # No reply is armed, so both saves wait.
        script = """
          (function () {
            window.__markitdownEmit('tauri://drag-drop', {payload: {paths: ['/data/old.csv']}});
            window.__markitdownEmit('tauri://drag-drop', {payload: {paths: ['/data/new.csv']}});
          })()
        """
        self.run_js(script, lambda: self.poll_saves(2, self.release_stale))

    def poll_saves(self, count, then, left=40):
        if self.error:
            return
        pending = [call for call in self.waiting if call["command"] == "save_conversion"]
        if len(pending) >= count:
            then()
            return
        if left <= 0:
            self.fail(f"expected {count} in-flight saves, waiting={self.waiting}")
            return
        self.glib.timeout_add(25, lambda: self.poll_saves(count, then, left - 1) or False)

    def release_stale(self):
        pending = [call for call in self.waiting if call["command"] == "save_conversion"]
        first, second = pending[0], pending[1]
        self.waiting = [call for call in self.waiting if call not in (first, second)]
        self.answer(first["id"], {"status": "written", "path": "/data/old.md", "markdown": "OLD"})
        self.later(lambda: self.assert_old_ignored(second))

    def assert_old_ignored(self, second):
        def check(preview):
            if preview == "OLD":
                self.fail("stale conversion replaced the newer one")
                return
            self.answer(second["id"], {"status": "written", "path": "/data/new.md", "markdown": "NEW"})
            self.later(
                lambda: self.wait_until(
                    "document.getElementById('preview').textContent === 'NEW'",
                    self.next,
                )
            )

        self.value("document.getElementById('preview').textContent", check)

    def step_mode_switch_ignores_inflight(self):
        script = """
          window.__markitdownEmit('tauri://drag-drop', {payload: {paths: ['/data/late.csv']}});
        """
        self.run_js(script, lambda: self.later(self.switch_during_flight))

    def switch_during_flight(self):
        pending = [call for call in self.waiting if call["command"] == "save_conversion" and call["args"].get("source") == "/data/late.csv"]
        if not pending:
            self.fail(f"in-flight drop was not recorded: {self.waiting}")
            return
        call = pending[-1]
        script = """
          (function () {
            var explicit = document.querySelector('input[value="explicit"]');
            explicit.checked = true;
            explicit.dispatchEvent(new Event('change'));
          })()
        """
        self.run_js(script, lambda: self.answer_late(call))

    def answer_late(self, call):
        self.answer(call["id"], self.written("/data/late.md"))
        self.later(
            lambda: self.value(
                "({preview: document.getElementById('preview').hidden, status: document.getElementById('status').textContent})",
                lambda got: self.next()
                if got["preview"] is True and got["status"] == ""
                else self.fail(f"mode switch lost to a late response {got}"),
            )
        )

    def step_browse_from_beside_uses_the_chosen_path(self):
        self.arm("/data/from-beside.md")
        self.arm(self.written("/data/from-beside.md"))
        script = """
          (function () {
            var beside = document.querySelector('input[value="beside"]');
            beside.checked = true;
            beside.dispatchEvent(new Event('change'));
            document.getElementById('choose-output').click();
          })()
        """
        self.run_js(
            script,
            lambda: self.wait_until(
                "document.querySelector('input[value=\"explicit\"]').checked && document.getElementById('output-path').value === '/data/from-beside.md' && document.getElementById('status').textContent === 'Saved.'",
                self.assert_beside_browse,
            ),
        )

    def assert_beside_browse(self):
        latest = [call for call in self.calls if call["command"] == "save_conversion"][-1]["args"]
        if latest.get("destination") != "/data/from-beside.md":
            self.fail("browse from beside mode wrote %s" % latest)
            return
        self.next()

    def step_live_beside_then_explicit_then_replace(self):
        if not os.path.isfile(BINARY):
            self.log.append("live skipped, desktop binary not built")
            self.next()
            return
        self.live_dir = tempfile.mkdtemp(prefix="markitdown-ui-")
        source = os.path.join(self.live_dir, "notes.csv")
        with open(source, "wb") as handle:
            handle.write(b"name,city\nAda,London\n")
        self.live = True
        self.live_source = source
        script = """
          (function () {
            var beside = document.querySelector('input[value="beside"]');
            beside.checked = true;
            beside.dispatchEvent(new Event('change'));
            var event = new Event('drop', {cancelable: true});
            Object.defineProperty(event, 'dataTransfer', {value: {
              files: [{path: %s}],
              getData: function () { return ''; }
            }});
            document.getElementById('drop-target').dispatchEvent(event);
          })()
        """ % json.dumps(source)
        self.run_js(script, lambda: self.wait_until(
            "document.getElementById('status').textContent === 'Saved.'",
            self.assert_live_beside,
            tries=80,
        ))

    def assert_live_beside(self):
        beside = os.path.join(self.live_dir, "notes.md")
        body = open(beside, encoding="utf-8").read()
        self.value(
            "document.getElementById('preview').textContent",
            lambda preview: self.live_explicit()
            if preview == body and "Ada" in body and "London" in body
            else self.fail("live beside preview %r file %r" % (preview, body)),
        )

    def live_explicit(self):
        os.remove(os.path.join(self.live_dir, "notes.md"))
        os.makedirs(os.path.join(self.live_dir, "out"))
        dest = os.path.join(self.live_dir, "out", "chosen.md")
        self.live_dest = dest
        script = """
          (function () {
            var explicit = document.querySelector('input[value="explicit"]');
            explicit.checked = true;
            explicit.dispatchEvent(new Event('change'));
            document.getElementById('output-path').value = %s;
            document.getElementById('convert').click();
          })()
        """ % json.dumps(dest)
        self.run_js(script, lambda: self.wait_until(
            "document.getElementById('status').textContent === 'Saved.'",
            self.assert_live_explicit,
            tries=80,
        ))

    def assert_live_explicit(self):
        beside = os.path.join(self.live_dir, "notes.md")
        if os.path.exists(beside):
            self.fail("explicit live save also wrote beside the source")
            return
        body = open(self.live_dest, encoding="utf-8").read()
        self.value(
            "document.getElementById('preview').textContent",
            lambda preview: self.live_replace(body)
            if preview == body
            else self.fail("live explicit preview %r file %r" % (preview, body)),
        )

    def live_replace(self, body):
        with open(self.live_dest, "w", encoding="utf-8") as handle:
            handle.write("OLD")
        self.live_body = body
        self.run_js(
            "document.getElementById('convert').click()",
            lambda: self.wait_until(
                "document.getElementById('confirm-replace').hidden === false",
                self.assert_live_held,
                tries=80,
            ),
        )

    def assert_live_held(self):
        if open(self.live_dest, encoding="utf-8").read() != "OLD":
            self.fail("confirmation wrote the file early")
            return
        self.value(
            "document.getElementById('preview').textContent",
            lambda preview: self.click_live_replace()
            if preview == self.live_body
            else self.fail("confirmation hid the pending markdown %r" % preview),
        )

    def click_live_replace(self):
        self.run_js(
            "document.getElementById('confirm-replace').click()",
            lambda: self.wait_until(
                "document.getElementById('status').textContent === 'Saved.'",
                self.assert_live_replaced,
                tries=80,
            ),
        )

    def assert_live_replaced(self):
        body = open(self.live_dest, encoding="utf-8").read()
        if body != self.live_body:
            self.fail("replace left %r" % body)
            return
        names = os.listdir(os.path.dirname(self.live_dest))
        if any("markitdown-bak" in name or "markitdown-tmp" in name for name in names):
            self.fail("replace left scratch files %s" % names)
            return
        self.live = False
        self.next()

    def step_tar_suggestion(self):
        before = len([call for call in self.calls if call["command"] == "save_conversion"])
        script = """
          (function () {
            document.getElementById('output-path').value = '';
            var event = new Event('drop', {cancelable: true});
            Object.defineProperty(event, 'dataTransfer', {value: {
              files: [{path: 'D:\\\\Docs\\\\archive.tar.gz'}],
              getData: function () { return ''; }
            }});
            document.getElementById('drop-target').dispatchEvent(event);
          })()
        """
        self.run_js(
            script,
            lambda: self.later(
                lambda: self.value(
                    "document.getElementById('output-path').value",
                    lambda value: self.next()
                    if value == "D:\\Docs\\archive.tar.md"
                    and len([call for call in self.calls if call["command"] == "save_conversion"]) == before
                    else self.fail(f"tar suggestion {value!r}"),
                )
            ),
        )


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Fail as err:
        print(err, file=sys.stderr)
        sys.exit(1)
