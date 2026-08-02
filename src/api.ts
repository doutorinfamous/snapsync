import { invoke } from "@tauri-apps/api/core";
import type {
  AppConfig,
  AppSnapshot,
  DirectConnectRequest,
  DiscoveredPrinter,
  PrinterConfig,
  SyncSummary,
} from "./types";

export const backend = {
  snapshot: () => invoke<AppSnapshot>("get_snapshot"),
  discover: () => invoke<DiscoveredPrinter[]>("discover_printers"),
  connectDirect: (request: DirectConnectRequest) =>
    invoke<PrinterConfig>("connect_direct", { request }),
  saveConfig: (config: AppConfig) =>
    invoke<AppConfig>("save_config", { config }),
  pauseCurrent: () => invoke<void>("pause_current_sync"),
  syncNow: () => invoke<SyncSummary>("sync_now"),
  verifyPrinter: () => invoke<void>("verify_printer"),
  forgetPrinter: () => invoke<void>("forget_printer"),
  clearHistory: () => invoke<void>("clear_history"),
};
