use dashmap::DashMap;
use lazy_static::lazy_static;
use prometheus::{labels, opts, register_counter_vec, register_gauge, register_histogram_vec};
use prometheus::{CounterVec, Encoder, Gauge, Histogram, HistogramVec, TextEncoder};

lazy_static! {
    static ref HTTP_COUNTER: CounterVec = register_counter_vec!(
        opts!(
            "silverwind_http_requests_total",
            "Number of HTTP requests made.",
        ),
        &["port", "request_path", "status_code"]
    )
    .unwrap();
    static ref HTTP_BODY_GAUGE: Gauge = register_gauge!(opts!(
        "silverwind_http_response_size_bytes",
        "The HTTP response sizes in bytes.",
        labels! {"handler" => "all",}
    ))
    .unwrap();
    static ref HTTP_REQ_HISTOGRAM: HistogramVec = register_histogram_vec!(
        "silverwind_http_request_duration_seconds",
        "The HTTP request latencies in seconds.",
        &["port", "request_path"]
    )
    .unwrap();
}
pub fn inc(key: String, path: String, code: u16) {
    HTTP_COUNTER
        .with_label_values(&[key.as_str(), path.as_str(), code.to_string().as_str()])
        .inc();
    HTTP_COUNTER
        .with_label_values(&[key.as_str(), "all", "all"])
        .inc();
    HTTP_COUNTER.with_label_values(&["all", "all", "all"]).inc();
}
pub fn get_timer_list(key: String, path: String) -> Vec<Histogram> {
    vec![
        HTTP_REQ_HISTOGRAM.with_label_values(&[key.as_str(), path.as_str()]),
        HTTP_REQ_HISTOGRAM.with_label_values(&[key.as_str(), "all"]),
        HTTP_REQ_HISTOGRAM.with_label_values(&["all", "all"]),
    ]
}
