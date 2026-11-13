# wslproxy

`wslproxy` 支持两种 TCP 端口转发. 正向规则将指定 Windows IPv4 地址上的连接转发到 WSL 服务. 反向规则将 WSL loopback 连接转发到 Windows loopback 服务. 它适合让个人热点或受限局域网中的设备访问 WSL SSH, 也能让 WSL 应用安全访问只绑定 Windows 本机回环地址的服务.

## 前提

- Windows 运行 WSL 2 NAT 网络.
- 目标 WSL 服务使用 `wsl-ip` 模式时, 必须监听 `0.0.0.0` 或 WSL 的 eth0 地址.
- 目标 WSL 服务仅监听 `127.0.0.1` 时, 使用 `localhost` 模式并保持 WSL localhost forwarding 可用.
- Windows 防火墙必须允许客户端访问配置的监听地址和端口. `wslproxy` 不会创建或删除防火墙规则.
- 代理链路不加密. 外部访问应使用 SSH, TLS 或目标服务自身的认证机制.
- 反向规则要求 WSL 发行版架构与运行 `wslproxy` 的 Windows 架构一致: x86_64 版内嵌 x86_64 agent, aarch64 版内嵌 aarch64 agent. `just install` 和官方发布包都会内嵌对应架构的 agent. 单独执行不带 `bundled-agent` feature 的 `cargo install --path wslproxy` 只能使用正向规则.
- 构建内嵌 agent 的版本需要 Zig, `cargo-zigbuild` 和当前架构对应的 Rust musl target.

## 安装和运行

```shell
just install
wslproxy --print-default-config
wslproxy check
wslproxy run
```

默认配置文件是 `~/.config/wslproxy/config.toml`. 也可通过 `-c` 指定其他路径.

规则变更需要重启 `wslproxy run` 才会生效.

## 配置

```toml
[daemon]
connect_timeout_seconds = 5
max_connections_per_rule = 128

[[forwards]]
name = "ubuntu-ssh"
distro = "Ubuntu"
allow = ["192.168.137.0/24"]

[forwards.listen]
address = "192.168.137.1"
port = 2222

[forwards.target]
mode = "wsl-ip"
port = 2222

[[reverse_forwards]]
name = "windows-20002"
distro = "Ubuntu"

[reverse_forwards.listen]
port = 20003

[reverse_forwards.target]
port = 20002
```

每条规则包含以下字段:

- `name`: 唯一规则名.
- `distro`: WSL 发行版名, 例如 `Ubuntu`.
- `allow`: 允许来源的 IPv4 CIDR 列表. 空列表和 IPv6 CIDR 无效.
- `listen.address`: 要绑定的 Windows IPv4 地址. 使用 `0.0.0.0` 监听全部 IPv4 网卡.
- `listen.interface`: 可替代 `listen.address`, 使用精确 Windows 网卡名称解析唯一 IPv4 地址.
- `listen.port`: Windows 监听 TCP 端口.
- `target.mode`: `wsl-ip` 或 `localhost`.
- `target.port`: WSL 目标 TCP 端口.

`listen.address` 与 `listen.interface` 必须二选一. 使用网卡名称时, 如果该网卡没有唯一的已启用 IPv4 地址, 程序会拒绝启动.

每条反向规则包含以下字段:

- `name`: 与正向规则共用的唯一规则名.
- `distro`: WSL 发行版名, 例如 `Ubuntu`.
- `reverse_forwards.listen.port`: WSL 内的 `127.0.0.1` 监听端口.
- `reverse_forwards.target.port`: Windows 内的 `127.0.0.1` 目标端口.

反向规则固定映射 `WSL 127.0.0.1:<listen.port>` 到 `Windows 127.0.0.1:<target.port>`. 同一个发行版不能有两条规则占用相同 WSL 监听端口. `wslproxy` 自身不会创建 Windows 入站监听器, 不会暴露 Windows 服务到局域网, 也不会创建 Windows 防火墙规则.

建议让反向规则的 `listen.port` 与 `target.port` 使用不同端口. WSL 2 NAT 的原生 localhost forwarding 会检测 WSL 内的 TCP 监听器, 并可能启动 `C:\Program Files\WSL\wslrelay.exe` 在 Windows loopback 上监听相同端口. 如果两个端口相同, `wslrelay.exe` 可能先于 Windows 目标服务占用该端口, 导致目标服务绑定失败. Windows 端代理也可能误连到 `wslrelay.exe`, 形成递归转发并耗尽连接数限制. 该 Windows 监听器由 WSL 自动创建, 不是 `wslproxy` 直接启动或控制的监听器.

### 关闭 WSL localhost forwarding

如果需要在 WSL 和 Windows 两端使用相同端口, 可以在 Windows 用户目录的 `.wslconfig` 中关闭 WSL 2 NAT 的原生 localhost forwarding. 在现有 `[wsl2]` 区段中设置:

```ini
[wsl2]
localhostForwarding=false
```

然后在 PowerShell 中完全关闭 WSL, 再重新启动发行版:

```powershell
wsl --shutdown
```

关闭前需要注意:

- 该选项全局影响全部 WSL 2 发行版, 不能只排除一个端口.
- Windows 将无法继续通过 `localhost` 访问 WSL 服务.
- 正向规则的 `target.mode = "localhost"` 将无法工作.
- 正向规则的 `target.mode = "wsl-ip"` 不受影响.
- `reverse_forwards` 使用自己的 agent 管道, 不依赖原生 localhost forwarding.
- mirrored networking 下该选项可能不生效. 本项目要求使用 NAT 网络. 如果不能切换到 NAT, 应让反向规则使用不同的监听端口和目标端口.

## 行为

启动时程序会先校验全部规则和监听地址. 对反向规则, 程序会将内嵌的 `wslproxy-agent` 原子部署到发行版用户缓存目录, 然后在 WSL `127.0.0.1` 绑定监听器并完成协议握手. 任一规则无法绑定或 agent 无法启动时, 守护进程不会部分启动.

收到正向连接后, 程序先检查来源地址是否在 `allow` 中. 被拒绝的连接不会访问 WSL. 允许的连接会在目标不可用时刷新 WSL 地址并重试一次. 目标 WSL 停止时, Windows 监听端口仍会保持可用. 后续允许的连接会按 1 秒到 30 秒的退避窗口重新探测 WSL.

反向规则使用每条规则独立的 `wsl.exe` helper 管道和多路 TCP 会话. WSL 或 helper 在运行中退出时, 该规则会按 1 秒到 30 秒退避重启, 不影响其他正向或反向规则. 持久 helper 会帮助保持 WSL 活跃, 但不能阻止显式 WSL 关闭操作.

所有内部 `wsl.exe` 子进程都以无控制台方式运行, 并通过捕获输出或显式管道通信. 它们不会修改 `wslproxy` 所在终端的控制台模式, 也不会接收该终端的 `Ctrl+C`. 退出信号由 `wslproxy` 主进程统一处理.

`wslproxy check` 会验证正向规则的网卡解析, WSL 启动和目标 TCP 连通性. 对反向规则, 它会启动 agent 并完成握手, 再探测 Windows loopback 目标. `check` 不创建正向或反向监听器, 也不会修改防火墙.

## 发布构建

官方发布包会先为 Linux 构建静态 agent, 再以 `bundled-agent` feature 将其嵌入 Windows 程序. `just dist` 会依次完成这两步并打出 zip. 运行中的 Windows 程序只会将 agent 复制到目标发行版自己的缓存目录, 不会修改系统目录.

`just agent` 只构建当前架构对应的 agent, 需要 Zig, `cargo-zigbuild` 和与宿主架构一致的 Rust musl target:

```shell
rustup target add x86_64-unknown-linux-musl
cargo install cargo-zigbuild
just agent
```

arm64 的 Windows 换用 `aarch64-unknown-linux-musl`, 因为 arm64 WSL 里执行不了 x86_64 的 agent.
