use std::process::ExitCode;

use stockbit_cli::cli;

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> ExitCode {
    cli::run().await
}
