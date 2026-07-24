use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("缺少 OUT_DIR 环境变量"));
    let generated = out_dir.join("embedded_agent.rs");
    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("缺少 CARGO_MANIFEST_DIR 环境变量"),
    );
    let workspace_dir = manifest_dir.parent().expect("无法定位工作区目录");
    let source = workspace_dir
        .join("target")
        .join("x86_64-unknown-linux-musl")
        .join("release")
        .join("wslproxy-agent");
    let bundled = env::var_os("CARGO_FEATURE_BUNDLED_AGENT").is_some();

    println!("cargo:rerun-if-changed={}", source.display());
    let content = if bundled {
        if !source.is_file() {
            panic!("未找到 Linux agent: {}. 请先执行 just wslproxy-release", source.display());
        }

        fs::copy(&source, out_dir.join("wslproxy-agent"))
            .expect("无法复制 wslproxy-agent 资产");
        "pub const EMBEDDED_AGENT: Option<&[u8]> = Some(include_bytes!(concat!(env!(\"OUT_DIR\"), \"/wslproxy-agent\")));\n"
    } else {
        "pub const EMBEDDED_AGENT: Option<&[u8]> = None;\n"
    };

    fs::write(generated, content).expect("无法生成内嵌 agent 源码");
}
