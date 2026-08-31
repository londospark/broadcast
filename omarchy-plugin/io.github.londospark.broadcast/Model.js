.pragma library

// Pure helpers shared by BarWidget.qml and Panel.qml — keeping JSON
// parsing and display formatting out of the QML property bindings so
// a malformed `broadcast-ctl` payload can't throw inside a binding.

function parseStatus(raw) {
  try {
    var s = JSON.parse(raw)
    if (!s || typeof s !== "object") return null
    return s
  } catch (e) {
    return null
  }
}

function parseApps(raw) {
  try {
    var a = JSON.parse(raw)
    return Array.isArray(a) ? a : []
  } catch (e) {
    return []
  }
}

function backendLabel(backend) {
  return backend === "maxine" ? "Maxine (GPU)" : "DeepFilterNet (CPU)"
}

function healthLabel(status) {
  if (!status) return "Unknown"
  if (!status.active) return "Off"
  return status.health === "ok" ? "Healthy" : "Degraded"
}

function parseHyprClients(raw) {
  try {
    var arr = JSON.parse(raw)
    return Array.isArray(arr) ? arr : []
  } catch (e) {
    return []
  }
}

// Chromium-based browsers run every window's audio through one shared
// process with identical PipeWire metadata ("Brave"/"Playback" for all of
// them), so there's no property to key routing or labeling on per-window.
// Hyprland's window list is the closest thing to real, distinguishing
// titles — the omarchy webapp windows even get their own window class
// (e.g. "brave-music.youtube.com__-Default") separate from the plain
// browser's "brave-browser". This is still a best-effort *count-based*
// pairing (nothing ties a specific audio stream to a specific window), so
// it's only used when the number of matching windows equals the number of
// same-named streams — otherwise callers should fall back to a generic
// label rather than show a title that might belong to the wrong stream.
function browserWindowTitles(binary, clients) {
  var key = String(binary || "").toLowerCase()
  if (!key) return []
  var list = Array.isArray(clients) ? clients : []
  var titles = []
  for (var i = 0; i < list.length; i++) {
    var c = list[i]
    if (!c) continue
    var cls = String(c["class"] || "").toLowerCase()
    if (cls.indexOf(key) === -1) continue
    var title = String(c.title || "").trim()
    if (title) titles.push(title)
  }
  return titles
}
