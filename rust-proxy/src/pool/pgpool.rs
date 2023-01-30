use diesel::r2d2;
use diesel::r2d2::ConnectionManager;
use diesel::PgConnection;

use dotenvy::dotenv;
use std::env;
use lazy_static::lazy_static;

pub type DbConnection = r2d2::PooledConnection<ConnectionManager<PgConnection>>;
pub type Pool = r2d2::Pool<ConnectionManager<PgConnection>>;

// Database URL: "postgresql://postgres:postgres@127.0.0.1:5432/mydb"

// Side Node: Specify `min_idle as 0 or 1` works. I think this is something to do with simultaneous connections

lazy_static! {
    pub static ref CONNECTION_POOL: Pool = get_connection_pool();
}
pub fn get_connection_pool() -> Pool {
    dotenv().ok();
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    info!("Database URL: {}", database_url);
    let manager = ConnectionManager::<PgConnection>::new(database_url);
    return Pool::builder().max_size(50).build(manager).unwrap();
}
