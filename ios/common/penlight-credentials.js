/*
 * Penlight Dream Box / portable UID + UUID helper.
 *
 * Compatible with Loon, Surge, Stash, Shadowrocket and Quantumult X.
 * It reads only the matched request URL and the X-Signature header. Values
 * stay in the proxy app's local persistent store; game traffic is untouched.
 */

var STORE_KEY = "penlight-dream-box.uid-uuid.v1";

function finish(result) {
  if (typeof $done === "function") $done(result || {});
}

function hasSurgeVersion() {
  return (
    typeof $environment !== "undefined" &&
    $environment &&
    typeof $environment["surge-version"] !== "undefined"
  );
}

function hasStashVersion() {
  return (
    typeof $environment !== "undefined" &&
    $environment &&
    typeof $environment["stash-version"] !== "undefined"
  );
}

function isStashTile() {
  return (
    typeof $script !== "undefined" &&
    $script &&
    $script.type === "tile"
  );
}

function isLoonGeneric() {
  return (
    typeof $loon !== "undefined" &&
    (typeof $request === "undefined" || !$request || !$request.url)
  );
}

function readStoredValue() {
  if (
    typeof $prefs !== "undefined" &&
    $prefs &&
    typeof $prefs.valueForKey === "function"
  ) {
    return $prefs.valueForKey(STORE_KEY) || "";
  }
  if (
    typeof $persistentStore !== "undefined" &&
    $persistentStore &&
    typeof $persistentStore.read === "function"
  ) {
    return $persistentStore.read(STORE_KEY) || "";
  }
  return "";
}

function writeStoredValue(value) {
  if (
    typeof $prefs !== "undefined" &&
    $prefs &&
    typeof $prefs.setValueForKey === "function"
  ) {
    return $prefs.setValueForKey(value, STORE_KEY);
  }
  if (
    typeof $persistentStore !== "undefined" &&
    $persistentStore &&
    typeof $persistentStore.write === "function"
  ) {
    return $persistentStore.write(value, STORE_KEY);
  }
  return false;
}

function readSaved() {
  var raw = readStoredValue();
  if (!raw) return {};
  try {
    var value = JSON.parse(raw);
    return value && typeof value === "object" ? value : {};
  } catch (_) {
    return {};
  }
}

function headerValue(headers, wantedName) {
  if (!headers || typeof headers !== "object") return "";
  var wanted = wantedName.toLowerCase();
  for (var name in headers) {
    if (!Object.prototype.hasOwnProperty.call(headers, name)) continue;
    if (name.toLowerCase() !== wanted) continue;
    var value = headers[name];
    if (Array.isArray(value)) value = value[0];
    return value == null ? "" : String(value).trim();
  }
  return "";
}

function uidFromUrl(url) {
  var match = String(url || "").match(
    /\/api\/user\/([0-9]+)(?:[\/?]|$)/i
  );
  return match ? match[1] : "";
}

function savedJson(record) {
  return JSON.stringify(
    { uid: record.uid || "", uuid: record.uuid || "" },
    null,
    2
  );
}

function postNotification(title, subtitle, body, options) {
  if (options) {
    try {
      $notification.post(title, subtitle, body, options);
      return;
    } catch (_) {}
  }
  try {
    $notification.post(title, subtitle, body);
  } catch (_) {}
}

function notify(title, subtitle, body, clipboardText) {
  var text = clipboardText ? String(clipboardText) : "";

  // Quantumult X supports updating the pasteboard after the notification is
  // tapped. Omit the fourth argument when empty or on older QX builds.
  if (typeof $notify === "function") {
    if (text) {
      try {
        $notify(title, subtitle, body, {
          "update-pasteboard": text
        });
        return;
      } catch (_) {}
    }
    try {
      $notify(title, subtitle, body);
    } catch (_) {}
    return;
  }

  if (
    typeof $notification !== "undefined" &&
    $notification &&
    typeof $notification.post === "function"
  ) {
    // Pass only the attachment keys documented by each client so strict native
    // bridges do not drop notifications or fail with empty clipboard strings.
    if (!text || hasStashVersion()) {
      postNotification(title, subtitle, body, null);
      return;
    }

    if (hasSurgeVersion()) {
      postNotification(title, subtitle, body, {
        action: "clipboard",
        text: text
      });
      return;
    }

    if (typeof $loon !== "undefined") {
      postNotification(title, subtitle, body, {
        clipboard: text
      });
      return;
    }

    // Shadowrocket and generic clients: keep best-effort options while falling
    // back cleanly if a client rejects unknown attachment keys.
    try {
      $notification.post(title, subtitle, body, {
        action: "clipboard",
        text: text,
        clipboard: text
      });
      return;
    } catch (_) {
      postNotification(title, subtitle, body, {
        clipboard: text
      });
    }
  }
}

function show(record) {
  var text = savedJson(record);
  var hasRecord = Boolean(record.uid || record.uuid);

  if (isStashTile()) {
    finish(
      hasRecord
        ? {
            title: "Penlight UID / UUID",
            content: text,
            icon: "doc.on.clipboard",
            backgroundColor: "#1d1d1f"
          }
        : {
            title: "Penlight UID / UUID",
            content: "还没有捕获到 UID / UUID",
            icon: "exclamationmark.circle",
            backgroundColor: "#8e8e93"
          }
    );
    return;
  }

  if (!hasRecord) {
    notify(
      "Penlight Dream Box",
      "还没有捕获到 UID / UUID",
      "请打开游戏并进入个人资料或其他会访问 api.garupa.jp 的页面。",
      ""
    );
    finish(
      isLoonGeneric()
        ? {
            title: "Penlight UID / UUID",
            content: "还没有捕获到 UID / UUID"
          }
        : {}
    );
    return;
  }

  // Explicit manual export only: do not log credentials on game requests.
  if (typeof console !== "undefined" && typeof console.log === "function") {
    console.log(text);
  }
  notify(
    "Penlight Dream Box",
    "完整 JSON（也已输出到本次脚本日志）",
    text,
    text
  );
  finish(
    isLoonGeneric()
      ? {
          title: "Penlight UID / UUID",
          content: text
        }
      : {}
  );
}

function normalizeArgument(value) {
  if (typeof value === "string") {
    var trimmed = value.trim().replace(/^(['"])(.*)\1$/, "$2").trim();
    if (trimmed === "show" || trimmed === "capture") return trimmed;
    if (trimmed.charAt(0) === "{") {
      try {
        return normalizeArgument(JSON.parse(trimmed));
      } catch (_) {}
    }
    return trimmed;
  }
  if (value && typeof value === "object") {
    if (value.show === true) return "show";
    var candidate = value.mode || value.action || value.argument;
    if (typeof candidate === "string") return normalizeArgument(candidate);
  }
  return "";
}

var argument =
  typeof $argument !== "undefined" ? normalizeArgument($argument) : "";
var request = typeof $request !== "undefined" ? $request : null;

if (argument === "show" || !request || !request.url) {
  show(readSaved());
} else {
  var uid = uidFromUrl(request.url);
  var uuid = headerValue(request.headers, "X-Signature");
  // Only save a pair observed on the same request. Never combine accounts.
  if (!uid || !uuid) {
    finish();
  } else {
    var current = { uid: uid, uuid: uuid };
    writeStoredValue(JSON.stringify(current));
    // A matching request is a fresh opportunity to export, even for the same
    // account. Persistent value-based deduplication made repeat entry silent.
    notify(
      "Penlight Dream Box",
      "已捕获完整 JSON（展开查看；复制取决于客户端支持）",
      savedJson(current),
      savedJson(current)
    );
    finish();
  }
}
