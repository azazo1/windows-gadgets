use std::collections::HashSet;
use std::fs;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use ipnet::{IpNet, Ipv4Net};
use serde::Deserialize;
use tracing::info;

const DEFAULT_SETTINGS: &str = include_str!("../../wslproxy.toml.example");
const DEFAULT_CONNECT_TIMEOUT_SECONDS: u64 = 5;
const DEFAULT_MAX_CONNECTIONS_PER_RULE: usize = 128;

#[derive(Debug, Clone)]
pub struct Settings {
    pub daemon: DaemonSettings,
    pub forwards: Vec<ForwardRule>,
    pub reverse_forwards: Vec<ReverseForwardRule>,
}

#[derive(Debug, Clone)]
pub struct DaemonSettings {
    pub connect_timeout: Duration,
    pub max_connections_per_rule: usize,
}

#[derive(Debug, Clone)]
pub struct ForwardRule {
    pub name: String,
    pub distro: String,
    pub allow: Vec<Ipv4Net>,
    pub listen: ListenSpec,
    pub target: TargetSpec,
}

#[derive(Debug, Clone)]
pub struct ListenSpec {
    pub source: ListenSource,
    pub port: u16,
}

#[derive(Debug, Clone)]
pub enum ListenSource {
    Address(Ipv4Addr),
    Interface(String),
}

#[derive(Debug, Clone)]
pub struct TargetSpec {
    pub mode: TargetMode,
    pub port: u16,
}

#[derive(Debug, Clone)]
pub struct ReverseForwardRule {
    pub name: String,
    pub distro: String,
    pub listen: ReverseListenSpec,
    pub target: ReverseTargetSpec,
}

#[derive(Debug, Clone)]
pub struct ReverseListenSpec {
    pub port: u16,
}

#[derive(Debug, Clone)]
pub struct ReverseTargetSpec {
    pub port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetMode {
    WslIp,
    Localhost,
}

#[derive(Debug, Deserialize)]
struct SettingsFile {
    #[serde(default)]
    daemon: DaemonFile,
    #[serde(default)]
    forwards: Vec<ForwardFile>,
    #[serde(default)]
    reverse_forwards: Vec<ReverseForwardFile>,
}

#[derive(Debug, Deserialize)]
struct DaemonFile {
    #[serde(default = "default_connect_timeout_seconds")]
    connect_timeout_seconds: u64,
    #[serde(default = "default_max_connections_per_rule")]
    max_connections_per_rule: usize,
}

impl Default for DaemonFile {
    fn default() -> Self {
        Self {
            connect_timeout_seconds: default_connect_timeout_seconds(),
            max_connections_per_rule: default_max_connections_per_rule(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ForwardFile {
    name: String,
    distro: String,
    #[serde(default)]
    allow: Vec<String>,
    listen: ListenFile,
    target: TargetFile,
}

#[derive(Debug, Deserialize)]
struct ListenFile {
    address: Option<String>,
    interface: Option<String>,
    port: u16,
}

#[derive(Debug, Deserialize)]
struct TargetFile {
    mode: TargetModeFile,
    port: u16,
}

#[derive(Debug, Deserialize)]
struct ReverseForwardFile {
    name: String,
    distro: String,
    listen: ReverseListenFile,
    target: ReverseTargetFile,
}

#[derive(Debug, Deserialize)]
struct ReverseListenFile {
    port: u16,
}

#[derive(Debug, Deserialize)]
struct ReverseTargetFile {
    port: u16,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum TargetModeFile {
    WslIp,
    Localhost,
}

impl Settings {
    pub fn load(explicit_path: Option<PathBuf>) -> Result<Self> {
        let path = resolve_settings_path(explicit_path)?;
        let source = fs::read_to_string(&path)
            .with_context(|| format!("无法读取配置文件: {}", path.display()))?;
        let settings = Self::parse(&source)
            .with_context(|| format!("配置文件无效: {}", path.display()))?;
        info!(
            path = %path.display(),
            forward_rules = settings.forwards.len(),
            reverse_rules = settings.reverse_forwards.len(),
            "已加载转发配置"
        );
        Ok(settings)
    }

    pub fn default_source() -> &'static str {
        DEFAULT_SETTINGS
    }

    pub(super) fn parse(source: &str) -> Result<Self> {
        let file: SettingsFile = toml::from_str(source).context("无法解析 TOML 配置")?;
        if file.forwards.is_empty() && file.reverse_forwards.is_empty() {
            bail!("至少需要一条转发规则");
        }
        if file.daemon.connect_timeout_seconds == 0 {
            bail!("connect_timeout_seconds 必须大于 0");
        }
        if file.daemon.max_connections_per_rule == 0 {
            bail!("max_connections_per_rule 必须大于 0");
        }

        let mut names = HashSet::new();
        let mut fixed_endpoints = HashSet::new();
        let mut interface_endpoints = HashSet::new();
        let mut forwards = Vec::with_capacity(file.forwards.len());
        let mut reverse_forwards = Vec::with_capacity(file.reverse_forwards.len());
        let mut reverse_endpoints = HashSet::new();

        for forward in file.forwards {
            let rule = compile_forward(forward)?;
            if !names.insert(rule.name.clone()) {
                bail!("规则名重复: {}", rule.name);
            }

            match &rule.listen.source {
                ListenSource::Address(address) => {
                    let endpoint = SocketAddrV4::new(*address, rule.listen.port);
                    if !fixed_endpoints.insert(endpoint) {
                        bail!("固定监听端点重复: {endpoint}");
                    }
                }
                ListenSource::Interface(name) => {
                    let endpoint = (name.to_ascii_lowercase(), rule.listen.port);
                    if !interface_endpoints.insert(endpoint) {
                        bail!("网卡监听端点重复: {}:{}", name, rule.listen.port);
                    }
                }
            }

            forwards.push(rule);
        }

        for reverse_forward in file.reverse_forwards {
            let rule = compile_reverse_forward(reverse_forward)?;
            if !names.insert(rule.name.clone()) {
                bail!("规则名重复: {}", rule.name);
            }

            let endpoint = (rule.distro.to_ascii_lowercase(), rule.listen.port);
            if !reverse_endpoints.insert(endpoint) {
                bail!(
                    "反向规则在发行版 `{}` 上重复监听端口: {}",
                    rule.distro,
                    rule.listen.port
                );
            }
            reverse_forwards.push(rule);
        }

        Ok(Self {
            daemon: DaemonSettings {
                connect_timeout: Duration::from_secs(file.daemon.connect_timeout_seconds),
                max_connections_per_rule: file.daemon.max_connections_per_rule,
            },
            forwards,
            reverse_forwards,
        })
    }
}

fn compile_reverse_forward(file: ReverseForwardFile) -> Result<ReverseForwardRule> {
    let name = required_value("规则名", file.name)?;
    let distro = required_value("发行版名", file.distro)?;
    if file.listen.port == 0 {
        bail!("反向规则 `{name}` 的监听端口必须大于 0");
    }
    if file.target.port == 0 {
        bail!("反向规则 `{name}` 的目标端口必须大于 0");
    }

    Ok(ReverseForwardRule {
        name,
        distro,
        listen: ReverseListenSpec {
            port: file.listen.port,
        },
        target: ReverseTargetSpec {
            port: file.target.port,
        },
    })
}

fn compile_forward(file: ForwardFile) -> Result<ForwardRule> {
    let name = required_value("规则名", file.name)?;
    let distro = required_value("发行版名", file.distro)?;
    if file.allow.is_empty() {
        bail!("规则 `{name}` 的 allow 不能为空");
    }
    if file.listen.port == 0 {
        bail!("规则 `{name}` 的监听端口必须大于 0");
    }
    if file.target.port == 0 {
        bail!("规则 `{name}` 的目标端口必须大于 0");
    }

    let source = match (file.listen.address, file.listen.interface) {
        (Some(address), None) => {
            let address = Ipv4Addr::from_str(address.trim())
                .with_context(|| format!("规则 `{name}` 的监听地址无效"))?;
            ListenSource::Address(address)
        }
        (None, Some(interface)) => ListenSource::Interface(required_value("网卡名", interface)?),
        (Some(_), Some(_)) => bail!("规则 `{name}` 的 listen.address 与 listen.interface 只能指定一个"),
        (None, None) => bail!("规则 `{name}` 必须指定 listen.address 或 listen.interface"),
    };

    let mut allow = Vec::with_capacity(file.allow.len());
    for source in file.allow {
        let network = IpNet::from_str(source.trim())
            .with_context(|| format!("规则 `{name}` 的 allow CIDR 无效"))?;
        let IpNet::V4(network) = network else {
            bail!("规则 `{name}` 的 allow 只支持 IPv4 CIDR");
        };
        allow.push(network);
    }

    let mode = match file.target.mode {
        TargetModeFile::WslIp => TargetMode::WslIp,
        TargetModeFile::Localhost => TargetMode::Localhost,
    };

    Ok(ForwardRule {
        name,
        distro,
        allow,
        listen: ListenSpec {
            source,
            port: file.listen.port,
        },
        target: TargetSpec {
            mode,
            port: file.target.port,
        },
    })
}

fn required_value(label: &str, value: String) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        bail!("{label} 不能为空");
    }
    Ok(value.to_string())
}

fn resolve_settings_path(explicit_path: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit_path {
        if !path.is_file() {
            bail!("配置文件不存在: {}", path.display());
        }
        return Ok(path);
    }

    let home = dirs_next::home_dir().context("无法确定用户主目录")?;
    let path = home.join(".config").join("wslproxy").join("config.toml");
    if !path.is_file() {
        bail!("配置文件不存在: {}", path.display());
    }
    Ok(path)
}

const fn default_connect_timeout_seconds() -> u64 {
    DEFAULT_CONNECT_TIMEOUT_SECONDS
}

const fn default_max_connections_per_rule() -> usize {
    DEFAULT_MAX_CONNECTIONS_PER_RULE
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::{ListenSource, Settings, TargetMode};

    fn parse(source: &str) -> Settings {
        Settings::parse(source).unwrap()
    }

    fn base_rule() -> &'static str {
        r#"
            [[forwards]]
            name = "ssh"
            distro = "Ubuntu"
            allow = ["192.168.137.0/24"]

            [forwards.listen]
            address = "192.168.137.1"
            port = 2222

            [forwards.target]
            mode = "wsl-ip"
            port = 2222
        "#
    }

    #[test]
    fn parses_a_valid_rule() {
        let settings = parse(base_rule());
        let rule = &settings.forwards[0];

        assert_eq!(rule.name, "ssh");
        assert_eq!(rule.target.mode, TargetMode::WslIp);
        assert!(matches!(
            rule.listen.source,
            ListenSource::Address(address) if address == Ipv4Addr::new(192, 168, 137, 1)
        ));
    }

    #[test]
    fn permits_an_unspecified_listen_address() {
        let source = base_rule().replace("192.168.137.1", "0.0.0.0");
        let settings = parse(&source);
        let rule = &settings.forwards[0];

        assert!(matches!(
            rule.listen.source,
            ListenSource::Address(address) if address == Ipv4Addr::UNSPECIFIED
        ));
    }

    #[test]
    fn rejects_missing_or_ambiguous_listen_source() {
        let missing = base_rule().replace("address = \"192.168.137.1\"\n            ", "");
        assert!(Settings::parse(&missing).is_err());

        let ambiguous = base_rule().replace(
            "address = \"192.168.137.1\"",
            "address = \"192.168.137.1\"\n            interface = \"本地连接* 2\"",
        );
        assert!(Settings::parse(&ambiguous).is_err());
    }

    #[test]
    fn rejects_duplicate_fixed_endpoints() {
        let duplicate = format!("{}\n{}", base_rule(), base_rule().replace("name = \"ssh\"", "name = \"ssh-2\""));
        assert!(Settings::parse(&duplicate).is_err());
    }

    #[test]
    fn rejects_empty_or_invalid_allow_networks() {
        let empty = base_rule().replace("allow = [\"192.168.137.0/24\"]", "allow = []");
        assert!(Settings::parse(&empty).is_err());

        let invalid = base_rule().replace("192.168.137.0/24", "not-a-network");
        assert!(Settings::parse(&invalid).is_err());
    }

    #[test]
    fn parses_localhost_target_mode() {
        let source = base_rule().replace("mode = \"wsl-ip\"", "mode = \"localhost\"");
        let settings = parse(&source);

        assert_eq!(settings.forwards[0].target.mode, TargetMode::Localhost);
    }

    #[test]
    fn parses_forward_and_reverse_rules_together() {
        let source = format!(
            "{}\n\n            [[reverse_forwards]]\n            name = \"windows-http\"\n            distro = \"Ubuntu\"\n\n            [reverse_forwards.listen]\n            port = 20002\n\n            [reverse_forwards.target]\n            port = 8080\n",
            base_rule()
        );
        let settings = parse(&source);

        assert_eq!(settings.forwards.len(), 1);
        assert_eq!(settings.reverse_forwards.len(), 1);
        assert_eq!(settings.reverse_forwards[0].listen.port, 20002);
        assert_eq!(settings.reverse_forwards[0].target.port, 8080);
    }

    #[test]
    fn supports_reverse_only_settings() {
        let source = r#"
            [[reverse_forwards]]
            name = "windows-http"
            distro = "Ubuntu"

            [reverse_forwards.listen]
            port = 20002

            [reverse_forwards.target]
            port = 20002
        "#;
        let settings = parse(source);

        assert!(settings.forwards.is_empty());
        assert_eq!(settings.reverse_forwards.len(), 1);
    }

    #[test]
    fn rejects_cross_direction_name_conflicts() {
        let source = format!(
            "{}\n\n            [[reverse_forwards]]\n            name = \"ssh\"\n            distro = \"Ubuntu\"\n\n            [reverse_forwards.listen]\n            port = 20002\n\n            [reverse_forwards.target]\n            port = 20002\n",
            base_rule()
        );

        assert!(Settings::parse(&source).is_err());
    }

    #[test]
    fn rejects_duplicate_reverse_listener_ports_per_distro() {
        let source = r#"
            [[reverse_forwards]]
            name = "one"
            distro = "Ubuntu"

            [reverse_forwards.listen]
            port = 20002

            [reverse_forwards.target]
            port = 20002

            [[reverse_forwards]]
            name = "two"
            distro = "ubuntu"

            [reverse_forwards.listen]
            port = 20002

            [reverse_forwards.target]
            port = 3000
        "#;

        assert!(Settings::parse(source).is_err());
    }

    #[test]
    fn rejects_invalid_reverse_ports() {
        let source = r#"
            [[reverse_forwards]]
            name = "windows-http"
            distro = "Ubuntu"

            [reverse_forwards.listen]
            port = 0

            [reverse_forwards.target]
            port = 20002
        "#;

        assert!(Settings::parse(source).is_err());
    }

    #[test]
    fn rejects_reverse_rules_missing_required_sections() {
        let source = r#"
            [[reverse_forwards]]
            name = "windows-http"
            distro = "Ubuntu"

            [reverse_forwards.listen]
            port = 20002
        "#;

        assert!(Settings::parse(source).is_err());
    }
}
