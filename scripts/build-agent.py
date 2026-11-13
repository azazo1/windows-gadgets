#!/usr/bin/env python3

# 构建当前架构对应的 WSL musl helper, 供 Windows 版 wslproxy 内嵌.
# 用 cargo zigbuild 在任意宿主上交叉编译到 Linux musl, 目标架构跟随宿主架构或 TARGET_ARCH.

import os

import release_common as rc

TARGETS = {
    "x86_64": "x86_64-unknown-linux-musl",
    "aarch64": "aarch64-unknown-linux-musl",
}


def main():
    rc.configure_output_encoding()
    rc.repo_root()

    arch = rc.target_arch()
    target = TARGETS.get(arch)
    if not target:
        rc.die(f"没有为 {arch} 定义 WSL helper target, 请在 scripts/build-agent.py 里补充")

    print(f"构建 WSL helper ({arch}) -> {target}")
    rc.run(
        [
            "cargo",
            "zigbuild",
            "-p",
            "wslproxy-agent",
            "--release",
            "--locked",
            "--target",
            target,
        ],
        capture=False,
    )
    print(os.path.join("target", target, "release", "wslproxy-agent"))


if __name__ == "__main__":
    main()
