use dodo_api::{config::Config, db, psp_client::PspClient, webhooks, AppState};
use std::sync::Arc;

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,dodo_api=info")),
        )
        .init();

    let config = Config::from_env();
    tracing::info!(?config, "starting dodo-api");

    let pool = db::connect(&config.database_url).await?;
    db::migrate(&pool).await?;

    let psp = Arc::new(PspClient::new(config.psp_base_url.clone(), config.psp_timeout));
    let state = AppState {
        db: pool.clone(),
        psp,
        config: Arc::new(config.clone()),
    };

    // Background workers. Tokio tasks in-process for the MVP — production
    // would split these into separate processes (see DESIGN.md §7).
    let dispatcher_pool = pool.clone();
    tokio::spawn(async move {
        webhooks::dispatcher::run(dispatcher_pool, config.dispatcher_interval).await;
    });

    let reconciler_state = state.clone();
    tokio::spawn(async move {
        webhooks::reconciler::run(
            reconciler_state,
            config.reconciler_interval,
            config.reconciler_stale_after,
        )
        .await;
    });

    let bind = config.bind.clone();
    tracing::info!(%bind, "dodo-api listening");
    dodo_api::serve(state, &bind).await?;
    Ok(())
}
