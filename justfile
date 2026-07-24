default: install

# 安装全部工具.
install:
    cargo install --path fncaps
    cargo install --path imeswitch
    cargo install --path pathclip
    cargo zigbuild -p wslproxy-agent --release --target x86_64-unknown-linux-musl
    cargo install --path wslproxy --features bundled-agent

# 构建内嵌 Linux agent 的 wslproxy Windows 发布程序.
wslproxy-release:
    cargo zigbuild -p wslproxy-agent --release --target x86_64-unknown-linux-musl
    cargo zigbuild -p wslproxy --release --features bundled-agent
