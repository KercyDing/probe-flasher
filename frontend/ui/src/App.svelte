<script>
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";

  let ports = [];
  let selectedPort = "";
  let baudRate = 115200;
  let bootMode = "rts-low-dtr-high";
  let hexPath = "";
  let resetAfter = true;
  let isFlashing = false;
  let isIdentifying = false;
  let logs = [];
  let progress = { phase: "", percent: 0, done: 0, total: 0 };
  let shouldAutoScroll = true;
  let isDarkTheme = false;

  let openDropdown = null;

  function toggleTheme() {
    isDarkTheme = !isDarkTheme;
    document.documentElement.classList.toggle("dark", isDarkTheme);
  }

  function toggleDropdown(dropdown) {
    if (isFlashing) return;
    openDropdown = openDropdown === dropdown ? null : dropdown;
  }

  function closeDropdowns() {
    openDropdown = null;
  }

  function handleClickOutside(event) {
    if (!event.target.closest(".custom-select")) {
      closeDropdowns();
    }
  }

  const baudRates = [
    600, 1200, 4800, 9600, 14400, 19200, 38400, 57600, 76800, 115200, 128000,
    230400, 256000, 460800,
  ];

  const bootModes = [
    { value: "none", label: "不操作 DTR/RTS" },
    // DTR 控制复位
    { value: "dtr-low-rts-high", label: "DTR 低电平复位, RTS 高电平进 Boot" },
    { value: "dtr-high-rts-high", label: "DTR 高电平复位, RTS 高电平进 Boot" },
    { value: "dtr-high-rts-low", label: "DTR 高电平复位, RTS 低电平进 Boot" },
    { value: "dtr-high-only", label: "DTR 高电平复位" },
    // RTS 控制复位
    { value: "rts-low-dtr-high", label: "RTS 低电平复位, DTR 高电平进 Boot" },
    { value: "rts-low-dtr-low", label: "RTS 低电平复位, DTR 低电平进 Boot" },
    { value: "rts-low-only", label: "RTS 低电平复位" },
    { value: "rts-high-only", label: "RTS 高电平复位" },
  ];

  function addLog(level, message, timestamp = null) {
    const log = {
      level,
      message,
      timestamp:
        timestamp || new Date().toLocaleTimeString("zh-CN", { hour12: false }),
      id: Date.now() + Math.random(),
    };
    logs = [...logs, log];

    if (shouldAutoScroll) {
      setTimeout(() => {
        const logContainer = document.querySelector(".log-container");
        if (logContainer) {
          logContainer.scrollTop = logContainer.scrollHeight;
        }
      }, 10);
    }
  }

  function handleLogScroll(event) {
    const container = event.target;
    const isAtBottom =
      container.scrollHeight - container.scrollTop - container.clientHeight <
      50;
    shouldAutoScroll = isAtBottom;
  }

  async function refreshPorts() {
    try {
      addLog("info", "正在刷新端口列表...");
      ports = await invoke("list_ports");
      addLog("info", `找到 ${ports.length} 个串口`);
      if (ports.length > 0 && !selectedPort) {
        selectedPort = ports[0].port_name;
      }
    } catch (error) {
      addLog("error", `刷新端口失败: ${error}`);
    }
  }

  async function identify() {
    if (!selectedPort) {
      addLog("error", "请先选择串口");
      return;
    }

    isIdentifying = true;
    try {
      addLog("info", `正在识别 ${selectedPort}...`);
      const result = await invoke("identify_port", {
        port: selectedPort,
        baud: baudRate,
        bootMode: bootMode,
      });

      if (result.ok) {
        addLog("info", "识别成功！");
        if (result.bootloader_version) {
          addLog(
            "info",
            `  Bootloader 版本: 0x${result.bootloader_version.toString(16).toUpperCase().padStart(2, "0")}`,
          );
        }
        if (result.product_id) {
          addLog(
            "info",
            `  产品 ID: 0x${result.product_id.toString(16).toUpperCase().padStart(4, "0")}`,
          );
        }
      } else {
        addLog("error", `识别失败: ${result.error || "未知错误"}`);
      }
    } catch (error) {
      addLog("error", `识别出错: ${error}`);
    } finally {
      isIdentifying = false;
    }
  }

  async function selectHexFile() {
    try {
      const selected = await open({
        multiple: false,
        filters: [
          {
            name: "Intel HEX",
            extensions: ["hex"],
          },
        ],
      });

      if (selected) {
        hexPath = selected;
        addLog("info", `已选择文件: ${hexPath}`);
      }
    } catch (error) {
      addLog("error", `选择文件失败: ${error}`);
    }
  }

  async function flashFirmware() {
    if (!selectedPort) {
      addLog("error", "请先选择串口");
      return;
    }
    if (!hexPath) {
      addLog("error", "请先选择固件文件");
      return;
    }

    isFlashing = true;
    progress = { phase: "", percent: 0, done: 0, total: 0 };

    try {
      addLog("info", "========== 开始烧录 ==========");
      addLog("info", `端口: ${selectedPort}`);
      addLog("info", `波特率: ${baudRate}`);
      addLog("info", `固件: ${hexPath}`);

      const result = await invoke("flash_firmware", {
        port: selectedPort,
        hexPath: hexPath,
        baud: baudRate,
        bootMode: bootMode,
        resetAfter: resetAfter,
      });

      if (result.ok) {
        addLog(
          "info",
          `========== 烧录成功！(${result.duration_ms}ms) ==========`,
        );
      } else {
        addLog("error", `========== 烧录失败: ${result.error} ==========`);
      }
    } catch (error) {
      addLog("error", `烧录出错: ${error}`);
    } finally {
      isFlashing = false;
      progress = { phase: "", percent: 0, done: 0, total: 0 };
    }
  }

  function clearLogs() {
    logs = [];
    progress = { phase: "", percent: 0, done: 0, total: 0 };
  }

  onMount(async () => {
    await listen("log-line", (event) => {
      const log = event.payload;
      addLog(log.level, log.message, log.timestamp);
    });

    await listen("flash-progress", (event) => {
      progress = event.payload;

      if (shouldAutoScroll) {
        setTimeout(() => {
          const logContainer = document.querySelector(".log-container");
          if (logContainer) {
            logContainer.scrollTop = logContainer.scrollHeight;
          }
        }, 10);
      }
    });

    await listen("flash-done", (event) => {
      const result = event.payload;
      if (result.ok) {
        addLog("info", result.message);
      } else {
        addLog("error", result.message);
      }
    });

    await refreshPorts();
  });
</script>

<svelte:window on:click={handleClickOutside} />

<svelte:head>
  <style>
    body {
      margin: 0;
      padding: 0;
      font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text",
        "SF Pro Display", "Helvetica Neue", sans-serif;
      -webkit-font-smoothing: antialiased;
      overflow: hidden;
    }
  </style>
</svelte:head>

<main class:dark={isDarkTheme}>
  <div
    class="flex h-screen bg-gradient-to-b from-gray-100 to-gray-200 dark:from-gray-800 dark:to-gray-900 backdrop-blur-xl"
  >
    <div
      class="w-96 bg-white/70 dark:bg-gray-900/70 backdrop-blur-2xl border-r border-gray-200/50 dark:border-gray-700/50 shadow-xl flex flex-col gap-4 overflow-y-auto px-6 py-8"
    >
      <div class="flex items-center justify-between mb-4">
        <h1
          class="text-lg font-semibold tracking-wide text-gray-900 dark:text-gray-50"
        >
          PROBE FLASHER
        </h1>
        <button
          on:click={toggleTheme}
          class="p-2 rounded-lg bg-gray-100/80 dark:bg-gray-800/80 hover:bg-gray-200 dark:hover:bg-gray-700 transition-all duration-200 hover:scale-110 border border-gray-300/50 dark:border-gray-600/50"
          title="切换主题"
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 16 16"
            fill="none"
            class="text-gray-700 dark:text-gray-300"
          >
            {#if isDarkTheme}
              <path
                d="M14 8.5C14 11.5376 11.5376 14 8.5 14C5.46243 14 3 11.5376 3 8.5C3 5.46243 5.46243 3 8.5 3C8.67009 3 8.83867 3.00819 9.00534 3.02426C8.37168 3.82118 8 4.82616 8 5.92C8 8.40641 10.0936 10.5 12.58 10.5C13.3234 10.5 14.0176 10.2988 14.6208 9.94662C14.5405 10.1233 14.4518 10.2957 14.3553 10.4633C13.8699 11.3314 13.1587 12.0426 12.2906 12.528C11.4224 13.0134 10.4303 13.2585 9.42 13.2372C8.40973 13.2159 7.43002 12.9291 6.58433 12.4059C5.73865 11.8827 5.05759 11.1428 4.61339 10.2653C4.16919 9.38785 3.97943 8.40543 4.06449 7.42711C4.14955 6.44879 4.50619 5.51349 5.09715 4.72426C5.68811 3.93504 6.48995 3.32385 7.41111 2.96187C8.33226 2.59989 9.33726 2.50131 10.3133 2.67699C11.2893 2.85267 12.2001 3.29622 12.9459 3.95834C13.6916 4.62046 14.2437 5.47554 14.5407 6.42961C14.8377 7.38368 14.8688 8.40091 14.631 9.37076"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            {:else}
              <circle
                cx="8"
                cy="8"
                r="3.5"
                stroke="currentColor"
                stroke-width="1.5"
              />
              <line
                x1="8"
                y1="1"
                x2="8"
                y2="2.5"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
              />
              <line
                x1="8"
                y1="13.5"
                x2="8"
                y2="15"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
              />
              <line
                x1="15"
                y1="8"
                x2="13.5"
                y2="8"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
              />
              <line
                x1="2.5"
                y1="8"
                x2="1"
                y2="8"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
              />
              <line
                x1="12.5"
                y1="3.5"
                x2="11.5"
                y2="4.5"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
              />
              <line
                x1="4.5"
                y1="11.5"
                x2="3.5"
                y2="12.5"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
              />
              <line
                x1="12.5"
                y1="12.5"
                x2="11.5"
                y2="11.5"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
              />
              <line
                x1="4.5"
                y1="4.5"
                x2="3.5"
                y2="3.5"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
              />
            {/if}
          </svg>
        </button>
      </div>

      <div class="flex flex-col gap-2">
        <label
          class="text-xs uppercase tracking-wider text-gray-500 dark:text-gray-400 font-semibold"
          for="port-select">串口</label
        >
        <div class="flex gap-2">
          <div class="relative flex-1 custom-select">
            <button
              type="button"
              on:click={() => toggleDropdown("port")}
              disabled={isFlashing}
              class="w-full px-3 py-2.5 pr-10 text-sm text-left bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-xl shadow-sm focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent disabled:opacity-50 disabled:cursor-not-allowed text-gray-900 dark:text-gray-100 transition-all duration-200 hover:border-gray-300 dark:hover:border-gray-600"
            >
              {#if selectedPort}
                {ports.find((p) => p.port_name === selectedPort)?.label ||
                  selectedPort}
              {:else}
                选择串口
              {/if}
            </button>
            <div
              class="absolute inset-y-0 right-0 flex items-center pr-3 pointer-events-none"
            >
              <svg
                class="w-4 h-4 text-gray-500 dark:text-gray-400 transition-transform {openDropdown ===
                'port'
                  ? 'rotate-180'
                  : ''}"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  d="M19 9l-7 7-7-7"
                />
              </svg>
            </div>
            {#if openDropdown === "port"}
              <div
                class="absolute z-50 w-full mt-1 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-xl shadow-lg max-h-60 overflow-auto"
              >
                {#each ports as port}
                  <button
                    type="button"
                    on:click={() => {
                      selectedPort = port.port_name;
                      closeDropdowns();
                    }}
                    class="w-full px-3 py-2 text-sm text-left hover:bg-gray-100 dark:hover:bg-gray-700 {selectedPort ===
                    port.port_name
                      ? 'bg-blue-50 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400'
                      : 'text-gray-900 dark:text-gray-100'} transition-colors"
                  >
                    {port.label}
                  </button>
                {/each}
              </div>
            {/if}
          </div>
          <button
            on:click={refreshPorts}
            disabled={isFlashing}
            class="px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-200 bg-gray-100/90 dark:bg-gray-700/80 border border-gray-300/50 dark:border-gray-600/50 rounded-lg hover:bg-gray-200 dark:hover:bg-gray-600 disabled:opacity-50 disabled:cursor-not-allowed shadow-sm hover:shadow-md transition-all duration-200"
          >
            刷新
          </button>
        </div>
      </div>

      <div class="flex flex-col gap-2">
        <label
          class="text-xs uppercase tracking-wider text-gray-500 dark:text-gray-400 font-semibold"
          for="baudrate-select">波特率</label
        >
        <div class="relative custom-select">
          <button
            type="button"
            on:click={() => toggleDropdown("baud")}
            disabled={isFlashing}
            class="w-full px-3 py-2.5 pr-10 text-sm text-left bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-xl shadow-sm focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent disabled:opacity-50 disabled:cursor-not-allowed text-gray-900 dark:text-gray-100 transition-all duration-200 hover:border-gray-300 dark:hover:border-gray-600"
          >
            {baudRate}
          </button>
          <div
            class="absolute inset-y-0 right-0 flex items-center pr-3 pointer-events-none"
          >
            <svg
              class="w-4 h-4 text-gray-500 dark:text-gray-400 transition-transform {openDropdown ===
              'baud'
                ? 'rotate-180'
                : ''}"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M19 9l-7 7-7-7"
              />
            </svg>
          </div>
          {#if openDropdown === "baud"}
            <div
              class="absolute z-50 w-full mt-1 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-xl shadow-lg max-h-60 overflow-auto"
            >
              {#each baudRates as rate}
                <button
                  type="button"
                  on:click={() => {
                    baudRate = rate;
                    closeDropdowns();
                  }}
                  class="w-full px-3 py-2 text-sm text-left hover:bg-gray-100 dark:hover:bg-gray-700 {baudRate ===
                  rate
                    ? 'bg-blue-50 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400'
                    : 'text-gray-900 dark:text-gray-100'} transition-colors"
                >
                  {rate}
                </button>
              {/each}
            </div>
          {/if}
        </div>
      </div>

      <div class="flex flex-col gap-2">
        <label
          class="text-xs uppercase tracking-wider text-gray-500 dark:text-gray-400 font-semibold"
          for="bootmode-select">进入 Bootloader</label
        >
        <div class="relative custom-select">
          <button
            type="button"
            on:click={() => toggleDropdown("boot")}
            disabled={isFlashing}
            class="w-full px-3 py-2.5 pr-10 text-sm text-left bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-xl shadow-sm focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent disabled:opacity-50 disabled:cursor-not-allowed text-gray-900 dark:text-gray-100 transition-all duration-200 hover:border-gray-300 dark:hover:border-gray-600"
          >
            {bootModes.find((m) => m.value === bootMode)?.label || bootMode}
          </button>
          <div
            class="absolute inset-y-0 right-0 flex items-center pr-3 pointer-events-none"
          >
            <svg
              class="w-4 h-4 text-gray-500 dark:text-gray-400 transition-transform {openDropdown ===
              'boot'
                ? 'rotate-180'
                : ''}"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M19 9l-7 7-7-7"
              />
            </svg>
          </div>
          {#if openDropdown === "boot"}
            <div
              class="absolute z-50 w-full mt-1 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-xl shadow-lg max-h-60 overflow-auto"
            >
              {#each bootModes as mode}
                <button
                  type="button"
                  on:click={() => {
                    bootMode = mode.value;
                    closeDropdowns();
                  }}
                  class="w-full px-3 py-2 text-sm text-left hover:bg-gray-100 dark:hover:bg-gray-700 {bootMode ===
                  mode.value
                    ? 'bg-blue-50 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400'
                    : 'text-gray-900 dark:text-gray-100'} transition-colors"
                >
                  {mode.label}
                </button>
              {/each}
            </div>
          {/if}
        </div>
      </div>

      <button
        on:click={identify}
        disabled={isFlashing || isIdentifying || !selectedPort}
        class="px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-200 bg-gray-100/90 dark:bg-gray-700/80 border border-gray-300/50 dark:border-gray-600/50 rounded-lg hover:bg-gray-200 dark:hover:bg-gray-600 disabled:opacity-50 disabled:cursor-not-allowed shadow-sm hover:shadow-md transition-all duration-200"
      >
        {isIdentifying ? "识别中..." : "识别设备"}
      </button>

      <div
        class="h-px bg-gradient-to-r from-transparent via-gray-300 dark:via-gray-600 to-transparent my-2"
      ></div>

      <div class="flex flex-col gap-2">
        <label
          class="text-xs uppercase tracking-wider text-gray-500 dark:text-gray-400 font-semibold"
          for="hex-path">固件文件</label
        >
        <div class="flex gap-2">
          <input
            id="hex-path"
            type="text"
            value={hexPath}
            readonly
            placeholder="未选择文件"
            class="flex-1 px-3 py-2 text-sm bg-white/90 dark:bg-gray-800/80 border border-gray-300/50 dark:border-gray-600/50 rounded-lg shadow-sm text-gray-900 dark:text-gray-100 placeholder:text-gray-400 dark:placeholder:text-gray-500 transition-all duration-200"
          />
          <button
            on:click={selectHexFile}
            disabled={isFlashing}
            class="px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-200 bg-gray-100/90 dark:bg-gray-700/80 border border-gray-300/50 dark:border-gray-600/50 rounded-lg hover:bg-gray-200 dark:hover:bg-gray-600 disabled:opacity-50 disabled:cursor-not-allowed shadow-sm hover:shadow-md transition-all duration-200"
          >
            浏览
          </button>
        </div>
      </div>

      <label
        class="flex items-center gap-2 cursor-pointer text-sm text-gray-700 dark:text-gray-300"
      >
        <input
          type="checkbox"
          bind:checked={resetAfter}
          disabled={isFlashing}
          class="w-4 h-4 rounded border-gray-300 dark:border-gray-600 text-blue-500 focus:ring-2 focus:ring-blue-500/50 dark:focus:ring-blue-400/50 disabled:opacity-50 disabled:cursor-not-allowed"
        />
        <span>烧录后自动运行程序</span>
      </label>

      <button
        on:click={flashFirmware}
        disabled={isFlashing || !selectedPort || !hexPath}
        class="px-6 py-3 mt-2 text-sm font-semibold text-white bg-gradient-to-b from-blue-500 to-blue-600 dark:from-blue-500 dark:to-blue-700 rounded-lg shadow-lg shadow-blue-500/30 dark:shadow-blue-500/40 hover:shadow-xl hover:shadow-blue-500/40 dark:hover:shadow-blue-500/50 disabled:opacity-50 disabled:cursor-not-allowed disabled:shadow-none transition-all duration-200 hover:from-blue-600 hover:to-blue-700 dark:hover:from-blue-600 dark:hover:to-blue-800"
      >
        {isFlashing ? "烧录中..." : "开始烧录"}
      </button>
    </div>

    <div
      class="flex-1 flex flex-col bg-gradient-to-br from-gray-50/50 to-gray-100/50 dark:from-gray-900/50 dark:to-black/50 backdrop-blur-2xl"
    >
      <div
        class="flex items-center justify-between px-6 py-5 border-b border-gray-200/50 dark:border-gray-700/50 backdrop-blur-xl"
      >
        <h2
          class="text-sm uppercase tracking-widest text-gray-500 dark:text-gray-400 font-semibold"
        >
          日志输出
        </h2>
        <button
          on:click={clearLogs}
          class="px-3 py-1.5 text-xs font-medium text-gray-600 dark:text-gray-300 bg-gray-100/80 dark:bg-gray-800/80 border border-gray-300/50 dark:border-gray-600/50 rounded-md hover:bg-gray-200 dark:hover:bg-gray-700 shadow-sm hover:shadow transition-all duration-200"
        >
          清空
        </button>
      </div>

      <div
        class="log-container flex-1 overflow-y-auto px-6 py-4 bg-white/60 dark:bg-black/40 backdrop-blur-md font-mono text-sm leading-relaxed"
        on:scroll={handleLogScroll}
      >
        {#each logs as log (log.id)}
          <div class="py-1">
            <span class="text-gray-400 dark:text-gray-600 mr-2"
              >[{log.timestamp}]</span
            >
            <span
              class:text-gray-800={log.level === "info"}
              class:dark:text-gray-200={log.level === "info"}
              class:text-orange-600={log.level === "warn"}
              class:dark:text-orange-400={log.level === "warn"}
              class:text-red-600={log.level === "error"}
              class:dark:text-red-400={log.level === "error"}
            >
              {log.message}
            </span>
          </div>
        {/each}
      </div>

      {#if progress.percent > 0}
        <div
          class="px-6 py-4 bg-white/80 dark:bg-gray-900/80 border-t border-gray-200/50 dark:border-gray-700/50 backdrop-blur-xl"
        >
          <div class="flex items-center justify-between mb-2 text-xs">
            <span
              class="uppercase tracking-wider text-gray-500 dark:text-gray-400"
              >{progress.phase}</span
            >
            <span class="font-semibold text-gray-800 dark:text-gray-100"
              >{progress.percent}%</span
            >
          </div>
          <div
            class="h-1 bg-gray-200 dark:bg-gray-700 rounded-full overflow-hidden"
          >
            <div
              class="h-full bg-gradient-to-r from-blue-500 to-blue-600 dark:from-blue-400 dark:to-blue-500 transition-all duration-300 rounded-full"
              style="width: {progress.percent}%"
            ></div>
          </div>
          <div class="mt-2 text-xs text-right text-gray-500 dark:text-gray-400">
            {progress.done} / {progress.total}
          </div>
        </div>
      {/if}
    </div>
  </div>
</main>
