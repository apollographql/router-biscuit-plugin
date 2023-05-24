# Experimental Biscuit router plugin

⚠️  *This is experimental work and not a supported product* ⚠️

This router plugin tests authorization with [Biscuit tokens](https://www.biscuitsec.org/), which support
public key signatures and offline attenuatin, along with a Datalog based authorization language.

The goal here is to explore authorization patterns.

## Setup

### Install the biscuit CLI

Install the biscuit CLI with `cargo install biscuit-cli` or download the [latest release](https://github.com/biscuit-auth/biscuit-cli/releases).

### Generate the root key pair

This will be used to mint and verify tokens.

```shell
$ biscuit keypair
Generating a new random keypair
Private key: d6f6ba4981352d4d1c23693d04063b956a0d7d7330f5873ffce3df581449d18b
Public key: 36ba0f350d7605e4e4f724f108594cf7ddf55037728d5735cbb9b58366801170
```

Then create the router configuration file with the public key:

```yaml
plugins:
  biscuit.auth:
    public_root: "36ba0f350d7605e4e4f724f108594cf7ddf55037728d5735cbb9b58366801170"
```

### Add the authorization policies to the router

The plugin automatically adds the following facts that you can use in token checks:
- `query("root_operation_name")` or `mutation("root_operation_name")`
- `time(2022-09-27T12:00:00Z)`

The policies are added as follows:

```yaml
plugins:
  biscuit.auth:
    public_root: "36ba0f350d7605e4e4f724f108594cf7ddf55037728d5735cbb9b58366801170"
    code: authorizer.datalog
```

And the `authorizer.datalog` file:

```datalog
// allow introspection
allow if query("__schema");

allow if user($id);
// only the test root operation is available as unauthenticated user
deny if query($op), !($op == "test");
allow if true;
```

Here we check for the presence of a `user` fact, that would be provided by the token,
and if that policy does not match, we then try the next one. That next policy will
reject unauthenticated queries using any root operation other than `test`.

### Create a token

Using the root private key we generated previously, we will now create a token:

```shell
$ biscuit generate --private-key d6f6ba4981352d4d1c23693d04063b956a0d7d7330f5873ffce3df581449d18b -
Please input a datalog program, followed by <enter> and ^D
user(1);
EnYKDBgDIggKBggKEgIQARIkCAASIHOELxeY4pdj9OjG6aR6yBXdHianEYnvn7vfuItTXGRnGkCkIiSFZE0x9H7aPdvmxxDn5UtTw3OwnMslChtMW5Khp2of9PSy40ICB2MsBNl2DGIZYFK0jZXJe8PG1VJjq5oHIiIKIIn_0tlCe_UxNaoEW-L6CMP_zWkXWCvj1aiwx9BdbT90
$ biscuit generate --private-key d6f6ba4981352d4d1c23693d04063b956a0d7d7330f5873ffce3df581449d18b - > token.bc
Please input a datalog program, followed by <enter> and ^D
user(1);
```

You can verify the token and print its content like this:

```shell
$ biscuit inspect --public-key "36ba0f350d7605e4e4f724f108594cf7ddf55037728d5735cbb9b58366801170" token.bc 
Authority block:
== Datalog ==
user(1);

== Revocation id ==
19aa449f385d3e0c0f518222ee192511d8e2f7c9e56cff69afd9549dd5c40fdef0c784a598b7e0241843d50019f3f3c27e7e3b02663eb9f90d9b85ceab2b440e

==========

✅ Public key check succeeded 🔑
🙈 Datalog check skipped 🛡️
```

### Start the router

We are using the [federation-demo](https://github.com/apollographql/federation-demo) for this test:

```shell
$ cargo run -- -s local.graphql -c router.yaml
    Finished dev [unoptimized + debuginfo] target(s) in 0.14s
     Running `target/debug/router -s local.graphql -c router.yaml`
2022-09-27T12:46:17.574756Z  INFO Apollo Router v1.0.0 // (c) Apollo Graph, Inc. // Licensed as ELv2 (https://go.apollo.dev/elv2)
2022-09-27T12:46:17.574806Z  INFO Anonymous usage data is gathered to inform Apollo product development.  See https://go.apollo.dev/o/privacy for more info.
2022-09-27T12:46:17.784225Z  INFO healthcheck endpoint exposed at http://127.0.0.1:8088/health
2022-09-27T12:46:17.784795Z  INFO GraphQL endpoint exposed at http://127.0.0.1:4000/ 🚀
```

### Unauthenticated query

If we try to do the following query:

```graphql
query {
  me {
    name
  }

  topProducts {
    name
  }
}
```

That can be tested from Studio or with curl as follows:

```shell
curl --request POST \
    --header 'content-type: application/json' \
    --url 'http://127.0.0.1:4000/' \
    --data '{"query":"query {\n  me {\n    name\n  }\n\n  topProducts {\n    name\n  }\n}","variables":{}}'
```

We get this result, as expected:

```json
{
  "errors": [
    {
      "message": "authorization failed"
    }
  ]
}
```

The plugin printed this message, telling us about the authorizer's state and which checks or policies failed:

```
authorizer result Err(FailedLogic(Unauthorized { policy: Deny(2), checks: [] })):
World {
  facts: {
    Origin {
        inner: {
            18446744073709551615,
        },
    }: [
        "query(\"topProducts\")",
        "time(2022-09-27T12:51:58Z)",
        "query(\"me\")",
    ],
}
  rules: {}
  checks: []
  policies: [
    "allow if query(\"__schema\")",
    "allow if user($id)",
    "deny if query($op), !($op == \"test\")",
    "allow if true",
]
}
```

So it's the third policy (zero indexed) that failed: `deny if query($op), !($op == "test")`

### Authenticated query

Now we will use the token we created, by passing it in the `Authorization` header, with the
value `Bearer EnYKDBgDIggKBggKEgIQARIkCAASIK8bnAXtqMr3ZGaahJiF2eWh0MMdWqLg3X9Ld0yEcIvOGkAZqkSfOF0-DA9RgiLuGSUR2OL3yeVs_2mv2VSd1cQP3vDHhKWYt-AkGEPVABnz88J-fjsCZj65-Q2bhc6rK0QOIiIKIMaYRKo660tMRrl2u3spCTDD4q9WXc9-vtS2-_0Rn7So`.

This translates to this curl request:

```shell
curl --request POST \
    --header 'Authorization: Bearer EnYKDBgDIggKBggKEgIQARIkCAASIK8bnAXtqMr3ZGaahJiF2eWh0MMdWqLg3X9Ld0yEcIvOGkAZqkSfOF0-DA9RgiLuGSUR2OL3yeVs_2mv2VSd1cQP3vDHhKWYt-AkGEPVABnz88J-fjsCZj65-Q2bhc6rK0QOIiIKIMaYRKo660tMRrl2u3spCTDD4q9WXc9-vtS2-_0Rn7So' \
    --header 'content-type: application/json' \
    --url 'http://127.0.0.1:4000/' \
    --data '{"query":"query {\n  me {\n    name\n  }\n\n  topProducts {\n    name\n  }\n}","variables":{}}'
```

and gives us this result:

```json
{
  "data": {
    "me": {
      "name": "Ada Lovelace"
    },
    "topProducts": [
      {
        "name": "Table"
      },
      {
        "name": "Couch"
      },
      {
        "name": "Chair"
      }
    ]
  }
}
```

Great, now we can authorize queries according to policies that valid the root operation.
But can we go further?

### Authenticated query with an attenuated token

One of the main features of Biscuit tokens is attenuation: from an existing token, it is
possible to create a new one with less rights, without even going through the service that
created the token. The main policies will still apply, but you can add as many restrictions
as you want.

So let's attenuate our token. We will add two restrictions to our token:
- we only accept the `me` root operation, `__schema` for introspection and `_entities` for federation
- we set an expiration date

```shell
$ biscuit attenuate --block 'check all query($op), ["__schema", "_entities", "me"].contains($op); check if time($time), $time < 2022-09-30T16:32:00Z'  token.bc > attenuated_token.bc
# inspecting it
$ biscuit inspect --public-key "36ba0f350d7605e4e4f724f108594cf7ddf55037728d5735cbb9b58366801170" attenuated_token.bc
Authority block:
== Datalog ==
user(1);

== Revocation id ==
19aa449f385d3e0c0f518222ee192511d8e2f7c9e56cff69afd9549dd5c40fdef0c784a598b7e0241843d50019f3f3c27e7e3b02663eb9f90d9b85ceab2b440e

==========

Block n°1:
== Datalog ==
check all query($op), ["__schema", "_entities", "me"].contains($op);
check if time($time), $time < 2022-09-30T16:32:00Z;

== Revocation id ==
3de9f2b4b056926539281b8a6d045f21962fe455290441790b3185f7d33eaad1fe26ddbadc951e9a85b2e1abd153359f96855790a6e930489240d4e29abb510e

==========

✅ Public key check succeeded 🔑
🙈 Datalog check skipped 🛡️
```

Now if we try to query `me` and `topProducts`:

```shell
curl --request POST \
    --header 'Authorization: Bearer EnYKDBgDIggKBggKEgIQARIkCAASIK8bnAXtqMr3ZGaahJiF2eWh0MMdWqLg3X9Ld0yEcIvOGkAZqkSfOF0-DA9RgiLuGSUR2OL3yeVs_2mv2VSd1cQP3vDHhKWYt-AkGEPVABnz88J-fjsCZj65-Q2bhc6rK0QOGugBCn4KAm9wCghfX3NjaGVtYQoJX2VudGl0aWVzCgJtZRgDMjUKMQoCCBsSBwgbEgMIgAgaIgoTChE6DwoDGIEICgMYgggKAxiDCAoFCgMIgAgKBBoCCAUQATImCiQKAggbEgYIBRICCAUaFgoECgIIBQoICgYggLTcmQYKBBoCCAASJAgAEiB3lMQWAKUvwsMc4XnwY-HdHQySrS9V1U40vHOspX9-EBpAPenytLBWkmU5KBuKbQRfIZYv5FUpBEF5CzGF99M-qtH-Jt263JUemoWy4avRUzWfloVXkKbpMEiSQNTimrtRDiIiCiBO8Eb6ZM5-qi9NVJxZ20xl3MeskbRcLpIUtF6CWg1aKQ==' \
    --header 'content-type: application/json' \
    --url 'http://127.0.0.1:4000/' \
    --data '{"query":"query ExampleQuery {\n  me {\n    name\n  }\n\n  topProducts {\n    name\n  }\n}","variables":{}}'
```

We will get as expected:

```json
{
  "errors": [
    {
      "message": "authorization failed"
    }
  ]
}
```

And we see in the logs:

```
authorizer result Err(FailedLogic(Unauthorized { policy: Allow(1), checks: [Block(FailedBlockCheck { block_id: 1, check_id: 0, rule: "check all query($op), [\"me\", \"__schema\"].contains($op)" })] }))
```

While if we did a query only for `me`:

```shell
curl --request POST \
    --header 'Authorization: Bearer EnYKDBgDIggKBggKEgIQARIkCAASIK8bnAXtqMr3ZGaahJiF2eWh0MMdWqLg3X9Ld0yEcIvOGkAZqkSfOF0-DA9RgiLuGSUR2OL3yeVs_2mv2VSd1cQP3vDHhKWYt-AkGEPVABnz88J-fjsCZj65-Q2bhc6rK0QOGtgBCm4KAm9wCghfX3NjaGVtYQoCbWUYAzIwCiwKAggbEgcIGxIDCIAIGh0KDgoMOgoKAxiBCAoDGIIICgUKAwiACAoEGgIIBRABMiYKJAoCCBsSBggFEgIIBRoWCgQKAggFCggKBiCAtNyZBgoEGgIIABIkCAASIJe_J6Gx9s79Oxq9EmzQLA1ypWii1lUVpoSyJTCL8KQ-GkD_GLs5EXa21wcO7MXkJpczSLOmjgHB-XAZTN9dxx_KfpPHv83kNJuX5KWoLBHNVUvOHPze8jZzcuuspG782jwBIiIKIGeQ8Z2YMHvHafQSpQmGL-x1C5uLpGOr8HlZKN1Q-oj1' \
    --header 'content-type: application/json' \
    --url 'http://127.0.0.1:4000/' \
    --data '{"query":"query ExampleQuery {\n  me {\n    name\n  }\n}","variables":{}}'
```

The query succeeds and we receive:

```json
{
  "data": {
    "me": {
      "name": "Ada Lovelace"
    }
  }
}
```

### Attenuated queries to subgraphs

The router automatically attenuates the token before sending it to the subgraph,
by adding (here for the `products` subgraph) the check `check if subgraph("products)`.
It assumes that each subgraph will authorize the query, and provide the `subgraph` fact
to the authorizer.

This has two consequences:
- if the subgraph is compromised and the token stolen to query another subgraph, it won't work
because other subgraphs will have different names
- if the token is stolen, it cannot be used to query the router either, because the router's
authorizer does not provide the  `subgraph` fact

Let's try: we extracted the subgtraph sent to the user backend in `subgraph.bc`, if we inspect it,
we see:

```shell
Authority block:
== Datalog ==
user(1);

== Revocation id ==
19aa449f385d3e0c0f518222ee192511d8e2f7c9e56cff69afd9549dd5c40fdef0c784a598b7e0241843d50019f3f3c27e7e3b02663eb9f90d9b85ceab2b440e

==========

Block n°1:
== Datalog ==
check all query($op), ["__schema", "_entities", "me"].contains($op);
check if time($time), $time < 2022-09-30T16:32:00Z;

== Revocation id ==
3de9f2b4b056926539281b8a6d045f21962fe455290441790b3185f7d33eaad1fe26ddbadc951e9a85b2e1abd153359f96855790a6e930489240d4e29abb510e

==========

Block n°2:
== Datalog ==
check if subgraph("user");

== Revocation id ==
b1c8fbc5261d50e685b58a23c74e3bf660e3c66df3773e17b59169929af2285cab522e52fb99d531f7206c289ab1dcb29054f2a3bdd40b33da730b2b60b1da00

==========

✅ Public key check succeeded 🔑
🙈 Datalog check skipped 🛡️
```

The third block contains the check `check if subgraph("user")`. Now if we try to use itto query the router, we get the
"authorization failed" message, and in the router's logs:

```
authorizer result Err(FailedLogic(Unauthorized { policy: Allow(1), checks: [Block(FailedBlockCheck { block_id: 2, check_id: 0, rule: "check if subgraph(\"user\")" })] }))```
```

This check fails because the router does not provide the subgraph fact.

### Mixing authorization contexts: third party blocks

This authorization system puts a lot of trust in the token creator: they can mint
tokens with any access level possible to the subgraphs. This is concerning in the case
of supergraphs made from subgraphs coming from multiple origins. How much should the
subgraphs trust each other? How much should they trust the router operator?

With biscuit's third party blocks though, it is possible to assemble different security
domains in one token. We can make a token containing information coming from multiple
parties, and services will be able to require data from some of them to validate the
request.

Here, we will add an operation to the organizations subgraph, that can only be requested
if we were explicitely given the permission by the company managing that subgraph. Other
users will only be able to go through a federated query based on the `me` operation.

First, we will generate a new key pair. That one is controlled by the organizations subgraph.

```shell
$ biscuit keypair
Generating a new random keypair
Private key: 62359062f245505dc6c8f9a0a9c3fd42deb20faf779c763eca6eab9061fdcda0
Public key: b8a73872297bb052b3a8c9b64a23b127cdfc64ba30d9634c10de8644ee6be13f
```

Next, we will add the `allOrganizations` operation to the organizations subgraph, that returns
the entire list of organizations.

```graphql
type Query
  @join__type(graph: ORGA)
  @join__type(graph: USER)
{
  me: User! @join__field(graph: USER)
  allOrganizations: [Organization!]! @join__field(graph: ORGA)
}
```

We will also modify the subgraph's policies to filter access to that operation:

```datalog
subgraph("orga");

allow if user($id), query("_entity");
allow if query("allorganizations"), orga_service_admin(true) trusting ed25519/b8a73872297bb052b3a8c9b64a23b127cdfc64ba30d9634c10de8644ee6be13f;

allow if query("_service");
deny if true;
```

As before, we allow queries for users authenticated with a router token, but
only for the operations `_entity`, for federation. If an
operation is not in that list, we have another test: `allow if query("allOrganizations"), orga_service_admin(true) trusting ed25519/b8a73872297bb052b3a8c9b64a23b127cdfc64ba30d9634c10de8644ee6be13f`

This succeeds if the operation is `allOganizations` and there is a fact
`orga_service_admin(true)` that is provided either by the authorizer, or by
a third party block cryptographically signed by the key `b8a73872297bb052b3a8c9b64a23b127cdfc64ba30d9634c10de8644ee6be13f`
(`ed25519` is the name of the signature algorithm).

With this, if we perform the query with the first token:

```graphql
query {
  me {
    id
    name
  }

  allOrganizations {
    id
    members {
      id
    }
  }
}
```

we will get the response:

```json
{
  "data": {
    "me": {
      "id": "1",
      "name": "A"
    },
    "allOrganizations": null
  },
  "errors": [
    {
      "message": "authorization failed"
    }
  ]
}
```

So now, let's make a token that can request that operation.

To create that third party block, we will start from the initial token, and create
a request for a third party block, that we would "send" to the orga subgraph authority.

```shell
$ biscuit generate-request token.bc
CiQIABIgrxucBe2oyvdkZpqEmIXZ5aHQwx1aouDdf0t3TIRwi84=
```

Now we bring that request to the authority that controls the subgraph's key:

```shell
$ biscuit generate-third-party-block --private-key "62359062f245505dc6c8f9a0a9c3fd42deb20faf779c763eca6eab9061fdcda0" --block "orga_service_admin(true)" -
Please input a base64-encoded third-party block request, followed by <enter> and ^D
CiQIABIgrxucBe2oyvdkZpqEmIXZ5aHQwx1aouDdf0t3TIRwi84=
CiEKEm9yZ2Ffc2VydmljZV9hZG1pbhgEIgkKBwiACBICMAESaApA0ANXnfGWF24-7M9md9ryxKrBSOQg1IVjA4t0w2czauefS8vCIdEJHGpNYJHiOwUr6NhVvt_FB7Q4OM-idwvvAhIkCAASILinOHIpe7BSs6jJtkojsSfN_GS6MNljTBDehkTua-E_
```

We can now create a new token from the initial one and the third party block:

```shell
$ biscuit append-third-party-block --block-contents "CiEKEm9yZ2Ffc2VydmljZV9hZG1pbhgEIgkKBwiACBICMAESaApA0ANXnfGWF24-7M9md9ryxKrBSOQg1IVjA4t0w2czauefS8vCIdEJHGpNYJHiOwUr6NhVvt_FB7Q4OM-idwvvAhIkCAASILinOHIpe7BSs6jJtkojsSfN_GS6MNljTBDehkTua-E_" token.bc > token_3rd_party.bc
```

Inspecting it, we get:

```
Authority block:
== Datalog ==
user(1);

== Revocation id ==
19aa449f385d3e0c0f518222ee192511d8e2f7c9e56cff69afd9549dd5c40fdef0c784a598b7e0241843d50019f3f3c27e7e3b02663eb9f90d9b85ceab2b440e

==========

Block n°1, (third party, signed by b8a73872297bb052b3a8c9b64a23b127cdfc64ba30d9634c10de8644ee6be13f):
== Datalog ==
orga_service_admin(true);

== Revocation id ==
95a98478087f0178bf7a2bdbf94d95cdedb9cee3c3c444b16973b6a24372508208ddcd6038a73f7e085b15607f671248d03392c6554dfd66076c422f426a3d07

==========

🙈 Public key check skipped 🔑
🙈 Datalog check skipped 🛡️
```

Now, if we use this token with the query:

```graphql
query {
  me {
    id
    name
  }

  allOrganizations {
    id
    members {
      id
    }
  }
}
```

We will get the successful response:

```json
{
  "data": {
    "me": {
      "id": "1",
      "name": "A"
    },
    "allOrganizations": [
      {
        "id": "1",
        "members": [
          {
            "id": "1"
          },
          {
            "id": "2"
          }
        ]
      },
      {
        "id": "2",
        "members": [
          {
            "id": "1"
          },
          {
            "id": "2"
          }
        ]
      }
    ]
  }
}
```

## Experimentations

### Router level authorization on the request

The router verifies that the root operations are in a set of authorized operations.
The token provides the list of authorized operations as a fact containing a set.
If the token is attenuated to remove operations, this should be validated as well.

### Subgraph query attenuation

The router will take the client provided token, and send it to the subgraph so it can
authorize queries too. But what happens if a subgraph is compromised? Can they take
that token and query the router, or other subgraphs?

Using attenuation, the router mints a token that can only be used to query the subgraph,
and that would not be authorized when querying the router or another subgraph.

### Third party blocks

The upcoming [third party blocks](https://github.com/biscuit-auth/biscuit/issues/88) feature
will allow a token to bring in data signed by external keys, and authorizers to add expectations
on that data. This could solve an issue that we will see with supergraphs created with graphs
from various companies: the usual authorization systems would give too much power to the token
creator, because it could get full access to anything in the subgraphs.

With third party blocks, we could have a token that is assembled from a token created by the
router's authorization server, and then an aggregation of third party blocks for the various
subgraphs, obtained from each of the companies with the user's identity. That way they would
have fine grained way to reduces the rights of the user, independently of the router's
authorization server.

### Router level filtering on the response (not tested yet)

The token or authorizer policies provide fine grained rules on what kind of information
is available.

Examples:
- the `User` type contains a `private` field that is accessible
if the token provided user id matches the one of the `User` object
- the organization has a `private` field only accessible if the
 token provided user id matches a user that belongs to the organization
