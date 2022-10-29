use futures_util::StreamExt;
use std::{collections::HashMap, env, net::SocketAddr, sync::Arc};
use tokio::sync::{mpsc, RwLock};
use warp::{ws::WebSocket, Filter};

type WsMsg = Result<warp::ws::Message, warp::Error>;
type UsersMap = HashMap<usize, mpsc::UnboundedSender<WsMsg>>;
type Users = Arc<RwLock<UsersMap>>;

static NEXT_USERID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

async fn connect(socket: WebSocket, users: Users) {
    let id = NEXT_USERID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    println!("Welcome User with ID {}", id);

    let (user_tx, mut user_rx) = socket.split();
    let (tx, rx) = mpsc::unbounded_channel();

    let rx = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);

    tokio::spawn(rx.forward(user_tx));

    users.write().await.insert(id, tx);

    while let Some(result) = user_rx.next().await {
        broadcast_msg(result.expect("Failed to featch message"), &users).await;
    }

    disconnect(id, &users).await;
}

async fn broadcast_msg(msg: warp::ws::Message, users: &Users) {
    if msg.to_str().is_err() {
        return;
    };

    for (&uid, tx) in users.read().await.iter() {
        tx.send(Ok(msg.clone()))
            .unwrap_or_else(|_| panic!("Failed to send message {}", uid));
    }
}

async fn disconnect(id: usize, users: &Users) {
    println!("Good bye user with ID {}", id);
    users.write().await.remove(&id);
}

#[tokio::main]
async fn main() {
    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| String::from("127.0.0.1:8089"));
    let socket_addr: SocketAddr = addr.parse().expect("Invalid socket address.");

    let users = Users::default();
    let users = warp::any().map(move || users.clone());

    let optional_name = warp::path::param::<String>()
        .map(Some)
        .or_else(|_| async { Ok::<(Option<String>,), std::convert::Infallible>((None,)) });

    let hello = warp::get()
        .and(warp::path("hello"))
        .and(optional_name)
        .and(warp::path::end())
        .map(|name: Option<String>| {
            format!(
                "Hello, {}!",
                name.unwrap_or_else(|| String::from("dear guest"))
            )
        });

    let chat = warp::path("chat")
        .and(warp::ws())
        .and(users)
        .map(|ws: warp::ws::Ws, users| ws.on_upgrade(move |socket| connect(socket, users)));

    let files = warp::fs::dir("./static");

    let res_404 = warp::any().map(|| {
        warp::http::Response::builder()
            .status(warp::http::StatusCode::NOT_FOUND)
            .body(
                std::fs::read_to_string("./static/404.html")
                    .expect("Error opening or reading 404 template"),
            )
    });

    let routes = chat.or(hello).or(files).or(res_404);

    let server = warp::serve(routes).try_bind(socket_addr);
    println!("Running server at {}!", addr);

    server.await;
}
