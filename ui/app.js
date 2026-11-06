(function () {
  var state = {
    source: "",
    markdown: "",
  };

  function installDropTarget() {
    var drop = document.getElementById("drop-target");
    if (!drop) {
      drop = document.createElement("div");
      drop.id = "drop-target";
      drop.tabIndex = 0;
      drop.setAttribute("role", "button");
      var parent = document.body || document.documentElement;
      if (parent) parent.appendChild(drop);
    }
    if (drop.getAttribute("data-installed") === "1") return drop;
    drop.setAttribute("data-installed", "1");
    drop.addEventListener("dragover", function (event) {
      event.preventDefault();
      drop.classList.add("dragover");
    });
    drop.addEventListener("dragleave", function () {
      drop.classList.remove("dragover");
    });
    drop.addEventListener("drop", function (event) {
      event.preventDefault();
      drop.classList.remove("dragover");
      var path = pathFromBrowserDrop(event);
      if (path) beginConvert(path, false);
    });
    drop.addEventListener("click", function () {
      chooseSource();
    });
    drop.addEventListener("keydown", function (event) {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        chooseSource();
      }
    });
    window.__markitdownDropReady = true;
    return drop;
  }

  function $(id) {
    return document.getElementById(id);
  }

  function mode() {
    var selected = document.querySelector('input[name="mode"]:checked');
    return selected && selected.value === "explicit" ? "explicit" : "beside";
  }

  function outputPath() {
    var input = $("output-path");
    return input ? input.value.trim() : "";
  }

  function setStatus(text, kind) {
    var status = $("status");
    if (!status) return;
    status.textContent = text || "";
    status.className = kind || "";
  }

  function showPreview(markdown, path) {
    state.markdown = markdown || "";
    var preview = $("preview");
    var copy = $("copy");
    var written = $("written-path");
    if (preview) {
      preview.hidden = !markdown;
      preview.textContent = markdown || "";
    }
    if (copy) copy.hidden = !markdown;
    if (written) written.textContent = path ? "Wrote " + path : "";
  }

  function clearResult() {
    showPreview("", "");
    var confirm = $("confirm-replace");
    if (confirm) confirm.hidden = true;
  }

  function suggestOutput(source) {
    var input = $("output-path");
    if (!input || input.value.trim() || !source) return;
    var slash = Math.max(source.lastIndexOf("/"), source.lastIndexOf("\\"));
    var base = slash >= 0 ? source.slice(slash + 1) : source;
    var dot = base.lastIndexOf(".");
    var stem = dot > 0 ? base.slice(0, dot) : base;
    var dir = slash >= 0 ? source.slice(0, slash + 1) : "";
    input.value = dir + (stem || "output") + ".md";
  }

  function syncOutputRow() {
    var row = $("output-row");
    if (!row) return;
    if (mode() === "explicit") row.classList.add("visible");
    else row.classList.remove("visible");
  }

  function tauri() {
    return window.__TAURI__ && window.__TAURI__.core ? window.__TAURI__ : null;
  }

  function invoke(command, args) {
    var bridge = tauri();
    if (!bridge) return Promise.reject(new Error("Desktop bridge is unavailable"));
    return bridge.core.invoke(command, args);
  }

  function beginConvert(source, replaceExisting) {
    if (!source) {
      setStatus("Choose a file first.", "error");
      return;
    }
    state.source = source;
    var label = $("source-label");
    if (label) label.textContent = "Source: " + source;
    var destination = null;
    if (mode() === "explicit") {
      destination = outputPath();
      if (!destination) {
        suggestOutput(source);
        clearResult();
        setStatus("Choose an output path before saving.", "error");
        return;
      }
    }
    clearResult();
    setStatus("Converting…", "");
    invoke("save_conversion", {
      source: source,
      destination: destination,
      replaceExisting: !!replaceExisting,
    }).then(function (response) {
      if (!response || response.status === "failed") {
        setStatus((response && response.message) || "Conversion failed.", "error");
        return;
      }
      if (response.status === "needsConfirmation") {
        setStatus(response.message || "That file already exists.", "error");
        var confirm = $("confirm-replace");
        if (confirm) confirm.hidden = false;
        return;
      }
      if (response.status === "written") {
        showPreview(response.markdown || "", response.path || "");
        setStatus("Saved.", "ok");
        return;
      }
      setStatus("Unexpected response from the converter.", "error");
    }, function (err) {
      var message = err && err.message ? err.message : String(err);
      setStatus(message, "error");
    });
  }

  function pathFromBrowserDrop(event) {
    var transfer = event.dataTransfer;
    if (!transfer) return "";
    if (transfer.files && transfer.files.length) {
      var file = transfer.files[0];
      if (file && typeof file.path === "string" && file.path) return file.path;
    }
    if (transfer.getData) {
      var uriList = transfer.getData("text/uri-list") || transfer.getData("text/plain");
      var lines = String(uriList).split(/\r?\n/);
      for (var i = 0; i < lines.length; i++) {
        var line = lines[i].trim();
        if (!line || line.charAt(0) === "#") continue;
        if (line.indexOf("file://") === 0) return fileUrlToPath(line);
      }
    }
    return "";
  }

  function fileUrlToPath(url) {
    var path = url.replace(/^file:\/\//, "");
    if (path.charAt(0) !== "/" && path.charAt(1) === ":") {
      // Windows file URL without a leading slash is already a drive path.
    } else if (/^\/[A-Za-z]:/.test(path)) {
      path = path.slice(1);
    }
    try {
      path = decodeURIComponent(path);
    } catch (err) {
      void err;
    }
    return path;
  }

  function takeFirstPath(paths) {
    if (!paths || !paths.length) return "";
    if (paths.length > 1) {
      setStatus("Using the first file. Drop one file at a time.", "");
    }
    return paths[0];
  }

  function listenForNativeDrop(drop) {
    var bridge = tauri();
    if (!bridge || !bridge.event || !bridge.event.listen) return;
    bridge.event.listen("tauri://drag-enter", function () {
      drop.classList.add("dragover");
    });
    bridge.event.listen("tauri://drag-leave", function () {
      drop.classList.remove("dragover");
    });
    bridge.event.listen("tauri://drag-drop", function (event) {
      drop.classList.remove("dragover");
      var payload = event && event.payload;
      var paths = [];
      if (payload && Array.isArray(payload.paths)) paths = payload.paths;
      else if (Array.isArray(payload)) paths = payload;
      var first = takeFirstPath(paths.filter(function (path) { return typeof path === "string"; }));
      if (first) beginConvert(first, false);
    });
  }

  function chooseSource() {
    var bridge = tauri();
    if (bridge) {
      invoke("plugin:dialog|open", {
        options: { multiple: false, directory: false, title: "Choose a file to convert" },
      }).then(function (picked) {
        if (typeof picked === "string" && picked) beginConvert(picked, false);
      }, function (err) {
        setStatus(err && err.message ? err.message : String(err), "error");
      });
      return;
    }
    var input = $("file-input");
    if (input) input.click();
  }

  function chooseOutput() {
    var bridge = tauri();
    var suggested = outputPath();
    if (!bridge) {
      var input = $("output-path");
      if (input) input.focus();
      return;
    }
    invoke("plugin:dialog|save", {
      options: {
        title: "Save Markdown",
        defaultPath: suggested || undefined,
        filters: [{ name: "Markdown", extensions: ["md"] }],
      },
    }).then(function (picked) {
      if (typeof picked !== "string" || !picked) return;
      var field = $("output-path");
      if (field) field.value = picked;
      if (state.source) beginConvert(state.source, false);
    }, function (err) {
      setStatus(err && err.message ? err.message : String(err), "error");
    });
  }

  function copyMarkdown() {
    var text = state.markdown;
    if (!text) return;
    var done = function () { setStatus("Copied Markdown.", "ok"); };
    var failed = function () { setStatus("Could not copy Markdown.", "error"); };
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(done, function () {
        if (copyWithSelection(text)) done();
        else failed();
      });
      return;
    }
    if (copyWithSelection(text)) done();
    else failed();
  }

  function copyWithSelection(text) {
    var area = document.createElement("textarea");
    area.value = text;
    document.body.appendChild(area);
    area.select();
    var ok = false;
    try {
      ok = document.execCommand("copy");
    } catch (err) {
      ok = false;
    }
    area.remove();
    return ok;
  }

  var drop = installDropTarget();
  syncOutputRow();
  listenForNativeDrop(drop);

  document.querySelectorAll('input[name="mode"]').forEach(function (input) {
    input.addEventListener("change", function () {
      syncOutputRow();
      clearResult();
      setStatus("", "");
    });
  });

  var chooseFile = $("choose-file");
  if (chooseFile) chooseFile.addEventListener("click", chooseSource);
  var convert = $("convert");
  if (convert) {
    convert.addEventListener("click", function () {
      beginConvert(state.source, false);
    });
  }
  var chooseOutputButton = $("choose-output");
  if (chooseOutputButton) chooseOutputButton.addEventListener("click", chooseOutput);
  var copy = $("copy");
  if (copy) copy.addEventListener("click", copyMarkdown);
  var confirmReplace = $("confirm-replace");
  if (confirmReplace) {
    confirmReplace.addEventListener("click", function () {
      beginConvert(state.source, true);
    });
  }
  var fileInput = $("file-input");
  if (fileInput) {
    fileInput.addEventListener("change", function () {
      var file = fileInput.files && fileInput.files[0];
      if (file && typeof file.path === "string" && file.path) beginConvert(file.path, false);
      else setStatus("Choose a file from the desktop dialog so the path is available.", "error");
      fileInput.value = "";
    });
  }
})();
