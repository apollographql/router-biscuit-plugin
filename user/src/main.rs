use async_graphql::{http::GraphiQLSource, Data, EmptyMutation, EmptySubscription, Schema};
use async_graphql::{Context, Object, Result, SimpleObject, ID};
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;

struct Query;

use once_cell::sync::Lazy;

static USERS: Lazy<HashMap<&str, User>> = Lazy::new(|| {
    println!("initializing");
    let mut m = HashMap::new();
    m.insert(
        "1",
        User {
            id: ID("1".to_string()),
            name: "A".to_string(),
        },
    );
    m.insert(
        "2",
        User {
            id: ID("2".to_string()),
            name: "B".to_string(),
        },
    );
    m
});

struct UserId(ID);

#[Object]
impl Query {
    async fn me<'ctx>(&self, ctx: &Context<'ctx>) -> Result<&User> {
        let id = ctx.data::<UserId>()?;
        USERS
            .get(id.0 .0.as_str())
            .ok_or(async_graphql::Error::new("user not found"))
    }

    #[graphql(entity)]
    async fn find_user_by_id(&self, id: ID) -> Result<&User> {
        USERS
            .get(id.0.as_str())
            .ok_or(async_graphql::Error::new("user not found"))
    }
}

struct User {
    id: ID,
    name: String,
}

#[Object]
impl User {
    async fn id(&self) -> &ID {
        &self.id
    }

    async fn name(&self) -> &str {
        &self.name
    }
}

async fn handle(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let schema = Schema::build(Query, EmptyMutation, EmptySubscription)
        .enable_federation()
        .finish();

    let (parts, body) = req.into_parts();
    let body = hyper::body::to_bytes(body).await.unwrap();

    println!("got req: {}", std::str::from_utf8(&*body).unwrap());
    let request: async_graphql::Request = serde_json::from_slice(&*body).unwrap();

    let response = schema
        .execute(request.data(UserId(ID("1".to_string()))))
        .await;

    Ok(Response::new(Body::from(
        serde_json::to_string(&response).unwrap(),
    )))
}

#[tokio::main]
async fn main() {
    let addr = SocketAddr::from(([127, 0, 0, 1], 4001));

    let make_service = make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(handle)) });
    let server = Server::bind(&addr).serve(make_service);

    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}
