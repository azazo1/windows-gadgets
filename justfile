default: install

# 安装全部工具, 含内嵌 WSL helper 的 wslproxy.
install: agent
    cargo install --path fncaps
    cargo install --path imeswitch
    cargo install --path pathclip
    cargo install --path wslproxy --features bundled-agent

# 构建当前架构对应的 WSL musl helper, 供 wslproxy 内嵌.
agent:
    uv run --no-project python scripts/build-agent.py

# 生成当前平台的发布产物 (单个 zip).
dist: agent
    uv run --no-project python scripts/dist.py
