//! 日志模块

use anyhow::Result;

pub fn setup_logger(log_file: Option<&str>) -> Result<()> {
    let mut base_config = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{} [{}] {}",
                chrono::Local::now().format("%Y-%m-%dT%H:%M:%S"),
                record.level(),
                message
            ))
        });

    if let Some(file) = log_file {
        base_config = base_config.chain(fern::log_file(file)?);
    }

    base_config.apply()?;

    Ok(())
}
