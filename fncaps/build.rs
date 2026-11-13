// 注入构建版本号. 只在设置了 PROJECT_BUILD_VERSION 时注入, 日常开发构建回落到 dev-build.
// 这里刻意不读取 .git: 让构建结果不随 commit 变化, 否则每次 commit 都会让增量编译
// 缓存失效, target/ 会持续膨胀.

fn main() {
    println!("cargo:rerun-if-env-changed=PROJECT_BUILD_VERSION");

    let package_version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let (build_version, display_version) = match std::env::var("PROJECT_BUILD_VERSION") {
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
