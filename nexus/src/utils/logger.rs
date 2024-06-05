use log::*;
use simplelog::{ColorChoice, Config, TermLogger, TerminalMode};

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
  TermLogger::init(
    log_level,
    Config::default(),
    TerminalMode::Mixed,
    ColorChoice::Auto,
  )
  .expect("Failed to initialize logger");
}
