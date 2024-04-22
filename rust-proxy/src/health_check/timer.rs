use crate::vojo::app_error::AppError;
use futures::Future;
use std::{collections::HashMap, time::Duration};
use tokio::sync::oneshot;
use tokio::time;

pub struct TaskPool {
    pub data_map: HashMap<String, oneshot::Sender<()>>,
}
impl TaskPool {
    pub async fn submit_task<Fut, F>(&mut self, route_id: String, task: F, interval: u64)
    where
        Fut: Future<Output = Result<(), AppError>> + Send + 'static + Sync,
        F: FnMut() -> Fut + Send + 'static,
    {
        let (sender, receiver) = oneshot::channel();
        let mut timer = HealthCheckTimer::new(interval, receiver, task, route_id.clone());
        tokio::spawn(async move {
            timer.run().await;
        });
        self.data_map.insert(route_id, sender);
    }
    pub fn remove_task(&mut self, task_id: String) -> Result<(), AppError> {
        if !self.data_map.contains_key(&task_id) {
            return Err(AppError("Task not found".to_string()));
        }
        self.data_map.remove(&task_id);
        Ok(())
    }
    pub fn new() -> Self {
        TaskPool {
            data_map: HashMap::new(),
        }
    }
}

pub struct HealthCheckTimer<Fut, F>
where
    Fut: Future<Output = Result<(), AppError>> + Send + 'static,
    F: FnMut() -> Fut + Send + 'static,
{
    pub interval: u64,
    pub receiver: oneshot::Receiver<()>,
    pub task: F,
    pub route_id: String,
}
impl<Fut, F> HealthCheckTimer<Fut, F>
where
    Fut: Future<Output = Result<(), AppError>> + Send + 'static,
    F: FnMut() -> Fut + Send + 'static,
{
    #[instrument(skip(self), fields(self.route_id = %self.route_id))]
    pub async fn run(&mut self) {
        let mut interval = time::interval(Duration::from_secs(self.interval));
        let task = &mut self.task;
        loop {
            // let task_cloned = task.clone();
            tokio::select! {
                _ = interval.tick() => {
                    let _=task().await;
                },
                _=&mut self.receiver => {
                    info!("Health check timer stop!");
                    return
                },

            }
        }
    }
    pub fn new(interval: u64, receiver: oneshot::Receiver<()>, task: F, route_id: String) -> Self {
        HealthCheckTimer {
            interval,
            receiver,
            task,
            route_id,
        }
    }
}
