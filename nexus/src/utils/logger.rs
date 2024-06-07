use log::*;
use simplelog::{ColorChoice, ConfigBuilder, TermLogger, TerminalMode};

pub fn init_logger() {
  let log_level = std::env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
  let log_level = match log_level.to_lowercase().as_str() {
    "trace" => LevelFilter::Trace,
    "debug" => LevelFilter::Debug,
    "info" => LevelFilter::Info,
    "warn" => LevelFilter::Warn,
    "error" => LevelFilter::Error,
    _ => LevelFilter::Info,
  };

  let mut cfg = ConfigBuilder::new();
  cfg.set_time_offset(time::UtcOffset::from_hms(-4, 0, 0).unwrap());
  let cfg = cfg.build();

  TermLogger::init(log_level, cfg, TerminalMode::Mixed, ColorChoice::Auto)
    .expect("Failed to initialize logger");
}
