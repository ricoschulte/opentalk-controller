use janus_client::transport::RabbitMqConfig;
use janus_client::transport::WebSocketConfig;
use janus_client::types::incoming;
use janus_client::types::outgoing;
use janus_client::*;
use lapin::Connection;
use lapin::ConnectionProperties;
use std::sync::Arc;
use test_log::test;
use tokio::sync::mpsc;

async fn create_rmq_config() -> RabbitMqConfig {
    let rabbit_addr =
        std::env::var("AMQP_ADDR").unwrap_or_else(|_| "amqp://localhost:5672".to_owned());
    let connection = Connection::connect(&rabbit_addr, ConnectionProperties::default())
        .await
        .expect("Could not connect to rabbitmq");
    let channel = connection
        .create_channel()
        .await
        .expect("Could not create channel");
    RabbitMqConfig::new_from_channel(
        channel,
        "to-janus".to_owned(),
        "janus-exchange".to_owned(),
        "from-janus".to_owned(),
        "k3k-signaling-echo-external-channel".to_owned(),
    )
}

fn create_ws_config() -> WebSocketConfig {
    let ws_url = std::env::var("JANUS_HOST").unwrap_or_else(|_| "ws://localhost:8188".to_owned());

    WebSocketConfig::new(ws_url)
}

#[test(tokio::test)]
async fn echo_external_channel() {
    let config = create_rmq_config().await;

    let id = ClientId(Arc::from("janus-test-echo"));

    let (sink, _recv) = mpsc::channel(48);
    let client = Client::new(config, id, sink).await.unwrap();
    let mut session = client.create_session().await.unwrap();
    let echo_handle = session
        .attach_to_plugin(JanusPlugin::Echotest)
        .await
        .unwrap();

    let echo = echo_handle
        .send(outgoing::EchoPluginUnnamed {
            audio: Some(true),
            ..Default::default()
        })
        .await
        .unwrap();
    match echo.0 {
        incoming::EchoPluginDataEvent::Ok { result } => {
            assert_eq!(result, "ok")
        }
        incoming::EchoPluginDataEvent::Error(_) => panic!(),
    }

    echo_handle.detach(false).await.unwrap();
    session.destroy(false).await.unwrap();
}

#[test(tokio::test)]
async fn create_and_list_rooms() {
    let config = create_rmq_config().await;
    let id = ClientId(Arc::from("janus-test-list"));

    let (sink, _recv) = mpsc::channel(48);
    let client = Client::new(config, id, sink).await.unwrap();
    let mut session = client.create_session().await.unwrap();
    let handle = session
        .attach_to_plugin(JanusPlugin::VideoRoom)
        .await
        .unwrap();

    let room1 = handle
        .send(outgoing::VideoRoomPluginCreate {
            description: "Testroom1".to_owned(),
            ..Default::default()
        })
        .await
        .unwrap();

    let room1 = match room1.0 {
        incoming::VideoRoomPluginDataCreated::Ok { room, permanent } => {
            assert!(Into::<u64>::into(room) > 0);
            assert!(!permanent);
            room
        }
        _ => panic!(),
    };

    let room2 = handle
        .send(outgoing::VideoRoomPluginCreate {
            description: "Testroom2".to_owned(),
            ..Default::default()
        })
        .await
        .unwrap();

    let room2 = match room2.0 {
        incoming::VideoRoomPluginDataCreated::Ok { room, permanent } => {
            assert!(Into::<u64>::into(room) > 0);
            assert!(!permanent);
            room
        }
        _ => panic!(),
    };

    let rooms = handle
        .send(outgoing::VideoRoomPluginListRooms)
        .await
        .unwrap();

    match rooms.0 {
        incoming::VideoRoomPluginDataSuccess::List { list } => {
            // We should see at least our two test rooms here
            assert!(list.len() >= 2);
            assert!(list.iter().any(|s| *s.description() == "Testroom1"));
            assert!(list.iter().any(|s| *s.description() == "Testroom2"));
        }
    }

    handle
        .send(outgoing::VideoRoomPluginDestroy {
            room: room1,
            secret: None,
            permanent: None,
            token: None,
        })
        .await
        .unwrap();

    handle
        .send(outgoing::VideoRoomPluginDestroy {
            room: room2,
            secret: None,
            permanent: None,
            token: None,
        })
        .await
        .unwrap();

    handle.detach(false).await.unwrap();
    session.destroy(false).await.unwrap();
}

#[test(tokio::test)]
async fn send_offer() {
    let config = create_rmq_config().await;

    let id = ClientId(Arc::from("janus-test-offer"));

    let (sink, _recv) = mpsc::channel(48);
    let client = Client::new(config, id, sink).await.unwrap();
    let mut session = client.create_session().await.unwrap();
    let publisher_handle = session
        .attach_to_plugin(JanusPlugin::VideoRoom)
        .await
        .unwrap();

    let room1 = publisher_handle
        .send(outgoing::VideoRoomPluginCreate {
            description: "SendOfferTestroom1".to_owned(),
            ..Default::default()
        })
        .await
        .unwrap();

    let room_id = match room1.0 {
        incoming::VideoRoomPluginDataCreated::Ok { room, permanent } => {
            assert!(Into::<u64>::into(room) > 0);
            assert!(!permanent);
            room
        }
        _ => panic!(),
    };

    let rooms = publisher_handle
        .send(outgoing::VideoRoomPluginListRooms)
        .await
        .unwrap();
    match rooms.0 {
        incoming::VideoRoomPluginDataSuccess::List { list } => {
            assert!(list
                .iter()
                .any(|s| *s.description() == "SendOfferTestroom1"));
        }
    }
    publisher_handle
        .send(outgoing::VideoRoomPluginJoinPublisher {
            room: room_id,
            id: Some(1),
            display: None,
            token: None,
        })
        .await
        .unwrap();

    publisher_handle
        .send(outgoing::VideoRoomPluginDestroy {
            room: room_id,
            secret: None,
            permanent: None,
            token: None,
        })
        .await
        .unwrap();

    publisher_handle.detach(false).await.unwrap();
    session.destroy(false).await.unwrap();
}

#[test(tokio::test)]
async fn send_offer_websocket() {
    let config = create_ws_config();

    let id = ClientId(Arc::from("janus-test-offer"));

    let (sink, _recv) = mpsc::channel(48);
    let client = Client::new(config, id, sink).await.unwrap();
    let mut session = client.create_session().await.unwrap();
    let publisher_handle = session
        .attach_to_plugin(JanusPlugin::VideoRoom)
        .await
        .unwrap();

    let room1 = publisher_handle
        .send(outgoing::VideoRoomPluginCreate {
            description: "SendOfferTestroom1".to_owned(),
            ..Default::default()
        })
        .await
        .unwrap();

    let room_id = match room1.0 {
        incoming::VideoRoomPluginDataCreated::Ok { room, permanent } => {
            assert!(Into::<u64>::into(room) > 0);
            assert!(!permanent);
            room
        }
        _ => panic!(),
    };

    let rooms = publisher_handle
        .send(outgoing::VideoRoomPluginListRooms)
        .await
        .unwrap();
    match rooms.0 {
        incoming::VideoRoomPluginDataSuccess::List { list } => {
            assert!(list
                .iter()
                .any(|s| *s.description() == "SendOfferTestroom1"));
        }
    }
    publisher_handle
        .send(outgoing::VideoRoomPluginJoinPublisher {
            room: room_id,
            id: Some(1),
            display: None,
            token: None,
        })
        .await
        .unwrap();

    publisher_handle
        .send(outgoing::VideoRoomPluginDestroy {
            room: room_id,
            secret: None,
            permanent: None,
            token: None,
        })
        .await
        .unwrap();

    publisher_handle.detach(false).await.unwrap();
    session.destroy(false).await.unwrap();
}
