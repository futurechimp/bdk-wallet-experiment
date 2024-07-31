use tracing::Level;
use tracing_subscriber::{filter, fmt, layer::SubscriberExt, Layer, Registry};

/// Set up the tracing subscriber, filtering logs to show only info level logs and above
pub(crate) fn tracing_setup() {
    // show only info level logs and above:
    let info = filter::LevelFilter::from_level(Level::INFO);

    // set up the tracing subscriber:
    let subscriber = Registry::default().with(fmt::layer().with_filter(info));
    tracing::subscriber::set_global_default(subscriber).unwrap();

    tracing::info!("Tracing initialized.");
}
