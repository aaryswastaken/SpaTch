#[macro_use] extern crate rocket;

use std::panic::resume_unwind;
use rocket::futures::{SinkExt, StreamExt};
use futures::future::join_all;
use futures::lock::Mutex;

use rocket_db_pools::{Connection, Database, deadpool_redis};
use rocket_db_pools::deadpool_redis::redis::AsyncCommands;
use ws::stream::DuplexStream;

use std::sync::Arc;
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

    let new_record = match conn.rpush::<&str, Vec<&str>, i32>("messages", vec![message]).await {
        Ok(id) => (id - 1).to_string(),
        Err(_) => "An error occurred".to_string()
    };

    match new_record {
        Ok(id) => {
            // Broadcast new message to all channels
            let ws_msg = Message::text(format!("N{}", id));
            let channels_lock = channels_state.channels.try_lock().unwrap();

            for channel in channels_lock.iter() {
                let mut channel_clone = channel.clone();
                let mut channel_clone_lock = channel_clone.lock().await;

                let _ = channel_clone_lock.send(ws_msg.clone()).await.unwrap();
            }

            id.to_string()
        }
        Err(err) => err
    }
}


#[get("/echo?channel")]
async fn echo_channel<'a>(ws: ws::WebSocket, channels_state: &'a rocket::State<Channels>) -> ws::Channel<'a> {
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