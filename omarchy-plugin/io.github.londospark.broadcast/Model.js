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

// Broadcast routes per app, not per window/tab — an app can have several
// audio streams open at once (e.g. several browser tabs), and they're
// always routed together as one unit. Collapse the stream list down to
// one row per binary for display, so the panel shows one simple toggle
// per app rather than one row per ephemeral stream.
function dedupeAppsByBinary(apps) {
  var list = Array.isArray(apps) ? apps : []
  var seen = {}
  var out = []
  for (var i = 0; i < list.length; i++) {
    var a = list[i]
    if (!a) continue
    var key = String(a.binary || a.name || "").toLowerCase()
    if (!key || seen[key]) continue
    seen[key] = true
    out.push(a)
  }
  return out
}
