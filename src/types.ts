export type ScheduleUnit = "hours" | "days";

export interface PrinterConfig {
  id: string;
  name: string;
  host: string;
  http_port: number;
  sn: string;
  machine_type: string;
  paired: boolean;
}

export interface AppConfig {
  version: number;
  printer: PrinterConfig | null;
  destination: string;
  schedule_enabled: boolean;
  interval_value: number;
  interval_unit: ScheduleUnit;
  download_thumbnails: boolean;
  autostart: boolean;
  run_in_background: boolean;
}

export interface DiscoveredPrinter {
  id: string;
  name: string;
  host: string;
  port: number;
  sn: string;
  machine_type: string;
  device_name: string;
  link_mode: string;
  region: string;
}

export interface SyncStatus {
  phase: string;
  running: boolean;
  last_sync: string | null;
  next_sync: string | null;
  current_file: string | null;
  progress_percent: number;
  downloaded: number;
  skipped: number;
  failed: number;
  last_error: string | null;
}

export type HistoryResult = "downloaded" | "already_present" | "failed";

export interface HistoryEntry {
  id: string;
  remote_key: string;
  printer_sn: string;
  gcode_name: string;
  remote_path: string;
  local_path: string;
  size: number;
  completed_at: string;
  result: HistoryResult;
  remote_deleted: boolean;
  message: string;
}

export interface AppSnapshot {
  config: AppConfig;
  status: SyncStatus;
  history: HistoryEntry[];
}

export interface SyncSummary {
  downloaded: number;
  skipped: number;
  failed: number;
}

export interface DirectConnectRequest {
  host: string;
  name?: string;
  http_port?: number;
}
