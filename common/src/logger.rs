use log::*;
use simplelog::{ColorChoice, Config, TermLogger, TerminalMode};

pub fn init_logger() {
    TermLogger::init(
        LevelFilter::Info,
        Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )
    .expect("Failed to initialize logger");
}
