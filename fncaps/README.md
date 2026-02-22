# fncaps - CapsLock 增强型快捷键守护进程

**fncaps** 是一个跨平台的 CapsLock 快捷键增强工具，通过将 CapsLock 转换为功能键（类似 Vim/Emacs 风格），提供高效的窗口切换、输入法切换、应用启动等功能。所有快捷键行为都可通过 TOML 配置文件自定义。

## 🎯 特性

- **全局键盘拦截** - 使用低级钩子（`rdev::grab`）捕获并消费快捷键，确保不会传递给其他应用
- **灵活的快捷键绑定** - 通过 TOML 配置完全控制 CapsLock 组合的行为，无需修改代码
- **跨平台支持** - Windows、macOS、Linux 统一配置目录结构
- **参数化 Action 系统** - 支持打开任意程序、切换指定窗口、条件启动等高级操作
- **窗口智能切换** - 按方向角度加权重算法选择最近窗口
- **输入法自动切换** - 支持英文/中文输入法快速切换
- **详细日志输出** - 使用 `tracing` 框架记录所有操作，便于调试
- **单实例保护** - 通过 TCP 端口锁定确保同时仅运行一个实例

## 📋 快速开始

### 1. 编译

```bash
cd fncaps
cargo build --release
```

编译后的可执行文件位于 `target/release/fncaps.exe` (Windows) 或 `target/release/fncaps` (Linux/macOS)

### 2. 配置

配置文件自动加载位置 **（按平台标准）**：

| 平台    | 路径                                                      | 例子                                                       |
|---------|------------------------------------------------------------|------------------------------------------------------------|
| Linux   | `$XDG_CONFIG_HOME/fncaps/fncaps.toml` 或 `$HOME/.config/fncaps/fncaps.toml` | `/home/alice/.config/fncaps/fncaps.toml` |
| macOS   | `$HOME/Library/Application Support/fncaps/fncaps.toml`   | `/Users/Alice/Library/Application Support/fncaps/fncaps.toml` |
| Windows | `%APPDATA%\fncaps\fncaps.toml`                            | `C:\Users\Alice\AppData\Roaming\fncaps\fncaps.toml`       |

**自定义配置路径** - 设置环境变量 `FNCAPS_CONFIG`：
```bash
# Linux/macOS
export FNCAPS_CONFIG=/custom/path/to/fncaps.toml

# Windows (PowerShell)
$env:FNCAPS_CONFIG = "C:\custom\path\to\fncaps.toml"
```

#### 快速创建配置

1. 创建配置目录：
   - **Windows**: `mkdir %APPDATA%\fncaps`
   - **Linux/macOS**: `mkdir -p ~/.config/fncaps`

2. 复制示例配置：
   ```bash
   cp fncaps.toml.example ~/.config/fncaps/fncaps.toml  # Linux/macOS
   # 或
   copy fncaps.toml.example %APPDATA%\fncaps\fncaps.toml  # Windows
   ```

3. 编辑配置文件（参考 [配置说明](#配置说明) 部分）

### 3. 运行

```bash
# 直接运行
./target/release/fncaps

# 或添加到系统启动（Windows）
# 创建快捷方式在 C:\Users\<username>\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup
```

## ⌨️ 默认快捷键

| 快捷键 | 动作 | 说明 |
|--------|------|------|
| **CapsLock** (单击) | 切换输入法 | 在英文/中文输入法之间切换 |
| **CapsLock + H** / **CapsLock + ←** | 切换窗口到左 | 按方向加权重选择左边最近的窗口 |
| **CapsLock + L** / **CapsLock + →** | 切换窗口到右 | 按方向加权重选择右边最近的窗口 |
| **CapsLock + K** / **CapsLock + ↑** | 切换窗口到上 | 按方向加权重选择上方最近的窗口 |
| **CapsLock + J** / **CapsLock + ↓** | 切换窗口到下 | 按方向加权重选择下方最近的窗口 |
| **CapsLock + Shift + K** / **CapsLock + Shift + ↑** | 鼠标滚轮向上 | 向上滚动 (需要临时释放 Shift) |
| **CapsLock + Shift + J** / **CapsLock + Shift + ↓** | 鼠标滚轮向下 | 向下滚动 (需要临时释放 Shift) |
| **CapsLock + E** | 打开记事本 | 打开 `notepad.exe` |
| **CapsLock + V** | 打开/切换 VSCode | 切换到 VSCode 窗口或打开 `Code.exe` |
| **CapsLock + P** | 打开/切换 PowerShell | 切换到 PowerShell 窗口或打开 `pwsh.exe` |

所有快捷键都可通过 TOML 配置文件自定义！

## 📝 配置说明

### 配置文件格式

```toml
[caps]
# CapsLock 单击时执行的动作
tap_action = "switch_ime"

# 快捷键绑定规则
[[caps.bindings]]
key = "h"
action = "switch_left"
shift = "any"
suppress = true
pending = true
```

### 配置项详解

#### `key` (必需)
按键名称，支持：
- **字母**: `a` ~ `z`
- **数字**: `0` ~ `9`
- **方向键**: `left`, `right`, `up`, `down`
- **特殊键**: `space`, `enter`, `tab`, `esc`, `backspace`, `home`, `end`, `pageup`, `pagedown`, `insert`, `delete`, `f1` ~ `f12`

#### `action` (必需)
执行的动作，支持两类：

**简单动作**（无参数）：
- `none` - 无操作
- `switch_ime` - 切换输入法
- `switch_left` / `switch_right` / `switch_up` / `switch_down` - 按方向切换窗口
- `scroll_up` / `scroll_down` - 滚动鼠标滚轮

**参数化动作**（用冒号分割）：
- `open_program:notepad.exe` - 打开指定程序（自动通过 `which` 查找 PATH）
- `switch_window:Firefox` - 切换到标题包含 "Firefox" 的窗口（精确匹配优先）
- `switch_or_open:VSCode:Code.exe` - 切换到 VSCode 窗口，不存在则打开
  - 也支持管道符分割：`switch_or_open:VSCode|Code.exe`

#### `shift` (可选，默认 `any`)
Shift 键的状态要求：
- `any` / 不指定 - CapsLock + key（不关心 Shift）
- `down` / `pressed` - CapsLock + Shift + key（Shift 必须按下）
- `up` / `released` - CapsLock + key（Shift 必须未按下）

#### `suppress` (可选，默认 `true`)
是否拦截键盘事件，阻止系统处理此按键：
- `true` - 吞掉键盘事件，不让其他应用收到
- `false` - 传递给其他应用继续处理

#### `pending` (可选，默认 `true`)
是否在按住同一键时吞掉后续重复事件：
- `true` - 只触发一次，防止长按重复
- `false` - 允许长按时重复触发

### 配置示例

#### 示例 1: 打开自定义程序

```toml
[[caps.bindings]]
key = "w"
action = "open_program:explorer.exe"
shift = "any"
```

#### 示例 2: 切换指定应用

```toml
[[caps.bindings]]
key = "f"
action = "switch_window:Firefox"
shift = "any"

[[caps.bindings]]
key = "c"
action = "switch_window:Google Chrome"
shift = "any"
```

#### 示例 3: 条件启动（窗口存在则切换，否则打开）

```toml
[[caps.bindings]]
key = "s"
action = "switch_or_open:Sublime Text:subl.exe"
shift = "any"
```

#### 示例 4: 完整路径打开程序

```toml
[[caps.bindings]]
key = "x"
action = "open_program:C:\\Program Files\\Notepad++\\notepad++.exe"
shift = "any"
```

## 🔍 日志与调试

### 启用日志输出

设置环境变量 `RUST_LOG` 控制日志级别：

```bash
# Linux/macOS
export RUST_LOG=fncaps=debug
./target/release/fncaps

# Windows (PowerShell)
$env:RUST_LOG = "fncaps=debug"
.\target\release\fncaps.exe
```

日志等级（由低到高）：
- `trace` - 最详细，包括每个按键事件
- `debug` - 调试信息，用于排查问题
- `info` - 重要信息，包括快捷键触发
- `warn` - 警告，如找不到配置文件
- `error` - 错误，如启动失败

### 常见日志

```
[INFO] fncaps::config: hotkey config loaded, rules = 15
[INFO] fncaps::hotkey: global keyboard capture started
[DEBUG] fncaps::hotkey: capslock state changed, pressing = true
[INFO] fncaps::hotkey: matched configured caps binding, action = SwitchTo(Left)
[INFO] fncaps::windows: switching focus to selected window
[DEBUG] fncaps::launch: program spawned successfully, program = "notepad.exe"
```

## 🚀 完整使用流程示例

### 场景：配置快速应用启动和窗口切换

**目标：**
- CapsLock + E: 打开记事本
- CapsLock + F: 切换到 Firefox（不存在则打开）
- CapsLock + C: 切换到 VSCode

**步骤：**

1. **创建配置目录并复制示例：**
   ```bash
   mkdir %APPDATA%\fncaps
   copy fncaps.toml.example %APPDATA%\fncaps\fncaps.toml
   ```

2. **编辑 TOML 文件，修改或添加绑定：**
   ```toml
   [caps]
   tap_action = "switch_ime"

   [[caps.bindings]]
   key = "e"
   action = "open_program:notepad.exe"
   shift = "any"
   suppress = true
   pending = true

   [[caps.bindings]]
   key = "f"
   action = "switch_or_open:Firefox:firefox.exe"
   shift = "any"
   suppress = true
   pending = true

   [[caps.bindings]]
   key = "c"
   action = "switch_window:Visual Studio Code"
   shift = "any"
   suppress = true
   pending = true
   ```

3. **构建和运行：**
   ```bash
   cargo build --release
   .\target\release\fncaps.exe
   ```

4. **测试快捷键：**
   - 按 CapsLock + E：打开记事本 ✓
   - 按 CapsLock + F：打开 Firefox 或切换到已打开的 Firefox ✓
   - 按 CapsLock + C：切换到 VSCode ✓
   - 按 CapsLock：切换输入法 ✓

5. **设置开机自启（Windows）：**
   - 按 `Win + R`，输入 `shell:startup` 打开启动文件夹
   - 创建 `fncaps.exe` 的快捷方式放入此文件夹
   - 重启电脑后自动启动

## 🔧 故障排除

### 问题 1: 快捷键不工作

**排查步骤：**
1. 确认 fncaps 正在运行：检查系统任务栏或进程管理器
2. 启用日志查看是否收到按键事件：
   ```bash
   $env:RUST_LOG = "fncaps=trace"
   .\target\release\fncaps.exe
   ```
3. 检查配置文件是否存在且格式正确：
   ```bash
   # Windows
   dir %APPDATA%\fncaps\
   ```
4. 校验 TOML 语法：使用 TOML 格式检验工具
5. 确认快捷键没有被其他应用占用

### 问题 2: 配置文件找不到

**解决方案：**
1. 确认配置目录存在：
   ```bash
   # Linux/macOS
   mkdir -p ~/.config/fncaps

   # Windows
   mkdir %APPDATA%\fncaps
   ```
2. 复制示例文件到正确位置
3. 或设置 FNCAPS_CONFIG 环境变量明确指定路径

### 问题 3: 窗口切换没有找到目标窗口

**原因与解决：**
- **窗口标题不匹配**：检查实际窗口标题（可以从日志看到），确保 `switch_window` 中的标题是子串
- **窗口被隐藏**：只能切换到可见的窗口
- **权限问题**：某些系统窗口可能无法访问

### 问题 4: 程序无法打开

**排查方法：**
1. 确保程序存在或在 PATH 中：
   ```bash
   # Linux/macOS
   which notepad
   # 或用 `which` 查找

   # Windows
   where notepad.exe
   ```
2. 使用完整路径指定程序：`C:\\Program Files\\...\\app.exe`
3. 查看日志错误信息：
   ```bash
   $env:RUST_LOG = "fncaps::launch=debug"
   ```

### 问题 5: 输入法切换不工作

**仅限 Windows，且需要配置的输入法 locale 匹配**
- 英文输入法 locale: `1033`
- 简体中文输入法 locale: `2052`
- 配置其他输入法需要查询对应的 locale ID

## 📚 更多配置资源

- 完整示例配置：`fncaps.toml.example`
- 日志输出：使用 RUST_LOG 环境变量启用详细日志
- 单实例保护：通过 TCP 127.0.0.1:23982 确保仅一个实例运行

## 🛠️ 开发

### 项目结构

```
fncaps/
├── src/
│   ├── main.rs           # 应用入口和初始化
│   └── app/
│       ├── mod.rs        # 模块定义
│       ├── config.rs     # TOML 配置加载和解析
│       ├── hotkey.rs     # 全局键盘拦截和快捷键处理
│       ├── state.rs      # 状态管理和 Action 定义
│       ├── windows_ops.rs # 窗口操作（切换、查询等）
│       ├── ime.rs        # 输入法操作
│       ├── launch.rs     # 程序启动
│       └── logging.rs    # 日志初始化
├── Cargo.toml
├── fncaps.toml.example
└── README.md
```

### 添加新快捷键

1. 在 TOML 配置中添加新的 `[[caps.bindings]]` 节点
2. 无需重新编译，重启 fncaps 即可加载新配置

### 扩展 Action 类型

1. 在 `src/app/state.rs` 中的 `Action` 枚举添加新变体
2. 在 `src/app/config.rs` 的 `parse_action()` 中添加解析逻辑
3. 在 `src/app/hotkey.rs` 的 `execute_action()` 中添加执行逻辑

## 📄 许可证

MIT

## 🤝 贡献

欢迎提交 Issue 和 Pull Request！

---

**更新日期**: 2026-02-22
