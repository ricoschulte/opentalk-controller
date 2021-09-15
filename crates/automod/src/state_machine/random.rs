use super::{Error, StateMachineOutput};
use crate::config::{Parameter, SelectionStrategy, StorageConfig};
use crate::{rabbitmq, storage};
use anyhow::Result;
use controller::db::rooms::RoomId;
use controller::prelude::*;
use rand::seq::SliceRandom;
use rand::Rng;
use redis::aio::ConnectionManager;

/// Depending on the config will select a random participant to be speaker. This may be used when
/// the selection_strategy ist `random` or a moderator issues a `Select::Random` command.
pub async fn select_random<R: Rng>(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    config: &StorageConfig,
    rng: &mut R,
) -> Result<Option<StateMachineOutput>, Error> {
    let participant = match &config.parameter {
        Parameter {
            selection_strategy:
                SelectionStrategy::None | SelectionStrategy::Random | SelectionStrategy::Nomination,
            allow_double_selection,
            ..
        } => {
            if config.parameter.animation_on_random {
                let pool = storage::allow_list::get_all(redis_conn, room).await?;
                let selection = pool.choose(rng).copied();

                if let Some(result) = selection {
                    return Ok(Some(StateMachineOutput::StartAnimation(
                        rabbitmq::StartAnimation { pool, result },
                    )));
                } else {
                    None
                }
            } else if *allow_double_selection {
                // GET RANDOM MEMBER FROM ALLOW_LIST
                storage::allow_list::random(redis_conn, room).await?
            } else {
                // POP RANDOM MEMBER FROM ALLOW_LIST
                storage::allow_list::pop_random(redis_conn, room).await?
            }
        }
        Parameter {
            selection_strategy: SelectionStrategy::Playlist,
            ..
        } => {
            // GET RANDOM MEMBER FROM PLAYLIST, REMOVE FROM PLAYLIST
            let playlist = storage::playlist::get_all(redis_conn, room).await?;

            if let Some(participant) = playlist.choose(rng).copied() {
                storage::playlist::remove_first(redis_conn, room, participant).await?;

                Some(participant)
            } else {
                None
            }
        }
    };

    super::map_select_unchecked(
        super::select_unchecked(redis_conn, room, config, participant).await,
    )
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::state_machine::test::{rng, setup, unix_epoch, ROOM};
    use crate::state_machine::StateMachineOutput;
    use serial_test::serial;
    use storage::history::{Entry, EntryKind};

    #[tokio::test]
    #[serial]
    /// Test that our storage works and always just returns the entries since the specified date
    /// 3 entries are added. Two before t and one after t. Only the one after t should be returned.
    async fn history_returns_since() {
        let mut redis_conn = setup().await;

        let p1 = ParticipantId::new_test(1);
        let p2 = ParticipantId::new_test(2);
        let p3 = ParticipantId::new_test(3);

        storage::history::add(
            &mut redis_conn,
            ROOM,
            &Entry {
                timestamp: unix_epoch(1),
                participant: p1,
                kind: EntryKind::Start,
            },
        )
        .await
        .unwrap();

        storage::history::add(
            &mut redis_conn,
            ROOM,
            &Entry {
                timestamp: unix_epoch(2),
                participant: p1,
                kind: EntryKind::Stop,
            },
        )
        .await
        .unwrap();

        storage::history::add(
            &mut redis_conn,
            ROOM,
            &Entry {
                timestamp: unix_epoch(3),
                participant: p3,
                kind: EntryKind::Start,
            },
        )
        .await
        .unwrap();

        storage::history::add(
            &mut redis_conn,
            ROOM,
            &Entry {
                timestamp: unix_epoch(4),
                participant: p3,
                kind: EntryKind::Stop,
            },
        )
        .await
        .unwrap();

        storage::history::add(
            &mut redis_conn,
            ROOM,
            &Entry {
                timestamp: unix_epoch(10),
                participant: p2,
                kind: EntryKind::Start,
            },
        )
        .await
        .unwrap();

        let history = storage::history::get(&mut redis_conn, ROOM, unix_epoch(5))
            .await
            .unwrap();

        assert_eq!(history, vec![p2]);
    }

    /// Test that history works when selecting a random member
    /// 3 entries are added, assert that every time select_random returns an entry, it is also appended to the history.
    #[tokio::test]
    #[serial]
    async fn history_addition() {
        let mut redis_conn = setup().await;
        let mut rng = rng();

        let p1 = ParticipantId::new_test(1);
        let p2 = ParticipantId::new_test(2);
        let p3 = ParticipantId::new_test(3);

        storage::allow_list::set(&mut redis_conn, ROOM, &[p1, p2, p3])
            .await
            .unwrap();

        let config = StorageConfig {
            started: unix_epoch(0),
            parameter: Parameter {
                selection_strategy: SelectionStrategy::None,
                show_list: false,
                consider_hand_raise: false,
                time_limit: None,
                allow_double_selection: false,
                animation_on_random: false,
            },
        };

        // === SELECT FIRST
        assert!(matches!(
            select_random(&mut redis_conn, ROOM, &config, &mut rng)
                .await
                .unwrap(),
            Some(StateMachineOutput::SpeakerUpdate(_))
        ));

        let first = storage::speaker::get(&mut redis_conn, ROOM)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            storage::history::get(&mut redis_conn, ROOM, unix_epoch(0))
                .await
                .unwrap(),
            vec![first]
        );

        // === SELECT SECOND
        select_random(&mut redis_conn, ROOM, &config, &mut rng)
            .await
            .unwrap();

        let second = storage::speaker::get(&mut redis_conn, ROOM)
            .await
            .unwrap()
            .unwrap();

        assert_ne!(first, second);

        assert_eq!(
            storage::history::get(&mut redis_conn, ROOM, unix_epoch(0))
                .await
                .unwrap(),
            vec![first, second]
        );

        // === SELECT THIRD
        select_random(&mut redis_conn, ROOM, &config, &mut rng)
            .await
            .unwrap();

        let third = storage::speaker::get(&mut redis_conn, ROOM)
            .await
            .unwrap()
            .unwrap();

        assert_ne!(first, third);
        assert_ne!(second, third);

        assert_eq!(
            storage::history::get(&mut redis_conn, ROOM, unix_epoch(0))
                .await
                .unwrap(),
            vec![first, second, third]
        );
    }

    /// Tests that random selection returns a StartAnimation response when animation_on_random is true
    #[tokio::test]
    #[serial]
    async fn start_animation() {
        let mut redis_conn = setup().await;
        let mut rng = rng();

        let p1 = ParticipantId::new_test(1);
        let p2 = ParticipantId::new_test(2);
        let p3 = ParticipantId::new_test(3);

        storage::allow_list::set(&mut redis_conn, ROOM, &[p1, p2, p3])
            .await
            .unwrap();

        let config = StorageConfig {
            started: unix_epoch(0),
            parameter: Parameter {
                selection_strategy: SelectionStrategy::None,
                show_list: false,
                consider_hand_raise: false,
                time_limit: None,
                allow_double_selection: false,
                animation_on_random: true,
            },
        };

        assert!(matches!(
            select_random(&mut redis_conn, ROOM, &config, &mut rng)
                .await
                .unwrap(),
            Some(StateMachineOutput::StartAnimation(_))
        ));
    }

    /// Test random selection when selection_strategy is None and double selection is forbidden
    /// 3 entries are added to the allow_list, two entries are added to the history.
    /// Assert that the third entry is returned by select_random
    #[tokio::test]
    #[serial]
    async fn select_random_when_none() {
        let mut redis_conn = setup().await;
        let mut rng = rng();

        let p1 = ParticipantId::new_test(1);
        let p2 = ParticipantId::new_test(2);
        let p3 = ParticipantId::new_test(3);

        storage::allow_list::set(&mut redis_conn, ROOM, &[p1, p2, p3])
            .await
            .unwrap();

        let config = StorageConfig {
            started: unix_epoch(0),
            parameter: Parameter {
                selection_strategy: SelectionStrategy::None,
                show_list: false,
                consider_hand_raise: false,
                time_limit: None,
                allow_double_selection: false,
                animation_on_random: false,
            },
        };

        select_random(&mut redis_conn, ROOM, &config, &mut rng)
            .await
            .unwrap();

        let speaker = storage::speaker::get(&mut redis_conn, ROOM)
            .await
            .unwrap()
            .unwrap();

        assert!([p1, p2, p3].contains(&speaker));
    }

    /// Test random selection when selection_strategy is Playlist
    /// 3 entries are added to the playlist, one entry is added to the history (stopped).
    /// Assert that select_random removes the entries from playlist and adds them to the history.
    #[tokio::test]
    #[serial]
    async fn select_random_when_playlist() {
        let mut redis_conn = setup().await;
        let mut rng = rng();

        let p1 = ParticipantId::new_test(1);
        let p2 = ParticipantId::new_test(2);
        let p3 = ParticipantId::new_test(3);

        storage::playlist::set(&mut redis_conn, ROOM, &[p1, p2, p3])
            .await
            .unwrap();

        storage::history::add(
            &mut redis_conn,
            ROOM,
            &Entry {
                timestamp: unix_epoch(1),
                participant: p1,
                kind: EntryKind::Start,
            },
        )
        .await
        .unwrap();

        storage::history::add(
            &mut redis_conn,
            ROOM,
            &Entry {
                timestamp: unix_epoch(2),
                participant: p1,
                kind: EntryKind::Stop,
            },
        )
        .await
        .unwrap();

        let config = StorageConfig {
            started: unix_epoch(0),
            parameter: Parameter {
                selection_strategy: SelectionStrategy::Playlist,
                show_list: false,
                consider_hand_raise: false,
                time_limit: None,
                allow_double_selection: false,
                animation_on_random: false,
            },
        };

        // === SELECT FIRST
        select_random(&mut redis_conn, ROOM, &config, &mut rng)
            .await
            .unwrap();

        let speaker = storage::speaker::get(&mut redis_conn, ROOM)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(speaker, p3);
        assert_eq!(
            storage::playlist::get_all(&mut redis_conn, ROOM)
                .await
                .unwrap(),
            vec![p1, p2]
        );

        // === SELECT SECOND
        select_random(&mut redis_conn, ROOM, &config, &mut rng)
            .await
            .unwrap();

        let speaker = storage::speaker::get(&mut redis_conn, ROOM)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(speaker, p2);
        assert_eq!(
            storage::playlist::get_all(&mut redis_conn, ROOM)
                .await
                .unwrap(),
            vec![p1]
        );

        // === SELECT THIRD
        select_random(&mut redis_conn, ROOM, &config, &mut rng)
            .await
            .unwrap();

        let speaker = storage::speaker::get(&mut redis_conn, ROOM)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(speaker, p1);
        assert_eq!(
            storage::playlist::get_all(&mut redis_conn, ROOM)
                .await
                .unwrap(),
            vec![]
        );

        // === SELECT LAST MUST BE NONE
        select_random(&mut redis_conn, ROOM, &config, &mut rng)
            .await
            .unwrap();

        assert_eq!(
            storage::speaker::get(&mut redis_conn, ROOM).await.unwrap(),
            None
        );
    }

    /// Test random selection when selection_strategy is Random and reselection is allowed
    /// 3 entries are added to the allow_list. Select 4 times. Assert that at least once a double selection was encountered
    #[tokio::test]
    #[serial]
    async fn select_random_when_random_allow_double_select() {
        let mut redis_conn = setup().await;
        let mut rng = rng();

        let p1 = ParticipantId::new_test(1);
        let p2 = ParticipantId::new_test(2);
        let p3 = ParticipantId::new_test(3);

        storage::allow_list::set(&mut redis_conn, ROOM, &[p1, p2, p3])
            .await
            .unwrap();

        storage::history::add(
            &mut redis_conn,
            ROOM,
            &Entry {
                timestamp: unix_epoch(1),
                participant: p1,
                kind: EntryKind::Start,
            },
        )
        .await
        .unwrap();

        storage::history::add(
            &mut redis_conn,
            ROOM,
            &Entry {
                timestamp: unix_epoch(2),
                participant: p1,
                kind: EntryKind::Stop,
            },
        )
        .await
        .unwrap();

        let config = StorageConfig {
            started: unix_epoch(0),
            parameter: Parameter {
                selection_strategy: SelectionStrategy::Random,
                show_list: false,
                consider_hand_raise: false,
                time_limit: None,
                allow_double_selection: true,
                animation_on_random: false,
            },
        };

        // === SELECT FIRST
        let mut selected = Vec::new();

        for _ in 0..4 {
            select_random(&mut redis_conn, ROOM, &config, &mut rng)
                .await
                .unwrap();

            let speaker = storage::speaker::get(&mut redis_conn, ROOM)
                .await
                .unwrap()
                .unwrap();

            if selected.contains(&speaker) {
                return;
            } else {
                selected.push(speaker);
            }
        }

        panic!("selected did not contain any duplicates ???")
    }
}
