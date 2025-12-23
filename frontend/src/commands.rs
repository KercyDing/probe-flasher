use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

use probe_flasher::stm32_uart::{self, BootLineConfig, BootMode, FlashOptions};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortInfo {
    pub port_name: String,
    pub label: String,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub serial: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentifyResult {
    pub ok: bool,
    pub bootloader_version: Option<u8>,
    pub product_id: Option<u16>,
    pub supported_commands: Vec<u8>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashResult {
    pub ok: bool,
    pub duration_ms: u64,
    pub bytes_written: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEvent {
    pub level: String,
    pub message: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressEvent {
    pub phase: String,
    pub percent: u8,
    pub done: usize,
    pub total: usize,
}

pub struct TauriLogger {
    app: AppHandle,
}

impl TauriLogger {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    fn emit_log(&self, level: &str, message: &str) {
        let event = LogEvent {
            level: level.to_string(),
            message: message.to_string(),
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
        };
        let _ = self.app.emit("log-line", &event);
    }
}

impl stm32_uart::Logger for TauriLogger {
    fn line(&self, level: &'static str, msg: &str) {
        // 处理进度格式: "PROGRESS:phase:current:total"
        if msg.starts_with("PROGRESS:") {
            let parts: Vec<&str> = msg.split(':').collect();
            if parts.len() >= 4
                && let (Ok(current), Ok(total)) =
                    (parts[2].parse::<usize>(), parts[3].parse::<usize>())
            {
                let percent = if total > 0 {
                    ((current as f64 / total as f64) * 100.0) as u8
                } else {
                    0
                };

                let event = ProgressEvent {
                    phase: parts[1].to_string(),
                    percent,
                    done: current,
                    total,
                };
                let _ = self.app.emit("flash-progress", &event);
                return;
            }
        }

        self.emit_log(level, msg);
    }
}

#[derive(Default)]
pub struct AppState {
    pub is_flashing: Arc<Mutex<bool>>,
}

#[tauri::command]
pub fn list_ports() -> Result<Vec<PortInfo>, String> {
    stm32_uart::list_ports()
        .map(|ports| {
            ports
                .into_iter()
                .map(|p| PortInfo {
                    port_name: p.port_name.clone(),
                    label: p.label,
                    vid: p.vid,
                    pid: p.pid,
                    serial: p.serial,
                })
                .collect()
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn identify_port(
    app: AppHandle,
    port: String,
    baud: u32,
    boot_mode: String,
) -> Result<IdentifyResult, String> {
    let boot_mode = parse_boot_mode(&boot_mode)?;

    let opts = FlashOptions {
        baud_rate: baud,
        boot_mode,
        lines: BootLineConfig::default(),
        verify: false,
        reset_after: false,
        read_timeout: Duration::from_millis(800),
    };

    let logger = TauriLogger::new(app);
    let result = stm32_uart::identify(&port, &opts, &logger);

    Ok(IdentifyResult {
        ok: result.ok,
        bootloader_version: result.bootloader_version,
        product_id: result.product_id,
        supported_commands: result.supported_commands,
        error: result.error,
    })
}

#[tauri::command]
pub async fn flash_firmware(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    port: String,
    hex_path: String,
    baud: u32,
    boot_mode: String,
    reset_after: bool,
) -> Result<FlashResult, String> {
    {
        let mut is_flashing = state.is_flashing.lock().unwrap();
        if *is_flashing {
            return Err("Already flashing".to_string());
        }
        *is_flashing = true;
    }

    let boot_mode = parse_boot_mode(&boot_mode)?;
    let hex_path = PathBuf::from(hex_path);

    let opts = FlashOptions {
        baud_rate: baud,
        boot_mode,
        lines: BootLineConfig::default(),
        verify: false,
        reset_after,
        read_timeout: Duration::from_millis(800),
    };

    let logger = TauriLogger::new(app.clone());
    let start = std::time::Instant::now();

    let result = stm32_uart::flash_hex(&port, &hex_path, &opts, &logger);

    {
        let mut is_flashing = state.is_flashing.lock().unwrap();
        *is_flashing = false;
    }

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(()) => {
            let _ = app.emit(
                "flash-done",
                serde_json::json!({
                    "ok": true,
                    "message": "烧录完成！"
                }),
            );
            Ok(FlashResult {
                ok: true,
                duration_ms,
                bytes_written: None,
                error: None,
            })
        }
        Err(e) => {
            let error_msg = e.to_string();
            let _ = app.emit(
                "flash-done",
                serde_json::json!({
                    "ok": false,
                    "message": format!("烧录失败: {}", error_msg)
                }),
            );
            Ok(FlashResult {
                ok: false,
                duration_ms,
                bytes_written: None,
                error: Some(error_msg),
            })
        }
    }
}

fn parse_boot_mode(mode: &str) -> Result<BootMode, String> {
    match mode {
        "none" => Ok(BootMode::None),
        "dtr-low-rts-high" => Ok(BootMode::DtrLowRtsHigh),
        "dtr-high-rts-high" => Ok(BootMode::DtrHighRtsHigh),
        "dtr-high-rts-low" => Ok(BootMode::DtrHighRtsLow),
        "dtr-high-only" => Ok(BootMode::DtrHighOnly),
        "rts-low-dtr-high" => Ok(BootMode::RtsLowDtrHigh),
        "rts-low-dtr-low" => Ok(BootMode::RtsLowDtrLow),
        "rts-low-only" => Ok(BootMode::RtsLowOnly),
        "rts-high-only" => Ok(BootMode::RtsHighOnly),
        _ => Err(format!("Unknown boot mode: {}", mode)),
    }
}
