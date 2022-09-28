#!/bin/sh
rover subgraph introspect http://localhost:4001/ > user.graphql
rover subgraph introspect http://localhost:4002/ > orga.graphql
