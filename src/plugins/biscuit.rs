use apollo_compiler::values::OperationType;
use apollo_router::layers::ServiceBuilderExt;
use apollo_router::plugin::Plugin;
use apollo_router::plugin::PluginInit;
use apollo_router::register_plugin;
use apollo_router::services::subgraph;
use apollo_router::services::supergraph;
use biscuit::macros::authorizer;
use biscuit_auth as biscuit;
use schemars::JsonSchema;
use serde::Deserialize;
use tower::BoxError;
use tower::ServiceBuilder;
use tower::ServiceExt;

use std::error::Error;
use std::ops::ControlFlow;

#[derive(Debug)]
struct Biscuit {
    #[allow(dead_code)]
    configuration: Conf,
    root: biscuit::PublicKey,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
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
        Ok(Biscuit {
            configuration: init.config,
            root,
        })
    }

    fn supergraph_service(&self, service: supergraph::BoxService) -> supergraph::BoxService {
        ServiceBuilder::new()
            .checkpoint(|request: supergraph::Request| {
                // Do some async call here to auth, and decide if to continue or not.

                Ok(ControlFlow::Continue(request))
            })
            .buffered()
            .service(service)
            .boxed()
    }

    fn subgraph_service(
        &self,
        _subgraph_name: &str,
        service: subgraph::BoxService,
    ) -> subgraph::BoxService {
        ServiceBuilder::new()
            .map_request(|request: subgraph::Request| request)
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
    // a verifier can come with allow/deny policies. While checks are all tested
    // and must all succeeed, allow/deny policies are tried one by one in order,
    // and we stop verification on the first that matches
    //
    // here we will check that the token has the corresponding right
    allow if right("/a/file1.txt", "read");
    // explicit catch-all deny. here it is not necessary: if no policy
    // matches, a default deny applies
    deny if true;
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

        let opt_token_str = match request.supergraph_request.headers().get("Authorization") {
            None => None,
            Some(value) => {
                let value = value.to_str()?;
                if !value.starts_with("Bearer ") {
                    return Err(Box::<dyn Error + Send + Sync>::from("not a bearer token"));
                }
                Some(&value[6..])
            }
        };
        let opt_token = match opt_token_str {
            None => None,
            Some(s) => Some(biscuit::Biscuit::from_base64(s, &self.root)?),
        };

        if let Some(token) = opt_token.as_ref() {
            authorizer.add_token(token)?;
        }

        authorizer.authorize()?;

        if let Some(token) = opt_token_str {
            request.context.insert("biscuit", token.to_string())?;
        }

        Ok(())
    }
}

// This macro allows us to use it in our plugin registry!
// register_plugin takes a group name, and a plugin name.
register_plugin!("biscuit_1", "biscuit", Biscuit);

#[cfg(test)]
mod tests {
    use apollo_router::services::supergraph;
    use apollo_router::TestHarness;
    use tower::BoxError;
    use tower::ServiceExt;

    #[tokio::test]
    async fn basic_test() -> Result<(), BoxError> {
        let test_harness = TestHarness::builder()
            .configuration_json(serde_json::json!({
                "plugins": {
                    "biscuit_1.biscuit": {
                        "message" : "Starting my plugin"
                    }
                }
            }))
            .unwrap()
            .build()
            .await
            .unwrap();
        let request = supergraph::Request::canned_builder().build().unwrap();
        let mut streamed_response = test_harness.oneshot(request).await?;

        let first_response = streamed_response
            .next_response()
            .await
            .expect("couldn't get primary response");

        assert!(first_response.data.is_some());

        println!("first response: {:?}", first_response);
        let next = streamed_response.next_response().await;
        println!("next response: {:?}", next);

        // You could keep calling .next_response() until it yields None if you're expexting more parts.
        assert!(next.is_none());
        Ok(())
    }
}
