use core::fmt::Debug;
use dyn_clone::DynClone;
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
#[typetag::serde(tag = "type")]
pub trait LoadbalancerStrategy: Sync + Send + DynClone {
    fn get_route(&mut self) -> Result<String, anyhow::Error>;

    fn get_debug(&self) -> String {
        String::from("debug")
    }
}
dyn_clone::clone_trait_object!(LoadbalancerStrategy);

impl Debug for dyn LoadbalancerStrategy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let routes = self.get_debug().clone();
        write!(f, "Series{{{}}}", routes)
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct BaseRoute {
    pub endpoint: String,
    #[serde(default = "default_resource")]
    pub weight: i32,
}
fn default_resource() -> i32 {
    100
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RandomRoute {
    pub routes: Vec<BaseRoute>,
}
#[typetag::serde]
impl LoadbalancerStrategy for RandomRoute {
    fn get_route(&mut self) -> Result<String, anyhow::Error> {
        let mut rng = thread_rng();
        let index = rng.gen_range(0..self.routes.len());
        let dst = self.routes[index].clone();
        Ok(String::from(dst.endpoint))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PollRoute {
    #[serde(skip_serializing, skip_deserializing)]
    pub current_index: Arc<AtomicUsize>,
    pub routes: Vec<BaseRoute>,
    #[serde(skip_serializing, skip_deserializing)]
    pub lock: Arc<Mutex<i32>>,
}
#[typetag::serde]
impl LoadbalancerStrategy for PollRoute {
    fn get_route(&mut self) -> Result<String, anyhow::Error> {
        let older = self.current_index.fetch_add(1, Ordering::SeqCst);
        let len = self.routes.len();
        let current_index = older % len;
        let dst = self.routes[current_index].clone();
        debug!("PollRoute current index:{}", current_index as i32);
        Ok(String::from(dst.endpoint))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeightRoute {
    #[serde(skip_serializing, skip_deserializing)]
    pub indexs: Arc<RwLock<Vec<AtomicIsize>>>,
    pub routes: Vec<BaseRoute>,
}
impl WeightRoute {}
#[typetag::serde]
impl LoadbalancerStrategy for WeightRoute {
    fn get_route(&mut self) -> Result<String, anyhow::Error> {
        let indexs = self.indexs.read().unwrap();
        for (pos, e) in indexs.iter().enumerate() {
            let old_value = e.fetch_sub(1, Ordering::SeqCst);
            if old_value > 0 {
                let res = &self.routes[pos].clone();
                debug!("WeightRoute current index:{}", pos as i32);
                return Ok(res.endpoint.clone());
            }
        }
        drop(indexs);

        let mut new_lock = self.indexs.write().unwrap();
        let check_alive = new_lock.iter().any(|f| {
            let tt = f.load(Ordering::SeqCst);
            tt.is_positive()
        });
        if !check_alive {
            let s = self
                .routes
                .iter()
                .map(|s| AtomicIsize::from(s.weight as isize))
                .collect::<Vec<AtomicIsize>>();
            *new_lock = s;
        }
        drop(new_lock);

        let indexs = self.indexs.read().unwrap();
        for (pos, e) in indexs.iter().enumerate() {
            let old_value = e.fetch_sub(1, Ordering::SeqCst);
            debug!("the value is :{}", old_value);
            if old_value > 0 {
                let res = &self.routes[pos].clone();
                debug!("WeightRoute current index:{}", pos as i32);
                return Ok(res.endpoint.clone());
            }
        }
        Err(anyhow!("WeightRoute get route error"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_max_value() {
        let atomic = AtomicUsize::new(0);
        let old_value = atomic.fetch_add(1, Ordering::SeqCst);
        println!("{}", old_value);
    }
}
