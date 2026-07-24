use std::net::Ipv4Addr;

use ipnet::Ipv4Net;

pub fn allows(networks: &[Ipv4Net], address: Ipv4Addr) -> bool {
    networks.iter().any(|network| network.contains(&address))
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use ipnet::Ipv4Net;

    use super::allows;

    #[test]
    fn allows_addresses_in_any_configured_subnet() {
        let networks = [
            "192.168.137.0/24".parse::<Ipv4Net>().unwrap(),
            "10.0.0.0/8".parse::<Ipv4Net>().unwrap(),
        ];

        assert!(allows(&networks, Ipv4Addr::new(192, 168, 137, 45)));
        assert!(allows(&networks, Ipv4Addr::new(10, 20, 30, 40)));
        assert!(!allows(&networks, Ipv4Addr::new(192, 168, 138, 45)));
    }
}
