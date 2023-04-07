use log::LevelFilter;
use log4rs::append::console::ConsoleAppender;
use log4rs::append::rolling_file::policy::compound::roll::fixed_window::FixedWindowRoller;
use log4rs::append::rolling_file::policy::compound::trigger::size::SizeTrigger;
use log4rs::append::rolling_file::policy::compound::CompoundPolicy;
use log4rs::append::rolling_file::RollingFileAppender;
use log4rs::config::{Appender, Config, Logger, Root};
use log4rs::encode::pattern::PatternEncoder;
pub fn start_logger() {
    let stdout = ConsoleAppender::builder()
        .encoder(Box::new(PatternEncoder::new(
            "{d(%Y-%m-%d %H:%M:%S)(local)} - {h({l})}: {m}{n}",
        )))
        .build();

    let window_size = 10;
    let fixed_window_roller = FixedWindowRoller::builder()
        .build("log/app-{}", window_size)
        .unwrap();

    let size_limit = 10 * 1024 * 1024;
    let size_trigger = SizeTrigger::new(size_limit);
    let compound_policy1 = CompoundPolicy::new(
        Box::new(size_trigger),
        Box::new(fixed_window_roller.clone()),
    );
    let compound_policy2 =
        CompoundPolicy::new(Box::new(size_trigger), Box::new(fixed_window_roller));

    let requests = RollingFileAppender::builder()
        .encoder(Box::new(PatternEncoder::new(
            "{d(%Y-%m-%d %H:%M:%S)(local)} - {h({l})}$${m}{n}",
        )))
        .build("log/app.log", Box::new(compound_policy1))
        .unwrap();
    let common = RollingFileAppender::builder()
        .encoder(Box::new(PatternEncoder::new(
            "{d(%Y-%m-%d %H:%M:%S)(local)} - {h({l})}$${m}{n}",
        )))
        .build("log/common.log", Box::new(compound_policy2))
        .unwrap();

    let config = Config::builder()
        .appender(Appender::builder().build("stdout", Box::new(stdout)))
        .appender(Appender::builder().build("app", Box::new(requests)))
        .appender(Appender::builder().build("common", Box::new(common)))
        .logger(
            Logger::builder()
                .appender("app")
                .additive(false)
                .build("app", LevelFilter::Info),
        )
        .build(
            Root::builder()
                .appender("stdout")
                .appender("common")
                .build(LevelFilter::Info),
        )
        .unwrap();

    let _handle = log4rs::init_config(config);

    // use handle to change logger configuration at runtime
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
