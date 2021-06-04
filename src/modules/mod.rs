use actix_web::web::ServiceConfig;
use http::HttpModule;
use std::sync::Arc;

pub mod http;

/// Builder type for an [`Application`]
#[derive(Default)]
pub struct ApplicationBuilder {
    http_modules: Vec<Box<dyn HttpModule>>,
}

impl ApplicationBuilder {
    /// Add a implementation of [`HttpModule`] to the application
    pub fn add_http_module<M>(&mut self, module: M) -> &mut Self
    where
        M: HttpModule,
    {
        self.http_modules.push(Box::new(module));
        self
    }

    /// Finalize the builder into a [`Application`]
    pub fn finish(self) -> Application {
        Application {
            http_modules: Arc::from(self.http_modules.into_boxed_slice()),
        }
    }
}

/// Contains all modules and app-data collected from [`ApplicationBuilder::finish`].
///
/// Is used to configure actix app
#[derive(Clone)]
pub struct Application {
    http_modules: Arc<[Box<dyn HttpModule>]>,
}

impl Application {
    /// Function which returns a closure used to configure an actix app.
    ///
    /// # Example
    ///
    /// ```
    ///# use k3k_controller_core::application::ApplicationBuilder;
    ///
    /// let app = ApplicationBuilder::default().finish();
    ///
    /// actix_web::App::new().configure(app.configure());
    /// ```
    pub fn configure(&self) -> impl FnOnce(&mut ServiceConfig) + '_ {
        move |app| {
            for http_module in &*self.http_modules {
                http_module.register(app);
            }
        }
    }
}
