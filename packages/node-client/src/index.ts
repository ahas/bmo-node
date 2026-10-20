// The node service's client (docs/api.md, "The clients and the harness plugin"): the types openapi-typescript
// generates from docs/openapi.json, and openapi-fetch's typed calls over them, each route by its path.

import createClient, { type Client } from "openapi-fetch";

import type { components, paths } from "./schema.ts";

export type { components, operations, paths } from "./schema.ts";

type Schemas = components["schemas"];

export type Node = Schemas["Node"];
export type NodePage = Schemas["NodePage"];
export type Target = Schemas["Target"];
export type NodeCheck = Schemas["NodeCheck"];
export type Hub = Schemas["Hub"];
export type HubPage = Schemas["HubPage"];
export type Machine = Schemas["Machine"];
export type MachinePage = Schemas["MachinePage"];
export type Reading = Schemas["Reading"];
export type ReadingPage = Schemas["ReadingPage"];
export type Event = Schemas["Event"];
export type EventPage = Schemas["EventPage"];
export type EventKind = Schemas["EventKind"];
export type Health = Schemas["Health"];
export type WireError = Schemas["Error"];

export type NodeClient = Client<paths>;

/** The node service at `baseUrl`, asked with `token` on every call. */
export function createNodeClient(baseUrl: string, token: string, fetcher?: typeof fetch): NodeClient {
  return createClient<paths>({
    baseUrl,
    headers: { Authorization: `Bearer ${token}` },
    ...(fetcher ? { fetch: fetcher } : {}),
  });
}
