# Biscuit router plugin

This router plugin tests authorization with [Biscuit tokens](https://www.biscuitsec.org/), which support
public key signatures and offline attenuatin, along with a Datalog based authorization language.

The goal here is to explore authorization patterns.

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

### Router level filtering on the response

The token or authorizer policies provide fine grained rules on what kind of information
is available.

Examples:
- the `User` type contains a `private` field that is accessible
if the token provided user id matches the one of the `User` object
- the organization has a `private` field only accessible if the
 token provided user id matches a user that belongs to the organization
