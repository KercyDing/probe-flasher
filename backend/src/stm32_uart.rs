use std::{
    collections::BTreeMap,
    path::Path,
    time::{Duration, Instant},
};

use ihex::Record;
use serialport::{DataBits, FlowControl, Parity, SerialPort, SerialPortType, StopBits};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum BootMode {
    /// 不操作 DTR/RTS
    None,

    // DTR 控制复位
    /// DTR 低电平复位, RTS 高电平进 Boot
    DtrLowRtsHigh,
    /// DTR 高电平复位, RTS 高电平进 Boot
    DtrHighRtsHigh,
    /// DTR 高电平复位, RTS 低电平进 Boot
    DtrHighRtsLow,
    /// DTR 高电平复位
    DtrHighOnly,

    // RTS 控制复位
    /// RTS 低电平复位, DTR 高电平进 Boot
    RtsLowDtrHigh,
    /// RTS 低电平复位, DTR 低电平进 Boot
    RtsLowDtrLow,
    /// RTS 低电平复位
    RtsLowOnly,
    /// RTS 高电平复位
    RtsHighOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Level {
    Low,
    High,
}

#[derive(Debug, Clone, Copy)]
pub struct BootLineConfig {
    pub boot_level: Level,
    pub reset_assert_level: Level,
}

impl Default for BootLineConfig {
    fn default() -> Self {
        // FlyMcu 默认配置
        Self {
            boot_level: Level::High,
            reset_assert_level: Level::Low,
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("serial port error: {0}")]
    Serial(#[from] serialport::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("hex parse error: {0}")]
    Hex(String),
    #[error("bootloader: unexpected response byte 0x{0:02X}")]
    UnexpectedResponse(u8),
    #[error("bootloader: timeout waiting for response")]
    Timeout,
    #[error("bootloader: device returned NACK")]
    Nack,
    #[error("bootloader: no supported erase command")]
    NoEraseSupport,
    #[error("port '{0}' not found or cannot be opened")]
    PortNotFound(String),
    #[error("hex file '{0}' not found")]
    HexFileNotFound(String),
    #[error("hex file is empty or contains no valid data")]
    HexFileEmpty,
}

pub type Result<T> = std::result::Result<T, Error>;

const ACK: u8 = 0x79;
const NACK: u8 = 0x1F;

const CMD_GET: u8 = 0x00;
const CMD_GET_ID: u8 = 0x02;
const CMD_GO: u8 = 0x21;
const CMD_WRITE_MEMORY: u8 = 0x31;
const CMD_ERASE: u8 = 0x43;
const CMD_EXTENDED_ERASE: u8 = 0x44;

#[derive(Debug, Clone)]
pub struct PortInfo {
    pub id: String,
    pub label: String,
    pub port_name: String,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub serial: Option<String>,
}

pub fn list_ports() -> Result<Vec<PortInfo>> {
    let ports = serialport::available_ports()?;
    let mut out = Vec::with_capacity(ports.len());

    for p in ports {
        // 在 macOS 上过滤掉不需要的端口
        #[cfg(target_os = "macos")]
        {
            if !p.port_name.starts_with("/dev/cu.") {
                continue;
            }
            if p.port_name.contains("Bluetooth") || p.port_name.contains("debug-console") {
                continue;
            }
        }

        let (vid, pid, serial, product) = match &p.port_type {
            SerialPortType::UsbPort(info) => (
                Some(info.vid),
                Some(info.pid),
                info.serial_number.clone(),
                info.product.clone(),
            ),
            _ => (None, None, None, None),
        };

        let mut label = p.port_name.clone();
        if let Some(mut prod) = product {
            if let Some(paren_pos) = prod.find(" (") {
                prod.truncate(paren_pos);
            }
            label.push_str(&format!(" - {prod}"));
        }

        out.push(PortInfo {
            id: p.port_name.clone(),
            label,
            port_name: p.port_name,
            vid,
            pid,
            serial,
        });
    }

    Ok(out)
}

#[derive(Debug, Clone)]
pub struct IdentifyResult {
    pub ok: bool,
    pub bootloader_version: Option<u8>,
    pub supported_commands: Vec<u8>,
    pub product_id: Option<u16>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FlashOptions {
    pub baud_rate: u32,
    pub boot_mode: BootMode,
    pub lines: BootLineConfig,
    pub verify: bool,
    pub reset_after: bool,
    pub read_timeout: Duration,
}

impl Default for FlashOptions {
    fn default() -> Self {
        Self {
            baud_rate: 115_200,
            boot_mode: BootMode::None,
            lines: BootLineConfig::default(),
            verify: false,
            reset_after: false,
            read_timeout: Duration::from_millis(800),
        }
    }
}

pub trait Logger {
    fn line(&self, level: &'static str, msg: &str);
}

pub struct StdoutLogger;

impl Logger for StdoutLogger {
    fn line(&self, level: &'static str, msg: &str) {
        println!("[{level}] {msg}");
    }
}

fn xor_checksum(bytes: impl IntoIterator<Item = u8>) -> u8 {
    bytes.into_iter().fold(0u8, |acc, b| acc ^ b)
}

fn read_byte_with_timeout(port: &mut dyn SerialPort, timeout: Duration) -> Result<u8> {
    let start = Instant::now();
    let mut buf = [0u8; 1];

    while start.elapsed() < timeout {
        match port.read(&mut buf) {
            Ok(1) => return Ok(buf[0]),
            Ok(0) => continue,
            Ok(_) => return Ok(buf[0]), // 不应该发生（1字节缓冲区）
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(e) => return Err(Error::Io(e)),
        }
    }

    Err(Error::Timeout)
}

fn expect_ack(port: &mut dyn SerialPort, timeout: Duration) -> Result<()> {
    let b = read_byte_with_timeout(port, timeout)?;
    match b {
        ACK => Ok(()),
        NACK => Err(Error::Nack),
        other => Err(Error::UnexpectedResponse(other)),
    }
}

fn send_cmd(port: &mut dyn SerialPort, cmd: u8, timeout: Duration) -> Result<()> {
    let pkt = [cmd, cmd ^ 0xFF];
    port.write_all(&pkt)?;
    port.flush()?;
    expect_ack(port, timeout)
}

fn send_address(port: &mut dyn SerialPort, address: u32, timeout: Duration) -> Result<()> {
    let a = address.to_be_bytes();
    let c = xor_checksum(a);
    port.write_all(&a)?;
    port.write_all(&[c])?;
    port.flush()?;
    expect_ack(port, timeout)
}

fn write_memory(
    port: &mut dyn SerialPort,
    address: u32,
    data: &[u8],
    timeout: Duration,
) -> Result<()> {
    if data.is_empty() || data.len() > 256 {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "write size must be 1..=256",
        )));
    }

    send_cmd(port, CMD_WRITE_MEMORY, timeout)?;
    send_address(port, address, timeout)?;

    let len_minus_one = (data.len() as u8).wrapping_sub(1);
    let checksum = xor_checksum(std::iter::once(len_minus_one).chain(data.iter().copied()));

    port.write_all(&[len_minus_one])?;
    port.write_all(data)?;
    port.write_all(&[checksum])?;
    port.flush()?;

    expect_ack(port, timeout)
}

fn extended_erase_all(
    port: &mut dyn SerialPort,
    timeout: Duration,
    long_timeout: Duration,
) -> Result<()> {
    send_cmd(port, CMD_EXTENDED_ERASE, timeout)?;

    // 全擦除
    port.write_all(&[0xFF, 0xFF, 0x00])?;
    port.flush()?;

    expect_ack(port, long_timeout)
}

fn erase_all(port: &mut dyn SerialPort, timeout: Duration, long_timeout: Duration) -> Result<()> {
    send_cmd(port, CMD_ERASE, timeout)?;

    // 全擦除（旧版）
    port.write_all(&[0xFF, 0x00])?;
    port.flush()?;

    expect_ack(port, long_timeout)
}

fn go_command(port: &mut dyn SerialPort, address: u32, timeout: Duration) -> Result<()> {
    send_cmd(port, CMD_GO, timeout)?;
    send_address(port, address, timeout)?;
    // GO 命令后 Bootloader 跳转，不会响应
    Ok(())
}

fn do_hardware_reset(port: &mut dyn SerialPort) -> Result<()> {
    // 设置 BOOT0=LOW 然后脉冲复位
    port.write_request_to_send(false)?;
    std::thread::sleep(Duration::from_millis(50));

    port.write_data_terminal_ready(true)?;
    std::thread::sleep(Duration::from_millis(100));

    port.write_data_terminal_ready(false)?;
    std::thread::sleep(Duration::from_millis(100));

    port.write_data_terminal_ready(true)?;
    std::thread::sleep(Duration::from_millis(100));

    Ok(())
}

fn connect_bootloader_with_log(
    port: &mut dyn SerialPort,
    timeout: Duration,
    _logger: &dyn Logger,
) -> Result<()> {
    // 清除接收缓冲区
    let _ = port.clear(serialport::ClearBuffer::Input);
    std::thread::sleep(Duration::from_millis(50));
    let _ = port.clear(serialport::ClearBuffer::Input);

    // macOS 需要更多的稳定时间
    #[cfg(target_os = "macos")]
    {
        std::thread::sleep(Duration::from_millis(100));
        let _ = port.clear(serialport::ClearBuffer::All);
        std::thread::sleep(Duration::from_millis(50));
    }

    // 自动波特率同步
    let mut last_err = Error::Timeout;
    for attempt in 1..=5 {
        port.write_all(&[0x7F])?;
        port.flush()?;

        // macOS 需要更长的等待时间
        #[cfg(target_os = "macos")]
        std::thread::sleep(Duration::from_millis(150));

        #[cfg(not(target_os = "macos"))]
        std::thread::sleep(Duration::from_millis(100));

        match expect_ack(port, timeout) {
            Ok(()) => return Ok(()),
            Err(Error::Timeout) if attempt < 5 => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(Error::UnexpectedResponse(_)) if attempt < 5 => {
                // 清除旧数据并重试
                let _ = port.clear(serialport::ClearBuffer::Input);
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => {
                last_err = e;
                if attempt < 5 {
                    let _ = port.clear(serialport::ClearBuffer::Input);
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
            }
        }
    }

    Err(last_err)
}

fn get_info(port: &mut dyn SerialPort, timeout: Duration) -> Result<(u8, Vec<u8>)> {
    send_cmd(port, CMD_GET, timeout)?;

    let n = read_byte_with_timeout(port, timeout)? as usize;
    let version = read_byte_with_timeout(port, timeout)?;

    let mut cmds = vec![0u8; n];
    let mut read_total = 0usize;
    while read_total < cmds.len() {
        match port.read(&mut cmds[read_total..]) {
            Ok(0) => {}
            Ok(k) => read_total += k,
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => return Err(Error::Io(e)),
        }
    }

    expect_ack(port, timeout)?;
    Ok((version, cmds))
}

fn get_id(port: &mut dyn SerialPort, timeout: Duration) -> Result<u16> {
    send_cmd(port, CMD_GET_ID, timeout)?;

    let n = read_byte_with_timeout(port, timeout)? as usize;
    let mut pid_bytes = vec![0u8; n + 1];

    let mut read_total = 0usize;
    while read_total < pid_bytes.len() {
        match port.read(&mut pid_bytes[read_total..]) {
            Ok(0) => {}
            Ok(k) => read_total += k,
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => return Err(Error::Io(e)),
        }
    }

    expect_ack(port, timeout)?;

    let pid = match pid_bytes.as_slice() {
        [msb, lsb] => u16::from_be_bytes([*msb, *lsb]),
        [single] => *single as u16,
        _ => return Err(Error::UnexpectedResponse(0x00)),
    };

    Ok(pid)
}

pub fn open_port(
    port_name: &str,
    baud_rate: u32,
    read_timeout: Duration,
) -> Result<Box<dyn SerialPort>> {
    #[allow(unused_mut)] // macOS need
    let mut p = serialport::new(port_name, baud_rate)
        .timeout(read_timeout)
        .data_bits(DataBits::Eight)
        .stop_bits(StopBits::One)
        .parity(Parity::Even)
        .flow_control(FlowControl::None)
        .open()
        .map_err(|e| match &e.kind {
            serialport::ErrorKind::NoDevice | serialport::ErrorKind::Io(_) => {
                Error::PortNotFound(port_name.to_string())
            }
            _ => Error::Serial(e),
        })?;

    // macOS 特定初始化
    #[cfg(target_os = "macos")]
    {
        p.write_data_terminal_ready(false)?;
        p.write_request_to_send(false)?;

        std::thread::sleep(Duration::from_millis(200));

        let _ = p.clear(serialport::ClearBuffer::All);
        std::thread::sleep(Duration::from_millis(50));
    }

    Ok(p)
}

pub fn apply_boot_mode(
    port: &mut dyn SerialPort,
    boot_mode: BootMode,
    _lines: BootLineConfig,
    _logger: &dyn Logger,
) -> Result<()> {
    if boot_mode == BootMode::None {
        return Ok(());
    }

    match boot_mode {
        BootMode::DtrLowRtsHigh => {
            port.write_data_terminal_ready(true)?;
            port.write_request_to_send(false)?;
            std::thread::sleep(Duration::from_millis(100));

            port.write_request_to_send(true)?;
            std::thread::sleep(Duration::from_millis(50));

            port.write_data_terminal_ready(false)?;
            std::thread::sleep(Duration::from_millis(100));

            port.write_data_terminal_ready(true)?;
            std::thread::sleep(Duration::from_millis(200));
        }
        BootMode::DtrHighRtsHigh => {
            port.write_data_terminal_ready(false)?;
            std::thread::sleep(Duration::from_millis(100));

            port.write_request_to_send(true)?;
            std::thread::sleep(Duration::from_millis(50));

            port.write_data_terminal_ready(true)?;
            std::thread::sleep(Duration::from_millis(100));

            port.write_data_terminal_ready(false)?;
            std::thread::sleep(Duration::from_millis(200));
        }
        BootMode::DtrHighRtsLow => {
            port.write_data_terminal_ready(false)?;
            std::thread::sleep(Duration::from_millis(100));

            port.write_request_to_send(false)?;
            std::thread::sleep(Duration::from_millis(50));

            port.write_data_terminal_ready(true)?;
            std::thread::sleep(Duration::from_millis(100));

            port.write_data_terminal_ready(false)?;
            std::thread::sleep(Duration::from_millis(200));
        }
        BootMode::DtrHighOnly => {
            port.write_data_terminal_ready(false)?;
            std::thread::sleep(Duration::from_millis(100));

            port.write_data_terminal_ready(true)?;
            std::thread::sleep(Duration::from_millis(100));

            port.write_data_terminal_ready(false)?;
            std::thread::sleep(Duration::from_millis(200));
        }

        BootMode::RtsLowDtrHigh => {
            port.write_request_to_send(true)?;
            std::thread::sleep(Duration::from_millis(100));

            port.write_data_terminal_ready(true)?;
            std::thread::sleep(Duration::from_millis(50));

            port.write_request_to_send(false)?;
            std::thread::sleep(Duration::from_millis(100));

            port.write_request_to_send(true)?;
            std::thread::sleep(Duration::from_millis(200));
        }
        BootMode::RtsLowDtrLow => {
            port.write_request_to_send(true)?;
            std::thread::sleep(Duration::from_millis(100));

            port.write_data_terminal_ready(false)?;
            std::thread::sleep(Duration::from_millis(50));

            port.write_request_to_send(false)?;
            std::thread::sleep(Duration::from_millis(100));

            port.write_request_to_send(true)?;
            std::thread::sleep(Duration::from_millis(200));
        }
        BootMode::RtsLowOnly => {
            port.write_request_to_send(true)?;
            std::thread::sleep(Duration::from_millis(100));

            port.write_request_to_send(false)?;
            std::thread::sleep(Duration::from_millis(100));

            port.write_request_to_send(true)?;
            std::thread::sleep(Duration::from_millis(200));
        }
        BootMode::RtsHighOnly => {
            port.write_request_to_send(false)?;
            std::thread::sleep(Duration::from_millis(100));

            port.write_request_to_send(true)?;
            std::thread::sleep(Duration::from_millis(100));

            port.write_request_to_send(false)?;
            std::thread::sleep(Duration::from_millis(200));
        }

        BootMode::None => unreachable!(),
    }

    Ok(())
}

pub fn identify(port_name: &str, options: &FlashOptions, logger: &dyn Logger) -> IdentifyResult {
    match (|| -> Result<IdentifyResult> {
        let mut port = open_port(port_name, options.baud_rate, options.read_timeout)?;
        apply_boot_mode(&mut *port, options.boot_mode, options.lines, logger)?;
        connect_bootloader_with_log(&mut *port, options.read_timeout, logger)?;
        let (ver, cmds) = get_info(&mut *port, options.read_timeout)?;
        let pid = get_id(&mut *port, options.read_timeout).ok();
        Ok(IdentifyResult {
            ok: true,
            bootloader_version: Some(ver),
            supported_commands: cmds,
            product_id: pid,
            error: None,
        })
    })() {
        Ok(ok) => ok,
        Err(e) => IdentifyResult {
            ok: false,
            bootloader_version: None,
            supported_commands: vec![],
            product_id: None,
            error: Some(e.to_string()),
        },
    }
}

pub fn parse_hex_to_image(path: &Path) -> Result<BTreeMap<u32, u8>> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::HexFileNotFound(path.display().to_string())
        } else {
            Error::Io(e)
        }
    })?;

    let mut image = BTreeMap::<u32, u8>::new();

    let mut upper: u32 = 0;

    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }

        let rec = ihex::Record::from_record_string(line).map_err(|e| Error::Hex(format!("{e}")))?;

        match rec {
            Record::Data { offset, value } => {
                let base = (upper << 16) | (offset as u32);
                for (i, b) in value.into_iter().enumerate() {
                    image.insert(base + (i as u32), b);
                }
            }
            Record::ExtendedLinearAddress(hi) => {
                upper = hi as u32;
            }
            Record::EndOfFile => break,
            _ => {
                // 忽略 MVP 中不支持的记录类型
            }
        }
    }

    if image.is_empty() {
        return Err(Error::HexFileEmpty);
    }

    Ok(image)
}

fn image_to_blocks(image: &BTreeMap<u32, u8>) -> Vec<(u32, Vec<u8>)> {
    let mut blocks: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut cur_addr: Option<u32> = None;
    let mut cur: Vec<u8> = Vec::new();

    for (&addr, &b) in image.iter() {
        match cur_addr {
            None => {
                cur_addr = Some(addr);
                cur.push(b);
            }
            Some(a0) => {
                let expected = a0 + (cur.len() as u32);
                if addr == expected {
                    cur.push(b);
                } else {
                    blocks.push((a0, std::mem::take(&mut cur)));
                    cur_addr = Some(addr);
                    cur.push(b);
                }
            }
        }
    }

    if let Some(a0) = cur_addr
        && !cur.is_empty()
    {
        blocks.push((a0, cur));
    }

    blocks
}

pub fn flash_hex(
    port_name: &str,
    hex_path: &Path,
    options: &FlashOptions,
    logger: &dyn Logger,
) -> Result<()> {
    let image = parse_hex_to_image(hex_path)?;
    let blocks = image_to_blocks(&image);

    logger.line("info", &format!("已加载固件：{} 字节", image.len()));

    let mut port = open_port(port_name, options.baud_rate, options.read_timeout)?;
    apply_boot_mode(&mut *port, options.boot_mode, options.lines, logger)?;

    logger.line("info", "正在连接 Bootloader...");
    connect_bootloader_with_log(&mut *port, options.read_timeout, logger)?;

    logger.line("info", "正在查询支持的命令...");
    let (_ver, cmds) = get_info(&mut *port, options.read_timeout)?;

    let supports_ext_erase = cmds.contains(&CMD_EXTENDED_ERASE);
    let supports_erase = cmds.contains(&CMD_ERASE);

    logger.line("info", "正在擦除...");
    let erase_timeout = Duration::from_secs(25);
    if supports_ext_erase {
        extended_erase_all(&mut *port, options.read_timeout, erase_timeout)?;
    } else if supports_erase {
        erase_all(&mut *port, options.read_timeout, erase_timeout)?;
    } else {
        return Err(Error::NoEraseSupport);
    }

    logger.line("info", "正在写入...");
    let total = image.len() as u64;
    let mut written: u64 = 0;

    for (base, data) in blocks {
        let mut offset = 0usize;
        while offset < data.len() {
            let end = (offset + 256).min(data.len());
            let chunk = &data[offset..end];
            let addr = base + offset as u32;
            write_memory(&mut *port, addr, chunk, options.read_timeout)?;
            written += chunk.len() as u64;

            logger.line("info", &format!("PROGRESS:写入中:{written}:{total}"));

            offset = end;
        }
    }

    if options.reset_after {
        // 使用 GO 命令跳转到用户程序地址 0x08000000
        let supports_go = cmds.contains(&CMD_GO);
        if supports_go {
            logger.line("info", "正在启动用户程序...");
            if let Err(e) = go_command(&mut *port, 0x08000000, options.read_timeout) {
                logger.line("warn", &format!("GO 命令失败: {}, 尝试硬件复位", e));
                // 回退到硬件复位
                do_hardware_reset(&mut *port)?;
            }
        } else {
            logger.line("info", "正在复位以运行用户程序...");
            do_hardware_reset(&mut *port)?;
        }
        logger.line("info", "程序已启动");
    }

    if options.verify {
        logger.line("warn", "verify not implemented in MVP yet");
    }

    Ok(())
}
