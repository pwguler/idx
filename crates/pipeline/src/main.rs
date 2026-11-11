use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    pipeline::cli::run().await
}
