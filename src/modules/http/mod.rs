use actix_web::web::ServiceConfig;

/// Extension to an actix app.
///
/// Middleware can be used by scoping services.
///
/// # Example
///
/// ```
/// use k3k_controller_core::application::{HttpModule, AppDataMap};
/// use actix_web::web::ServiceConfig;
/// use actix_web::web::scope;
/// use actix_web::middleware::DefaultHeaders;
///
/// #[actix_web::get("/hello")]
/// async fn serve_hello() -> String {
///     String::from("Hello!")
/// }
///
/// pub struct ServeHelloModule;
///
/// impl HttpModule for ServeHelloModule {
///     fn register(&self, app: &mut ServiceConfig) {
///         let scope = scope("")
///             .wrap(DefaultHeaders::default())
///             .service(serve_hello);
///
///         app.service(scope);
///     }
/// }
/// ```
pub trait HttpModule: Send + Sync + 'static {
    /// Will be called when creating an actix app
    fn register(&self, app: &mut ServiceConfig);
}
