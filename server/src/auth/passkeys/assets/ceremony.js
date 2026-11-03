// Kaiku desktop passkey ceremony (C29 step 3). No dependencies.
// options -> navigator.credentials.get -> form POST of the credential to
// .../finish; "Back to the app" posts .../cancel. The server answers both
// with the hand-off page for the app's loopback listener.
(function () {
  "use strict";
  var base = location.pathname.replace(/\/+$/, "");
  var statusEl = document.getElementById("status");
  var startBtn = document.getElementById("start");
  var finishForm = document.getElementById("finish");

  function b64uToBuf(s) {
    var b = s.replace(/-/g, "+").replace(/_/g, "/");
    while (b.length % 4) b += "=";
    var bin = atob(b);
    var out = new Uint8Array(bin.length);
    for (var i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out.buffer;
  }
  function bufToB64u(buf) {
    var bytes = new Uint8Array(buf);
    var bin = "";
    for (var i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
    return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  }
  function requestOptions(json) {
    if (window.PublicKeyCredential && PublicKeyCredential.parseRequestOptionsFromJSON) {
      return PublicKeyCredential.parseRequestOptionsFromJSON(json);
    }
    var o = Object.assign({}, json);
    o.challenge = b64uToBuf(json.challenge);
    o.allowCredentials = (json.allowCredentials || []).map(function (c) {
      return Object.assign({}, c, { id: b64uToBuf(c.id) });
    });
    return o;
  }
  function credentialJSON(cred) {
    if (typeof cred.toJSON === "function") {
      try { return cred.toJSON(); } catch (e) { /* fall through */ }
    }
    var r = cred.response;
    return {
      id: cred.id,
      rawId: bufToB64u(cred.rawId),
      type: cred.type,
      authenticatorAttachment: cred.authenticatorAttachment || null,
      clientExtensionResults: cred.getClientExtensionResults ? cred.getClientExtensionResults() : {},
      response: {
        clientDataJSON: bufToB64u(r.clientDataJSON),
        authenticatorData: bufToB64u(r.authenticatorData),
        signature: bufToB64u(r.signature),
        userHandle: r.userHandle ? bufToB64u(r.userHandle) : null
      }
    };
  }
  function show(id) { var el = document.getElementById(id); if (el) el.hidden = false; }
  function setStatus(text) { statusEl.textContent = text || ""; }

  var ua = navigator.userAgent || "";
  if (ua.indexOf("Linux") !== -1 && ua.indexOf("Android") === -1) {
    show(ua.indexOf("Firefox") !== -1 ? "linux-firefox" : "linux-other");
  }
  if (!window.PublicKeyCredential || !navigator.credentials) {
    show("unsupported");
    startBtn.hidden = true;
    return;
  }

  function run() {
    startBtn.disabled = true;
    setStatus(statusEl.getAttribute("data-working"));
    fetch(base + "/options", { method: "POST", headers: { "Content-Type": "application/json" }, body: "{}", credentials: "omit" })
      .then(function (res) {
        // An ended flow: show the server's "expired" page once (no loop:
        // the server ends a flow that cannot continue).
        if (res.status === 400 || res.status === 404) { location.reload(); throw new Error("options"); }
        if (!res.ok) throw new Error("retry");
        return res.json();
      })
      .then(function (body) {
        finishForm.elements.ceremony_id.value = body.ceremony_id || "";
        return navigator.credentials.get({ publicKey: requestOptions(body.options.publicKey) });
      })
      .then(function (cred) {
        finishForm.elements.credential.value = JSON.stringify(credentialJSON(cred));
        finishForm.submit();
      })
      .catch(function (err) {
        if (err && err.message === "options") return;
        startBtn.disabled = false;
        startBtn.textContent = startBtn.getAttribute("data-retry");
        setStatus(statusEl.getAttribute("data-cancelled"));
      });
  }
  startBtn.addEventListener("click", run);
})();
