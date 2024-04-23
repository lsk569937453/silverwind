use tracing_appender::non_blocking::{NonBlockingBuilder, WorkerGuard};
use tracing_appender::rolling;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;
use tracing_subscriber::{fmt, layer::SubscriberExt};
pub fn start_logger() -> WorkerGuard {
    let app_file = rolling::daily("./log", "app");
    let (non_blocking_appender, guard) = NonBlockingBuilder::default()
        .buffered_lines_limit(128000)
        .finish(app_file);
    let file_layer = tracing_subscriber::fmt::Layer::new()
        .with_target(true)
        .with_ansi(false)
        .with_writer(non_blocking_appender)
        .with_filter(tracing_subscriber::filter::LevelFilter::INFO);

    // let console_layer = tracing_subscriber::fmt::Layer::new()
    //     .with_target(true)
    //     .with_filter(tracing_subscriber::filter::LevelFilter::INFO);
    tracing_subscriber::registry()
        .with(file_layer)
        // .with(console_layer)
        .with(tracing_subscriber::filter::LevelFilter::TRACE)
        .init();
    guard
}
#[cfg(test)]
mod tests {
    #[test]
    fn test_stdout_log() {
        debug!("test debug");
        info!("test info");
        error!("test error");
        trace!("test trace");
    }
    #[test]
    fn test_app_log() {
        debug!(target: "app","test debug");
        info!(target: "app","test info");
        error!(target: "app","test error");
        trace!(target: "app","test trace");
    }
}
