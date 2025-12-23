# Windows 使用 PowerShell
set windows-powershell := true

# 默认显示所有命令
default:
    @just --list

# 构建 CLI 工具
build-cli:
    cargo build --package probe-flasher --bin probe-flasher --release

# 构建前端
build-front: install
    npm --prefix frontend/ui run build

# 构建 GUI 应用
build-gui: build-front
    cd frontend
    cargo tauri build

# 运行 CLI（开发模式）
run *ARGS:
    cargo run --package probe-flasher --bin probe-flasher -- {{ARGS}}

# 构建所有
build-all: build-cli build-gui

# 清理构建产物
clean:
    cargo clean
    -npx rimraf frontend/ui/dist
    -npx rimraf frontend/ui/node_modules

# 安装前端依赖
install:
    npm --prefix frontend/ui install

# 运行所有检查（格式、Clippy、测试）
check:
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test --all
    @echo "All check passed!"
