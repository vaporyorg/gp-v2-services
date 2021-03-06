use anyhow::Result;
use prometheus::{HistogramOpts, HistogramVec, IntGaugeVec, Opts, Registry};
use std::{convert::Infallible, sync::Arc, time::Instant};
use warp::{reply::Response, Filter, Reply};

pub struct Metrics {
    requests: HistogramVec,
    db_table_row_count: IntGaugeVec,
}

impl Metrics {
    pub fn new(registry: &Registry) -> Result<Self> {
        let opts = HistogramOpts::new(
            "gp_v2_api_requests",
            "API Request durations labelled by route and response status code",
        );
        let requests = HistogramVec::new(opts, &["response", "request_type"]).unwrap();
        registry.register(Box::new(requests.clone()))?;

        let db_table_row_count = IntGaugeVec::new(
            Opts::new("gp_v2_api_table_rows", "Number of rows in db tables."),
            &["table"],
        )?;
        registry.register(Box::new(db_table_row_count.clone()))?;

        Ok(Self {
            requests,
            db_table_row_count,
        })
    }

    pub fn set_table_row_count(&self, table: &str, count: i64) {
        self.db_table_row_count
            .with_label_values(&[table])
            .set(count);
    }
}

// Response wrapper needed because we cannot inspect the reply's status code without consuming it
struct MetricsReply {
    response: Response,
}

impl Reply for MetricsReply {
    fn into_response(self) -> Response {
        self.response
    }
}

// Wrapper struct to annotate a reply with a handler label for logging purposes
pub struct LabelledReply {
    inner: Box<dyn Reply>,
    label: &'static str,
}

impl LabelledReply {
    pub fn new(inner: impl Reply + 'static, label: &'static str) -> Self {
        Self {
            inner: Box::new(inner),
            label,
        }
    }
}

impl Reply for LabelledReply {
    fn into_response(self) -> Response {
        self.inner.into_response()
    }
}

pub fn start_request() -> impl Filter<Extract = (Instant,), Error = Infallible> + Clone {
    warp::any().map(Instant::now)
}

pub fn end_request(metrics: Arc<Metrics>, timer: Instant, reply: LabelledReply) -> impl Reply {
    let LabelledReply { inner, label } = reply;
    let response = inner.into_response();
    let elapsed = timer.elapsed().as_secs_f64();
    metrics
        .requests
        .with_label_values(&[response.status().as_str(), label])
        .observe(elapsed);
    MetricsReply { response }
}
