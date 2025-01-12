#[macro_use] extern crate rocket;

use std::panic::resume_unwind;
use rocket::futures::{SinkExt, StreamExt};
use futures::future::join_all;

use rocket_db_pools::{Connection, Database, deadpool_redis};
use rocket_db_pools::deadpool_redis::redis::AsyncCommands;
use ws::stream::DuplexStream;

use std::sync::{Mutex, Arc};
use futures::stream::SplitSink;
use ws::Message;

#[derive(Database)]
#[database("redis_pool")]
pub struct RedisPool(deadpool_redis::Pool);

struct Channels {
    channels: Arc<Mutex<Vec<Arc<Mutex<SplitSink<DuplexStream, Message>>>>>>
}



#[get("/")]
fn index() -> &'static str {
    "Hello, world!"
}

#[get("/last_message")]
async fn get_last_message(mut conn: Connection<RedisPool>) -> String {
    conn.lindex("messages", -1).await.unwrap_or_else(|_| "An error occured".to_string())
}

#[get("/message/<id>")]
async fn get_message(id: isize, mut conn: Connection<RedisPool>) -> String {
    conn.lindex("messages", id).await.unwrap_or_else(|_| "An error occured".to_string())
}

#[post("/new_message/<message>")]
async fn push_new_message(message: &str, mut conn: Connection<RedisPool>, channels_state: &rocket::State<Channels>) -> String {
    println!("New message: {}", message);

    let ws_msg = Message::text("olaaaa");
    let mut send_futures = Vec::new();

    // First, collect all channel references while holding the outer lock
    let channel_refs = {
        let channels = channels_state.channels.lock().unwrap();
        channels.iter().cloned().collect::<Vec<_>>()
    };

    // Create futures for each send operation
    for channel in channel_refs {
        let msg = ws_msg.clone();
        send_futures.push(async move {
            // Take a new lock for each send operation
            let message_result = {
                if let Ok(mut stream) = channel.lock() {
                    stream.send(msg)
                } else {
                    return println!("Failed to acquire lock for channel");
                }
            };  // Lock is dropped here

            // Now await the send operation without holding the lock
            if let Err(e) = message_result.await {
                println!("Failed to send message: {:?}", e);
            }
        });
    }

    // Execute all send operations concurrently
    join_all(send_futures).await;

    match conn.rpush::<&str, Vec<&str>, i32>("messages", vec![message]).await {
        Ok(id) => (id - 1).to_string(),
        Err(_) => "An error occurred".to_string()
    }
}


#[get("/echo?channel")]
async fn echo_channel<'a>(ws: ws::WebSocket, channels_state: &'a rocket::State<Channels>) -> ws::Channel<'a> {
    /*ws.channel(move |mut stream| Box::pin(async move {
        let boxed = Box::new(stream);

        let channels_inner = channels.inner();
        channels_inner.channels.lock().unwrap().push(boxed);


        /* while let Some(message) = stream.next().await {
            let _ = stream.send(message?).await;
        } */

        Ok(())
    }))*/

    /*ws.channel(move |stream| Box::pin(async move {
        let boxed = Box::new(stream);

        let channels_inner = channels.inner();
        channels_inner.channels.lock().unwrap().push(boxed);

        Ok(())
    }))*/

    ws.channel(move |stream| Box::pin(async move {
        let (sender, _) = stream.split();
        let arc_sender = Arc::new(Mutex::new(sender));

        let mut chan = channels_state.channels.try_lock().unwrap();
        chan.push(arc_sender.clone());

        Ok(())
    }))
}

#[launch]
fn rocket() -> _ {
    let channels = Channels { channels: Arc::new(Mutex::new(vec![])) };

    rocket::build()
        .attach(RedisPool::init())
        .manage(channels)
        .mount("/", routes![index, get_last_message, get_message, push_new_message, echo_channel])
}