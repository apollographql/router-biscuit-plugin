use apollo_router::layers::ServiceBuilderExt;
use apollo_router::plugin::Plugin;
use apollo_router::plugin::PluginInit;
use apollo_router::register_plugin;
use apollo_router::services::subgraph;
use apollo_router::services::supergraph;
use schemars::JsonSchema;
use serde::Deserialize;
use std::ops::ControlFlow;
use tower::BoxError;
use tower::ServiceBuilder;
use tower::ServiceExt;

use biscuit_auth as biscuit;

#[derive(Debug)]
struct Biscuit {
    #[allow(dead_code)]
    configuration: Conf,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
struct Conf {
    // Put your plugin configuration here. It will automatically be deserialized from JSON.
    // Always put some sort of config here, even if it is just a bool to say that the plugin is enabled,
    // otherwise the yaml to enable the plugin will be confusing.
    message: String,
}
// This plugin is a skeleton for doing authentication that requires a remote call.
#[async_trait::async_trait]
impl Plugin for Biscuit {
    type Config = Conf;

    async fn new(init: PluginInit<Self::Config>) -> Result<Self, BoxError> {
        tracing::info!("{}", init.config.message);
        Ok(Biscuit {
            configuration: init.config,
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

fn validate_request(request: &mut supergraph::Request) -> Result<(), String> {
    todo!()
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
