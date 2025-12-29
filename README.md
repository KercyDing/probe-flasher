# Probe Flasher

STM32 串口烧录工具，通过 UART 接口烧录 Intel HEX 固件到 STM32 芯片。支持自动进入 Bootloader 模式（DTR/RTS 控制），提供图形界面和命令行两种使用方式。

<img width="1112" height="812" alt="截屏2025-12-30 07 49 52" src="https://github.com/user-attachments/assets/3faeaa2b-5829-455e-a3e1-313a69c1d30d" />

## 功能特性

- 自动检测可用串口
- 自动进入 Bootloader（DTR/RTS 控制）
- 识别 Bootloader 版本和芯片 ID
- 烧录 Intel HEX 格式固件
- 实时进度显示和日志输出
- 跨平台支持（Windows / macOS）

## 技术栈

- 后端：Rust + serialport + ihex
- GUI：Tauri 2.0 + Svelte 4 + Tailwind CSS 3
- 协议：STM32 UART Bootloader Protocol

## 安装

从 [GitHub Releases](https://github.com/KercyDing/probe-flasher/releases) 下载对应平台的安装包：

- Windows：`.exe` 安装程序
- macOS：`.dmg` 镜像文件（Universal Binary，同时支持 Intel 和 Apple Silicon）

## 构建

### 环境要求

- Rust 1.75+
- Node.js 18+
- Just 构建工具（`cargo install just`）
- Windows：Visual Studio Build Tools / MSVC
- macOS：Xcode Command Line Tools

### 构建命令

```bash
# 克隆项目
git clone https://github.com/your-username/probe-flasher.git
cd probe-flasher

# 构建 GUI 应用
just build-gui

# 构建 CLI 工具
just build-cli
```

## 使用说明

### GUI 图形界面

运行安装包或构建后的应用程序：

```bash
# macOS
open "target/release/bundle/macos/Probe Flasher.app"

# Windows
.\target\release\probe-flasher-gui.exe
```

操作流程：
1. 选择串口和波特率（默认 115200）
2. 选择 Boot 模式（推荐 `rts-low-dtr-high`）
3. 点击"识别设备"读取芯片信息
4. 选择 `.hex` 固件文件并点击"烧录"

### CLI 命令行

```bash
# 列出可用串口
just run list-ports

# 识别芯片
just run identify --port COM9 --boot-mode rts-low-dtr-high

# 烧录固件
just run flash --port COM9 --hex firmware.hex --boot-mode rts-low-dtr-high
```

常用参数：
- `--port <PORT>` - 串口名称（必需）
- `--hex <FILE>` - 固件文件路径（烧录时必需）
- `--baud <BAUD>` - 波特率，默认 115200
- `--boot-mode <MODE>` - Boot 模式，默认 `rts-low-dtr-high`
- `--no-reset` - 烧录后不自动复位运行

## Boot 模式说明

Boot 模式决定如何通过 DTR/RTS 控制芯片进入 Bootloader：

| 模式 | 说明 |
|------|------|
| `none` | 手动操作 BOOT0/RESET |
| `rts-low-dtr-high` | RTS 低电平复位，DTR 高电平进 Boot（推荐） |
| `dtr-low-rts-high` | DTR 低电平复位，RTS 高电平进 Boot |
| `rts-low-dtr-low` | RTS 低电平复位，DTR 低电平进 Boot |

其他组合请根据硬件电路选择。

## 硬件接线

自动控制 STM32 进入 Bootloader 需要以下接线：

| USB-UART 引脚 | STM32 引脚 | 说明 |
|--------------|-----------|------|
| RTS          | NRST      | 复位控制（RTS 低电平有效） |
| DTR          | BOOT0     | Boot 模式（DTR 高电平进入 Bootloader） |
| TX           | PA10 (RX) | USART1 接收 |
| RX           | PA9 (TX)  | USART1 发送 |
| GND          | GND       | 共地 |

注：部分开发板已集成此电路，可直接使用。若无硬件连接，选择 `none` 模式手动操作 BOOT0 和 RESET。

## 开发

```bash
# 安装前端依赖
just install

# 构建前端
just build-front

# 清理构建产物
just clean

# 运行代码检查
just check
```

## License

[MIT](LICENSE)
