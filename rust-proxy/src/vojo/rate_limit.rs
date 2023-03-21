use std::time::{SystemTime, UNIX_EPOCH};

use core::fmt::Debug;
use dashmap::DashMap;
use dyn_clone::DynClone;
use http::HeaderMap;
use http::HeaderValue;
use ipnet::Ipv4Net;
use iprange::IpRange;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
#[typetag::serde(tag = "type")]
pub trait RatelimitStrategy: Sync + Send + DynClone {
    fn should_limit(
        &mut self,
        headers: HeaderMap<HeaderValue>,
        remote_ip: String,
    ) -> Result<bool, anyhow::Error>;

    fn get_debug(&self) -> String {
        String::from("debug")
    }
    fn as_any(&self) -> &dyn Any;
}
dyn_clone::clone_trait_object!(RatelimitStrategy);

impl Debug for dyn RatelimitStrategy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let routes = self.get_debug().clone();
        write!(f, "{{{}}}", routes)
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IPBasedRatelimit {
    pub value: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeaderBasedRatelimit {
    pub key: String,
    pub value: String,
}
impl HeaderBasedRatelimit {
    fn get_key(&self) -> String {
        format!("{}:{}", self.key, self.value)
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpRangeBasedRatelimit {
    pub value: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LimitLocation {
    IP(IPBasedRatelimit),
    Header(HeaderBasedRatelimit),
    IPRANGE(IpRangeBasedRatelimit),
}
impl LimitLocation {
    pub fn get_key(&self) -> String {
        match self {
            LimitLocation::Header(headers) => headers.get_key(),
            LimitLocation::IP(ip) => ip.value.clone(),
            LimitLocation::IPRANGE(ip_range) => ip_range.value.clone(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TimeUnit {
    MillionSecond,
    Second,
    Minute,
    Hour,
    Day,
}
impl TimeUnit {
    pub fn get_million_second(&self) -> u128 {
        match self {
            Self::MillionSecond => 1,
            Self::Second => 1_000,
            Self::Minute => 60_000,
            Self::Hour => 3_600_000,
            Self::Day => 86_400_000,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBucketRateLimit {
    pub rate_per_unit: u128,
    pub unit: TimeUnit,
    pub capacity: i32,
    pub limit_location: LimitLocation,
    #[serde(skip_serializing, skip_deserializing)]
    pub current_count: Arc<AtomicIsize>,
    #[serde(skip_serializing, skip_deserializing)]
    pub lock: Arc<Mutex<i32>>,
    #[serde(skip_serializing, skip_deserializing, default = "default_time")]
    pub last_update_time: SystemTime,
}

fn default_time() -> SystemTime {
    SystemTime::now()
}
fn get_time_key(time_unit: TimeUnit) -> Result<String, anyhow::Error> {
    let current_time = SystemTime::now();
    let since_the_epoch = current_time
        .duration_since(UNIX_EPOCH)
        .map_err(|err| anyhow!(err.to_string()))?;
    let in_ms =
        since_the_epoch.as_secs() * 1000 + since_the_epoch.subsec_nanos() as u64 / 1_000_000;
    let key_u64 = match time_unit {
        TimeUnit::MillionSecond => in_ms,
        TimeUnit::Second => in_ms / 1000,
        TimeUnit::Minute => in_ms / 60000,
        TimeUnit::Hour => in_ms / 3600000,
        TimeUnit::Day => in_ms / 86400000,
    };
    Ok(key_u64.to_string())
}

fn matched(
    limit_location: LimitLocation,
    headers: HeaderMap<HeaderValue>,
    remote_ip: String,
) -> Result<bool, anyhow::Error> {
    return match limit_location {
        LimitLocation::IP(ip_based_ratelimit) => {
            Ok(ip_based_ratelimit.clone().value.clone() == remote_ip)
        }
        LimitLocation::Header(header_based_ratelimit) => {
            if !headers.contains_key(header_based_ratelimit.key.clone()) {
                return Ok(false);
            }
            let header_value = headers.get(header_based_ratelimit.key.clone()).unwrap();
            let header_value_str = header_value
                .to_str()
                .map_err(|err| anyhow!(err.to_string()))?;

            return Ok(header_value_str == header_based_ratelimit.value);
        }
        LimitLocation::IPRANGE(ip_range_based_ratelimit) => {
            if !ip_range_based_ratelimit.value.contains("/") {
                return Err(anyhow!("The Ip Range should contain '/'."));
            }
            let ip_range: IpRange<Ipv4Net> = [ip_range_based_ratelimit.value.clone()]
                .iter()
                .map(|s| s.parse().unwrap())
                .collect();
            let source_ip = remote_ip
                .clone()
                .parse::<Ipv4Addr>()
                .map_err(|err| anyhow!(err.to_string()))?;
            return Ok(ip_range.contains(&source_ip));
        }
    };
}
#[typetag::serde]
impl RatelimitStrategy for TokenBucketRateLimit {
    fn should_limit(
        &mut self,
        headers: HeaderMap<HeaderValue>,
        remote_ip: String,
    ) -> Result<bool, anyhow::Error> {
        let match_or_not = matched(self.limit_location.clone(), headers, remote_ip)?;
        if !match_or_not {
            return Ok(false);
        }
        let current_value = self.current_count.fetch_sub(1, Ordering::SeqCst);
        if current_value <= 0 {
            let elapsed = self
                .last_update_time
                .elapsed()
                .map_err(|err| anyhow!(err.to_string()))?;
            let elapsed_millis = elapsed.as_millis();
            let mut added_count =
                elapsed_millis * self.rate_per_unit / self.unit.get_million_second();
            if added_count == 0 {
                return Ok(true);
            }
            let _res = self.lock.lock().map_err(|err| anyhow!(err.to_string()))?;
            if (self.current_count.load(Ordering::SeqCst) as i32) < 0 {
                if added_count > (self.capacity as u128) {
                    added_count = self.capacity as u128;
                }
                self.current_count = Arc::new(AtomicIsize::new(added_count as isize));
                self.last_update_time = SystemTime::now();
            }
            drop(_res);
            let current_value = self.current_count.fetch_sub(1, Ordering::SeqCst);
            if current_value <= 0 {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixedWindowRateLimit {
    pub rate_per_unit: u128,
    pub unit: TimeUnit,
    pub limit_location: LimitLocation,
    #[serde(skip_serializing, skip_deserializing)]
    pub count_map: DashMap<String, Arc<AtomicIsize>>,
    #[serde(skip_serializing, skip_deserializing)]
    pub lock: Arc<Mutex<i32>>,
}
#[typetag::serde]
impl RatelimitStrategy for FixedWindowRateLimit {
    fn should_limit(
        &mut self,
        headers: HeaderMap<HeaderValue>,
        remote_ip: String,
    ) -> Result<bool, anyhow::Error> {
        let match_or_not = matched(self.limit_location.clone(), headers, remote_ip)?;
        if !match_or_not {
            return Ok(false);
        }
        let time_unit_key = get_time_key(self.unit.clone())?;
        let location_key = self.limit_location.get_key();
        let key = format!("{}:{}", location_key, time_unit_key);
        if !self.count_map.contains_key(key.clone().as_str()) {
            let _lock = self.lock.lock().map_err(|err| anyhow!(err.to_string()))?;
            if !self.count_map.contains_key(key.clone().as_str()) {
                if self.count_map.len() > 100 {
                    let first = self.count_map.iter().next().unwrap();
                    let first_key = first.key().clone();
                    drop(first);
                    self.count_map.remove(first_key.as_str());
                }
                self.count_map
                    .insert(key.clone(), Arc::new(AtomicIsize::new(0)));
            }
        }
        let atomic_isize = self.count_map.get(key.as_str()).ok_or(anyhow!(
            "Can not find the key in the map of FixedWindowRateLimit!"
        ))?;
        let res = atomic_isize.fetch_add(1, Ordering::SeqCst);
        if res as i32 >= self.rate_per_unit as i32 {
            return Ok(true);
        }
        return Ok(false);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::vojo::app_config::ApiService;
    use std::{thread, time};

    #[test]
    fn test_token_bucket_rate_limit_ok1() {
        let mut token_bucket_ratelimit = TokenBucketRateLimit {
            rate_per_unit: 3,
            capacity: 10000,
            unit: TimeUnit::Minute,
            limit_location: LimitLocation::IP(IPBasedRatelimit {
                value: String::from("192.168.0.0"),
            }),
            current_count: Arc::new(AtomicIsize::new(3)),
            lock: Arc::new(Mutex::new(0)),
            last_update_time: SystemTime::now(),
        };
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("api_key", "test2".parse().unwrap());
        let res1 =
            token_bucket_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res1.unwrap(), false);
        let res2 =
            token_bucket_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res2.unwrap(), false);
        let res3 =
            token_bucket_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res3.unwrap(), false);

        let res4 =
            token_bucket_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res4.unwrap(), true);
    }
    #[test]
    fn test_token_bucket_rate_limit_ok2() {
        let mut token_bucket_ratelimit = TokenBucketRateLimit {
            rate_per_unit: 3,
            capacity: 10000,
            unit: TimeUnit::Minute,
            limit_location: LimitLocation::IPRANGE(IpRangeBasedRatelimit {
                value: String::from("245.168.0.0/8"),
            }),
            current_count: Arc::new(AtomicIsize::new(3)),
            lock: Arc::new(Mutex::new(0)),
            last_update_time: SystemTime::now(),
        };
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("api_key", "test2".parse().unwrap());
        let res1 =
            token_bucket_ratelimit.should_limit(headermap1.clone(), String::from("245.0.0.1"));
        assert_eq!(res1.unwrap(), false);
        let res2 =
            token_bucket_ratelimit.should_limit(headermap1.clone(), String::from("245.255.0.1"));
        assert_eq!(res2.unwrap(), false);
        let res3 =
            token_bucket_ratelimit.should_limit(headermap1.clone(), String::from("245.255.255.1"));
        assert_eq!(res3.unwrap(), false);

        let res4 = token_bucket_ratelimit
            .should_limit(headermap1.clone(), String::from("245.255.255.255"));
        assert_eq!(res4.unwrap(), true);
    }

    #[test]
    fn test_token_bucket_rate_limit_ok3() {
        let mut token_bucket_ratelimit = TokenBucketRateLimit {
            rate_per_unit: 3,
            capacity: 10000,
            unit: TimeUnit::Minute,
            limit_location: LimitLocation::Header(HeaderBasedRatelimit {
                key: String::from("lsk"),
                value: String::from("test"),
            }),
            current_count: Arc::new(AtomicIsize::new(3)),
            lock: Arc::new(Mutex::new(0)),
            last_update_time: SystemTime::now(),
        };
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("lsk", "test".parse().unwrap());
        let res1 = token_bucket_ratelimit.should_limit(headermap1.clone(), String::from(""));
        assert_eq!(res1.unwrap(), false);
        let res2 = token_bucket_ratelimit.should_limit(headermap1.clone(), String::from(""));
        assert_eq!(res2.unwrap(), false);
        let res3 = token_bucket_ratelimit.should_limit(headermap1.clone(), String::from(""));
        assert_eq!(res3.unwrap(), false);

        let res4 = token_bucket_ratelimit.should_limit(headermap1.clone(), String::from(""));
        assert_eq!(res4.unwrap(), true);
    }
    #[test]
    fn test_token_bucket_rate_limit_ok4() {
        let mut token_bucket_ratelimit = TokenBucketRateLimit {
            rate_per_unit: 3,
            capacity: 10000,
            unit: TimeUnit::Minute,
            limit_location: LimitLocation::Header(HeaderBasedRatelimit {
                key: String::from("lsk"),
                value: String::from("test"),
            }),
            current_count: Arc::new(AtomicIsize::new(3)),
            lock: Arc::new(Mutex::new(0)),
            last_update_time: SystemTime::now(),
        };
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("lsk", "test1".parse().unwrap());
        let res1 = token_bucket_ratelimit.should_limit(headermap1.clone(), String::from(""));
        assert_eq!(res1.unwrap(), false);
        let res2 = token_bucket_ratelimit.should_limit(headermap1.clone(), String::from(""));
        assert_eq!(res2.unwrap(), false);
        let res3 = token_bucket_ratelimit.should_limit(headermap1.clone(), String::from(""));
        assert_eq!(res3.unwrap(), false);

        let res4 = token_bucket_ratelimit.should_limit(headermap1.clone(), String::from(""));
        assert_eq!(res4.unwrap(), false);
    }
    #[test]
    fn test_token_bucket_rate_limit_ok5() {
        let mut token_bucket_ratelimit = TokenBucketRateLimit {
            rate_per_unit: 3,
            capacity: 10000,
            unit: TimeUnit::Minute,
            limit_location: LimitLocation::IPRANGE(IpRangeBasedRatelimit {
                value: String::from("245.168.0.0/8"),
            }),
            current_count: Arc::new(AtomicIsize::new(3)),
            lock: Arc::new(Mutex::new(0)),
            last_update_time: SystemTime::now(),
        };
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("api_key", "test2".parse().unwrap());
        let res1 =
            token_bucket_ratelimit.should_limit(headermap1.clone(), String::from("246.0.0.1"));
        assert_eq!(res1.unwrap(), false);
        let res2 =
            token_bucket_ratelimit.should_limit(headermap1.clone(), String::from("246.255.0.1"));
        assert_eq!(res2.unwrap(), false);
        let res3 =
            token_bucket_ratelimit.should_limit(headermap1.clone(), String::from("246.255.255.1"));
        assert_eq!(res3.unwrap(), false);
        let res4 = token_bucket_ratelimit
            .should_limit(headermap1.clone(), String::from("246.255.255.255"));
        assert_eq!(res4.unwrap(), false);
    }
    #[test]
    fn test_token_bucket_rate_limit_ok7() {
        let mut token_bucket_ratelimit = TokenBucketRateLimit {
            rate_per_unit: 3,
            capacity: 10000,
            unit: TimeUnit::Minute,
            limit_location: LimitLocation::IP(IPBasedRatelimit {
                value: String::from("192.168.0.0"),
            }),
            current_count: Arc::new(AtomicIsize::new(3)),
            lock: Arc::new(Mutex::new(0)),
            last_update_time: SystemTime::now(),
        };
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("api_key", "test2".parse().unwrap());
        let res1 =
            token_bucket_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.1"));
        assert_eq!(res1.unwrap(), false);
        let res2 =
            token_bucket_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.1"));
        assert_eq!(res2.unwrap(), false);
        let res3 =
            token_bucket_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.1"));
        assert_eq!(res3.unwrap(), false);

        let res4 =
            token_bucket_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.1"));
        assert_eq!(res4.unwrap(), false);
    }
    #[test]
    fn test_token_bucket_rate_limit_ok8() {
        let mut token_bucket_ratelimit = TokenBucketRateLimit {
            rate_per_unit: 3,
            capacity: 10000,
            unit: TimeUnit::Second,
            limit_location: LimitLocation::IP(IPBasedRatelimit {
                value: String::from("192.168.0.0"),
            }),
            current_count: Arc::new(AtomicIsize::new(3)),
            lock: Arc::new(Mutex::new(0)),
            last_update_time: SystemTime::now(),
        };
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("api_key", "test2".parse().unwrap());
        let res1 =
            token_bucket_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res1.unwrap(), false);
        let res2 =
            token_bucket_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res2.unwrap(), false);
        let res3 =
            token_bucket_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res3.unwrap(), false);

        let one_second = time::Duration::from_secs(1);
        thread::sleep(one_second);
        let res4 =
            token_bucket_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res4.unwrap(), false);
    }
    #[test]
    fn test_time_unit() {
        let million_second = TimeUnit::MillionSecond;
        assert_eq!(million_second.get_million_second(), 1);
        let second = TimeUnit::Second;
        assert_eq!(second.get_million_second(), 1_000);
        let minute = TimeUnit::Minute;
        assert_eq!(minute.get_million_second(), 60_000);
        let hour = TimeUnit::Hour;
        assert_eq!(hour.get_million_second(), 3_600_000);
        let day = TimeUnit::Day;
        assert_eq!(day.get_million_second(), 86_400_000);
    }
    #[test]
    fn test_fixed_window_ratelimit_ok() {
        let mut fixed_window_ratelimit = FixedWindowRateLimit {
            rate_per_unit: 3,
            unit: TimeUnit::Minute,
            limit_location: LimitLocation::IP(IPBasedRatelimit {
                value: String::from("192.168.0.0"),
            }),
            count_map: DashMap::new(),
            lock: Arc::new(Mutex::new(0)),
        };
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("api_key", "test2".parse().unwrap());
        let res1 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res1.unwrap(), false);
        let res2 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res2.unwrap(), false);
        let res3 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res3.unwrap(), false);

        let res4 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res4.unwrap(), true);
    }

    #[test]
    fn test_fixed_window_ratelimit_ok2() {
        let mut fixed_window_ratelimit = FixedWindowRateLimit {
            rate_per_unit: 3,
            unit: TimeUnit::Second,
            limit_location: LimitLocation::IP(IPBasedRatelimit {
                value: String::from("192.168.0.0"),
            }),
            count_map: DashMap::new(),
            lock: Arc::new(Mutex::new(0)),
        };
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("api_key", "test2".parse().unwrap());
        let res1 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res1.unwrap(), false);
        let res2 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res2.unwrap(), false);
        let res3 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res3.unwrap(), false);

        let one_second = time::Duration::from_secs(1);
        thread::sleep(one_second);
        let res4 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res4.unwrap(), false);
        let res5 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res5.unwrap(), false);
        let res6 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res6.unwrap(), false);

        let res7 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res7.unwrap(), true);
    }
    #[test]
    fn test_fixed_window_ratelimit_ok3() {
        let mut fixed_window_ratelimit = FixedWindowRateLimit {
            rate_per_unit: 3,
            unit: TimeUnit::MillionSecond,
            limit_location: LimitLocation::Header(HeaderBasedRatelimit {
                key: String::from("api_key"),
                value: String::from("test2"),
            }),
            count_map: DashMap::new(),
            lock: Arc::new(Mutex::new(0)),
        };
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("api_key", "test2".parse().unwrap());
        let res1 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res1.unwrap(), false);
        let res2 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res2.unwrap(), false);
        let res3 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res3.unwrap(), false);

        let res4 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.0"));
        assert_eq!(res4.unwrap(), true);
    }
    #[test]
    fn test_fixed_window_ratelimit_ok4() {
        let mut fixed_window_ratelimit = FixedWindowRateLimit {
            rate_per_unit: 3,
            unit: TimeUnit::MillionSecond,
            limit_location: LimitLocation::IPRANGE(IpRangeBasedRatelimit {
                value: String::from("192.168.0.1/8"),
            }),
            count_map: DashMap::new(),
            lock: Arc::new(Mutex::new(0)),
        };
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("api_key", "test2".parse().unwrap());
        let res1 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.1"));
        assert_eq!(res1.unwrap(), false);
        let res2 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.2"));
        assert_eq!(res2.unwrap(), false);
        let res3 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.3"));
        assert_eq!(res3.unwrap(), false);

        let res4 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.4"));
        assert_eq!(res4.unwrap(), true);
    }
    #[test]
    fn test_fixed_window_ratelimit_ok5() {
        let mut fixed_window_ratelimit = FixedWindowRateLimit {
            rate_per_unit: 3,
            unit: TimeUnit::Day,
            limit_location: LimitLocation::IPRANGE(IpRangeBasedRatelimit {
                value: String::from("192.168.0.1/8"),
            }),
            count_map: DashMap::new(),
            lock: Arc::new(Mutex::new(0)),
        };
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("api_key", "test2".parse().unwrap());
        let res1 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.1"));
        assert_eq!(res1.unwrap(), false);
        let res2 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.2"));
        assert_eq!(res2.unwrap(), false);
        let res3 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.3"));
        assert_eq!(res3.unwrap(), false);

        let res4 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.4"));
        assert_eq!(res4.unwrap(), true);
    }
    #[test]
    fn test_fixed_window_ratelimit_ok6() {
        let mut fixed_window_ratelimit = FixedWindowRateLimit {
            rate_per_unit: 3,
            unit: TimeUnit::Hour,
            limit_location: LimitLocation::IPRANGE(IpRangeBasedRatelimit {
                value: String::from("192.168.0.1/8"),
            }),
            count_map: DashMap::new(),
            lock: Arc::new(Mutex::new(0)),
        };
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("api_key", "test2".parse().unwrap());
        let res1 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.1"));
        assert_eq!(res1.unwrap(), false);
        let res2 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.2"));
        assert_eq!(res2.unwrap(), false);
        let res3 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.3"));
        assert_eq!(res3.unwrap(), false);

        let res4 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.4"));
        assert_eq!(res4.unwrap(), true);
    }
    #[test]
    fn test_fixed_window_ratelimit_ok7() {
        let mut fixed_window_ratelimit = FixedWindowRateLimit {
            rate_per_unit: 3,
            unit: TimeUnit::MillionSecond,
            limit_location: LimitLocation::IPRANGE(IpRangeBasedRatelimit {
                value: String::from("192.168.0.1/8"),
            }),
            count_map: DashMap::new(),
            lock: Arc::new(Mutex::new(0)),
        };
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("api_key", "test2".parse().unwrap());
        for _n in 0..100 {
            let _res1 = fixed_window_ratelimit
                .should_limit(headermap1.clone(), String::from("192.168.0.1"));
            let _res2 = fixed_window_ratelimit
                .should_limit(headermap1.clone(), String::from("192.168.0.2"));
            let _res3 = fixed_window_ratelimit
                .should_limit(headermap1.clone(), String::from("192.168.0.3"));

            let _res4 = fixed_window_ratelimit
                .should_limit(headermap1.clone(), String::from("192.168.0.4"));
            let sleep_time = time::Duration::from_millis(2);
            thread::sleep(sleep_time);
        }
    }
    #[test]
    fn test_fixed_window_ratelimit_as_any_ok() {
        let mut fixed_window_ratelimit = FixedWindowRateLimit {
            rate_per_unit: 3,
            unit: TimeUnit::Hour,
            limit_location: LimitLocation::IPRANGE(IpRangeBasedRatelimit {
                value: String::from("192.168.0.1/8"),
            }),
            count_map: DashMap::new(),
            lock: Arc::new(Mutex::new(0)),
        };
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("api_key", "test2".parse().unwrap());
        let res1 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.1"));
        assert_eq!(res1.unwrap(), false);
        let res2 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.2"));
        assert_eq!(res2.unwrap(), false);
        let res3 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.3"));
        assert_eq!(res3.unwrap(), false);

        let res4 =
            fixed_window_ratelimit.should_limit(headermap1.clone(), String::from("192.168.0.4"));
        assert_eq!(res4.unwrap(), true);
    }
    #[test]
    fn test_debug_trait() {
        let fixed_window_ratelimit = FixedWindowRateLimit {
            rate_per_unit: 3,
            unit: TimeUnit::Second,
            limit_location: LimitLocation::IP(IPBasedRatelimit {
                value: String::from("192.168.0.0"),
            }),
            count_map: DashMap::new(),
            lock: Arc::new(Mutex::new(0)),
        };
        let weight_route: Box<dyn RatelimitStrategy> = Box::new(fixed_window_ratelimit);
        assert_eq!(format!("{:?}", weight_route), "{debug}");
    }
    #[test]
    fn test_token_bucket_as_any() {
        let req = r#"[
            {
              "listen_port": 4486,
              "service_config": {
                "server_type": "HTTP",
                "cert_str": null,
                "key_str": null,
                "routes": [
                  {
                    "matcher": {
                      "prefix": "ss",
                      "prefix_rewrite": "ssss"
                    },
                    "allow_deny_list": null,
                    "authentication": null,
                    "ratelimit": {
                      "type": "TokenBucketRateLimit",
                      "rate_per_unit": 3,
                      "unit": {
                        "type": "Second"
                      },
                      "capacity": 10000,
                      "limit_location": {
                        "type": "IP",
                        "value": "192.168.0.0"
                      }
                    },
                    "route_cluster": {
                      "type": "PollRoute",
                      "routes": [
                        {
                          "endpoint": "/",
                          "try_file": null
                        }
                      ]
                    }
                  }
                ]
              }
            }
          ]"#;
        let api_services: Vec<ApiService> = serde_json::from_slice(req.as_bytes()).unwrap();
        let first_api_service = api_services
            .first()
            .unwrap()
            .service_config
            .routes
            .first()
            .unwrap()
            .clone();
        let ratelimit = first_api_service.ratelimit.unwrap();
        let token_bucket_ratelimit: &TokenBucketRateLimit =
            match ratelimit.as_any().downcast_ref::<TokenBucketRateLimit>() {
                Some(b) => b,
                None => panic!("error!"),
            };

        assert_eq!(token_bucket_ratelimit.capacity, 10000);
    }
    #[test]
    fn test_fixed_window_as_any() {
        let req = r#"[
            {
              "listen_port": 4486,
              "service_config": {
                "server_type": "HTTP",
                "cert_str": null,
                "key_str": null,
                "routes": [
                  {
                    "matcher": {
                      "prefix": "ss",
                      "prefix_rewrite": "ssss"
                    },
                    "allow_deny_list": null,
                    "authentication": null,
                    "ratelimit": {
                      "type": "FixedWindowRateLimit",
                      "rate_per_unit": 3,
                      "unit": {
                        "type": "Minute"
                      },
                      "limit_location": {
                        "type": "IP",
                        "value": "192.168.0.0"
                      }
                    },
                    "route_cluster": {
                      "type": "PollRoute",
                      "routes": [
                        {
                          "endpoint": "/",
                          "try_file": null
                        }
                      ]
                    }
                  }
                ]
              }
            }
          ]"#;
        let api_services: Vec<ApiService> = serde_json::from_slice(req.as_bytes()).unwrap();
        let first_api_service = api_services
            .first()
            .unwrap()
            .service_config
            .routes
            .first()
            .unwrap()
            .clone();
        let ratelimit = first_api_service.ratelimit.unwrap();
        let fixed_window_ratelimit: &FixedWindowRateLimit =
            match ratelimit.as_any().downcast_ref::<FixedWindowRateLimit>() {
                Some(b) => b,
                None => panic!("error!"),
            };

        assert_eq!(fixed_window_ratelimit.rate_per_unit, 3);
    }
}
