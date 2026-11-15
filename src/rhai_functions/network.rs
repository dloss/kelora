//! Networking helpers for Rhai scripts.
//!
//! Includes IP validation, CIDR matching, and private range checks.

use ipnet::IpNet;
use rhai::{Engine, EvalAltResult, ImmutableString};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

// ============================================================================
// IP Validation
// ============================================================================

/// Check if a string is a valid IPv4 address
/// Returns false for invalid input
/// Usage: is_ipv4(e.client_ip)
pub fn is_ipv4(ip: ImmutableString) -> bool {
    if let Ok(addr) = IpAddr::from_str(ip.as_str()) {
        matches!(addr, IpAddr::V4(_))
    } else {
        false
    }
}

/// Check if a string is a valid IPv6 address
/// Returns false for invalid input
/// Usage: is_ipv6(e.client_ip)
pub fn is_ipv6(ip: ImmutableString) -> bool {
    if let Ok(addr) = IpAddr::from_str(ip.as_str()) {
        matches!(addr, IpAddr::V6(_))
    } else {
        false
    }
}

// ============================================================================
// CIDR Matching
// ============================================================================

/// Check if an IP address is in a CIDR network
/// Returns error for invalid CIDR format
/// Usage: is_in_cidr(e.client_ip, "10.0.0.0/8")
pub fn is_in_cidr(ip: ImmutableString, cidr: ImmutableString) -> Result<bool, Box<EvalAltResult>> {
    // Parse the IP address
    let addr = IpAddr::from_str(ip.as_str()).map_err(|_| {
        let message = if ip.starts_with("<ip:") {
            // Left by --pseudonymize ip / pseudonymize_ips(): nothing to test
            format!(
                "'{}' is a pseudonym, not an IP address: a token keeps no network. To filter by network, use --pseudonymize ip-prefix and is_in_cidr(\"{}\".pseudonymize_ips(\"prefix\"))",
                ip, cidr
            )
        } else {
            format!("Invalid IP address: '{}'", ip)
        };
        EvalAltResult::ErrorRuntime(message.into(), rhai::Position::NONE)
    })?;

    // Parse the CIDR network
    let network = IpNet::from_str(cidr.as_str()).map_err(|_| {
        EvalAltResult::ErrorRuntime(
            format!("Invalid CIDR format: '{}'", cidr).into(),
            rhai::Position::NONE,
        )
    })?;

    // Check if the address is in the network
    Ok(network.contains(&addr))
}

// ============================================================================
// IP Masking
// ============================================================================

/// Mask IP address for privacy while preserving the network prefix.
fn mask_ip_impl(ip: &str, octets_to_mask: usize) -> String {
    match IpAddr::from_str(ip) {
        Ok(IpAddr::V4(addr)) => mask_ipv4(addr, octets_to_mask).to_string(),
        Ok(IpAddr::V6(addr)) => mask_ipv6(addr, octets_to_mask).to_string(),
        Err(_) => ip.to_string(),
    }
}

fn mask_ipv4(addr: Ipv4Addr, octets_to_mask: usize) -> Ipv4Addr {
    let mut octets = addr.octets();
    let mask_count = octets_to_mask.clamp(1, 4);
    for item in octets.iter_mut().skip(4 - mask_count) {
        *item = 0;
    }
    Ipv4Addr::from(octets)
}

fn mask_ipv6(addr: Ipv6Addr, hextets_to_mask: usize) -> Ipv6Addr {
    let mut segments = addr.segments();
    let mask_count = hextets_to_mask.clamp(1, 8);
    for item in segments.iter_mut().skip(8 - mask_count) {
        *item = 0;
    }
    Ipv6Addr::from(segments)
}

/// Check if IP address is in a private/internal range.
fn is_private_ip_impl(ip: &str) -> bool {
    match IpAddr::from_str(ip) {
        Ok(IpAddr::V4(addr)) => addr.is_private() || addr.is_loopback(),
        Ok(IpAddr::V6(addr)) => {
            addr.is_unique_local() || addr.is_unicast_link_local() || addr.is_loopback()
        }
        Err(_) => false,
    }
}

// ============================================================================
// Registration
// ============================================================================

/// Register network functions with the Rhai engine
pub fn register_functions(engine: &mut Engine) {
    // IPv4 validation
    engine.register_fn("is_ipv4", |ip: ImmutableString| -> bool { is_ipv4(ip) });

    // IPv6 validation
    engine.register_fn("is_ipv6", |ip: ImmutableString| -> bool { is_ipv6(ip) });

    // CIDR matching
    engine.register_fn(
        "is_in_cidr",
        |ip: ImmutableString, cidr: ImmutableString| -> Result<bool, Box<EvalAltResult>> {
            is_in_cidr(ip, cidr)
        },
    );

    // IP masking
    engine.register_fn("mask_ip", |ip: &str| -> String {
        mask_ip_impl(ip, 1) // Default: mask last octet
    });

    engine.register_fn("mask_ip", |ip: &str, octets: i64| -> String {
        mask_ip_impl(ip, octets.max(1) as usize)
    });

    // Private IP detection
    engine.register_fn("is_private_ip", |ip: &str| -> bool {
        is_private_ip_impl(ip)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_ipv4() {
        assert!(is_ipv4("192.168.1.1".into()));
        assert!(is_ipv4("10.0.0.1".into()));
        assert!(is_ipv4("0.0.0.0".into()));
        assert!(is_ipv4("255.255.255.255".into()));

        // Invalid cases
        assert!(!is_ipv4("not-an-ip".into()));
        assert!(!is_ipv4("256.1.1.1".into()));
        assert!(!is_ipv4("".into()));
        assert!(!is_ipv4("2001:db8::1".into())); // IPv6
    }

    #[test]
    fn test_is_ipv6() {
        assert!(is_ipv6("2001:db8::1".into()));
        assert!(is_ipv6("::1".into()));
        assert!(is_ipv6("fe80::1".into()));
        assert!(is_ipv6("2001:0db8:0000:0000:0000:0000:0000:0001".into()));

        // Invalid cases
        assert!(!is_ipv6("not-an-ip".into()));
        assert!(!is_ipv6("".into()));
        assert!(!is_ipv6("192.168.1.1".into())); // IPv4
    }

    #[test]
    fn test_is_in_cidr_ipv4() {
        // Matching cases
        assert!(is_in_cidr("192.168.1.1".into(), "192.168.1.0/24".into()).unwrap());
        assert!(is_in_cidr("10.0.0.1".into(), "10.0.0.0/8".into()).unwrap());
        assert!(is_in_cidr("172.16.5.4".into(), "172.16.0.0/16".into()).unwrap());

        // Non-matching cases
        assert!(!is_in_cidr("192.168.2.1".into(), "192.168.1.0/24".into()).unwrap());
        assert!(!is_in_cidr("11.0.0.1".into(), "10.0.0.0/8".into()).unwrap());

        // Edge cases
        assert!(is_in_cidr("192.168.1.1".into(), "192.168.1.1/32".into()).unwrap());
        assert!(is_in_cidr("0.0.0.0".into(), "0.0.0.0/0".into()).unwrap()); // Match all
    }

    #[test]
    fn test_is_in_cidr_ipv6() {
        // Matching cases
        assert!(is_in_cidr("2001:db8::1".into(), "2001:db8::/32".into()).unwrap());
        assert!(is_in_cidr("fe80::1".into(), "fe80::/10".into()).unwrap());

        // Non-matching cases
        assert!(!is_in_cidr("2001:db9::1".into(), "2001:db8::/32".into()).unwrap());
    }

    #[test]
    fn test_is_in_cidr_errors() {
        // Invalid IP
        assert!(is_in_cidr("not-an-ip".into(), "192.168.1.0/24".into()).is_err());

        // Invalid CIDR
        assert!(is_in_cidr("192.168.1.1".into(), "not-a-cidr".into()).is_err());
        assert!(is_in_cidr("192.168.1.1".into(), "192.168.1.0/33".into()).is_err());
    }

    #[test]
    fn test_is_in_cidr_mixed() {
        // IPv4 address against IPv6 network (no match)
        assert!(!is_in_cidr("192.168.1.1".into(), "2001:db8::/32".into()).unwrap());

        // IPv6 address against IPv4 network (no match)
        assert!(!is_in_cidr("2001:db8::1".into(), "192.168.1.0/24".into()).unwrap());
    }

    #[test]
    fn test_mask_ip_function() {
        use rhai::Scope;

        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("ip", "192.168.1.100");
        scope.push("invalid", "not-an-ip");

        let result: String = engine
            .eval_with_scope(&mut scope, r##"ip.mask_ip()"##)
            .unwrap();
        assert_eq!(result, "192.168.1.0");

        let result: String = engine
            .eval_with_scope(&mut scope, r##"ip.mask_ip(2)"##)
            .unwrap();
        assert_eq!(result, "192.168.0.0");

        let result: String = engine
            .eval_with_scope(&mut scope, r##"ip.mask_ip(3)"##)
            .unwrap();
        assert_eq!(result, "192.0.0.0");

        let result: String = engine
            .eval_with_scope(&mut scope, r##"ip.mask_ip(4)"##)
            .unwrap();
        assert_eq!(result, "0.0.0.0");

        // Invalid IP should return unchanged
        let result: String = engine
            .eval_with_scope(&mut scope, r##"invalid.mask_ip()"##)
            .unwrap();
        assert_eq!(result, "not-an-ip");

        // Edge case: 0 octets should clamp to 1
        let result: String = engine
            .eval_with_scope(&mut scope, r##"ip.mask_ip(0)"##)
            .unwrap();
        assert_eq!(result, "192.168.1.0");
        // Edge case: more than 4 should clamp to 4
        let result: String = engine
            .eval_with_scope(&mut scope, r##"ip.mask_ip(10)"##)
            .unwrap();
        assert_eq!(result, "0.0.0.0");
    }

    #[test]
    fn test_mask_ipv6_function() {
        use rhai::Scope;

        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("ip", "2001:db8:1:2:3:4:5:6");

        let result: String = engine
            .eval_with_scope(&mut scope, r##"ip.mask_ip()"##)
            .unwrap();
        assert_eq!(result, "2001:db8:1:2:3:4:5:0");

        let result: String = engine
            .eval_with_scope(&mut scope, r##"ip.mask_ip(2)"##)
            .unwrap();
        assert_eq!(result, "2001:db8:1:2:3:4::");

        let result: String = engine
            .eval_with_scope(&mut scope, r##"ip.mask_ip(8)"##)
            .unwrap();
        assert_eq!(result, "::");
    }

    #[test]
    fn test_is_private_ip_function() {
        use rhai::Scope;

        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("private1", "10.0.0.1");
        scope.push("private2", "172.16.0.1");
        scope.push("private3", "192.168.1.1");
        scope.push("loopback", "127.0.0.1");
        scope.push("private_v6", "fd12:3456:789a::1");
        scope.push("link_local_v6", "fe80::1234");
        scope.push("loopback_v6", "::1");
        scope.push("public1", "8.8.8.8");
        scope.push("public2", "1.1.1.1");
        scope.push("public_v6", "2001:4860:4860::8888");
        scope.push("edge1", "172.15.0.1"); // Not private (172.15 is outside 172.16-31)
        scope.push("edge2", "172.32.0.1"); // Not private (172.32 is outside 172.16-31)
        scope.push("invalid", "not-an-ip");

        let result: bool = engine
            .eval_with_scope(&mut scope, r##"private1.is_private_ip()"##)
            .unwrap();
        assert!(result);

        let result: bool = engine
            .eval_with_scope(&mut scope, r##"private2.is_private_ip()"##)
            .unwrap();
        assert!(result);

        let result: bool = engine
            .eval_with_scope(&mut scope, r##"private3.is_private_ip()"##)
            .unwrap();
        assert!(result);

        let result: bool = engine
            .eval_with_scope(&mut scope, r##"loopback.is_private_ip()"##)
            .unwrap();
        assert!(result);

        let result: bool = engine
            .eval_with_scope(&mut scope, r##"private_v6.is_private_ip()"##)
            .unwrap();
        assert!(result);

        let result: bool = engine
            .eval_with_scope(&mut scope, r##"link_local_v6.is_private_ip()"##)
            .unwrap();
        assert!(result);

        let result: bool = engine
            .eval_with_scope(&mut scope, r##"loopback_v6.is_private_ip()"##)
            .unwrap();
        assert!(result);

        // Public IPs
        let result: bool = engine
            .eval_with_scope(&mut scope, r##"public1.is_private_ip()"##)
            .unwrap();
        assert!(!result);

        let result: bool = engine
            .eval_with_scope(&mut scope, r##"public2.is_private_ip()"##)
            .unwrap();
        assert!(!result);

        let result: bool = engine
            .eval_with_scope(&mut scope, r##"public_v6.is_private_ip()"##)
            .unwrap();
        assert!(!result);

        // Edge cases for 172.x.x.x range
        let result: bool = engine
            .eval_with_scope(&mut scope, r##"edge1.is_private_ip()"##)
            .unwrap();
        assert!(!result);

        let result: bool = engine
            .eval_with_scope(&mut scope, r##"edge2.is_private_ip()"##)
            .unwrap();
        assert!(!result);

        // Invalid IP
        let result: bool = engine
            .eval_with_scope(&mut scope, r##"invalid.is_private_ip()"##)
            .unwrap();
        assert!(!result);
    }
}
