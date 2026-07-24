# wslproxy

`wslproxy` 将指定 Windows IPv4 地址上的 TCP 连接转发到 WSL 服务. 它适合让个人热点或受限局域网中的设备访问 WSL SSH, Web 服务或其他 TCP 服务.

## 前提

- Windows 运行 WSL 2 NAT 网络.
- 目标 WSL 服务使用 `wsl-ip` 模式时, 必须监听 `0.0.0.0` 或 WSL 的 eth0 地址.
- 目标 WSL 服务仅监听 `127.0.0.1` 时, 使用 `localhost` 模式并保持 WSL localhost forwarding 可用.
- Windows 防火墙必须允许客户端访问配置的监听地址和端口. `wslproxy` 不会创建或删除防火墙规则.
- 代理链路不加密. 外部访问应使用 SSH, TLS 或目标服务自身的认证机制.

## 安装和运行

```shell
cargo install --path wslproxy
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

## 行为

启动时程序会先校验全部规则和监听地址. 任一规则无法绑定时, 守护进程不会部分启动.

收到连接后, 程序先检查来源地址是否在 `allow` 中. 被拒绝的连接不会访问 WSL. 允许的连接会在目标不可用时刷新 WSL 地址并重试一次. 目标 WSL 停止时, 监听端口仍会保持可用. 后续允许的连接会按 1 秒到 30 秒的退避窗口重新探测 WSL.

`wslproxy check` 会验证配置, 网卡解析, WSL 启动和目标 TCP 连通性, 但不会创建监听器或修改防火墙.
