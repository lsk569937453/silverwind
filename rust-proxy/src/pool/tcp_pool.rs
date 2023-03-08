use bb8::{Pool, RunError};
use dashmap::DashMap;
use diesel::r2d2::ConnectionManager;
use diesel::r2d2::{self, ManageConnection};
use dotenvy::dotenv;
use lazy_static::lazy_static;
use std::env;
use std::panic;
use std::sync::RwLock;
use std::thread::sleep;
use tokio::net::TcpStream;

use super::TcpConnectionManager;

#[derive(Debug, Clone)]
pub struct TcpConnectionPool {
    pub pool: Option<Pool<TcpConnectionManager>>,
}
lazy_static! {
    pub static ref TCP_CONNECTION_POOL: DashMap<String, TcpConnectionPool> = DashMap::new();
}

pub async fn get_tcp_pool(key: String) -> Pool<TcpConnectionManager> {
    let result = TCP_CONNECTION_POOL.get_mut(&key);
    if result.is_none() {
        let manager = TcpConnectionManager::new(key.clone()).unwrap();

        let pool = Pool::builder()
            .min_idle(Some(5))
            .build(manager)
            .await
            .unwrap();
        let new_connection_pool = TcpConnectionPool { pool: Some(pool) }.clone();
        TCP_CONNECTION_POOL.insert(key, new_connection_pool.clone());
        return new_connection_pool.pool.unwrap();
    }
    let s = result.unwrap();

    return s.clone().pool.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    // fn run_test<T>(test: T) -> ()
    // where
    //     T: FnOnce() -> () + panic::UnwindSafe,
    // {
    //     setup();
    //     let result = panic::catch_unwind(|| test());
    //     teardown();
    //     assert!(result.is_ok())
    // }
    #[test]
    fn test_get_connection_fail() {
        // let result_connection = get_connection();
        // assert_eq!(result_connection.is_err(), true);
    }
}
