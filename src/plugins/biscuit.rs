use apollo_compiler::values::OperationType;
use apollo_router::graphql;
use apollo_router::layers::ServiceBuilderExt;
use apollo_router::plugin::Plugin;
use apollo_router::plugin::PluginInit;
use apollo_router::register_plugin;
use apollo_router::services::subgraph;
use apollo_router::services::supergraph;
use biscuit::macros::authorizer;
use biscuit::macros::block;
use biscuit_auth as biscuit;
use schemars::JsonSchema;
use serde::Deserialize;
use tower::BoxError;
use tower::ServiceBuilder;
use tower::ServiceExt;

use std::error::Error;
use std::ops::ControlFlow;

#[derive(Debug, Clone)]
struct Biscuit {
    root: biscuit::PublicKey,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
struct Conf {
    // Put your plugin configuration here. It will automatically be deserialized from JSON.
    // Always put some sort of config here, even if it is just a bool to say that the plugin is enabled,
    // otherwise the yaml to enable the plugin will be confusing.
    message: String,

    public_root: String,
}
// This plugin is a skeleton for doing authentication that requires a remote call.
#[async_trait::async_trait]
impl Plugin for Biscuit {
    type Config = Conf;

    async fn new(init: PluginInit<Self::Config>) -> Result<Self, BoxError> {
        tracing::info!("{}", init.config.message);

        let root = biscuit::PublicKey::from_bytes_hex(&init.config.public_root)?;
        Ok(Biscuit { root })
    }

    fn supergraph_service(&self, service: supergraph::BoxService) -> supergraph::BoxService {
        let this = self.clone();
        ServiceBuilder::new()
            .checkpoint(move |mut request: supergraph::Request| {
                match this.validate_request(&mut request) {
                    Ok(()) => Ok(ControlFlow::Continue(request)),
                    Err(e) => Ok(ControlFlow::Break(
                        supergraph::Response::error_builder()
                            .error(graphql::Error::builder().message(e.to_string()).build())
                            .status_code(http::StatusCode::UNAUTHORIZED)
                            .context(request.context)
                            .build()?,
                    )),
                }
            })
            .service(service)
            .boxed()
    }

    fn subgraph_service(
        &self,
        service_name: &str,
        service: subgraph::BoxService,
    ) -> subgraph::BoxService {
        let this = self.clone();
        let service_name = service_name.to_string();

        ServiceBuilder::new()
            .checkpoint(move |mut request: subgraph::Request| {
                match this.attenuate(&service_name, &mut request) {
                    Ok(()) => Ok(ControlFlow::Continue(request)),
                    Err(e) => Ok(ControlFlow::Break(
                        subgraph::Response::error_builder()
                            .error(graphql::Error::builder().message(e.to_string()).build())
                            .status_code(http::StatusCode::UNAUTHORIZED)
                            .context(request.context)
                            .build()?,
                    )),
                }
            })
            .service(service)
            .boxed()
    }
}

impl Biscuit {
    fn validate_request(&self, request: &mut supergraph::Request) -> Result<(), BoxError> {
        let compiler = apollo_compiler::ApolloCompiler::new(
            &request
                .supergraph_request
                .body()
                .query
                .as_deref()
                .expect("there should be a query by now"),
        );

        let ops = compiler.operations();

        let operation = match request.supergraph_request.body().operation_name.as_ref() {
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

        let mut authorizer = authorizer!(
            r#"
            allow if true
 "#,
        );

        let operation_type = operation.operation_ty();
        for root_op in operation.fields(&compiler.db).iter() {
            match operation_type {
                OperationType::Query => {
                    authorizer.add_fact(format!("query(\"{}\")", root_op.name()).as_str())?
                }
                OperationType::Mutation => {
                    authorizer.add_fact(format!("mutation(\"{}\")", root_op.name()).as_str())?
                }
                OperationType::Subscription => {
                    authorizer.add_fact(format!("subscription(\"{}\")", root_op.name()).as_str())?
                }
            }
        }

        let opt_token = extract_token(&request.supergraph_request, &self.root)?;

        if let Some(token) = opt_token.as_ref() {
            authorizer.add_token(token)?;
        }

        let res = authorizer.authorize();
        println!("authorizer:\n{}", authorizer.print_world());
        res?;

        if let Some(token) = extract_token_string(&request.supergraph_request)? {
            request.context.insert("biscuit", token.to_string())?;
        }

        Ok(())
    }

    fn attenuate(
        &self,
        service_name: &str,
        request: &mut subgraph::Request,
    ) -> Result<(), BoxError> {
        if let Ok(Some(s)) = request.context.get::<_, String>("biscuit") {
            let token = biscuit::UnverifiedBiscuit::from_base64(s)?;

            let attenuated_token = token.append(block!(
                "check if subgraph({subgraph});",
                subgraph = service_name,
            ))?;

            request.subgraph_request.headers_mut().insert(
                "Authorization",
                format!("Bearer {}", attenuated_token.to_base64()?).parse()?,
            );
        }

        Ok(())
    }
}

fn extract_token_string(
    request: &http::Request<graphql::Request>,
) -> Result<Option<&str>, BoxError> {
    Ok(match request.headers().get("Authorization") {
        None => None,
        Some(value) => {
            let value = value.to_str()?;
            println!("Authorization: {}", value);
            if !value.starts_with("Bearer ") {
                return Err(Box::<dyn Error + Send + Sync>::from("not a bearer token"));
            }
            Some(&value[7..])
        }
    })
}

fn extract_token(
    request: &http::Request<graphql::Request>,
    root: &biscuit::PublicKey,
) -> Result<Option<biscuit::Biscuit>, BoxError> {
    let opt_token_str = extract_token_string(request)?;

    println!("parsing token from: {:?}", opt_token_str);
    Ok(match opt_token_str {
        None => None,
        Some(s) => Some(biscuit::Biscuit::from_base64(s, root)?),
    })
}

// This macro allows us to use it in our plugin registry!
// register_plugin takes a group name, and a plugin name.
register_plugin!("biscuit_1", "biscuit", Biscuit);

#[cfg(test)]
mod tests {
    use apollo_router::graphql;
    use apollo_router::plugin::test::MockSubgraph;
    use apollo_router::services::subgraph;
    use apollo_router::services::supergraph;
    use apollo_router::MockedSubgraphs;
    use apollo_router::TestHarness;
    use biscuit::macros::authorizer;
    use biscuit::macros::biscuit;
    use biscuit::macros::block;
    use biscuit_auth as biscuit;
    use tower::BoxError;
    use tower::ServiceExt;

    use crate::plugins::biscuit::extract_token;

    const SCHEMA: &'static str = r#"schema
    @core(feature: "https://specs.apollo.dev/core/v0.1")
    @core(feature: "https://specs.apollo.dev/join/v0.1")
    @core(feature: "https://specs.apollo.dev/inaccessible/v0.1")
     {
    query: Query
}
directive @core(feature: String!) repeatable on SCHEMA
directive @join__field(graph: join__Graph, requires: join__FieldSet, provides: join__FieldSet) on FIELD_DEFINITION
directive @join__type(graph: join__Graph!, key: join__FieldSet) repeatable on OBJECT | INTERFACE
directive @join__owner(graph: join__Graph!) on OBJECT | INTERFACE
directive @join__graph(name: String!, url: String!) on ENUM_VALUE
directive @inaccessible on OBJECT | FIELD_DEFINITION | INTERFACE | UNION
scalar join__FieldSet

enum join__Graph {
   USER @join__graph(name: "user", url: "http://localhost:4001/graphql")
   ORGA @join__graph(name: "organization", url: "http://localhost:4002/graphql")
}

type Query {
   me: User @join__field(graph: USER)
   otherUser(id: ID!): User @join__field(graph: USER)
   test: String @join__field(graph: USER)
}

type User
@join__owner(graph: USER)
@join__type(graph: ORGA, key: "id")
@join__type(graph: USER, key: "id") {
   id: ID!
   name: String
   private_data: String
   activeOrganization: Organization
}

type Organization
@join__owner(graph: ORGA)
@join__type(graph: ORGA, key: "id")
@join__type(graph: USER, key: "id") {
   id: ID
   creatorUser: User
}"#;

    #[tokio::test]
    async fn basic_test() -> Result<(), BoxError> {
        let root_keypair = biscuit::KeyPair::new();

        let mut subgraphs = MockedSubgraphs::default();
        subgraphs.insert
            ("user", MockSubgraph::builder().with_json(
                    serde_json::json!{{"query":"{currentUser{activeOrganization{__typename id}}}"}},
                    serde_json::json!{{"data": {"currentUser": { "activeOrganization": null }}}}
                ).build());
        subgraphs.insert("orga", MockSubgraph::default());
        let test_harness = TestHarness::builder()
            .configuration_json(serde_json::json!({
                "include_subgraph_errors": {
                    "all": true
                },
                "plugins": {
                    "biscuit_1.biscuit": {
                        "message" : "Starting my plugin",
                        "public_root": root_keypair.public().to_bytes_hex(),
                    }
                }
            }))
            .unwrap()
            .schema(SCHEMA)
            .extra_plugin(subgraphs)
            .build()
            .await
            .unwrap();

        let token = biscuit!(r#"check if query($query), ["me", "test"].contains($query);"#)
            .build(&root_keypair)
            .unwrap();
        let token = token.append(block!(r#"check if query("me")"#)).unwrap();

        let request = supergraph::Request::fake_builder()
            .header("Authorization", format!("Bearer {}", token.to_base64()?))
            .query("query { me { activeOrganization { id creatorUser { name } } } }")
            .build()
            .unwrap();
        let mut streamed_response = test_harness.oneshot(request).await?;

        let first_response = streamed_response
            .next_response()
            .await
            .expect("couldn't get primary response");

        println!("first response: {:?}", first_response);
        assert!(first_response.data.is_some());

        let next = streamed_response.next_response().await;
        println!("next response: {:?}", next);

        // You could keep calling .next_response() until it yields None if you're expexting more parts.
        assert!(next.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn failing_test() -> Result<(), BoxError> {
        let root_keypair = biscuit::KeyPair::new();

        let mut subgraphs = MockedSubgraphs::default();
        subgraphs.insert
            ("user", MockSubgraph::builder().with_json(
                    serde_json::json!{{"query":"{currentUser{activeOrganization{__typename id}}}"}},
                    serde_json::json!{{"data": {"currentUser": { "activeOrganization": null }}}}
                ).build());
        subgraphs.insert("organization", MockSubgraph::default());
        let test_harness = TestHarness::builder()
            .configuration_json(serde_json::json!({
                "include_subgraph_errors": {
                    "all": true
                },
                "plugins": {
                    "biscuit_1.biscuit": {
                        "message" : "Starting my plugin",
                        "public_root": root_keypair.public().to_bytes_hex(),
                    }
                }
            }))
            .unwrap()
            .schema(SCHEMA)
            .extra_plugin(subgraphs)
            .build()
            .await
            .unwrap();

        let token = biscuit!(r#"check if query($query), ["me", "test"].contains($query);"#)
            .build(&root_keypair)
            .unwrap();
        let token = token.append(block!(r#"check if query("me")"#)).unwrap();

        let request = supergraph::Request::fake_builder()
            .header("Authorization", format!("Bearer {}", token.to_base64()?))
            .query("query { otherUser(id: 1) { activeOrganization { id creatorUser { name } } } }")
            .build()
            .unwrap();
        let mut streamed_response = test_harness.oneshot(request).await?;

        let first_response = streamed_response
            .next_response()
            .await
            .expect("couldn't get primary response");

        println!("first response: {:?}", first_response);
        assert_eq!(
            first_response.errors.get(0).unwrap().message,
            "authorization failed"
        );

        Ok(())
    }

    fn validate(
        root: biscuit::PublicKey,
        service_name: &str,
        request: &subgraph::Request,
    ) -> Result<(), BoxError> {
        let mut authorizer = authorizer!(
            r#"
        subgraph({subgraph});
        allow if true
"#,
            subgraph = service_name
        );

        let opt_token = extract_token(&request.subgraph_request, &root)?;

        if let Some(token) = opt_token.as_ref() {
            authorizer.add_token(token)?;
        }

        let res = authorizer.authorize();
        println!("authorizer:\n{}", authorizer.print_world());
        res?;
        Ok(())
    }

    #[tokio::test]
    async fn attenuation() -> Result<(), BoxError> {
        let root_keypair = biscuit::KeyPair::new();
        let root_public = root_keypair.public();

        //let subgraphs = plugin.subgraph_service(subgraph_name, service)
        let test_harness = TestHarness::builder()
            .configuration_json(serde_json::json!({
                "include_subgraph_errors": {
                    "all": true
                },
                "plugins": {
                    "biscuit_1.biscuit": {
                        "message" : "Starting my plugin",
                        "public_root": root_keypair.public().to_bytes_hex(),
                    }
                }
            }))
            .unwrap()
            .schema(SCHEMA)
            .subgraph_hook(move |service_name, _| {

                if service_name == "user" {
                    tower::service_fn(move |request: subgraph::Request| async move {
                        match validate(root_public, "usera", &request) {
                            Err(e) => Ok(
                                subgraph::Response::error_builder()
                                    .error(graphql::Error::builder().message(e.to_string()).build())
                                    .status_code(http::StatusCode::UNAUTHORIZED)
                                    .context(request.context)
                                    .build()?,
                            ),
                            Ok(()) => {
                                    if request.subgraph_request.body().query.as_deref() == Some("{currentUser{activeOrganization{__typename id}}}") {
                                        return Ok(subgraph::Response::fake_builder()
                                        .data(serde_json::json!{{"data": {"currentUser": { "activeOrganization": null }}}})
                                        .status_code(http::StatusCode::OK)
                                        .context(request.context)
                                        .build());
                                    }
                                panic!("unexpected")
                            }
                        }
                    }).boxed()
                } else if service_name == "organization" {
                    tower::service_fn(move |request: subgraph::Request| async move {
                        match validate(root_public, "organization", &request) {
                            Err(e) => Ok(
                                subgraph::Response::error_builder()
                                    .error(graphql::Error::builder().message(e.to_string()).build())
                                    .status_code(http::StatusCode::UNAUTHORIZED)
                                    .context(request.context)
                                    .build()?,
                            ),
                            Ok(()) => {
                                todo!()
                            }
                        }
                    }).boxed()
                } else {
                    panic!()
                }

            })
            .build()
            .await
            .unwrap();

        let token = biscuit!(r#"authorized_queries("me");"#)
            .build(&root_keypair)
            .unwrap();

        let request = supergraph::Request::fake_builder()
            .header("Authorization", format!("Bearer {}", token.to_base64()?))
            .query("query { me { activeOrganization { id creatorUser { name } } } }")
            .build()
            .unwrap();
        let mut streamed_response = test_harness.oneshot(request).await?;

        let first_response = streamed_response
            .next_response()
            .await
            .expect("couldn't get primary response");

        println!("first response: {:?}", first_response);
        assert_eq!(
            first_response.errors.get(0).unwrap().message,
            "authorization failed"
        );

        Ok(())
    }
}
