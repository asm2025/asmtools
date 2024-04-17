use anyhow::{anyhow, Result};
pub use log4rs::{self as lib};
use std::path::{PathBuf, MAIN_SEPARATOR};

use super::{LogLevel, LOG_DATE_FORMAT, LOG_SIZE_MAX, LOG_SIZE_MIN};
use crate::string::StringEx;

pub fn configure(
    file_name: &str,
    level: LogLevel,
    limit: Option<usize>,
) -> Result<lib::config::runtime::ConfigBuilder> {
    if file_name.is_empty() {
        return Err(anyhow!("File name is empty"));
    }

    let path = PathBuf::from(file_name);
    let folder = match path.parent() {
        Some(folder) => folder.to_str().unwrap().suffix(MAIN_SEPARATOR),
        None => "".to_string(),
    };
    let base_name = path.file_stem().unwrap().to_str().unwrap().to_string();
    let extension = path.extension().unwrap().to_str().unwrap().to_string();
    let roller_pattern = format!("{}{}.{{}}.old.{}", folder, base_name, extension);
    let console = lib::append::console::ConsoleAppender::builder()
        .encoder(Box::new(lib::encode::pattern::PatternEncoder::new(
            "{l:5.5}| {M} | {m}{n}",
        )))
        .build();
    let size_trigger = lib::append::rolling_file::policy::compound::trigger::size::SizeTrigger::new(
        limit
            .unwrap_or(LOG_SIZE_MAX)
            .clamp(LOG_SIZE_MIN, LOG_SIZE_MAX) as u64,
    );
    let fix_window_roller =
        lib::append::rolling_file::policy::compound::roll::fixed_window::FixedWindowRoller::builder()
            .build(&roller_pattern, 6)?;
    let policy = lib::append::rolling_file::policy::compound::CompoundPolicy::new(
        Box::new(size_trigger),
        Box::new(fix_window_roller),
    );
    let file = lib::append::rolling_file::RollingFileAppender::builder()
        .encoder(Box::new(lib::encode::pattern::PatternEncoder::new(
            &format!(
                "{{d({})}} | {{l:5.5}} | {{M}} | {{m}}{{D( | {{f}}:{{L}})}}{{n}}",
                LOG_DATE_FORMAT
            ),
        )))
        .append(true)
        .build(file_name, Box::new(policy))?;
    let config = lib::Config::builder()
        .appender(lib::config::Appender::builder().build("console", Box::new(console)))
        .appender(lib::config::Appender::builder().build("file", Box::new(file)))
        .logger(
            lib::config::Logger::builder()
                .appender("console")
                .build("console", level.into()),
        )
        .logger(
            lib::config::Logger::builder()
                .appender("file")
                .build("file", level.into()),
        );
    Ok(config)
}

pub fn from_config(config: lib::Config) -> Result<lib::Handle> {
    log4rs::init_config(config).map_err(Into::into)
}

pub fn build(file_name: &str) -> Result<lib::Handle> {
    build_with(file_name, LogLevel::Info, None)
}

pub fn build_with(file_name: &str, level: LogLevel, limit: Option<usize>) -> Result<lib::Handle> {
    let config = configure(file_name, level, limit)?.build(
        lib::config::Root::builder()
            .appender("console")
            .appender("file")
            .build(level.into()),
    )?;
    log4rs::init_config(config).map_err(Into::into)
}

pub fn from_file(yaml_file_name: &str) -> Result<()> {
    if yaml_file_name.is_empty() {
        return Err(anyhow!("File name is empty"));
    }

    log4rs::init_file(yaml_file_name, Default::default()).map_err(Into::into)
}
