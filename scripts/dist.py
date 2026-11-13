#!/usr/bin/env python3

# 生成当前平台的发布产物: 一个 zip, 内含全部 Windows 工具的 exe 与 README.
# 需要同目录的 archive.py, build-version.py, release_common.py 与 output_encoding.py.

import os
import shutil

import release_common as rc

# 仓库名, 同时作为产物名前缀.
PROJECT_NAME = "windows-gadgets"
# 本仓库以 tag 为唯一版本来源, 各工具的 Cargo.toml 版本彼此独立, 不参与版本解析.
# 首个 tag 之前用这个中性占位版本兜底, 与 CI 的 `解析构建版本` 步骤保持一致.
PACKAGE_VERSION = "0.0.0"
# 打进同一个 zip 的二进制, 与 cargo package 名一致.
BINARIES = ["fncaps", "imeswitch", "pathclip", "wslproxy"]
# 需要内嵌 WSL helper 的包; 找不到对应架构的 musl helper 时构建会失败.
BUNDLED_AGENT_PACKAGE = "wslproxy"
# 冒烟检查参数, 要求每个二进制都能报出版本号后立即退出.
SMOKE_ARGS = ["--version"]
# 需要交叉编译时设置, 例如 aarch64-pc-windows-msvc; 交叉编译的产物无法本机执行,
# 只能做文件格式与架构检查, 并用 TARGET_PLATFORM / TARGET_ARCH 命名.
RUST_TARGET = os.environ.get("RUST_TARGET", "")

# WSL helper 的 musl target 按目标架构映射: arm64 WSL 里执行不了 x86_64 的 ELF.
AGENT_TARGETS = {
    "x86_64": "x86_64-unknown-linux-musl",
    "aarch64": "aarch64-unknown-linux-musl",
}


def package_version_fallback():
    """仓库还没有版本 tag 时返回占位版本, 与 CI 侧使用同一份数据."""
    return PACKAGE_VERSION


def agent_path(target_arch):
    """定位要内嵌的 WSL helper: WSLPROXY_AGENT_PATH 优先, 否则用 cargo 的默认输出位置."""
    explicit = os.environ.get("WSLPROXY_AGENT_PATH", "").strip()
    if explicit:
        return explicit
    target = AGENT_TARGETS.get(target_arch)
    if not target:
        rc.die(f"没有为 {target_arch} 定义 WSL helper target, 请在 scripts/dist.py 里补充")
    return os.path.join("target", target, "release", "wslproxy-agent")


def main():
    rc.configure_output_encoding()
    rc.repo_root()

    target_platform = rc.target_platform()
    target_arch = rc.target_arch()
    if target_platform != "windows":
        rc.die(f"本项目的发布产物只支持 Windows, 当前目标平台是 {target_platform}")

    display_version, archive_version = rc.resolve_versions(
        rc.script_path("build-version.py"), package_version_fallback
    )
    print(f"构建 {PROJECT_NAME} {display_version} ({target_platform}-{target_arch})")

    agent = agent_path(target_arch)
    if not os.path.isfile(agent):
        rc.die(
            f"缺少 WSL helper: {agent}\n"
            f"先运行 uv run --no-project python scripts/build-agent.py, "
            f"或把 WSLPROXY_AGENT_PATH 指向已构建的 {target_arch} musl helper"
        )

    cargo_env = os.environ.copy()
    # build.rs 只在设置了 PROJECT_BUILD_VERSION 时注入版本, 直接跑脚本时也要显式传,
    # 否则产物里还是 dev-build.
    cargo_env["PROJECT_BUILD_VERSION"] = display_version
    # build script 的工作目录是 crate 目录, 这里传绝对路径避免相对路径解析到 wslproxy/ 下.
    cargo_env["WSLPROXY_AGENT_PATH"] = os.path.abspath(agent)
    cargo_args = [
        "cargo",
        "build",
        "--release",
        "--locked",
        "--features",
        f"{BUNDLED_AGENT_PACKAGE}/bundled-agent",
    ]
    if RUST_TARGET:
        cargo_args += ["--target", RUST_TARGET]
    rc.run(cargo_args, env=cargo_env, capture=False)

    build_dir = f"target/{RUST_TARGET}/release" if RUST_TARGET else "target/release"
    staging = rc.stage_dir()
    binaries = [name + ".exe" for name in BINARIES]
    for name in binaries:
        shutil.copy(os.path.join(build_dir, name), os.path.join(staging, name))

    readme = "README.md"
    if not os.path.isfile(readme):
        rc.die("缺少 README.md, 无法打进归档")
    shutil.copy(readme, os.path.join(staging, "README.md"))

    license_file = "LICENSE"
    if not os.path.isfile(license_file):
        rc.die("缺少 LICENSE, 无法打进归档")
    shutil.copy(license_file, os.path.join(staging, "LICENSE"))

    assets = [*binaries, "README.md", "LICENSE"]
    if RUST_TARGET:
        print("交叉编译产物无法本机执行, 只检查文件格式, 架构与版本串")
        for name in binaries:
            path = os.path.join(staging, name)
            rc.check_binary_format(path, target_platform)
            rc.check_binary_arch(path, target_platform, target_arch)
            rc.check_version_string(path, display_version)
    else:
        for name in binaries:
            rc.smoke_check([os.path.join(staging, name)], SMOKE_ARGS, display_version)

    rc.archive(
        staging,
        assets,
        PROJECT_NAME,
        archive_version,
        target=(target_platform, target_arch),
    )


if __name__ == "__main__":
    main()
