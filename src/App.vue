<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { backend } from "./api";
import type {
  AppConfig,
  AppSnapshot,
  DiscoveredPrinter,
  HistoryEntry,
  SyncStatus,
} from "./types";

const defaultConfig: AppConfig = {
  version: 1,
  printer: null,
  destination: "",
  schedule_enabled: true,
  interval_value: 12,
  interval_unit: "hours",
  download_thumbnails: true,
  autostart: false,
  run_in_background: true,
};

const defaultStatus: SyncStatus = {
  phase: "idle",
  running: false,
  last_sync: null,
  next_sync: null,
  current_file: null,
  progress_percent: 0,
  downloaded: 0,
  skipped: 0,
  failed: 0,
  last_error: null,
};

const config = reactive<AppConfig>({ ...defaultConfig });
const status = ref<SyncStatus>({ ...defaultStatus });
const history = ref<HistoryEntry[]>([]);
const discovered = ref<DiscoveredPrinter[]>([]);
const loading = ref(true);
const discovering = ref(false);
const connecting = ref(false);
const saving = ref(false);
const verifying = ref(false);
const settingsOpen = ref(false);
const settingsTab = ref<"printer" | "general">("printer");
const notice = ref("");
const noticeKind = ref<"success" | "error">("success");
const printerForm = reactive({
  host: "",
  httpPort: 7125,
  name: "",
});

let unlistenStatus: UnlistenFn | undefined;
let unlistenPaired: UnlistenFn | undefined;

const isReady = computed(
  () => Boolean(config.printer?.paired && config.destination),
);
const nextExecutionLabel = computed(() =>
  config.schedule_enabled ? formatDate(status.value.next_sync) : "Paused",
);

function applySnapshot(snapshot: AppSnapshot) {
  Object.assign(config, snapshot.config);
  status.value = snapshot.status;
  history.value = snapshot.history;
  if (snapshot.config.printer) {
    printerForm.host = snapshot.config.printer.host;
    printerForm.name = snapshot.config.printer.name;
    printerForm.httpPort = snapshot.config.printer.http_port;
  }
}

function showNotice(message: string, kind: "success" | "error" = "success") {
  notice.value = message;
  noticeKind.value = kind;
  window.setTimeout(() => {
    if (notice.value === message) notice.value = "";
  }, 6000);
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

async function refresh() {
  applySnapshot(await backend.snapshot());
}

function openSettings(tab: "printer" | "general" = "printer") {
  settingsTab.value = tab;
  settingsOpen.value = true;
}

async function searchPrinters() {
  discovering.value = true;
  try {
    discovered.value = await backend.discover();
    if (!discovered.value.length) {
      showNotice(
        "No U1 printer found. Enter its IP address manually.",
        "error",
      );
    }
  } catch (error) {
    showNotice(errorMessage(error), "error");
  } finally {
    discovering.value = false;
  }
}

function choosePrinter(printer: DiscoveredPrinter) {
  printerForm.host = printer.host;
  printerForm.name = printer.name;
}

async function connectDirect() {
  if (!printerForm.host.trim()) {
    showNotice("Enter the Snapmaker U1 IP address.", "error");
    return;
  }
  connecting.value = true;
  try {
    const printer = await backend.connectDirect({
      host: printerForm.host.trim(),
      name: printerForm.name.trim() || undefined,
      http_port: printerForm.httpPort,
    });
    config.printer = printer;
    showNotice("HTTP access to the Snapmaker U1 confirmed.");
  } catch (error) {
    showNotice(errorMessage(error), "error");
  } finally {
    connecting.value = false;
  }
}

async function selectDestination() {
  const selected = await open({
    directory: true,
    multiple: false,
    title: "Choose where to save timelapses",
  });
  if (typeof selected === "string") config.destination = selected;
}

async function saveSettings() {
  saving.value = true;
  try {
    Object.assign(config, await backend.saveConfig({ ...config }));
    showNotice("Settings saved.");
  } catch (error) {
    showNotice(errorMessage(error), "error");
  } finally {
    saving.value = false;
  }
}

async function syncNow() {
  try {
    const summary = await backend.syncNow();
    await refresh();
    showNotice(
      `Completed: ${summary.downloaded} downloaded, ${summary.skipped} already present, ${summary.failed} failed.`,
      summary.failed ? "error" : "success",
    );
  } catch (error) {
    showNotice(errorMessage(error), "error");
    await refresh();
  }
}

async function pauseCurrent() {
  try {
    await backend.pauseCurrent();
  } catch (error) {
    showNotice(errorMessage(error), "error");
  }
}

async function verifyPrinter() {
  verifying.value = true;
  try {
    await backend.verifyPrinter();
    showNotice("HTTP connection and timelapse directory verified.");
  } catch (error) {
    showNotice(errorMessage(error), "error");
  } finally {
    verifying.value = false;
  }
}

async function forgetPrinter() {
  if (!window.confirm("Remove this printer from SnapSync?")) return;
  try {
    await backend.forgetPrinter();
    config.printer = null;
    printerForm.host = "";
    printerForm.name = "";
    showNotice("Printer removed.");
  } catch (error) {
    showNotice(errorMessage(error), "error");
  }
}

async function clearHistory() {
  if (!window.confirm("Clear local history? Downloaded videos will not be deleted.")) return;
  await backend.clearHistory();
  history.value = [];
}

function formatDate(value: string | null) {
  if (!value) return "—";
  return new Intl.DateTimeFormat("en-US", {
    dateStyle: "short",
    timeStyle: "short",
  }).format(new Date(value));
}

function formatBytes(value: number) {
  if (!value) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const index = Math.min(Math.floor(Math.log(value) / Math.log(1024)), 3);
  return `${(value / 1024 ** index).toFixed(index ? 1 : 0)} ${units[index]}`;
}

function historyLabel(entry: HistoryEntry) {
  if (entry.result === "failed") return "Failed";
  if (entry.result === "already_present") return "Already present";
  return "Downloaded";
}

onMounted(async () => {
  try {
    await refresh();
    unlistenStatus = await listen<SyncStatus>("sync-status", (event) => {
      status.value = event.payload;
    });
    unlistenPaired = await listen("printer-paired", () => refresh());
    if (!config.printer) openSettings("printer");
  } catch (error) {
    showNotice(errorMessage(error), "error");
  } finally {
    loading.value = false;
  }
});

onBeforeUnmount(() => {
  unlistenStatus?.();
  unlistenPaired?.();
});
</script>

<template>
  <div class="app-shell">
    <header class="topbar">
      <div class="brand">
        <div class="brand-mark" aria-hidden="true">
          <span></span><span></span><span></span>
        </div>
        <div>
          <strong>SnapSync</strong>
          <small>Snapmaker U1 timelapses</small>
        </div>
      </div>

      <div class="top-actions">
        <span class="connection-pill" :class="{ active: config.printer?.paired }">
          <i></i>
          {{ config.printer?.paired ? config.printer.name : "No printer" }}
        </span>
        <button
          class="settings-trigger"
          aria-label="Open settings"
          title="Settings"
          @click="openSettings('printer')"
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="M12 8.6a3.4 3.4 0 1 0 0 6.8 3.4 3.4 0 0 0 0-6.8Z" />
            <path
              d="m19.4 13.2 1.4 1.1-1.8 3.1-1.7-.7a7 7 0 0 1-2.2 1.3l-.2 1.8h-3.6l-.2-1.8a7 7 0 0 1-2.2-1.3l-1.7.7-1.8-3.1 1.4-1.1a7 7 0 0 1 0-2.4L5.4 9.7l1.8-3.1 1.7.7A7 7 0 0 1 11.1 6l.2-1.8h3.6l.2 1.8a7 7 0 0 1 2.2 1.3l1.7-.7 1.8 3.1-1.4 1.1a7 7 0 0 1 0 2.4Z"
            />
          </svg>
        </button>
        <button
          v-if="status.running"
          class="button pause-action"
          :disabled="status.phase === 'stopping'"
          @click="pauseCurrent"
        >
          {{ status.phase === "stopping" ? "Pausing…" : "Pause" }}
        </button>
        <button
          v-else
          class="button primary"
          :disabled="!isReady"
          @click="syncNow"
        >
          Sync now
        </button>
      </div>
    </header>

    <main v-if="!loading" class="content">
      <section class="metrics" aria-label="Sync summary">
        <article>
          <span class="metric-icon mint">↓</span>
          <div><small>DOWNLOADED</small><strong>{{ status.downloaded }}</strong></div>
        </article>
        <article>
          <span class="metric-icon coral">!</span>
          <div><small>FAILED</small><strong>{{ status.failed }}</strong></div>
        </article>
        <article>
          <span class="metric-icon amber">↷</span>
          <div><small>ALREADY PRESENT</small><strong>{{ status.skipped }}</strong></div>
        </article>
        <article>
          <span class="metric-icon blue">◷</span>
          <div><small>NEXT SYNC</small><strong>{{ nextExecutionLabel }}</strong></div>
        </article>
      </section>

      <div v-if="status.running" class="progress-card">
        <div>
          <span>
            {{
              status.phase === "stopping"
                ? "Finishing the current file before pausing…"
                : status.current_file || "Checking the Snapmaker U1…"
            }}
          </span>
          <strong>{{ status.progress_percent }}%</strong>
        </div>
        <progress :value="status.progress_percent" max="100"></progress>
      </div>

      <div v-if="status.last_error" class="inline-alert">
        <strong>Latest warning</strong>
        <span>{{ status.last_error }}</span>
      </div>

      <section class="history-panel">
        <div class="history-heading">
          <div>
            <p class="eyebrow">ACTIVITY</p>
            <h2>Recent history</h2>
          </div>
          <button v-if="history.length" class="text-button history-clear" @click="clearHistory">
            Clear
          </button>
        </div>
        <div v-if="history.length" class="history-list">
          <div v-for="entry in history" :key="entry.id" class="history-row">
            <span class="file-badge" :class="{ failed: entry.result === 'failed' }">▶</span>
            <div class="history-name">
              <strong>{{ entry.gcode_name || "Timelapse" }}</strong>
              <small :title="entry.local_path">{{ entry.local_path || entry.remote_path }}</small>
            </div>
            <span>{{ formatBytes(entry.size) }}</span>
            <span>{{ formatDate(entry.completed_at) }}</span>
            <b :class="{ error: entry.result === 'failed' }">{{ historyLabel(entry) }}</b>
          </div>
        </div>
        <div v-else class="empty-history">
          <span>◫</span>
          <strong>No timelapses synced yet</strong>
          <p>Details will appear here after the first download.</p>
        </div>
      </section>
    </main>

    <main v-else class="loading-screen">
      <div class="brand-mark"><span></span><span></span><span></span></div>
      <p>Preparing SnapSync…</p>
    </main>

    <Transition name="drawer">
      <div v-if="settingsOpen" class="settings-overlay" @click.self="settingsOpen = false">
        <aside class="settings-drawer" aria-label="Settings">
          <header>
            <div>
              <p class="eyebrow">SNAPSYNC</p>
              <h2>Settings</h2>
            </div>
            <button class="drawer-close" aria-label="Close" @click="settingsOpen = false">×</button>
          </header>

          <nav class="settings-tabs" aria-label="Settings sections">
            <button
              :class="{ active: settingsTab === 'printer' }"
              @click="settingsTab = 'printer'"
            >
              Printer
            </button>
            <button
              :class="{ active: settingsTab === 'general' }"
              @click="settingsTab = 'general'"
            >
              General
            </button>
          </nav>

          <div v-if="settingsTab === 'printer'" class="settings-content">
            <div class="drawer-section-heading">
              <div>
                <h3>Snapmaker U1</h3>
                <p>Direct HTTP connection on your local network.</p>
              </div>
              <button class="button subtle" :disabled="discovering" @click="searchPrinters">
                {{ discovering ? "Searching…" : "Search network" }}
              </button>
            </div>

            <div v-if="config.printer" class="connected-card">
              <span class="connected-icon">✓</span>
              <div>
                <small>CONNECTED</small>
                <strong>{{ config.printer.name }}</strong>
                <p>{{ config.printer.host }} · Direct HTTP</p>
              </div>
              <div class="connected-actions">
                <button class="text-button" :disabled="verifying" @click="verifyPrinter">
                  {{ verifying ? "Testing…" : "Test" }}
                </button>
                <button class="text-button danger" @click="forgetPrinter">Remove</button>
              </div>
            </div>

            <template v-else>
              <div v-if="discovered.length" class="device-list">
                <button
                  v-for="printer in discovered"
                  :key="printer.id"
                  class="device-option"
                  :class="{ selected: printerForm.host === printer.host }"
                  @click="choosePrinter(printer)"
                >
                  <span class="device-dot"></span>
                  <span>
                    <strong>{{ printer.name }}</strong>
                    <small>{{ printer.host }}</small>
                  </span>
                  <b>Use</b>
                </button>
              </div>
              <form class="direct-form" @submit.prevent="connectDirect">
                <label>
                  Name
                  <input v-model="printerForm.name" placeholder="Snapmaker U1" />
                </label>
                <label>
                  IP address
                  <input v-model="printerForm.host" placeholder="192.168.1.50" autocomplete="off" />
                </label>
                <label>
                  HTTP port
                  <input
                    v-model.number="printerForm.httpPort"
                    type="number"
                    min="1"
                    max="65535"
                  />
                </label>
                <button class="button primary wide" :disabled="connecting" type="submit">
                  {{ connecting ? "Checking…" : "Connect by IP" }}
                </button>
              </form>
            </template>
          </div>

          <div v-else class="settings-content">
            <div class="setting-block">
              <label>Destination folder</label>
              <button class="folder-field" @click="selectDestination">
                <span>▣</span>
                <b>{{ config.destination || "Choose a folder…" }}</b>
                <i>Change</i>
              </button>
            </div>

            <div class="toggle-list">
              <label>
                <span>
                  <strong>Automatic sync</strong>
                  <small>Runs the schedule in the background</small>
                </span>
                <input v-model="config.schedule_enabled" type="checkbox" role="switch" />
              </label>
            </div>

            <div v-if="config.schedule_enabled" class="schedule-fields">
              <label>
                Sync every
                <input v-model.number="config.interval_value" type="number" min="1" max="8760" />
              </label>
              <label>
                Period
                <select v-model="config.interval_unit">
                  <option value="hours">Hours</option>
                  <option value="days">Days</option>
                </select>
              </label>
            </div>

            <div class="toggle-list">
              <label>
                <span>
                  <strong>Download thumbnails</strong>
                  <small>Saves a JPG next to each video</small>
                </span>
                <input v-model="config.download_thumbnails" type="checkbox" role="switch" />
              </label>
              <label>
                <span>
                  <strong>Start with the system</strong>
                  <small>Runs after you sign in to Windows or macOS</small>
                </span>
                <input v-model="config.autostart" type="checkbox" role="switch" />
              </label>
              <label>
                <span>
                  <strong>Keep running in the tray</strong>
                  <small>Closing the window does not stop the app</small>
                </span>
                <input v-model="config.run_in_background" type="checkbox" role="switch" />
              </label>
            </div>

            <button class="button primary save-button" :disabled="saving" @click="saveSettings">
              {{ saving ? "Saving…" : "Save settings" }}
            </button>
          </div>
        </aside>
      </div>
    </Transition>

    <Transition name="toast">
      <div v-if="notice" class="toast" :class="noticeKind">{{ notice }}</div>
    </Transition>
  </div>
</template>
