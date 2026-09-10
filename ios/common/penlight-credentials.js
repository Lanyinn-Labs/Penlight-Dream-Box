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
  if (!headers) return "";
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

function isStashTile() {
  return (
    typeof $script !== "undefined" &&
    $script &&
    $script.type === "tile"
  );
}

function notify(title, subtitle, body, clipboardText) {
  // Quantumult X supports updating the pasteboard after the notification is
  // tapped. The fourth argument is ignored by older QX builds.
  if (typeof $notify === "function") {
    try {
      $notify(title, subtitle, body, {
        "update-pasteboard": clipboardText || ""
      });
      return;
    } catch (_) {
      try {
        $notify(title, subtitle, body);
        return;
      } catch (_) {}
    }
  }

  if (
    typeof $notification !== "undefined" &&
    $notification &&
    typeof $notification.post === "function"
  ) {
    // These are best-effort clipboard options. Some clients silently ignore
    // unsupported options, so the notification must also contain full JSON.
    var attach = {
      action: "clipboard",
      text: clipboardText || "",
      clipboard: clipboardText || ""
    };
    try {
      $notification.post(title, subtitle, body, attach);
      return;
    } catch (_) {
      try {
        $notification.post(title, subtitle, body, {
          clipboard: clipboardText || ""
        });
        return;
      } catch (_) {
        try {
          $notification.post(title, subtitle, body);
        } catch (_) {}
      }
    }
  }
}

function show(record) {
  var text = savedJson(record);
  if (!record.uid && !record.uuid) {
    if (isStashTile()) {
      finish({
        title: "Penlight UID / UUID",
        content: "还没有捕获到 UID / UUID",
        icon: "exclamationmark.circle",
        backgroundColor: "#8e8e93"
      });
    } else {
      notify(
        "Penlight Dream Box",
        "还没有捕获到 UID / UUID",
        "请打开游戏并进入个人资料或其他会访问 api.garupa.jp 的页面。",
        ""
      );
      finish();
    }
    return;
  }

  if (isStashTile()) {
    finish({
      title: "Penlight UID / UUID",
      content: text,
      icon: "doc.on.clipboard",
      backgroundColor: "#1d1d1f"
    });
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
  finish();
}

var argument =
  typeof $argument !== "undefined" && typeof $argument === "string"
    ? $argument
    : "";

if (argument === "show" || typeof $request === "undefined") {
  show(readSaved());
} else {
  var request = typeof $request !== "undefined" ? $request : null;
  var uid = uidFromUrl(request && request.url);
  var uuid = headerValue(request && request.headers, "X-Signature");
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
