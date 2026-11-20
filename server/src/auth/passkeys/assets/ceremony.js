// Kaiku desktop passkey ceremony (C29 step 3). No dependencies.
// options -> navigator.credentials.get -> form POST of the credential to
// .../finish; "Back to the app" posts .../cancel. The server answers both
// with the hand-off page for the app's loopback listener.
// "Set up this PC" pages (spec Step 5): the keypad posts the phone's code to
// .../code (the answer sets the browser-secret cookie), then the passkey
// step runs the same ceremony with navigator.credentials.create. Both send
// that cookie (same-origin); sign-in and step-up pages send none.
(function () {
  "use strict";
  var base = location.pathname.replace(/\/+$/, "");
  var statusEl = document.getElementById("status");
  var startBtn = document.getElementById("start");
  var finishForm = document.getElementById("finish");
  var codeForm = document.getElementById("code-form");
  var create = !!startBtn && startBtn.getAttribute("data-ceremony") === "create";

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
  function creationOptions(json) {
    if (window.PublicKeyCredential && PublicKeyCredential.parseCreationOptionsFromJSON) {
      return PublicKeyCredential.parseCreationOptionsFromJSON(json);
    }
    var o = Object.assign({}, json);
    o.challenge = b64uToBuf(json.challenge);
    o.user = Object.assign({}, json.user, { id: b64uToBuf(json.user.id) });
    o.excludeCredentials = (json.excludeCredentials || []).map(function (c) {
      return Object.assign({}, c, { id: b64uToBuf(c.id) });
    });
    return o;
  }
  function credentialJSON(cred) {
    if (typeof cred.toJSON === "function") {
      try { return cred.toJSON(); } catch (e) { /* fall through */ }
    }
    var r = cred.response;
    var response;
    if (r.attestationObject) {
      response = {
        clientDataJSON: bufToB64u(r.clientDataJSON),
        attestationObject: bufToB64u(r.attestationObject)
      };
      if (r.getTransports) response.transports = r.getTransports();
    } else {
      response = {
        clientDataJSON: bufToB64u(r.clientDataJSON),
        authenticatorData: bufToB64u(r.authenticatorData),
        signature: bufToB64u(r.signature),
        userHandle: r.userHandle ? bufToB64u(r.userHandle) : null
      };
    }
    return {
      id: cred.id,
      rawId: bufToB64u(cred.rawId),
      type: cred.type,
      authenticatorAttachment: cred.authenticatorAttachment || null,
      clientExtensionResults: cred.getClientExtensionResults ? cred.getClientExtensionResults() : {},
      response: response
    };
  }
  function show(id) { var el = document.getElementById(id); if (el) el.hidden = false; }
  function setStatus(text) { statusEl.textContent = text || ""; }

  var ua = navigator.userAgent || "";
  if (ua.indexOf("Linux") !== -1 && ua.indexOf("Android") === -1) {
    show(ua.indexOf("Firefox") !== -1 ? "linux-firefox" : "linux-other");
  }
  if (!window.PublicKeyCredential || !navigator.credentials) {
    // Before the keypad too: a code typed here could never be used.
    show("unsupported");
    if (startBtn) startBtn.hidden = true;
    if (codeForm) codeForm.hidden = true;
    return;
  }

  if (codeForm) {
    var input = codeForm.elements.code;
    var submitBtn = codeForm.querySelector("button");
    var say = function (attr) { setStatus(codeForm.getAttribute(attr)); };
    var done = function (attr) { say(attr); codeForm.hidden = true; };
    codeForm.addEventListener("submit", function (ev) {
      ev.preventDefault();
      var code = (input.value || "").replace(/\s+/g, "");
      if (!/^[0-9]{6}$/.test(code)) { say("data-invalid"); return; }
      submitBtn.disabled = true;
      fetch(base + "/code", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ code: code }),
        credentials: "same-origin"
      })
        .then(function (res) {
          return res.json().catch(function () { return {}; }).then(function (body) {
            return { status: res.status, ok: res.ok, body: body };
          });
        })
        .then(function (r) {
          input.value = "";
          submitBtn.disabled = false;
          // The cookie is set: the server now shows the passkey step.
          if (r.ok && r.body.state === "enrolled") { location.reload(); return; }
          if (r.ok && r.body.state === "code") {
            setStatus(codeForm.getAttribute("data-wrong").replace("{n}", String(r.body.tries_left)));
            input.focus();
            return;
          }
          if (r.ok && r.body.state === "failed") { done("data-failed"); return; }
          if (r.status === 400) { say("data-invalid"); return; }
          if (r.status >= 500) { say("data-unavailable"); return; }
          done("data-expired");
        })
        .catch(function () {
          submitBtn.disabled = false;
          say("data-unavailable");
        });
    });
    return;
  }

  function run() {
    startBtn.disabled = true;
    setStatus(statusEl.getAttribute("data-working"));
    fetch(base + "/options", { method: "POST", headers: { "Content-Type": "application/json" }, body: "{}", credentials: create ? "same-origin" : "omit" })
      .then(function (res) {
        // An ended flow: show the server's "expired" page once (no loop:
        // the server ends a flow that cannot continue).
        // A refusal (the flow ended, the attempt ended, the passkey limit): the
        // reloaded page shows it or hands it to the app; a retry here cannot work.
        if (res.status === 400 || res.status === 404 || res.status === 409 || res.status === 410) {
          location.reload();
          throw new Error("options");
        }
        if (!res.ok) throw new Error("retry");
        return res.json();
      })
      .then(function (body) {
        finishForm.elements.ceremony_id.value = body.ceremony_id || "";
        if (create) {
          return navigator.credentials.create({ publicKey: creationOptions(body.options.publicKey) });
        }
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
        // The authenticator already holds a passkey of this account.
        if (err && err.name === "InvalidStateError" && startBtn.getAttribute("data-already")) {
          setStatus(startBtn.getAttribute("data-already"));
          return;
        }
        setStatus(statusEl.getAttribute("data-cancelled"));
      });
  }
  startBtn.addEventListener("click", run);
})();
