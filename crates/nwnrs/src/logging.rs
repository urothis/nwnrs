use std::io::IsTerminal as _;

use tracing_subscriber::EnvFilter;

use crate::args::ColorMode;

pub(crate) fn init_tracing(color: ColorMode) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_error| {
        EnvFilter::new(
            "warn,nwnrs::launcher=info,nwnrs::console=info,nwnrs::server=info,nwnrs::runtime=info,\
             nwnrs::script=info",
        )
    });
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(color_enabled(color))
        .without_time()
        .with_target(true)
        .try_init();
}

fn color_enabled(color: ColorMode) -> bool {
    match color {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => {
            std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ColorMode, color_enabled, init_tracing};

    #[test]
    fn tracing_initialization_is_idempotent() {
        init_tracing(ColorMode::Never);
        init_tracing(ColorMode::Never);
    }

    #[test]
    fn explicit_color_modes_do_not_depend_on_the_terminal() {
        assert!(color_enabled(ColorMode::Always));
        assert!(!color_enabled(ColorMode::Never));
    }
}
