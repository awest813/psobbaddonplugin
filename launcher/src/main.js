const { invoke } = window.__TAURI__.core;

const statusEl = document.querySelector("#status");
const latestReleaseEl = document.querySelector("#latest-release");
const logsEl = document.querySelector("#logs");
const resultEl = document.querySelector("#result");

const installPathEl = document.querySelector("#install-path");
const zipPathEl = document.querySelector("#zip-path");

const setResult = (value, isError = false) => {
  resultEl.textContent =
    typeof value === "string" ? value : JSON.stringify(value, null, 2);
  resultEl.classList.toggle("error", isError);
};

const confirmDanger = (message) => window.confirm(message);

const safeInvoke = async (command, payload = undefined) => {
  try {
    const response = await invoke(command, payload);
    setResult(response);
    return response;
  } catch (error) {
    setResult(String(error), true);
    throw error;
  }
};

async function refreshStatus() {
  const status = await safeInvoke("get_status");
  statusEl.textContent = JSON.stringify(status, null, 2);
  if (status.install_path) {
    installPathEl.value = status.install_path;
  }
}

async function refreshLogs() {
  const logs = await safeInvoke("read_logs", { tail_lines: 300 });
  logsEl.textContent = logs || "No logs yet.";
}

async function fetchLatestRelease() {
  const release = await safeInvoke("fetch_latest_release");
  latestReleaseEl.textContent = JSON.stringify(release, null, 2);
}

async function saveInstallPath() {
  const installPath = installPathEl.value.trim();
  if (!installPath) {
    setResult("Install path is required", true);
    return;
  }

  await safeInvoke("set_install_path", { install_path: installPath });
  await refreshStatus();
}

async function preflightLocal() {
  const zipPath = zipPathEl.value.trim();
  const installPath = installPathEl.value.trim() || null;
  if (!zipPath) {
    setResult("Zip path is required", true);
    return;
  }

  await safeInvoke("preflight_install", {
    zip_path: zipPath,
    install_path: installPath,
  });
}

async function installLocal(dryRun = false) {
  const zipPath = zipPathEl.value.trim();
  const installPath = installPathEl.value.trim() || null;
  if (!zipPath) {
    setResult("Zip path is required", true);
    return;
  }

  if (!dryRun) {
    const approved = confirmDanger(
      "Install will overwrite addon files in the selected folder. Continue?",
    );
    if (!approved) {
      return;
    }
  }

  await safeInvoke("install_from_zip", {
    request: {
      zip_path: zipPath,
      install_path: installPath,
      dry_run: dryRun,
      version: null,
    },
  });

  await refreshStatus();
  await refreshLogs();
}

async function installLatest(dryRun = false) {
  const installPath = installPathEl.value.trim() || null;

  if (!dryRun) {
    const approved = confirmDanger(
      "Install latest release will download and overwrite addon files. Continue?",
    );
    if (!approved) {
      return;
    }
  }

  await safeInvoke("install_latest_release", {
    request: {
      install_path: installPath,
      dry_run: dryRun,
      expected_sha256: null,
    },
  });

  await refreshStatus();
  await fetchLatestRelease();
  await refreshLogs();
}

async function rollbackLast() {
  const approved = confirmDanger(
    "Rollback will restore the last backup for tracked addon files. Continue?",
  );
  if (!approved) {
    return;
  }

  await safeInvoke("rollback_last_install");
  await refreshStatus();
  await refreshLogs();
}

async function launchGame() {
  await safeInvoke("launch_game");
}

document.querySelector("#refresh-status").addEventListener("click", refreshStatus);
document.querySelector("#launch-game").addEventListener("click", launchGame);
document.querySelector("#rollback").addEventListener("click", rollbackLast);
document
  .querySelector("#save-install-path")
  .addEventListener("click", saveInstallPath);
document.querySelector("#preflight-local").addEventListener("click", preflightLocal);
document
  .querySelector("#dry-run-local")
  .addEventListener("click", () => installLocal(true));
document
  .querySelector("#install-local")
  .addEventListener("click", () => installLocal(false));
document
  .querySelector("#fetch-latest")
  .addEventListener("click", fetchLatestRelease);
document
  .querySelector("#dry-run-latest")
  .addEventListener("click", () => installLatest(true));
document
  .querySelector("#install-latest")
  .addEventListener("click", () => installLatest(false));
document.querySelector("#refresh-logs").addEventListener("click", refreshLogs);

window.addEventListener("DOMContentLoaded", async () => {
  await refreshStatus();
  await fetchLatestRelease().catch(() => undefined);
  await refreshLogs();
});
