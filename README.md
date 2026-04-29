# windows-gadgets

一些偏 Windows 日常使用的 Rust 小工具。

## 包含内容

- `imeswitch`：输入法切换守护进程
- `fncaps`：把 `CapsLock` 扩展成一组高频快捷键，详见 [fncaps/README.md](/D:/pjs/rust/windows-gadgets/fncaps/README.md)

## imeswitch

`imeswitch` 会盯住当前前台窗口，并用 Windows 已有的输入法切换方式去发切换请求。

当前行为：

- 前台窗口变化时，自动切回英文输入法
- `Esc` / `Ctrl+[`：切到英文输入法
- 单独按左 `Alt`：切到英文输入法
- 单独按右 `Alt`：切到中文输入法
- `Alt + 非修饰键` 组合保持系统原本行为
- 当前布局已经是中文输入法时，后台自动确保处于中文模式

注意：

- 左右 `Alt` 只有在“单独按下并释放”时才会触发输入法切换
- `Alt` 只会在和非修饰键组合时放行；`Shift` / `Ctrl` / `Win` / `CapsLock` 这类不会让它进入放行模式
- 部分键盘或布局里，右 `Alt` 会显示为 `AltGr`
- `--no-escape-switching` 只会关闭 `Esc` / `Ctrl+[`
- `--no-alt-switching` 会完全关闭左右 `Alt` 的输入法切换功能
- `--no-ensure-chinese-mode` 会关闭“中文布局下自动拉回中文模式”的守护

安装：

```shell
cargo install --path imeswitch
```

开发时直接运行：

```shell
cargo run -p imeswitch --release --
```

常用参数：

```text
--no-ime-resetting         禁用窗口切换后自动切回英文
--no-escape-switching      禁用 Esc / Ctrl+[ 切英文
--no-alt-switching         禁用单独按左/右 Alt 切换输入法
--no-ensure-chinese-mode   禁用中文输入法布局下自动保持中文模式
--locale-en <ID>           英文输入法 locale，默认 1033
--locale-zh <ID>           中文输入法 locale，默认 2052
--poll-ms <MS>             轮询间隔，默认 300ms
```

示例：

```shell
imeswitch
imeswitch --no-ime-resetting --no-alt-switching
imeswitch --no-ensure-chinese-mode
imeswitch --locale-en 1033 --locale-zh 2052 --poll-ms 80
```
