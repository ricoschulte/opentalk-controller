use super::Error;
use crate::api::signaling::ws_modules::ee::automod::config::{
    Parameter, SelectionStrategy, StorageConfig,
};
use crate::api::signaling::ws_modules::ee::automod::rabbitmq;
use crate::api::signaling::ws_modules::ee::automod::storage;
use crate::api::signaling::ParticipantId;
use anyhow::Result;
use rand::Rng;
use redis::aio::ConnectionManager;
use uuid::Uuid;

/// Depending on the config will inspect/change the state_machine's state to select the next
/// user to be speaker.
pub async fn select_next<R: Rng>(
    redis_conn: &mut ConnectionManager,
    room: Uuid,
    config: &StorageConfig,
    user_selected: Option<ParticipantId>,
    rng: &mut R,
) -> Result<Option<rabbitmq::SpeakerUpdate>, Error> {
    let participant = match config.parameter {
        Parameter {
            selection_strategy: SelectionStrategy::None,
            ..
        } => None,
        Parameter {
            selection_strategy: SelectionStrategy::Playlist,
            ..
        } => storage::playlist::pop(redis_conn, room).await?,
        Parameter {
            selection_strategy: SelectionStrategy::Nomination,
            allow_double_selection,
            ..
        } => {
            select_next_nomination(
                redis_conn,
                room,
                config,
                user_selected,
                allow_double_selection,
            )
            .await?
        }
        Parameter {
            selection_strategy: SelectionStrategy::Random,
            ..
        } => return super::select_random(redis_conn, room, config, rng).await,
    };

    super::select_unchecked(redis_conn, room, config, participant).await
}

/// Returns the next (if any) participant to be selected inside a `Nomination` selection strategy.
async fn select_next_nomination(
    redis_conn: &mut ConnectionManager,
    room: Uuid,
    config: &StorageConfig,
    user_selected: Option<ParticipantId>,
    allow_double_selection: bool,
) -> Result<Option<ParticipantId>, Error> {
    // get user selection
    let participant = if let Some(participant) = user_selected {
        participant
    } else {
        // No next user nominated, unset current speaker
        return Ok(None);
    };

    // check if selected participant is permitted by allow_list
    if !storage::allow_list::is_allowed(redis_conn, room, participant).await? {
        return Err(Error::InvalidSelection);
    }

    if allow_double_selection {
        Ok(Some(participant))
    } else {
        let history = storage::history::get(redis_conn, room, config.started).await?;

        if history.contains(&participant) {
            Err(Error::InvalidSelection)
        } else {
            Ok(Some(participant))
        }
    }
}

#[cfg(test)]
mod test {
    use super::super::test::{rng, setup, unix_epoch, ROOM};
    use super::*;
    use crate::api::signaling::ParticipantId;
    use serial_test::serial;
    use storage::history::Entry;

    fn assert_history_without_timestamp(lhs: &[Entry], rhs: &[Entry]) {
        assert_eq!(lhs.len(), rhs.len());
        lhs.iter().zip(rhs.iter()).for_each(|(lhs, rhs)| {
            assert_eq!(lhs.kind, rhs.kind);
            assert_eq!(lhs.participant, rhs.participant);
        })
    }

    /// Test next when selection_strategy is Nomination and reselection is allowed
    #[tokio::test]
    #[serial]
    async fn nomination_reselection_allowed() {
        let mut redis_conn = setup().await;
        let mut rng = rng();

        let p1 = ParticipantId::new_test(1);

        // Add current speaker
        storage::history::add(&mut redis_conn, ROOM, &Entry::start(p1))
            .await
            .unwrap();
        storage::speaker::set(&mut redis_conn, ROOM, p1)
            .await
            .unwrap();

        let config = StorageConfig {
            started: unix_epoch(0),
            parameter: Parameter {
                selection_strategy: SelectionStrategy::Nomination,
                show_list: false,
                consider_hand_raise: false,
                time_limit: None,
                pause_time: None,
                allow_double_selection: true,
            },
        };

        // Check with nominee in history
        let next = select_next(&mut redis_conn, ROOM, &config, Some(p1), &mut rng)
            .await
            .unwrap();
        let history = storage::history::get(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_eq!(history, vec![p1, p1]);

        let full_history = storage::history::get_entries(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_history_without_timestamp(
            &full_history,
            &vec![Entry::start(p1), Entry::stop(p1), Entry::start(p1)],
        );

        let speaker = storage::speaker::get(&mut redis_conn, ROOM).await.unwrap();
        assert_eq!(speaker, Some(p1));

        assert_eq!(
            next,
            Some(rabbitmq::SpeakerUpdate {
                speaker: Some(p1),
                history: Some(vec![p1, p1]),
                remaining: None
            })
        );

        let config = StorageConfig {
            started: unix_epoch(0),
            parameter: Parameter {
                selection_strategy: SelectionStrategy::Nomination,
                show_list: false,
                consider_hand_raise: false,
                time_limit: None,
                pause_time: None,
                allow_double_selection: false,
            },
        };

        // Check with nominee in history
        let next = select_next(&mut redis_conn, ROOM, &config, Some(p1), &mut rng).await;
        assert!(matches!(next, Err(Error::InvalidSelection)));

        let history = storage::history::get(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_eq!(history, vec![p1, p1]);

        let full_history = storage::history::get_entries(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_history_without_timestamp(
            &full_history,
            &vec![Entry::start(p1), Entry::stop(p1), Entry::start(p1)],
        );

        let speaker = storage::speaker::get(&mut redis_conn, ROOM).await.unwrap();
        assert_eq!(speaker, Some(p1));
    }

    /// Test next when selection_strategy is Nomination and an empty allow_list
    #[tokio::test]
    #[serial]
    async fn nomination_but_empty_allow_list() {
        let mut redis_conn = setup().await;
        let mut rng = rng();

        let p1 = ParticipantId::new_test(1);
        let p2 = ParticipantId::new_test(2);

        let config = StorageConfig {
            started: unix_epoch(0),
            parameter: Parameter {
                selection_strategy: SelectionStrategy::Nomination,
                show_list: false,
                consider_hand_raise: false,
                time_limit: None,
                pause_time: None,
                allow_double_selection: false,
            },
        };

        let next = select_next(&mut redis_conn, ROOM, &config, Some(p1), &mut rng)
            .await
            .unwrap();

        let history = storage::history::get(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_eq!(history, vec![p1]);

        let full_history = storage::history::get_entries(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_history_without_timestamp(&full_history, &vec![Entry::start(p1)]);

        let speaker = storage::speaker::get(&mut redis_conn, ROOM).await.unwrap();
        assert_eq!(speaker, Some(p1));

        assert_eq!(
            next,
            Some(rabbitmq::SpeakerUpdate {
                speaker: Some(p1),
                history: Some(vec![p1]),
                remaining: None
            })
        );

        let next = select_next(&mut redis_conn, ROOM, &config, Some(p2), &mut rng)
            .await
            .unwrap();

        let history = storage::history::get(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_eq!(history, vec![p1, p2]);

        let full_history = storage::history::get_entries(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_history_without_timestamp(
            &full_history,
            &vec![Entry::start(p1), Entry::stop(p1), Entry::start(p2)],
        );

        let speaker = storage::speaker::get(&mut redis_conn, ROOM).await.unwrap();
        assert_eq!(speaker, Some(p2));

        assert_eq!(
            next,
            Some(rabbitmq::SpeakerUpdate {
                speaker: Some(p2),
                history: Some(vec![p1, p2]),
                remaining: None
            })
        );
    }

    /// Test next when selection_strategy is Nomination and an allow_list containing only 2 of 3
    /// possible participants
    #[tokio::test]
    #[serial]
    async fn nomination_with_allow_list() {
        let mut redis_conn = setup().await;
        let mut rng = rng();

        let p1 = ParticipantId::new_test(1);
        let p2 = ParticipantId::new_test(2);
        let p3 = ParticipantId::new_test(3);

        storage::allow_list::set(&mut redis_conn, ROOM, &vec![p1, p2])
            .await
            .unwrap();

        let config = StorageConfig {
            started: unix_epoch(0),
            parameter: Parameter {
                selection_strategy: SelectionStrategy::Nomination,
                show_list: false,
                consider_hand_raise: false,
                time_limit: None,
                pause_time: None,
                allow_double_selection: false,
            },
        };
        // Check allowed participant
        let next = select_next(&mut redis_conn, ROOM, &config, Some(p1), &mut rng)
            .await
            .unwrap();
        let history = storage::history::get(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_eq!(history, vec![p1]);

        let full_history = storage::history::get_entries(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_history_without_timestamp(&full_history, &vec![Entry::start(p1)]);

        let speaker = storage::speaker::get(&mut redis_conn, ROOM).await.unwrap();
        assert_eq!(speaker, Some(p1));

        assert_eq!(
            next,
            Some(rabbitmq::SpeakerUpdate {
                speaker: Some(p1),
                history: Some(vec![p1]),
                remaining: None
            })
        );

        // Check non-allowed participant
        let next = select_next(&mut redis_conn, ROOM, &config, Some(p3), &mut rng).await;
        assert!(matches!(next, Err(Error::InvalidSelection)));

        let history = storage::history::get(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_eq!(history, vec![p1]);

        let full_history = storage::history::get_entries(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_history_without_timestamp(&full_history, &vec![Entry::start(p1)]);

        let speaker = storage::speaker::get(&mut redis_conn, ROOM).await.unwrap();
        assert_eq!(speaker, Some(p1));

        // Check with nominee in history
        let next = select_next(&mut redis_conn, ROOM, &config, Some(p1), &mut rng).await;
        assert!(matches!(next, Err(Error::InvalidSelection)));

        let history = storage::history::get(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_eq!(history, vec![p1]);

        let full_history = storage::history::get_entries(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_history_without_timestamp(&full_history, &vec![Entry::start(p1)]);

        let speaker = storage::speaker::get(&mut redis_conn, ROOM).await.unwrap();
        assert_eq!(speaker, Some(p1));
    }

    /// Test next when selection_strategy is Nomination but no nomination was given and the allow_list
    /// is empty
    #[tokio::test]
    #[serial]
    async fn nomination_without_nomination_empty_allow_list() {
        let mut redis_conn = setup().await;
        let mut rng = rng();

        let config = StorageConfig {
            started: unix_epoch(0),
            parameter: Parameter {
                selection_strategy: SelectionStrategy::Nomination,
                show_list: false,
                consider_hand_raise: false,
                time_limit: None,
                pause_time: None,
                allow_double_selection: false,
            },
        };

        let next = select_next(&mut redis_conn, ROOM, &config, None, &mut rng)
            .await
            .unwrap();

        let history = storage::history::get(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_eq!(history, Vec::new());

        let full_history = storage::history::get_entries(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert!(full_history.is_empty());

        let speaker = storage::speaker::get(&mut redis_conn, ROOM).await.unwrap();
        assert_eq!(speaker, None);

        assert_eq!(next, None);
    }

    /// Test next when selection_strategy is Nomination but no nomination was given but the
    /// allow_list contains possible participants
    #[tokio::test]
    #[serial]
    async fn nomination_without_nomination_with_allow_list() {
        let mut redis_conn = setup().await;
        let mut rng = rng();

        let p1 = ParticipantId::new_test(1);
        let p2 = ParticipantId::new_test(2);
        let p3 = ParticipantId::new_test(3);

        storage::allow_list::set(&mut redis_conn, ROOM, &vec![p1, p2, p3])
            .await
            .unwrap();

        let config = StorageConfig {
            started: unix_epoch(0),
            parameter: Parameter {
                selection_strategy: SelectionStrategy::Nomination,
                show_list: false,
                consider_hand_raise: false,
                time_limit: None,
                pause_time: None,
                allow_double_selection: false,
            },
        };

        // select_next with empty history
        let next = select_next(&mut redis_conn, ROOM, &config, None, &mut rng)
            .await
            .unwrap();
        let history = storage::history::get(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_eq!(history, Vec::new());

        let full_history = storage::history::get_entries(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_history_without_timestamp(&full_history, &[]);

        let speaker = storage::speaker::get(&mut redis_conn, ROOM).await.unwrap();
        assert_eq!(speaker, None);

        assert_eq!(next, None);

        // Add current speaker
        storage::history::add(&mut redis_conn, ROOM, &Entry::start(p1))
            .await
            .unwrap();
        storage::speaker::set(&mut redis_conn, ROOM, p1)
            .await
            .unwrap();

        // select_next with non-empty history
        let next = select_next(&mut redis_conn, ROOM, &config, None, &mut rng)
            .await
            .unwrap();
        let history = storage::history::get(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_eq!(history, vec![p1]);

        let full_history = storage::history::get_entries(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_history_without_timestamp(&full_history, &[Entry::start(p1), Entry::stop(p1)]);

        let speaker = storage::speaker::get(&mut redis_conn, ROOM).await.unwrap();
        assert_eq!(speaker, None);

        assert_eq!(
            next,
            Some(rabbitmq::SpeakerUpdate {
                speaker: None,
                history: Some(vec![p1]),
                remaining: None
            })
        );
    }

    /// Test next when selection_strategy is None
    #[tokio::test]
    #[serial]
    async fn select_next_with_none() {
        let mut redis_conn = setup().await;
        let mut rng = rng();

        let p1 = ParticipantId::new_test(1);

        let config = StorageConfig {
            started: unix_epoch(0),
            parameter: Parameter {
                selection_strategy: SelectionStrategy::None,
                show_list: false,
                consider_hand_raise: false,
                time_limit: None,
                pause_time: None,
                allow_double_selection: false,
            },
        };

        let next = select_next(&mut redis_conn, ROOM, &config, None, &mut rng)
            .await
            .unwrap();

        let history = storage::history::get(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_eq!(history, vec![]);

        let full_history = storage::history::get_entries(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_history_without_timestamp(&full_history, &[]);

        let speaker = storage::speaker::get(&mut redis_conn, ROOM).await.unwrap();
        assert_eq!(speaker, None);

        assert_eq!(next, None);

        // Add current speaker
        storage::history::add(&mut redis_conn, ROOM, &Entry::start(p1))
            .await
            .unwrap();
        storage::speaker::set(&mut redis_conn, ROOM, p1)
            .await
            .unwrap();

        // select_next with non-empty history
        let next = select_next(&mut redis_conn, ROOM, &config, None, &mut rng)
            .await
            .unwrap();

        let history = storage::history::get(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_eq!(history, vec![p1]);

        let full_history = storage::history::get_entries(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_history_without_timestamp(&full_history, &[Entry::start(p1), Entry::stop(p1)]);

        let speaker = storage::speaker::get(&mut redis_conn, ROOM).await.unwrap();
        assert_eq!(speaker, None);

        assert_eq!(
            next,
            Some(rabbitmq::SpeakerUpdate {
                speaker: None,
                history: Some(vec![p1]),
                remaining: None
            })
        );
    }

    /// Test next when selection_strategy is Playlist
    #[tokio::test]
    #[serial]
    async fn select_next_with_playlist() {
        let mut redis_conn = setup().await;
        let mut rng = rng();

        let p1 = ParticipantId::new_test(1);
        let p2 = ParticipantId::new_test(2);
        let p3 = ParticipantId::new_test(3);

        let config = StorageConfig {
            started: unix_epoch(0),
            parameter: Parameter {
                selection_strategy: SelectionStrategy::Playlist,
                show_list: false,
                consider_hand_raise: false,
                time_limit: None,
                pause_time: None,
                allow_double_selection: false,
            },
        };

        // Test with empty playlist
        let next = select_next(&mut redis_conn, ROOM, &config, None, &mut rng)
            .await
            .unwrap();

        let history = storage::history::get(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_eq!(history, vec![]);

        let full_history = storage::history::get_entries(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_history_without_timestamp(&full_history, &[]);

        let speaker = storage::speaker::get(&mut redis_conn, ROOM).await.unwrap();
        assert_eq!(speaker, None);

        assert_eq!(next, None);

        // Create playlist
        storage::playlist::set(&mut redis_conn, ROOM, &[p2, p1, p3])
            .await
            .unwrap();

        // select_next with empty history and playlist
        let next = select_next(&mut redis_conn, ROOM, &config, None, &mut rng)
            .await
            .unwrap();

        let history = storage::history::get(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_eq!(history, vec![p2]);

        let full_history = storage::history::get_entries(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_history_without_timestamp(&full_history, &[Entry::start(p2)]);

        let speaker = storage::speaker::get(&mut redis_conn, ROOM).await.unwrap();
        assert_eq!(speaker, Some(p2));

        assert_eq!(
            next,
            Some(rabbitmq::SpeakerUpdate {
                speaker: Some(p2),
                history: Some(vec![p2]),
                remaining: Some(vec![p1, p3])
            })
        );

        // select_next with non-empty history and playlist
        let next = select_next(&mut redis_conn, ROOM, &config, None, &mut rng)
            .await
            .unwrap();

        let history = storage::history::get(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_eq!(history, vec![p2, p1]);

        let full_history = storage::history::get_entries(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_history_without_timestamp(
            &full_history,
            &[Entry::start(p2), Entry::stop(p2), Entry::start(p1)],
        );

        let speaker = storage::speaker::get(&mut redis_conn, ROOM).await.unwrap();
        assert_eq!(speaker, Some(p1));

        assert_eq!(
            next,
            Some(rabbitmq::SpeakerUpdate {
                speaker: Some(p1),
                history: Some(vec![p2, p1]),
                remaining: Some(vec![p3])
            })
        );

        // select_next with non-empty history and playlist, to drain playlist
        let next = select_next(&mut redis_conn, ROOM, &config, None, &mut rng)
            .await
            .unwrap();
        assert_eq!(
            next,
            Some(rabbitmq::SpeakerUpdate {
                speaker: Some(p3),
                history: Some(vec![p2, p1, p3]),
                remaining: Some(vec![])
            })
        );

        // select_next with non-empty history and empty-playlist
        let next = select_next(&mut redis_conn, ROOM, &config, None, &mut rng)
            .await
            .unwrap();

        let history = storage::history::get(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_eq!(history, vec![p2, p1, p3]);

        let full_history = storage::history::get_entries(&mut redis_conn, ROOM, unix_epoch(0))
            .await
            .unwrap();
        assert_history_without_timestamp(
            &full_history,
            &[
                Entry::start(p2),
                Entry::stop(p2),
                Entry::start(p1),
                Entry::stop(p1),
                Entry::start(p3),
                Entry::stop(p3),
            ],
        );

        let speaker = storage::speaker::get(&mut redis_conn, ROOM).await.unwrap();
        assert_eq!(speaker, None);

        assert_eq!(
            next,
            Some(rabbitmq::SpeakerUpdate {
                speaker: None,
                history: Some(vec![p2, p1, p3]),
                remaining: Some(vec![])
            })
        );
    }
}
