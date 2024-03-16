/// Prometheus handler
#[cfg(feature = "crack-metrics")]
#[cfg(not(tarpaulin_include))]
async fn metrics_handler() -> Result<impl warp::Reply, warp::Rejection> {
    let encoder = TextEncoder::new();
    let mut metric_families = prometheus::gather();
    metric_families.extend(REGISTRY.gather());
    // tracing::info!("Metrics: {:?}", metric_families);
    let mut buffer = vec![];
    encoder.encode(&metric_families, &mut buffer).unwrap();

    Ok(warp::reply::with_header(
        buffer,
        "content-type",
        encoder.format_type(),
    ))
}
