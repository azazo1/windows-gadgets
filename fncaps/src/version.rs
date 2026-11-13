// 与 build.rs 配套使用: 常量由 build.rs 注入, 缺少 build.rs 会编译失败.

/// 面向用户的版本展示串, 形如 `0.1.0 (v0.2.0-a1b2c3d)`, 开发构建为 dev-build.
pub const DISPLAY_VERSION: &str = env!("PROJECT_VERSION_DISPLAY");

/// 返回面向用户的版本展示串.
pub fn display() -> &'static str {
    DISPLAY_VERSION
}
