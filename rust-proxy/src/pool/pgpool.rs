use diesel::expression::is_aggregate::No;
use diesel::r2d2;
use diesel::r2d2::ConnectionManager;
use diesel::PgConnection;
use dotenvy::dotenv;
use lazy_static::lazy_static;
use std::{any, env};
pub type DbConnection = r2d2::PooledConnection<ConnectionManager<PgConnection>>;
pub type Pool = r2d2::Pool<ConnectionManager<PgConnection>>;
// use anyhow::Result;
// Database URL: "postgresql://postgres:postgres@127.0.0.1:5432/mydb"
use crate::pool;
use std::error::Error;
// Side Node: Specify `min_idle as 0 or 1` works. I think this is something to do with simultaneous connections
use crate::pool::pgpool::{self};
use std::backtrace::Backtrace;
use std::panic;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tokio::time;

use std::sync::RwLock;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct ConnectionPool {
    pub pool: Option<Pool>,
    pub status: Arc<Mutex<i32>>,
}
lazy_static! {
    pub static ref CONNECTION_POOL: RwLock<ConnectionPool> = RwLock::new(ConnectionPool {
        pool: None,
        status: Arc::new(Mutex::new(0)),
    });
}
impl ConnectionPool {
    fn get(&mut self) -> Result<DbConnection, r2d2::PoolError> {
        self.pool.clone().unwrap().get()
    }
}

pub async fn schedule_task_connection_pool() {
    let mut interval = time::interval(time::Duration::from_secs(5));
    loop {
        match connect_with_database() {
            Ok(()) => debug!("check database status is ok"),
            Err(err) => error!("connect_with_database is error{}", err),
        }
        interval.tick().await;
    }
}
fn connect_with_database() -> Result<(), anyhow::Error> {
    debug!("2");
    // let arc_connection_pool_status = CONNECTION_POOL.read().unwrap().to_owned().clone().status;
    let arc_connection_pool_status = match CONNECTION_POOL.read() {
        Ok(pool) => pool.to_owned().clone(),
        Err(err) => {
            error!("error is {}", err);
            return Err(anyhow!(err.to_string()));
        }
    };
    debug!("3");

    if let Ok(connection_pool_status) = arc_connection_pool_status.clone().status.lock() {
        debug!("4");

        if *connection_pool_status == 0 {
            debug!("5");
            // let new_connection_pool = create_connection_pool().read().unwrap().to_owned().clone();
            let rw_lock_pool = create_connection_pool();
            let new_connection_pool = match rw_lock_pool.read() {
                Ok(pool) => pool,
                Err(err) => return Err(anyhow!(err.to_string())),
            };
            debug!("6");
            let new_pool_status = new_connection_pool.status.lock().unwrap();
            if *new_pool_status != 0 {
                let mut default_pool = CONNECTION_POOL.write().unwrap();
                *default_pool = new_connection_pool.clone();
            }
        } else {
            debug!("connection_pool is ready")
        }
    } else {
        debug!("can not get lock");
    };
    Ok(())
}

fn create_connection_pool() -> RwLock<ConnectionPool> {
    dotenv().ok();
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    info!("Database URL: {}", database_url);
    let manager = ConnectionManager::<PgConnection>::new(database_url);
    // return Pool::builder().max_size(50).build(manager).unwrap();
    info!("pool is  11 ");

    let mut pool = panic::catch_unwind(|| {
        return Pool::builder()
            .min_idle(Some(5))
            .max_size(10)
            .build(manager);
    });
    info!("pool is error ");
    if pool.is_err() || pool.as_mut().unwrap().is_err() {
        if pool.is_err() {
            error!("panic when creating the pool")
        } else {
            error!("error is {}", pool.unwrap().unwrap_err())
        }
        return RwLock::new(ConnectionPool {
            pool: None,
            status: Arc::new(Mutex::new(0)),
        });
    } else {
        return RwLock::new(ConnectionPool {
            pool: Some(pool.unwrap().unwrap()),
            status: Arc::new(Mutex::new(1)),
        });
    }
}
pub fn get_connection() -> Result<DbConnection, anyhow::Error> {
    let cpool = CONNECTION_POOL.read().unwrap().to_owned().clone();
    let current_status = cpool.status.lock().unwrap();
    if *current_status == 0 {
        return Err(anyhow!("the connection pool is not ready"));
    } else {
        match cpool.clone().get() {
            Ok(connection) => Ok(connection),
            Err(err) => Err(anyhow!("get connection error: {}", err)),
        }
    }
}
