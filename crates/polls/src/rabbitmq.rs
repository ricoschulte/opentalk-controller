use crate::{Config, PollId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    Started(Config),
    Update(PollId),
    Finish(PollId),
}
