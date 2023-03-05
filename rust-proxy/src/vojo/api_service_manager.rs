use crate::vojo::app_config::Route;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct ApiServiceManager {
    pub routes: Vec<Route>,
    pub sender: mpsc::Sender<()>,
}
