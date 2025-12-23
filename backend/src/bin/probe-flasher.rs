use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand, builder::styling};
use probe_flasher::stm32_uart::{self, BootLineConfig, BootMode, FlashOptions, StdoutLogger};

const STYLES: styling::Styles = styling::Styles::styled()
    .header(styling::AnsiColor::Yellow.on_default().bold())
    .usage(styling::AnsiColor::Yellow.on_default().bold())
    .literal(styling::AnsiColor::Green.on_default().bold())
    .placeholder(styling::AnsiColor::Cyan.on_default());

#[derive(Parser)]
#[command(name = "probe-flasher")]
#[command(about = "STM32 UART Bootloader flashing CLI", version)]
#[command(styles = STYLES)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 列出可用的串口
    ListPorts,

    /// 识别选定串口的 STM32 Bootloader
    #[command(after_help = "示例: probe-flasher identify --port COM5")]
    Identify {
        /// 串口名称
        #[arg(short, long)]
        port: String,

        /// 波特率
        #[arg(short, long, default_value = "115200")]
        baud: u32,

        /// Boot 进入模式
        #[arg(short = 'm', long, value_enum, default_value = "dtr-low-rts-high")]
        boot_mode: BootMode,
    },

    /// 通过 UART Bootloader 烧录 .hex 文件到 STM32
    Flash {
        /// 串口名称
        #[arg(short, long)]
        port: String,

        /// .hex 文件路径
        #[arg(short = 'f', long)]
        hex: PathBuf,

        /// 波特率
        #[arg(short, long, default_value = "115200")]
        baud: u32,

        /// Boot 进入模式
        #[arg(short = 'm', long, value_enum, default_value = "dtr-low-rts-high")]
        boot_mode: BootMode,

        /// 烧录后跳过自动复位（如果 GO 命令不起作用）
        #[arg(long)]
        no_reset: bool,
    },
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();
    let logger = StdoutLogger;

    match cli.command {
        Commands::ListPorts => match stm32_uart::list_ports() {
            Ok(ports) => {
                if ports.is_empty() {
                    println!("No serial ports found.");
                } else {
                    println!("Available ports:");
                    for p in ports {
                        let marker = if p.vid.is_some() { "*" } else { " " };
                        println!("{} {}", marker, p.label);
                    }
                }
            }
            Err(e) => eprintln!("Error listing ports: {e}"),
        },

        Commands::Identify {
            port,
            baud,
            boot_mode,
        } => {
            let opts = FlashOptions {
                baud_rate: baud,
                boot_mode,
                lines: BootLineConfig::default(),
                verify: false,
                reset_after: false,
                read_timeout: Duration::from_millis(800),
            };

            let result = stm32_uart::identify(&port, &opts, &logger);
            if result.ok {
                println!("Identify OK");
                if let Some(ver) = result.bootloader_version {
                    println!("  Bootloader version: 0x{ver:02X}");
                }
                if let Some(pid) = result.product_id {
                    println!("  Product ID: 0x{pid:04X}");
                }
                println!("  Supported commands: {:02X?}", result.supported_commands);
            } else {
                eprintln!("Identify FAILED: {}", result.error.unwrap_or_default());
            }
        }

        Commands::Flash {
            port,
            hex,
            baud,
            boot_mode,
            no_reset,
        } => {
            let opts = FlashOptions {
                baud_rate: baud,
                boot_mode,
                lines: BootLineConfig::default(),
                verify: false,
                reset_after: !no_reset,
                read_timeout: Duration::from_millis(800),
            };

            match stm32_uart::flash_hex(&port, &hex, &opts, &logger) {
                Ok(()) => println!("Flash completed successfully!"),
                Err(e) => eprintln!("Flash FAILED: {e}"),
            }
        }
    }
}
