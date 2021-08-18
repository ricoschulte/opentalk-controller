use anyhow::Result;
use controller::Controller;

#[actix_web::main]
async fn main() {
    controller::try_or_exit(run()).await;
}

async fn run() -> Result<()> {
    if let Some(mut controller) = Controller::create().await? {
        chat::register(&mut controller);
        ee_chat::register(&mut controller);
        automod::register(&mut controller);
        janus_media::register(&mut controller).await?;

        controller.run().await?;
    }

    Ok(())
}
