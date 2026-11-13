use std::env;
use std::fs;
use std::path::PathBuf;

/// 目标架构对应的 WSL helper musl triple.
fn agent_target() -> Option<&'static str> {
    match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => Some("x86_64-unknown-linux-musl"),
        Ok("aarch64") => Some("aarch64-unknown-linux-musl"),
        _ => None,
    }
}

/// 注入构建版本号. 本文件同时负责内嵌 WSL helper, 所以版本注入也放在这里.
fn inject_build_version() {
    println!("cargo:rerun-if-env-changed=PROJECT_BUILD_VERSION");

    let package_version = env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let (build_version, display_version) = match env::var("PROJECT_BUILD_VERSION") {
        Ok(version) if !version.trim().is_empty() => {
            let version = version.trim().to_string();
            let display = format!("{package_version} ({version})");
            (version, display)
        }
        _ => ("dev-build".to_string(), "dev-build".to_string()),
    };

    println!("cargo:rustc-env=PROJECT_BUILD_VERSION={build_version}");
    println!("cargo:rustc-env=PROJECT_VERSION_DISPLAY={display_version}");
}

fn main() {
    inject_build_version();

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("缺少 OUT_DIR 环境变量"));
    let generated = out_dir.join("embedded_agent.rs");
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("缺少 CARGO_MANIFEST_DIR 环境变量"));
    let workspace_dir = manifest_dir.parent().expect("无法定位工作区目录");

    // CI 把对应架构的 helper 下载到任意位置后用 WSLPROXY_AGENT_PATH 指过来;
    // 本地开发不设这个变量, 沿用 cargo zigbuild 的默认输出位置.
    let source = match env::var_os("WSLPROXY_AGENT_PATH") {
        Some(path) if !path.is_empty() => {
            let path = PathBuf::from(path);
            // 相对路径按工作区根解析: build script 的工作目录是 crate 目录, 不是工作区.
            if path.is_absolute() {
                path
            } else {
                workspace_dir.join(path)
            }
        }
        _ => {
            let target = agent_target().expect("不支持的 WSL helper 目标架构");
            workspace_dir
                .join("target")
                .join(target)
                .join("release")
                .join("wslproxy-agent")
        }
    };
    let bundled = env::var_os("CARGO_FEATURE_BUNDLED_AGENT").is_some();

    println!("cargo:rerun-if-env-changed=WSLPROXY_AGENT_PATH");
    println!("cargo:rerun-if-changed={}", source.display());
    let content = if bundled {
        if !source.is_file() {
            panic!(
                "未找到 Linux agent: {}. 请先执行 just agent",
                source.display()
            );
        }

        fs::copy(&source, out_dir.join("wslproxy-agent"))
            .expect("无法复制 wslproxy-agent 资产");
        "pub const EMBEDDED_AGENT: Option<&[u8]> = Some(include_bytes!(concat!(env!(\"OUT_DIR\"), \"/wslproxy-agent\")));\n"
    } else {
        "pub const EMBEDDED_AGENT: Option<&[u8]> = None;\n"
    };

    fs::write(generated, content).expect("无法生成内嵌 agent 源码");
}
