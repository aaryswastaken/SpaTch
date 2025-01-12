#[macro_use] extern crate rocket;
use rocket::futures::{SinkExt, StreamExt};

use rocket_db_pools::{Connection, Database};
use rocket_db_pools::deadpool_redis::{self, redis::AsyncCommands};

use ws::stream::DuplexStream;
use ws::Message;

use std::sync::Arc;
use futures::stream::SplitSink;
use futures::lock::Mutex;


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
    // Get the last message, corresponding to a -1 index
    // NOTE: We use RPUSH instead of LPUSH in order to keep the messages in order, thus making us
    //          use -1 instead of 1 for the last index

    conn.lindex("messages", -1).await.unwrap_or_else(|_| "An error occured".to_string())
}

#[get("/message/<id>")]
async fn get_message(id: isize, mut conn: Connection<RedisPool>) -> String {
    // Get and return the message to the corresponding index
    // TODO: Should probably create different cases following what the error is

    conn.lindex("messages", id).await.unwrap_or_else(|_| "An error occured".to_string())
}

#[post("/new_message/<message>")]
async fn push_new_message(message: &str, mut conn: Connection<RedisPool>,
                          channels_state: &rocket::State<Channels>) -> String {
    // Create a new record and format the result
    let new_record = match conn.rpush::<&str, Vec<&str>, i32>(
        "messages", vec![message]).await {
        Ok(id) => Ok((id - 1).to_string()),
        Err(_) => Err("An error occurred".to_string())
    };

    match new_record {
        Ok(id) => {
            // Broadcast new message to all channels
            let ws_msg = Message::text(format!("N{}", id));  // format a new message
            let channels_lock = channels_state.channels.try_lock().unwrap();

            for channel in channels_lock.iter() {
                let channel_clone = channel.clone();
                let mut channel_clone_lock = channel_clone.lock().await;

                // TODO: Should probably instantiate a new thread to make sure it wont block when
                //          there will be big amounts of channel
                // I won't do it now tho because this function already costed my sanity
                let _ = channel_clone_lock.send(ws_msg.clone()).await.unwrap();
            }

            drop(channels_lock);  // Drop the lock sooner

            // Return the new message ID
            id.to_string()
        }
        Err(err) => err  // Returns the error
    }
}


// Creates a new ws channel to dispatch our new messages to
#[get("/notify?messages")]
async fn echo_channel<'a>(ws: ws::WebSocket,
                          channels_state: &'a rocket::State<Channels>) -> ws::Channel<'a> {
    ws.channel(move |stream| Box::pin(async move {
        let (sender, _) = stream.split();  // Splitting the stream
        let arc_sender = Arc::new(Mutex::new(sender)); // Creating a new ref

        // Get a lock on the channel vector and push
        let mut chan = channels_state.channels.try_lock().unwrap();
        chan.push(arc_sender.clone());
        drop(chan);  // drops the MutexGuard to free some allocation time

        Ok(())
    }))
}

#[launch]
fn rocket() -> _ {
    // Initialise our ws channel handler
    let channels = Channels { channels: Arc::new(Mutex::new(vec![])) };

    // Initialise our web server
    rocket::build()
        .attach(RedisPool::init())  // With the Redis connection
        .manage(channels)           // And a handle on our ws channel handler
        .mount("/", routes![index, get_last_message, get_message, push_new_message, echo_channel])
                                    // and define our routes
}