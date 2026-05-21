pub mod auth;
pub mod config;
pub mod db;
pub mod domain;
pub mod error;
pub mod money;
pub mod psp_client;
pub mod routes;
pub mod webhooks;

use actix_web::{web, App, HttpServer};
use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub psp: Arc<psp_client::PspClient>,
    pub config: Arc<config::Config>,
}

pub fn build_app(
    state: AppState,
) -> App<
    impl actix_web::dev::ServiceFactory<
        actix_web::dev::ServiceRequest,
        Response = actix_web::dev::ServiceResponse<actix_web::body::BoxBody>,
        Config = (),
        InitError = (),
        Error = actix_web::Error,
    >,
> {
    App::new()
        .app_data(web::Data::new(state))
        .app_data(web::JsonConfig::default().limit(256 * 1024))
        .route("/health", web::get().to(routes::health))
        .service(
            web::scope("/v1")
                .service(routes::bootstrap::scope())
                .service(routes::customers::scope())
                .service(routes::invoices::scope())
                .service(routes::webhook_endpoints::scope()),
        )
}

pub async fn serve(state: AppState, bind: &str) -> std::io::Result<()> {
    let state_for_factory = state.clone();
    HttpServer::new(move || build_app(state_for_factory.clone()))
        .bind(bind)?
        .run()
        .await
}
