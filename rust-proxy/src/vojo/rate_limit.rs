use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::constants::common_constants::DEFAULT_FIXEDWINDOW_MAP_SIZE;
use async_trait::async_trait;
use core::fmt::Debug;
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
use tokio::sync::RwLock;

use super::app_error::AppError;

#[typetag::serde(tag = "type")]
#[async_trait]
pub trait RatelimitStrategy: Sync + Send + DynClone {
    async fn should_limit(
        &mut self,
        headers: HeaderMap<HeaderValue>,
        remote_ip: String,
    ) -> Result<bool, AppError>;

    fn get_debug(&self) -> String {
        String::from("debug")
    }
    fn as_any(&self) -> &dyn Any;
}
dyn_clone::clone_trait_object!(RatelimitStrategy);

impl Debug for dyn RatelimitStrategy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let routes = self.get_debug();
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
    Iprange(IpRangeBasedRatelimit),
}
impl LimitLocation {
    pub fn get_key(&self) -> String {
        match self {
            LimitLocation::Header(headers) => headers.get_key(),
            LimitLocation::IP(ip) => ip.value.clone(),
            LimitLocation::Iprange(ip_range) => ip_range.value.clone(),
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
    pub current_count: Arc<RwLock<AtomicIsize>>,
    #[serde(skip_serializing, skip_deserializing)]
    pub lock: Arc<Mutex<i32>>,
    #[serde(skip_serializing, skip_deserializing, default = "default_time")]
    pub last_update_time: Arc<RwLock<SystemTime>>,
}

fn default_time() -> Arc<RwLock<SystemTime>> {
    Arc::new(RwLock::new(SystemTime::now()))
}
fn get_time_key(time_unit: TimeUnit) -> Result<String, AppError> {
    let current_time = SystemTime::now();
    let since_the_epoch = current_time
        .duration_since(UNIX_EPOCH)
        .map_err(|err| AppError(err.to_string()))?;
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
) -> Result<bool, AppError> {
    return match limit_location {
        LimitLocation::IP(ip_based_ratelimit) => Ok(ip_based_ratelimit.value == remote_ip),
        LimitLocation::Header(header_based_ratelimit) => {
            if !headers.contains_key(header_based_ratelimit.key.clone()) {
                return Ok(false);
            }
            let header_value = headers.get(header_based_ratelimit.key.clone()).unwrap();
            let header_value_str = header_value
                .to_str()
                .map_err(|err| AppError(err.to_string()))?;

            return Ok(header_value_str == header_based_ratelimit.value);
        }
        LimitLocation::Iprange(ip_range_based_ratelimit) => {
            if !ip_range_based_ratelimit.value.contains('/') {
                return Err(AppError(format!("The Ip Range should contain '/'.")));
            }
            let ip_range: IpRange<Ipv4Net> = [ip_range_based_ratelimit.value]
                .iter()
                .map(|s| s.parse().unwrap())
                .collect();
            let source_ip = remote_ip
                .parse::<Ipv4Addr>()
                .map_err(|err| AppError(err.to_string()))?;
            return Ok(ip_range.contains(&source_ip));
        }
    };
}
#[typetag::serde]
#[async_trait]
impl RatelimitStrategy for TokenBucketRateLimit {
    async fn should_limit(
        &mut self,
        headers: HeaderMap<HeaderValue>,
        remote_ip: String,
    ) -> Result<bool, AppError> {
        let match_or_not = matched(self.limit_location.clone(), headers, remote_ip)?;
        if !match_or_not {
            return Ok(false);
        }
        let read_lock = self.current_count.read().await;
        let current_value = read_lock.fetch_sub(1, Ordering::SeqCst);
        if current_value <= 0 {
            let elapsed = self
                .last_update_time
                .read()
                .await
                .elapsed()
                .map_err(|err| AppError(err.to_string()))?;
            let elapsed_millis = elapsed.as_millis();
            let mut added_count =
                elapsed_millis * self.rate_per_unit / self.unit.get_million_second();

            if added_count == 0 {
                return Ok(true);
            }
            drop(read_lock);
            let mut write_lock = self.current_count.write().await;
            if (write_lock.load(Ordering::SeqCst) as i32) < 0 {
                if added_count > (self.capacity as u128) {
                    added_count = self.capacity as u128;
                }
                *write_lock = AtomicIsize::new(added_count as isize);
                let mut write_timestamp = self.last_update_time.write().await;
                *write_timestamp = SystemTime::now();
            }
            drop(write_lock);
            let current_value = self
                .current_count
                .read()
                .await
                .fetch_sub(1, Ordering::SeqCst);
            if current_value <= 0 {
                return Ok(true);
            }
        }
        Ok(false)
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
    pub count_map: HashMap<String, i64>,
    #[serde(skip_serializing, skip_deserializing)]
    pub lock: i32,
}
#[typetag::serde]
#[async_trait]

impl RatelimitStrategy for FixedWindowRateLimit {
    async fn should_limit(
        &mut self,
        headers: HeaderMap<HeaderValue>,
        remote_ip: String,
    ) -> Result<bool, AppError> {
        let match_or_not = matched(self.limit_location.clone(), headers, remote_ip)?;
        if !match_or_not {
            return Ok(false);
        }
        let time_unit_key = get_time_key(self.unit.clone())?;
        let location_key = self.limit_location.get_key();
        let key = format!("{}:{}", location_key, time_unit_key);
        if !self.count_map.contains_key(key.as_str()) {
            if self.count_map.len() > DEFAULT_FIXEDWINDOW_MAP_SIZE as usize {
                let (key, _) = self
                    .count_map
                    .iter()
                    .next()
                    .ok_or(AppError(String::from("")))?;
                let cloned_key = key.clone();
                self.count_map.remove(&cloned_key);
            }
            self.count_map.insert(key.clone(), 0);
        }
        let atomic_isize = self
            .count_map
            .get(key.as_str())
            .ok_or(AppError(String::from(
                "Can not find the key in the map of FixedWindowRateLimit!",
            )))?;
        let res = atomic_isize + 1;
        if res as i32 >= self.rate_per_unit as i32 {
            return Ok(true);
        }
        Ok(false)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
