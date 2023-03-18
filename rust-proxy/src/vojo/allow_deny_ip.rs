use ipnet::Ipv4Net;
use iprange::IpRange;
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AllowDenyObject {
    pub limit_type: AllowType,
    pub value: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum AllowType {
    #[default]
    ALLOWALL,
    DENYWALL,
    ALLOW,
    DENY,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum AllowResult {
    #[default]
    ALLOW,
    DENY,
    NOTMAPPING,
}
impl AllowDenyObject {
    pub fn is_allow(&self, client_ip: String) -> Result<AllowResult, anyhow::Error> {
        if self.limit_type == AllowType::ALLOWALL {
            return Ok(AllowResult::ALLOW);
        }
        if self.limit_type == AllowType::DENYWALL {
            return Ok(AllowResult::DENY);
        }
        if self.value == None {
            return Err(anyhow!(
                "the value counld not be none when the limit_type is not AllowAll or DenyAll!"
            ));
        }
        let config_ip = self.value.clone().unwrap();
        let value_mapped_ip;
        if config_ip.contains("/") {
            let ip_range: IpRange<Ipv4Net> = [config_ip.clone()]
                .iter()
                .map(|s| s.parse().unwrap())
                .collect();
            let source_ip = client_ip.clone().parse::<Ipv4Addr>().unwrap();
            if ip_range.contains(&source_ip) {
                value_mapped_ip = true;
            } else {
                value_mapped_ip = false;
            }
        } else {
            if self.value.clone().unwrap() == client_ip {
                value_mapped_ip = true;
            } else {
                value_mapped_ip = false;
            }
        }
        if value_mapped_ip && self.limit_type == AllowType::ALLOW {
            return Ok(AllowResult::ALLOW);
        }
        if value_mapped_ip && self.limit_type == AllowType::DENY {
            return Ok(AllowResult::DENY);
        }

        Ok(AllowResult::NOTMAPPING)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_allow_allow_all() {
        let allow_object = AllowDenyObject {
            limit_type: AllowType::ALLOWALL,
            value: None,
        };
        let result = allow_object.is_allow(String::from("test"));
        assert_eq!(result.is_ok(), true);
        assert_eq!(result.unwrap(), AllowResult::ALLOW);
    }
    #[test]
    fn test_is_allow_deny_all() {
        let allow_object = AllowDenyObject {
            limit_type: AllowType::DENYWALL,
            value: None,
        };
        let result = allow_object.is_allow(String::from("test"));
        assert_eq!(result.is_ok(), true);
        assert_eq!(result.unwrap(), AllowResult::DENY);
    }
    #[test]
    fn test_is_allow_allow_ip() {
        let allow_object = AllowDenyObject {
            limit_type: AllowType::ALLOW,
            value: Some(String::from("192.168.0.1")),
        };
        let result = allow_object.is_allow(String::from("192.168.0.1"));
        assert_eq!(result.is_ok(), true);
        assert_eq!(result.unwrap(), AllowResult::ALLOW);
    }
    #[test]
    fn test_is_allow_allow_ip_range() {
        let allow_object = AllowDenyObject {
            limit_type: AllowType::ALLOW,
            value: Some(String::from("192.168.0.1/24")),
        };
        let result1 = allow_object.is_allow(String::from("192.168.0.254"));
        assert_eq!(result1.is_ok(), true);
        assert_eq!(result1.unwrap(), AllowResult::ALLOW);

        let result2 = allow_object.is_allow(String::from("192.168.0.1"));
        assert_eq!(result2.is_ok(), true);
        assert_eq!(result2.unwrap(), AllowResult::ALLOW);
    }
    #[test]
    fn test_is_allow_deny_ip() {
        let allow_object = AllowDenyObject {
            limit_type: AllowType::DENY,
            value: Some(String::from("192.168.0.1")),
        };
        let result = allow_object.is_allow(String::from("192.168.0.1"));
        assert_eq!(result.is_ok(), true);
        assert_eq!(result.unwrap(), AllowResult::DENY);
    }
    #[test]
    fn test_is_allow_deny_ip_range() {
        let allow_object = AllowDenyObject {
            limit_type: AllowType::DENY,
            value: Some(String::from("192.168.0.1/16")),
        };
        let result1 = allow_object.is_allow(String::from("192.168.255.254"));
        assert_eq!(result1.is_ok(), true);
        assert_eq!(result1.unwrap(), AllowResult::DENY);

        let result2 = allow_object.is_allow(String::from("192.168.0.1"));
        assert_eq!(result2.is_ok(), true);
        assert_eq!(result2.unwrap(), AllowResult::DENY);
    }

    #[test]
    fn test_is_allow_not_mapping1() {
        let allow_object = AllowDenyObject {
            limit_type: AllowType::ALLOW,
            value: Some(String::from("192.168.0.1")),
        };
        let result = allow_object.is_allow(String::from("192.168.3.4"));
        assert_eq!(result.is_ok(), true);
        assert_eq!(result.unwrap(), AllowResult::NOTMAPPING);
    }
    #[test]
    fn test_is_allow_not_mapping2() {
        let allow_object = AllowDenyObject {
            limit_type: AllowType::DENY,
            value: Some(String::from("192.168.0.1")),
        };
        let result = allow_object.is_allow(String::from("192.168.3.4"));
        assert_eq!(result.is_ok(), true);
        assert_eq!(result.unwrap(), AllowResult::NOTMAPPING);
    }
}
