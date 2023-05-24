use apollo_compiler::values::OperationType;
use async_graphql::{EmptyMutation, EmptySubscription, Schema};
use async_graphql::{Object, Result, ID};
use biscuit_auth as biscuit;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::convert::Infallible;
use std::error::Error;
use std::net::SocketAddr;

type BoxError = Box<dyn Error + Send + Sync>;

async fn handle(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let root = biscuit::PublicKey::from_bytes_hex(&std::env::var("ROOT_KEY").unwrap()).unwrap();

    let schema = Schema::build(Query, EmptyMutation, EmptySubscription)
        .enable_federation()
        .finish();
    let opt_token = extract_token(&req, &root).unwrap();

    let (_, body) = req.into_parts();
    let body = hyper::body::to_bytes(body).await.unwrap();

    println!("got req: {}", std::str::from_utf8(&*body).unwrap());
    let request: async_graphql::Request = serde_json::from_slice(&*body).unwrap();

    let opt_user_id = match validate_request(
        &request,
        opt_token,
        r#"
        subgraph("orga");

        allow if user($id), query("_entity");
        allow if query("allOrganizations"), orga_service_admin(true) trusting ed25519/b8a73872297bb052b3a8c9b64a23b127cdfc64ba30d9634c10de8644ee6be13f;

        allow if query("_service");
        deny if true;"#,
    ) {
        Ok(opt) => opt,
        Err(e) => {
            println!("error: {:?}", e);
            return Ok(Response::new(Body::from(
                serde_json::to_string(&async_graphql::Response::from_errors(vec![
                    async_graphql::ServerError::new(e.to_string(), None),
                ]))
                .unwrap(),
            )));
        }
    };

    let request = match opt_user_id {
        None => request,
        Some(id) => request.data(id),
    };

    let response = schema.execute(request).await;
    let data = serde_json::to_string(&response).unwrap();
    println!("returning {data}");

    Ok(Response::new(Body::from(data)))
}

fn validate_request(
    request: &async_graphql::Request,
    token: Option<biscuit::Biscuit>,
    authorizer_code: &str,
) -> Result<Option<UserId>, BoxError> {
    /*** Parse the query to observe the requested operation ***/
    let compiler = apollo_compiler::ApolloCompiler::new(&request.query);

    let ops = compiler.operations();
    let operation = match request.operation_name.as_ref() {
        None => ops.get(0),
        Some(name) => ops.iter().find(|op| op.name() == Some(name)),
    };

    let operation = match operation {
        None => {
            return Err(Box::<dyn Error + Send + Sync>::from(
                "cannot find operation",
            ))
        }
        Some(op) => op,
    };

    /*** Create the authorizer
     *
     * A fact will be added for each root operation, that can then be checked by the token
     *  ***/
    let mut authorizer = biscuit::Authorizer::new();
    authorizer.add_code(authorizer_code)?;
    authorizer.set_time();

    let operation_type = operation.operation_ty();
    for root_op in operation.fields(&compiler.db).iter() {
        match operation_type {
            OperationType::Query => {
                authorizer.add_fact(format!("query(\"{}\")", root_op.name()).as_str())?
            }
            OperationType::Mutation => {
                authorizer.add_fact(format!("mutation(\"{}\")", root_op.name()).as_str())?
            }
            // not supported by the router
            OperationType::Subscription => {}
        }
    }

    /*** Get the token from the request
     *
     * If there's no Authorization header, we can still apply the authorizer policies on an unauthenticated request
     * ***/

    if let Some(token) = token.as_ref() {
        authorizer.add_token(token)?;
    }

    let res = authorizer.authorize();
    println!("authorizer result {:?}:\n{}", res, authorizer.print_world());
    res?;

    let res: Vec<(i64,)> = authorizer.query("query($id) <- user($id)")?;
    Ok(res.get(0).map(|(id,)| UserId(ID(id.to_string()))))
}
static USERS: Lazy<HashMap<&str, User>> = Lazy::new(|| {
    println!("initializing");
    let mut m = HashMap::new();
    m.insert(
        "1",
        User {
            id: ID("1".to_string()),
            organizations: vec![ID("1".to_string()), ID("2".to_string())],
        },
    );
    m.insert(
        "2",
        User {
            id: ID("2".to_string()),
            organizations: vec![ID("1".to_string()), ID("2".to_string())],
        },
    );
    m
});

static ORGAS: Lazy<HashMap<&str, Organization>> = Lazy::new(|| {
    println!("initializing");
    let mut m = HashMap::new();
    m.insert(
        "1",
        Organization {
            id: ID("1".to_string()),
            members: vec![ID("1".to_string()), ID("2".to_string())],
        },
    );
    m.insert(
        "2",
        Organization {
            id: ID("2".to_string()),
            members: vec![ID("1".to_string()), ID("2".to_string())],
        },
    );
    m
});

struct UserId(ID);

struct Query;

#[Object]
impl Query {
    #[graphql(entity)]
    async fn find_orga_by_id(&self, id: ID) -> Result<&Organization> {
        ORGAS
            .get(id.0.as_str())
            .ok_or(async_graphql::Error::new("organization not found"))
    }

    #[graphql(entity)]
    async fn find_user_by_id(&self, id: ID) -> Result<&User> {
        USERS
            .get(id.0.as_str())
            .ok_or(async_graphql::Error::new("user not found"))
    }

    async fn all_organizations<'ctx>(&self) -> Option<Vec<&Organization>> {
        Some(ORGAS.values().collect())
    }
}

struct User {
    id: ID,
    organizations: Vec<ID>,
}

#[Object(extends)]
impl User {
    #[graphql(shareable)]
    async fn id(&self) -> &ID {
        &self.id
    }

    async fn organizations(&self) -> Vec<Option<&Organization>> {
        self.organizations
            .iter()
            .map(|id| ORGAS.get(id.0.as_str()))
            .collect()
    }
}

struct Organization {
    id: ID,
    members: Vec<ID>,
}

#[Object]
impl Organization {
    async fn id(&self) -> &ID {
        &self.id
    }

    async fn members(&self) -> Vec<Option<&User>> {
        self.members
            .iter()
            .map(|id| USERS.get(id.0.as_str()))
            .collect()
    }
}

fn extract_token_string(request: &Request<Body>) -> Result<Option<&str>, BoxError> {
    Ok(match request.headers().get("Authorization") {
        None => None,
        Some(value) => {
            let value = value.to_str()?;
            if !value.starts_with("Bearer ") {
                return Err(Box::<dyn Error + Send + Sync>::from("not a bearer token"));
            }
            Some(&value[7..])
        }
    })
}

fn extract_token(
    request: &Request<Body>,
    root: &biscuit::PublicKey,
) -> Result<Option<biscuit::Biscuit>, BoxError> {
    let opt_token_str = extract_token_string(request)?;

    println!("parsing token from: {:?}", opt_token_str);
    Ok(match opt_token_str {
        None => None,
        Some(s) => Some(biscuit::Biscuit::from_base64(s, root)?),
    })
}

#[tokio::main]
async fn main() {
    let addr = SocketAddr::from(([127, 0, 0, 1], 4002));

    let make_service = make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(handle)) });
    let server = Server::bind(&addr).serve(make_service);

    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}
