//! IP address utility functions for Rhai scripts.
//!
//! Provides functions for masking and analyzing IP addresses.

use rhai::Engine;

/// Mask IP address for privacy (replace last N octets with 'X')
fn mask_ip_impl(ip: &str, octets_to_mask: usize) -> String {
    // IPv4 pattern validation
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return ip.to_string(); // Return unchanged if not valid IPv4
    }

    // Validate each octet is numeric
    for part in &parts {
        if part.parse::<u8>().is_err() {
            return ip.to_string(); // Return unchanged if not numeric
        }
    }

    let mut result = parts.clone();
    let mask_count = octets_to_mask.clamp(1, 4);

    // Replace last N octets with 'X'
    for item in result.iter_mut().skip(4 - mask_count) {
        *item = "X";
    }

    result.join(".")
}

/// Check if IP address is in private range
fn is_private_ip_impl(ip: &str) -> bool {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return false; // Not valid IPv4
    }

    // Parse octets
    let octets: Result<Vec<u8>, _> = parts.iter().map(|s| s.parse::<u8>()).collect();
    let octets = match octets {
        Ok(o) => o,
        Err(_) => return false,
    };

    // Check private ranges
    match octets[0] {
        10 => true,                                // 10.0.0.0/8
        172 => octets[1] >= 16 && octets[1] <= 31, // 172.16.0.0/12
        192 => octets[1] == 168,                   // 192.168.0.0/16
        127 => true,                               // 127.0.0.0/8 (loopback)
        _ => false,
    }
}

/// Register IP utility functions with the Rhai engine
pub fn register_functions(engine: &mut Engine) {
    engine.register_fn("mask_ip", |ip: &str| -> String {
        mask_ip_impl(ip, 1) // Default: mask last octet
    });

    engine.register_fn("mask_ip", |ip: &str, octets: i64| -> String {
        mask_ip_impl(ip, octets.clamp(1, 4) as usize) // Clamp between 1-4
    });

    engine.register_fn("is_private_ip", |ip: &str| -> bool {
        is_private_ip_impl(ip)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhai::Scope;

    #[test]
    fn test_mask_ip_function() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("ip", "192.168.1.100");
        scope.push("invalid", "not-an-ip");

        let result: String = engine
            .eval_with_scope(&mut scope, r##"ip.mask_ip()"##)
            .unwrap();
        assert_eq!(result, "192.168.1.X");

        let result: String = engine
            .eval_with_scope(&mut scope, r##"ip.mask_ip(2)"##)
            .unwrap();
        assert_eq!(result, "192.168.X.X");

        let result: String = engine
            .eval_with_scope(&mut scope, r##"ip.mask_ip(3)"##)
            .unwrap();
        assert_eq!(result, "192.X.X.X");

        let result: String = engine
            .eval_with_scope(&mut scope, r##"ip.mask_ip(4)"##)
            .unwrap();
        assert_eq!(result, "X.X.X.X");

        // Invalid IP should return unchanged
        let result: String = engine
            .eval_with_scope(&mut scope, r##"invalid.mask_ip()"##)
            .unwrap();
        assert_eq!(result, "not-an-ip");

        // Edge case: 0 octets should clamp to 1
        let result: String = engine
            .eval_with_scope(&mut scope, r##"ip.mask_ip(0)"##)
            .unwrap();
        assert_eq!(result, "192.168.1.X");
        // Edge case: more than 4 should clamp to 4
        let result: String = engine
            .eval_with_scope(&mut scope, r##"ip.mask_ip(10)"##)
            .unwrap();
        assert_eq!(result, "X.X.X.X");
    }

    #[test]
    fn test_is_private_ip_function() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("private1", "10.0.0.1");
        scope.push("private2", "172.16.0.1");
        scope.push("private3", "192.168.1.1");
        scope.push("loopback", "127.0.0.1");
        scope.push("public1", "8.8.8.8");
        scope.push("public2", "1.1.1.1");
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

        // Public IPs
        let result: bool = engine
            .eval_with_scope(&mut scope, r##"public1.is_private_ip()"##)
            .unwrap();
        assert!(!result);

        let result: bool = engine
            .eval_with_scope(&mut scope, r##"public2.is_private_ip()"##)
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
