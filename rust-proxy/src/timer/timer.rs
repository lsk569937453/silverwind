use tokio::time;

use std::time::{Duration, Instant};

pub struct Timer {}
impl Timer {
    pub async fn startSyncTask() {
        let mut interval = time::interval(time::Duration::from_secs(2));
        for _i in 0..5 {
            interval.tick().await;
            info!("print a");
        }
    }
}
pub async fn startSyncTask() {
    let mut interval = time::interval(time::Duration::from_secs(2));
    loop {
        interval.tick().await;
        info!("print a");
    }
}