use prometheus::{labels, opts, register_counter_vec, register_gauge, register_histogram_vec};
use prometheus::{CounterVec, Gauge, Histogram, HistogramVec};

pub fn inc(key: String, path: String, code: u16) {
    // HTTP_COUNTER
    //     .with_label_values(&[key.as_str(), path.as_str(), code.to_string().as_str()])
    //     .inc();
    // HTTP_COUNTER
    //     .with_label_values(&[key.as_str(), "all", "all"])
    //     .inc();
    // HTTP_COUNTER.with_label_values(&["all", "all", "all"]).inc();
}
pub fn get_timer_list(key: String, path: String) -> Vec<Histogram> {
    // vec![
    //     HTTP_REQ_HISTOGRAM.with_label_values(&[key.as_str(), path.as_str()]),
    //     HTTP_REQ_HISTOGRAM.with_label_values(&[key.as_str(), "all"]),
    //     HTTP_REQ_HISTOGRAM.with_label_values(&["all", "all"]),
    // ]
    vec![]
}
