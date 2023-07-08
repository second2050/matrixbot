use std::{ops::Deref, collections::VecDeque};

use anyhow::Ok;
use matrix_sdk::{
    config::SyncSettings,
    room::Room,
    room::{MessagesOptions, Joined},
    ruma::{events::{room::{
        member::StrippedRoomMemberEvent,
        message::{MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent, RoomMessageEvent},
    }, TimelineEventType}, UserId, UInt},
    Client,
};
use tokio::time::{sleep, Duration};

const HOMESERVER_URL: &str = "https://matrix.example.tld";
const USERNAME: &str = "@bot:example.tld";
const PASSWORD: &str = "password";
const DEVICE_DISPLAY_NAME: &str = "Bot";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // setup stderr logging, configurable via the `RUST_LOG` env variable
    tracing_subscriber::fmt::init();

    // start our connection
    start_connection().await?;
    Ok(())
}

// main loop
async fn start_connection() -> anyhow::Result<()> {
    // setup our client connection
    let client = Client::builder()
        .homeserver_url(HOMESERVER_URL)
        .build()
        .await?;

    // authenticate to our server
    client.matrix_auth()
        .login_username(USERNAME, PASSWORD)
        .initial_device_display_name(DEVICE_DISPLAY_NAME)
        .await?;

    // output information about our account
    println!("Account Info:");
    println!("-> Display Name: {}", client.account().get_display_name().await.unwrap().unwrap());

    // react to invites that are sent to us, these sent us stripped member
    // state events so we should react to them specifically
    client.add_event_handler(on_stripped_state_member);

    // setup state by syncing so we dont respond to old messages, if the
    // `StateStore` finds a saved state in the location given the initial sync
    // will be skipped and the saved state will be used instead.
    let sync_token = client
        .sync_once(SyncSettings::default())
        .await
        .unwrap()
        .next_batch;

    // attach incoming message handler
    client.add_event_handler(on_room_message);
    
    // because we called `sync_once` before we entered our loop we need to pass
    // the sync token to `sync`
    let sync_settings = SyncSettings::default().token(sync_token);
    client.sync(sync_settings).await?; // this will loop until we kill this bot

    Ok(())
}

// event handler for invites
async fn on_stripped_state_member(
    room_member: StrippedRoomMemberEvent,
    client: Client,
    room: Room,
) {
    // ignore invites that are not for us
    if room_member.state_key != client.user_id().unwrap() {
        return;
    }

    // if room looks like an invite, join it
    if let Room::Invited(room) = room {
        // the event handlers are called before the sync happens, but methods
        // like joining or leaving wait for the sync to return the new state
        // so we need to a new task for them.
        tokio::spawn(async move {
            println!("Joining room {}", room.room_id());
            let mut delay = 2;

            while let Err(err) = room
                .accept_invitation()
                .await {
                    eprintln!("Error joining room: {} ({err:?}), Retrying in {delay} seconds",
                        room.room_id());
                    sleep(Duration::from_secs(delay)).await;
                    delay *= 2;

                    if delay > 3600 {
                        eprintln!("Failed to join room {} ({err:?})", room.room_id());
                        break;
                    }
                }
            println!("Joined room {}", room.room_id());
        });
    }
}

async fn on_room_message(
    event: OriginalSyncRoomMessageEvent,
    client: Client,
    room: Room
) {
    // act on messages that are only in rooms that we are joined to and unpack them
    let Room::Joined(room) = room else { return };
    let MessageType::Text(content) = event.content.msgtype else { return };

    // ignore messages that are from ourselves to not trigger recursion
    if event.sender == client.user_id().unwrap() {
        return;
    }

    let content_splitted: Vec<&str> = content.body.deref().split(" ").collect();

    match content_splitted.first().unwrap().deref() { // `.deref()` is needed because we are matching a `&&str` and not a `&str`
        "¡ping" => {
            println!("Responding to ¡ping in room {}", room.room_id());
            let content = RoomMessageEventContent::text_plain("おはようーー！");
            room.send(content, None).await.unwrap();
        },
        "uwu" => {
            println!("Responding to uwu in room {}", room.room_id());
            let content = RoomMessageEventContent::text_plain("owo");
            room.send(content, None).await.unwrap();
        },
        "!secret" => {
            println!("Responding to !secret in room {}", room.room_id());
            let content = RoomMessageEventContent::text_plain("https://www.youtube.com/watch?v=dQw4w9WgXcQ");
            room.send(content, None).await.unwrap();
        },
        "!help" => {
            println!("Responding to !help in room {}", room.room_id());
            let content = RoomMessageEventContent::text_plain("Commands: !secret, !help, !ping, uwu, !uwuify");
            room.send(content, None).await.unwrap();
        },
        "!uwuify" => {
            println!("Responding to !uwuify in room {}", room.room_id());
            let message = get_last_message(&room).await;
            if message != None {
                let content = RoomMessageEventContent::text_plain(uwuify_message(message.unwrap()));
                room.send(content, None).await.unwrap();
            }
        },
        "!set" => {
            if event.sender != "@second2050:fachschaften.org" {
                println!("Denying response to !set for {}", event.sender)
            } else {
                println!("Responding to !set in room {}", room.room_id());
                bot_set_helper(&room, client, content_splitted).await;
            }
        }
        // "!echolast" => {
        //     let last_message = get_last_message_from_sender(&room, &event.sender).await;
        //     println!("Responding to !echolast in room {} with '{}'", room.room_id(), last_message);
        //     println!("Current Room: {}", room.room_id());
        //     let content = RoomMessageEventContent::text_plain(last_message);
        //     room.send(content, None).await.unwrap();
        // },
        _ => {
            // special case for sed fix command
            if content.body.starts_with("s/") {
                println!("Responding to s/ in room {}", room.room_id());
                // get last message of user issuing the command
                let last_message = get_last_message_from_sender(&room, &event.sender).await;
                if last_message != None {
                    let last_message = last_message.unwrap();
                    println!("Fixing message: '{}' with '{}'", last_message, content.body);
                    let new_message = fix_message(last_message.to_string(), content.body);
                    let content = RoomMessageEventContent::text_plain(new_message);
                    room.send(content, None).await.unwrap();
                }
            }
        }
    }
}

// function to fix message with a sed like syntax, `s/old/new/`
fn fix_message(original_message: String, correction_message: String) -> String {
    let mut split_message = correction_message.split("/");
    split_message.next(); // skip first element, which is `s`
    let from = split_message.next().unwrap();
    let to = split_message.next().unwrap();
    let message = original_message.replace(from, to);
    println!("fix_message: from '{}' to '{}'", from, to);
    println!("fix_message: '{}' -> '{}'", original_message, message);
    return message
}

async fn get_last_message_from_sender(room: &Joined, user: &UserId) -> Option<String> {
    // get events from the latest to the oldest
    let mut messages_options = MessagesOptions::backward();
    messages_options.limit = UInt::new(20).unwrap(); // limit to 20 events
    let mut old_events = room.messages(messages_options).await.unwrap().chunk;

    // remove the first event, which is the message we are responding to, by
    // reversing the vector, popping the last element and reversing it again.
    old_events.reverse();
    old_events.pop();
    old_events.reverse();

    for old_event in old_events {
        // check if event is a message
        if old_event.event.deserialize().unwrap().event_type() != TimelineEventType::RoomMessage {
            continue;
        }
        // check if message was sent by the same user
        if old_event.event.deserialize().unwrap().sender() != user {
            continue;
        }
        
        // get message body
        let old_event_parsed: RoomMessageEvent = old_event.event.deserialize_as().unwrap();
        let old_event_body = old_event_parsed.as_original().unwrap().content.body();

        // return the message body
        println!("get_last_message_from_sender: Found message: '{}' from {} (id: {})", old_event_body, user, old_event.event.deserialize().unwrap().event_id());
        return Some(old_event_body.to_string());
    }
    // for now return an empty string when nothing was found
    println!("get_last_message_from_sender: No message found from {}", user);
    return None;
}

async fn get_last_message(room: &Joined) -> Option<String> {
    // get events from the latest to the oldest
    let mut messages_options = MessagesOptions::backward();
    messages_options.limit = UInt::new(20).unwrap(); // limit to 20 events
    let mut old_events = room.messages(messages_options).await.unwrap().chunk;

    // remove the first event, which is the message we are responding to, by
    // reversing the vector, popping the last element and reversing it again.
    old_events.reverse();
    old_events.pop();
    old_events.reverse();

    for old_event in old_events {
        // check if event is a message
        if old_event.event.deserialize().unwrap().event_type() != TimelineEventType::RoomMessage {
            continue;
        }
        
        // get message body
        let old_event_parsed: RoomMessageEvent = old_event.event.deserialize_as().unwrap();
        let old_event_body = old_event_parsed.as_original().unwrap().content.body();

        // return the message body
        println!("get_last_message: Found message: '{}' from {} (id: {})", old_event_body, old_event.event.deserialize().unwrap().sender(), old_event.event.deserialize().unwrap().event_id());
        return Some(old_event_body.to_string());
    }
    // for now return an empty string when nothing was found
    println!("get_last_message: No message found");
    return None;
}

fn uwuify_message(message: String) -> String {
    let mut message = message;
    message = message.replace("r", "w");
    message = message.replace("l", "w");
    message = message.replace("R", "W");
    message = message.replace("L", "W");
    message = message.replace("n", "ny");
    message = message.replace("N", "NY");
    message = message.replace("ove", "uv");
    message = message.replace("OVE", "UV");
    return message;
}

async fn bot_set_helper(room: &Joined, client: Client, command: Vec<&str>) {
    // convert to VecDeque and pop first element from command
    let mut command: VecDeque<&str> = command.into();
    command.pop_front();

    // match subcommand
    match command.pop_front().unwrap().deref() {
        "name" => {
            let mut new_name: String = "".to_string();
            for string in command {
                new_name += string;
                new_name += " ";
            }
            println!("set_name: setting display name -> {}", new_name);
            let result = client.account().set_display_name(Some(new_name.as_str())).await;
            if result.is_err() {
                println!("set_name: failed to set display name");
                let content = RoomMessageEventContent::text_plain("Failed to set display name!");
                room.send(content, None).await.unwrap();
            }
        }
        _ => {
            println!("set: unrecognized set command encountered");
        }
    }
}
