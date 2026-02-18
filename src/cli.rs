use std::path::PathBuf;

use clap::{Args, Parser, ValueEnum, ValueHint};
use clap_verbosity_flag::{InfoLevel, Verbosity};
use derive_more::Debug;
use strum::EnumIter;

#[derive(Clone, Debug, Parser, Default)]
#[command(name = "bandwhich", version)]
pub struct Opt {
    #[arg(short, long)]
    /// The network interface to listen on, eg. eth0
    pub interface: Option<String>,

    #[arg(short, long)]
    /// Machine friendlier output
    pub raw: bool,

    #[arg(long, value_hint = ValueHint::FilePath)]
    /// Enable debug logging to a file
    pub log_to: Option<PathBuf>,

    #[command(flatten)]
    pub verbosity: Verbosity<InfoLevel>,

    #[command(flatten)]
    pub render_opts: RenderOpts,
}

#[derive(Copy, Clone, Debug, Default, Args)]
pub struct RenderOpts {
    #[arg(short, long, value_enum, default_value_t)]
    /// Choose a specific family of units
    pub unit_family: UnitFamily,
}

// IMPRV: it would be nice if we can `#[cfg_attr(not(build), derive(strum::EnumIter))]` this
// unfortunately there is no configuration option for build script detection
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, ValueEnum, EnumIter)]
pub enum UnitFamily {
    #[default]
    /// bytes, in powers of 2^10
    BinBytes,
    /// bits, in powers of 2^10
    BinBits,
    /// bytes, in powers of 10^3
    SiBytes,
    /// bits, in powers of 10^3
    SiBits,
}
