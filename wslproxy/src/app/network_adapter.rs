use std::ffi::c_void;
use std::net::Ipv4Addr;

use anyhow::{Result, bail};
use windows::Win32::NetworkManagement::IpHelper::{
    GAA_FLAG_INCLUDE_PREFIX, GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN, SOCKET_ADDRESS};

const ERROR_BUFFER_OVERFLOW: u32 = 111;
const INITIAL_BUFFER_SIZE: usize = 15 * 1024;

pub trait AdapterResolver: Send + Sync {
    fn resolve_ipv4(&self, interface_name: &str) -> Result<Ipv4Addr>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemAdapterResolver;

impl AdapterResolver for SystemAdapterResolver {
    fn resolve_ipv4(&self, interface_name: &str) -> Result<Ipv4Addr> {
        let adapters = unsafe { read_system_adapters()? };
        select_ipv4(interface_name, &adapters)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AdapterRecord {
    friendly_name: String,
    ipv4_addresses: Vec<Ipv4Addr>,
}

fn select_ipv4(interface_name: &str, adapters: &[AdapterRecord]) -> Result<Ipv4Addr> {
    let mut addresses = adapters
        .iter()
        .filter(|adapter| adapter.friendly_name.eq_ignore_ascii_case(interface_name))
        .flat_map(|adapter| adapter.ipv4_addresses.iter().copied())
        .filter(|address| !address.is_unspecified() && !address.is_loopback())
        .collect::<Vec<_>>();
    addresses.sort_unstable();
    addresses.dedup();

    match addresses.as_slice() {
        [address] => Ok(*address),
        [] => bail!("找不到网卡 `{interface_name}` 的已启用 IPv4 地址"),
        _ => bail!("网卡 `{interface_name}` 存在多个 IPv4 地址, 请改用 listen.address"),
    }
}

unsafe fn read_system_adapters() -> Result<Vec<AdapterRecord>> {
    let mut buffer_size = INITIAL_BUFFER_SIZE as u32;

    for _ in 0..3 {
        let mut buffer = vec![0_u8; buffer_size as usize];
        let status = unsafe {
            GetAdaptersAddresses(
                AF_INET.0 as u32,
                GAA_FLAG_INCLUDE_PREFIX,
                None::<*const c_void>,
                Some(buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>()),
                &mut buffer_size,
            )
        };

        if status == 0 {
            return unsafe { parse_adapters(buffer.as_ptr().cast()) };
        }
        if status != ERROR_BUFFER_OVERFLOW {
            bail!("GetAdaptersAddresses 失败, Windows 错误码: {status}");
        }
    }

    bail!("GetAdaptersAddresses 缓冲区连续扩容失败")
}

unsafe fn parse_adapters(mut current: *const IP_ADAPTER_ADDRESSES_LH) -> Result<Vec<AdapterRecord>> {
    let mut adapters = Vec::new();

    while let Some(adapter) = unsafe { current.as_ref() } {
        let friendly_name = unsafe { wide_string(adapter.FriendlyName.0) };
        let mut addresses = Vec::new();
        let mut unicast = adapter.FirstUnicastAddress;

        while let Some(address) = unsafe { unicast.as_ref() } {
            if let Some(ipv4) = unsafe { socket_address_to_ipv4(&address.Address) } {
                addresses.push(ipv4);
            }
            unicast = address.Next;
        }

        if !friendly_name.is_empty() {
            adapters.push(AdapterRecord {
                friendly_name,
                ipv4_addresses: addresses,
            });
        }
        current = adapter.Next;
    }

    Ok(adapters)
}

unsafe fn wide_string(pointer: *const u16) -> String {
    if pointer.is_null() {
        return String::new();
    }

    let mut length = 0;
    while unsafe { *pointer.add(length) } != 0 {
        length += 1;
    }
    let value = unsafe { std::slice::from_raw_parts(pointer, length) };
    String::from_utf16_lossy(value)
}

unsafe fn socket_address_to_ipv4(address: &SOCKET_ADDRESS) -> Option<Ipv4Addr> {
    let sockaddr = address.lpSockaddr.cast::<SOCKADDR_IN>();
    let sockaddr = unsafe { sockaddr.as_ref()? };
    if sockaddr.sin_family != AF_INET {
        return None;
    }

    let octets = unsafe {
        std::slice::from_raw_parts(std::ptr::addr_of!(sockaddr.sin_addr).cast::<u8>(), 4)
    };
    Some(Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]))
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::{AdapterRecord, select_ipv4};

    #[test]
    fn resolves_a_single_address_case_insensitively() {
        let adapters = [AdapterRecord {
            friendly_name: "本地连接* 2".to_string(),
            ipv4_addresses: vec![Ipv4Addr::new(192, 168, 137, 1)],
        }];

        assert_eq!(
            select_ipv4("本地连接* 2", &adapters).unwrap(),
            Ipv4Addr::new(192, 168, 137, 1)
        );
    }

    #[test]
    fn rejects_missing_or_ambiguous_addresses() {
        let missing = [AdapterRecord {
            friendly_name: "Ethernet".to_string(),
            ipv4_addresses: Vec::new(),
        }];
        assert!(select_ipv4("Ethernet", &missing).is_err());

        let ambiguous = [AdapterRecord {
            friendly_name: "Ethernet".to_string(),
            ipv4_addresses: vec![
                Ipv4Addr::new(192, 168, 1, 1),
                Ipv4Addr::new(192, 168, 2, 1),
            ],
        }];
        assert!(select_ipv4("Ethernet", &ambiguous).is_err());
    }
}
