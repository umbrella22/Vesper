use anyhow::Result;

fn main() -> Result<()> {
    player_cli::ios_bridge_shim::run_cli(std::env::args().skip(1))
}
