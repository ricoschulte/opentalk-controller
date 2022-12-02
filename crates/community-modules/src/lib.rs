use anyhow::Result;
use controller::Controller;

pub async fn register(controller: &mut Controller) -> Result<()> {
    chat::register(controller);
    automod::register(controller);
    janus_media::register(controller).await?;
    legal_vote::register(controller);
    polls::register(controller);
    protocol::register(controller);
    timer::register(controller);
    whiteboard::register(controller);
    Ok(())
}
