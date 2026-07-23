# pathclip

`pathclip` 是一个 Windows 剪贴板路径转换守护进程. 它自动处理纯文本路径, 并支持通过全局热键将资源管理器中的文件对象转换成指定格式的路径文本.

## 行为

- 默认 profile 为 `slash`, 例如 `C:\Users\me\a.txt` 会转换为 `C:/Users/me/a.txt`.
- 自动模式仅处理绝对盘符路径, UNC 路径和扩展长度路径.
- 多行文本中的每个非空行都必须是路径, 否则整段内容保持不变.
- 资源管理器复制的文件对象不会被自动改写, 因此仍可正常粘贴文件.
- 用户按下 profile 热键后, 文件对象会被显式转换为 CRLF 分隔的路径文本.
- 配置在启动时加载, 修改后需要重启进程.

## 安装与运行

```shell
cargo install --path pathclip
pathclip
```

常用参数:

```text
--config <PATH>          指定配置文件
--print-default-config  输出默认配置并退出
```

配置查找顺序:

1. `--config <PATH>`.
2. `PATHCLIP_CONFIG` 环境变量.
3. `~/.config/pathclip/config.toml`.
4. 未找到文件时使用内置默认配置.

查看默认配置:

```shell
pathclip --print-default-config
```

项目内的完整示例位于 `pathclip.toml.example`.

## Profile

每个 profile 包含一个可选热键和一组顺序执行的转换步骤:

```toml
auto_profile = "slash"

[profiles.slash]
hotkey = ""
steps = [
  { type = "regex", pattern = '^"(.*)"$', replacement = '$1' },
  { type = "forward-slash" },
]

[profiles.wsl]
hotkey = "Ctrl+Shift+1"
steps = [
  { type = "regex", pattern = '^"(.*)"$', replacement = '$1' },
  { type = "wsl" },
]

[profiles.file-uri]
hotkey = "Ctrl+Shift+2"
steps = [
  { type = "regex", pattern = '^"(.*)"$', replacement = '$1' },
  { type = "file-uri" },
]
```

- `auto_profile = ""` 可关闭自动转换.
- `hotkey = ""` 或省略 `hotkey` 可禁用对应热键.
- 热键使用 `global-hotkey` 的语法, 修饰键必须写在普通按键之前.
- 重复热键, 无效热键, 无效正则或不存在的 `auto_profile` 会导致启动失败.

## 转换步骤

### forward-slash

将反斜杠转换为正斜杠:

```text
C:\Users\me\a.txt -> C:/Users/me/a.txt
\\server\share\a.txt -> //server/share/a.txt
```

### wsl

将盘符路径转换到 WSL 的默认挂载目录:

```text
D:\Work\a.txt -> /mnt/d/Work/a.txt
\\wsl.localhost\Ubuntu\home\me -> /home/me
```

普通 UNC 路径没有可靠的 WSL 挂载映射, 因此该步骤会拒绝转换并保留原剪贴板.

### file-uri

生成经过 URL 编码的 file URI:

```text
C:\Program Files\a.txt -> file:///C:/Program%20Files/a.txt
```

### regex

使用 Rust `regex` 的 `replace_all` 语义. 支持 `$0`, `$1` 和 `${name}`:

```toml
{ type = "regex", pattern = '^"(.*)"$', replacement = '$1' }
```

这个规则会移除一对包围整个路径的 ASCII 双引号.

## 日志

默认日志级别为 `pathclip=info`. 可以使用 `RUST_LOG` 调整级别:

```powershell
$env:RUST_LOG = "pathclip=debug"
pathclip
```

日志不会输出实际剪贴板内容, 只记录 profile, 来源类型, 路径数量和错误上下文.
