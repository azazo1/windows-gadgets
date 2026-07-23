set windows-shell := ["pwsh.exe", "-C"]

default: install

# 安装全部工具.
install:
    cargo install --path fncaps
    cargo install --path imeswitch
    cargo install --path pathclip
