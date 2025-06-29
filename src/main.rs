use rocket::{get, post, routes, launch, State, Shutdown};
use rocket::fs::{relative, FileServer};
use rocket::form::{Form, FromForm};
use rocket::response::stream::{EventStream, Event};
use rocket::serde::{Serialize, Deserialize};
use rocket::tokio::sync::broadcast::{channel, Sender, error::RecvError};
use rocket::tokio::select;
use std::collections::HashMap;
use rocket::tokio::sync::Mutex;
use rocket::http::Status;
use std::sync::Arc;
use rocket::delete;
use chrono::Utc;

#[derive(Debug, Clone, FromForm, Serialize, Deserialize)]
#[serde(crate = "rocket::serde")]

struct Message {
    #[field(validate = len(..30))]
    pub room: String,
    #[field(validate = len(..20))]
    pub username: String,
    pub message: String,
    pub timestamp: Option<String>,
}

// returns an infinite stream of server-sent events. each event is a message
// pulled from a briadcast queue sent by the 'post' handler
#[get("/events")]
async fn events(queue: &State<Sender<Message>>, mut end: Shutdown) -> EventStream![] {
    let mut rx = queue.subscribe();
    EventStream! {
        loop {
            let msg = select! {
                msg = rx.recv() => match msg {
                    Ok(msg) => msg,
                    Err(RecvError::Closed) => break,
                    Err(RecvError::Lagged(_)) => continue,
                },
                _ = &mut end => break,
            };
            yield Event::json(&msg);
        }
    }
}

type RoomList = Arc<Mutex<HashMap<String, Vec<Message>>>>;

#[delete("/room/<room>")]
async fn delete_room(room: String, state: &State<RoomList>) -> Status {
    let mut rooms = state.lock().await;
    if rooms.remove(&room).is_some() {
        Status::Ok
    } else {
        Status::NotFound
    }
}

// receive a msg from a form submission and broadcast it to any receivers
#[post("/message", data = "<form>")]
async fn post(
    form: Form<Message>,
    queue: &State<Sender<Message>>,
    room_list: &State<RoomList>
) 
{
    let mut msg = form.into_inner();
    msg.timestamp = Some(Utc::now().to_rfc3339());
   
    let mut rooms = room_list.lock().await;
    rooms.entry(msg.room.clone()).or_default().push(msg.clone());

    let _res = queue.send(msg);
}

#[launch]
fn rocket() -> _ {
    rocket::build()
        .manage(channel::<Message>(1024).0)
        .manage(Arc::new(Mutex::new(HashMap::<String, Vec<Message>>::new())))
        .mount("/", routes![post, events, delete_room])
        .mount("/", FileServer::from(relative!("static")))
} 