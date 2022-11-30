use anyhow::Result;
use controller::Controller;

#[actix_web::main]
async fn main() {
    controller::try_or_exit(run()).await;
}

async fn run() -> Result<()> {
    if let Some(mut controller) = Controller::create("K3K Controller Community Edition").await? {
        chat::register(&mut controller);
        automod::register(&mut controller);
        janus_media::register(&mut controller).await?;
        legal_vote::register(&mut controller);
        polls::register(&mut controller);
        protocol::register(&mut controller);
        timer::register(&mut controller);
        whiteboard::register(&mut controller);
        controller.run().await?;
    }

    Ok(())
}
