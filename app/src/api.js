import { invoke } from "@tauri-apps/api/core";

// Rust returns Err(String): Tauri rejects with that string. Normalise to Error objects.
const call = (cmd, args) =>
  invoke(cmd, args).catch((e) => {
    throw new Error(typeof e === "string" ? e : e && e.message ? e.message : String(e));
  });

export const api = {
  info: () => call("get_app_info"),
  savePrefs: (prefs) => call("save_prefs", { prefs }),
  lockInfo: () => call("lock_info"),
  detectKeys: () => call("detect_keys"),
  initArchive: (usb) => call("init_archive", { usb }),
  resetArchive: (usb, deleteOld, replaceKeyFile, confirm) => call("reset_archive", { usb, deleteOld, replaceKeyFile, confirm }),
  unlock: (files) => call("unlock", { files }),
  lock: () => call("lock_archive"),
  keyPresent: () => call("key_present"),
  integrity: () => call("integrity_info"),
  recheck: () => call("recheck"),
  summary: (fromMs, toMs) => call("summary", { fromMs, toMs }),
  timeline: (fromMs, toMs) => call("timeline", { fromMs, toMs }),
  focusList: (fromMs, toMs, app, limit) => call("focus_list", { fromMs, toMs, app: app ?? null, limit }),
  appDetail: (app, fromMs, toMs) => call("app_detail", { app, fromMs, toMs }),
  heatmap: (fromMs, toMs) => call("heatmap", { fromMs, toMs }),
  explore: (filter, offset, limit, newestFirst) => call("explore", { filter, offset, limit, newestFirst }),
  exportEvents: (filter, path, format) => call("export_events", { filter, path, format }),
  collectorInfo: () => call("collector_info"),
  collectorControl: (action) => call("collector_control", { action }),
  settingsGet: () => call("collection_settings_get"),
  settingsSet: (settings) => call("collection_settings_set", { settings }),
  autostart: (on) => call("autostart_set", { on }),
  openFolder: () => call("open_data_folder"),
};
