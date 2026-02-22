set windows-shell := ["pwsh.exe", "-C"]

default: install

install:
    cargo install --path fncaps
    cargo install --path imeswitch