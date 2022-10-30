// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use anyhow::Result;
use controller::Controller;

#[actix_web::main]
async fn main() {
    controller::try_or_exit(run()).await;
}

async fn run() -> Result<()> {
    if let Some(mut controller) = Controller::create("K3K Controller Community Edition").await? {
        community_modules::register(&mut controller).await?;
        controller.run().await?;
    }

    Ok(())
}
