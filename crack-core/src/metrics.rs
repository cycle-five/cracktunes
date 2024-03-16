#[cfg(feature = "crack-metrics")]
mod metrics_internal {
    use lazy_static::lazy_static;
    use prometheus::{Gauge, HistogramOpts, HistogramVec, IntCounterVec, Opts, Registry};

    lazy_static! {
        pub static ref REGISTRY: Registry = Registry::new();
        pub static ref COMMAND_EXECUTIONS: IntCounterVec = IntCounterVec::new(
            Opts::new("command_executions", "Command Executions"),
            &["command", "type"]
        )
        .expect("metric can be created");
        pub static ref COMMAND_ERRORS: IntCounterVec =
            IntCounterVec::new(Opts::new("command_errors", "Command Errors"), &["command"])
                .expect("metric can be created");
        pub static ref CONNECTED_CLIENTS: Gauge =
            Gauge::new("connected_clients", "Connected Clients").expect("metric can be created");
        pub static ref RESPONSE_TIME_COLLECTOR: HistogramVec = HistogramVec::new(
            HistogramOpts::new("response_time", "Response Times"),
            &["command"]
        )
        .expect("metric can be created");
    }

    /// Register custom metrics with the prometheus registry
    pub fn register_custom_metrics() {
        REGISTRY
            .register(Box::new(COMMAND_EXECUTIONS.clone()))
            .expect("collector can be registered");

        REGISTRY
            .register(Box::new(CONNECTED_CLIENTS.clone()))
            .expect("collector can be registered");

        REGISTRY
            .register(Box::new(COMMAND_ERRORS.clone()))
            .expect("collector can be registered");

        REGISTRY
            .register(Box::new(RESPONSE_TIME_COLLECTOR.clone()))
            .expect("collector can be registered");
    }

    #[cfg(test)]
    mod test {
        use super::*;

        #[test]
        pub fn metrics_test() {
            register_custom_metrics();
            COMMAND_EXECUTIONS
                .with_label_values(&["test", "test"])
                .inc();
            CONNECTED_CLIENTS.inc();
            COMMAND_ERRORS.with_label_values(&["test"]).inc();
            RESPONSE_TIME_COLLECTOR
                .with_label_values(&["test"])
                .observe(1.0);
        }
    }
}
#[cfg(not(feature = "crack-metrics"))]
mod metrics_internal {
    pub fn register_custom_metrics() {}
}

pub use metrics_internal::*;
