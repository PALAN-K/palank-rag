//! palank-rag CLI 진입점

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    // 로깅 초기화
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    // CLI 실행
    let cli = palank_rag::cli::Cli::parse();

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(palank_rag::cli::run(cli))
}
