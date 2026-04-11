//! Minimal Bevy example that discovers an NWN install, loads the default KEY
//! set, and logs the resolved KEY paths.

use bevy::{app::ScheduleRunnerPlugin, log::LogPlugin, prelude::*};
use nwnrs_bevy::{NwnInstallPlugin, NwnInstallSettings};

fn main() {
    App::new()
        .add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_once()))
        .add_plugins(LogPlugin::default())
        .add_plugins(NwnInstallPlugin::new(NwnInstallSettings::default()))
        .run();
}
