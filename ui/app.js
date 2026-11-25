(function () {
  var state = {
    source: "",
    markdown: "",
    confirmed: null,
  };
  var requestSerial = 0;
  var lastDrop = { path: "", at: 0 };

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

  function currentDestination() {
    return mode() === "explicit" ? outputPath() : null;
  }

  function setStatus(text, kind) {
    var status = $("status");
    if (!status) return;
    status.textContent = text || "";
    status.className = kind || "";
  }

  function setNotice(text) {
    var notice = $("notice");
    if (notice) notice.textContent = text || "";
  }

  function errorText(err) {
    if (err && err.message) return err.message;
    return String(err);
  }

  function noteSelection(count) {
    if (count > 1) setNotice("Using the first file. Drop one file at a time.");
    else setNotice("");
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
    state.confirmed = null;
    var confirm = $("confirm-replace");
    if (confirm) confirm.hidden = true;
  }

  function tauri() {
    return window.__TAURI__ && window.__TAURI__.core ? window.__TAURI__ : null;
  }

  function invoke(command, args) {
    var bridge = tauri();
    if (!bridge) return Promise.reject(new Error("Desktop bridge is unavailable"));
    return bridge.core.invoke(command, args);
  }

  // Same last-suffix rule as beside_source_path: archive.tar.gz -> archive.tar.md.
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

  // Returns the save to send, or null after the panel has already explained why it will not save.
  function prepareConversion(source, replaceExisting) {
    if (!source) {
      setStatus("Choose a file first.", "error");
      return null;
    }
    state.source = source;
    var label = $("source-label");
    if (label) label.textContent = "Source: " + source;
    var destination = currentDestination();
    if (mode() === "explicit" && !destination) {
      suggestOutput(source);
      clearResult();
      setStatus("Choose an output path before saving.", "error");
      return null;
    }
    var pending = state.confirmed;
    var allowed = !!(
      replaceExisting &&
      pending &&
      pending.source === source &&
      pending.destination === destination
    );
    clearResult();
    return {
      source: source,
      destination: destination,
      replaceExisting: allowed,
    };
  }

  function applySaveResponse(request, response) {
    if (!response || response.status === "failed") {
      setStatus((response && response.message) || "Conversion failed.", "error");
      return;
    }
    if (response.status === "needsConfirmation") {
      setStatus(response.message || "That file already exists.", "error");
      state.confirmed = { source: request.source, destination: request.destination };
      showPreview(response.markdown || "", "");
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
  }

  function beginConvert(source, replaceExisting) {
    var ticket = ++requestSerial;
    var request = prepareConversion(source, replaceExisting);
    if (!request) return;
    setStatus("Converting…", "");
    invoke("save_conversion", {
      source: request.source,
      destination: request.destination,
      replaceExisting: request.replaceExisting,
    }).then(function (response) {
      if (ticket !== requestSerial) return;
      applySaveResponse(request, response);
    }, function (err) {
      if (ticket !== requestSerial) return;
      setStatus(errorText(err), "error");
    });
  }

  function acceptDrop(path) {
    var now = Date.now();
    if (path === lastDrop.path && now - lastDrop.at < 400) return;
    lastDrop.path = path;
    lastDrop.at = now;
    beginConvert(path, false);
  }

  function reportPathlessDrop() {
    if (!tauri()) {
      setStatus("That drop did not include a file path. Use Choose file.", "error");
      return;
    }
    // A desktop drop also emits tauri://drag-drop with the real path. Wait
    // before reporting a pathless browser drop so that event can win.
    var droppedAt = Date.now();
    var ticket = requestSerial;
    window.setTimeout(function () {
      if (lastDrop.at >= droppedAt || requestSerial !== ticket) return;
      setStatus("That drop did not include a file path. Use Choose file.", "error");
    }, 60);
  }

  function pathFromBrowserDrop(event) {
    var transfer = event.dataTransfer;
    if (!transfer) return "";
    if (transfer.files && transfer.files.length) {
      noteSelection(transfer.files.length);
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
    noteSelection(paths.length);
    return paths[0];
  }

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
      var transfer = event.dataTransfer;
      var path = pathFromBrowserDrop(event);
      if (path) {
        acceptDrop(path);
        return;
      }
      var sawFile = transfer && transfer.files && transfer.files.length;
      if (!sawFile && tauri()) return;
      reportPathlessDrop();
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
      if (first) acceptDrop(first);
      else setStatus("Drop a file to convert.", "error");
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
        setStatus(errorText(err), "error");
      });
      return;
    }
    var input = $("file-input");
    if (input) input.click();
  }

  function useChosenOutput(picked) {
    var explicit = document.querySelector('input[value="explicit"]');
    if (explicit && !explicit.checked) {
      explicit.checked = true;
      explicit.dispatchEvent(new Event("change"));
    }
    var field = $("output-path");
    if (field) field.value = picked;
    if (state.source) beginConvert(state.source, false);
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
      useChosenOutput(picked);
    }, function (err) {
      setStatus(errorText(err), "error");
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

  function bindControls(drop) {
    document.querySelectorAll('input[name="mode"]').forEach(function (input) {
      input.addEventListener("change", function () {
        requestSerial += 1;
        syncOutputRow();
        clearResult();
        setNotice("");
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
    listenForNativeDrop(drop);
  }

  var drop = installDropTarget();
  syncOutputRow();
  bindControls(drop);
})();
