# API Contract

## Version 1 (v1)

Public base path: ```/api/v1```. Internal base path: ```/internal/v1``` (see [Internal API (Ops)](#internal-api-ops)).

### Authorization model

#### Client identity

```Authorization: Basic <base64(client_id:client_secret)>```

``client_id`` is the client's UUID (``Client.id``), not its audience. ``client_secret`` is issued by the service when the client is created or its secret is rotated (see [Clients](#clients)). Basic Auth is only accepted over TLS.

Used on:

- ```POST /links``` → creator client
- ```POST /links/{id}/resolve``` → audience client
- ```GET /links/{id}/status``` → creator or audience
- ```DELETE /links/{id}``` → creator or audience

#### User identity (optional)

```X-User-Authorization: Bearer <JWT access token with sub claim>```

- If present on creation → link becomes private, bound to the token's ``sub``
- On resolution of a private link → required, and its ``sub`` must match the link's
- On resolution of a public link → ignored, not validated

The token's ``aud`` must include the Deeplink Service (see [Architecture § Token Validation](architecture.md#token-validation)).

#### Operator identity (internal API only)

```Authorization: Bearer <operator's IdP access token>```

The token's ``aud`` must include the Deeplink Service, and it must carry the configured operator role claim. There is no shared service token.

### Error responses

Every error response uses the Problem Details format ([RFC 9457](https://www.rfc-editor.org/rfc/rfc9457), ``Content-Type: application/problem+json``) with an extra ``code`` member that clients can branch on:

```json
{
    "status": 400,
    "code": "MAPPING_NOT_FOUND",
    "title": "No scope mapping for this audience, scope and platform",
    "detail": "No mapping exists for (billing-service, onboarding:continue, IOS)",
    "field": "scope"
}
```

- ``status`` and ``code`` are always present. ``code`` is the stable contract; ``title`` and ``detail`` are for humans and may change.
- ``field`` is present when one request field caused the error.
- ``detail`` never reveals more than the status code already does. In particular, every 404 on a link uses the single code ``LINK_NOT_FOUND``, whatever the real reason (see [Resolve error cases](#resolve-error-cases)).

| Code | Status | Meaning |
| --- | --- | --- |
| ``MALFORMED_REQUEST`` | 400 | Body or query can't be parsed, or a required field is missing |
| ``INVALID_FIELD`` | 400 | A field is out of range or invalid, or an immutable field was sent on update |
| ``MAPPING_NOT_FOUND`` | 400 | No scope mapping for the requested (audience, scope, platform) |
| ``UNKNOWN_AUDIENCE`` | 400 | Internal API: the audience doesn't belong to an active client |
| ``INVALID_CLIENT_CREDENTIALS`` | 401 | Basic Auth missing or wrong, or the client is deactivated |
| ``USER_TOKEN_REQUIRED`` | 401 | Private link resolved without a user token |
| ``INVALID_USER_TOKEN`` | 401 | User token invalid, expired, or with the wrong ``aud`` |
| ``INVALID_OPERATOR_TOKEN`` | 401 | Internal API: operator token missing or invalid |
| ``USER_MISMATCH`` | 403 | Private link: valid user token, but for a different user |
| ``OPERATOR_ROLE_REQUIRED`` | 403 | Internal API: valid token without the operator role claim |
| ``LINK_NOT_FOUND`` | 404 | Link unknown, not the caller's, or no longer usable (indistinguishable by design) |
| ``NOT_FOUND`` | 404 | Internal API: client or scope mapping not found |
| ``CONFLICT`` | 409 | Internal API: audience or mapping tuple already exists |
| ``USER_CONTEXT_TOO_LARGE`` | 413 | ``userContext`` exceeds the size limit |
| ``IDEMPOTENCY_KEY_REUSED`` | 422 | ``Idempotency-Key`` already used for a different link |

### Create Deeplink

```POST /api/v1/links```

#### Create request headers

- ```Authorization: Basic <creator_client_id:secret>```
- ```X-User-Authorization: Bearer <user_access_token>``` (optional)

#### Request body (JSON)

```json
{
    "audience": "billing-service",
    "scope": "onboarding:continue",
    "platform": "WEB",
    "ttlSeconds": 3600,
    "maxUses": 1,
    "userContext": {
        "customerId": "12345",
        "foo": "bar"
    }
}
```

| Field | Required | Default | Limits |
| --- | --- | --- | --- |
| ``audience``, ``scope``, ``platform`` | Yes | | Must match an existing scope mapping |
| ``ttlSeconds`` | No | 600 (10 minutes) | 1 to 31,536,000 (one year) |
| ``maxUses`` | No | 1 (single-use) | 1 to 10 |
| ``userContext`` | No | none | JSON object, at most 16 KiB serialized |

When ``userContext`` is omitted the link still works for routing, TTL, and usage control; it just has nothing to hand off, and the resolve response omits the field.

#### Create response (JSON)

```201 Created```

```json
{
    "linkId": "018f2f9a-7b3c-7000-8a2e-1c9d4e5f6a7b",
    "url": "https://billing.example.com/onboarding?deeplinkId=018f2f9a-7b3c-7000-8a2e-1c9d4e5f6a7b&private=true",
    "expiresAt": "2026-04-28T16:00:00Z"
}
```

``url`` is built from the mapping's ``baseTargetUrl`` as described in [Architecture § Target URL Construction](architecture.md#target-url-construction), by appending two query parameters:

- ``deeplinkId``: the link id
- ``private``: ``true`` or ``false``, a UX hint for the audience app (not a security control)

Both names are reserved and can't appear in a ``baseTargetUrl``.

##### Create error cases

- ```400``` ``MALFORMED_REQUEST`` — malformed request
- ```400``` ``INVALID_FIELD`` — ``ttlSeconds`` or ``maxUses`` out of range
- ```400``` ``MAPPING_NOT_FOUND`` — no mapping for (audience, scope, platform)
- ```401``` ``INVALID_CLIENT_CREDENTIALS`` — invalid creator credentials
- ```401``` ``INVALID_USER_TOKEN`` — invalid user token
- ```413``` ``USER_CONTEXT_TOO_LARGE`` — ``userContext`` exceeds the size limit

A missing mapping is a 400, not a 404: the endpoint exists, and the problem is a value in the request body. The ``MAPPING_NOT_FOUND`` code tells the creator exactly which part of the request to fix.

### Resolve Deeplink

```POST /api/v1/links/{id}/resolve```

#### Resolve request headers

- ```Authorization: Basic <audience_client_id:secret>```
- ```X-User-Authorization: Bearer <user_access_token>``` (required if link is private; ignored otherwise)
- ```Idempotency-Key: <client-generated UUID>``` (optional, strongly recommended)

Resolution mutates state (see Notes: it's a ``POST``, not a ``GET``, precisely because of this), so a caller that times out waiting for a response can't safely retry without ``Idempotency-Key``. A lost response after a successful server-side mutation is indistinguishable from a failed one, and a blind retry on a ``maxUses: 1`` link would return 404 (exhausted) even though the caller's own first attempt succeeded.

When the header is present, the first outcome is stored for the idempotency window, bound to the audience client, the link id, and (for private links) the user ``sub``. A replay with the same key:

- for a different link id → ``422``
- for a private link → still requires a valid user token for the bound ``sub``, otherwise ``401``/``403`` as usual
- otherwise → the stored outcome, without consuming another use, even if the link has since become exhausted, expired, or revoked

#### Resolve response (JSON)

```json
{
    "userContext": {
        "customerId": "12345",
        "foo": "bar"
    }
}
```

##### Resolve error cases

Checks run in this order, and the first failure decides the response (see [Architecture § Resolution](architecture.md#resolution)):

1. ```401``` ``INVALID_CLIENT_CREDENTIALS`` — invalid audience credentials
1. ```422``` ``IDEMPOTENCY_KEY_REUSED`` — ``Idempotency-Key`` reused for a different link
1. ```404``` ``LINK_NOT_FOUND`` — link not found
1. ```404``` ``LINK_NOT_FOUND`` — audience mismatch
1. ```404``` ``LINK_NOT_FOUND`` — expired, exhausted (usage count reached ``maxUses``), or revoked (not checked on an idempotent replay)
1. ```401``` ``USER_TOKEN_REQUIRED`` / ``INVALID_USER_TOKEN`` — private link, user token missing or invalid
1. ```403``` ``USER_MISMATCH`` — private link, user token valid but for a different user

The 404s prevent information leakage: a caller can't tell a missing link from one that isn't theirs or is no longer usable.

The private link errors are only reachable for a usable link addressed to the caller's own audience. Splitting them into 401 and 403 lets the audience app tell "you need to provide a user token" from "the provided user token is valid but not the right one", and choose the UX accordingly (prompt for login vs. show "you don't have access to this link").

### Link Status

```GET /api/v1/links/{id}/status```

#### Status request headers

- ```Authorization: Basic <client_id:secret>```

#### Status response (JSON)

```json
{
    "linkId": "018f2f9a-7b3c-7000-8a2e-1c9d4e5f6a7b",
    "creatorClientId": "4b0f6d2e-9c1a-4f7e-8a51-2d3c4b5a6f70",
    "audience": "billing-service",
    "scope": "onboarding:continue",
    "platform": "WEB",
    "private": true,
    "maxUses": 1,
    "useCount": 0,
    "lastUsedAt": null,
    "expiresAt": "2026-04-28T16:00:00Z",
    "revokedAt": null,
    "createdAt": "2026-04-28T15:00:00Z"
}
```

##### Status error cases

- ```401``` ``INVALID_CLIENT_CREDENTIALS`` — invalid client credentials
- ```404``` ``LINK_NOT_FOUND`` — link not found, or found but this client is neither the link's creator nor its audience client (the two cases are indistinguishable by design)

### Revoke Deeplink

```DELETE /api/v1/links/{id}```

Soft-revokes the link (sets ``revokedAt`` and ``revokedBy``); the row itself is retained. See [Architecture § Revocation](architecture.md#revocation).

#### Revoke request headers

- ```Authorization: Basic <client_id:secret>``` — must be the link's creator or audience client

#### Revoke response

- ```204 No Content``` — revoked, or already inactive (revoked/expired/exhausted), in which case nothing changes

##### Revoke error cases

- ```401``` ``INVALID_CLIENT_CREDENTIALS`` — invalid client credentials
- ```404``` ``LINK_NOT_FOUND`` — link not found, or found but this client is neither the link's creator nor its audience client (same ownership rule as Status)

## Internal API (Ops)

Base path: ```/internal/v1```. Not exposed to the internet: reachable only from the internal network (private ingress/VPC), enforced at the network level.

Every internal endpoint requires an operator token (see [Operator identity](#operator-identity-internal-api-only)) and shares these error cases:

- ```401``` ``INVALID_OPERATOR_TOKEN`` — missing, invalid, or expired operator token
- ```403``` ``OPERATOR_ROLE_REQUIRED`` — valid token without the operator role claim

Every state-changing internal call is recorded as an audit event attributed to ``operator:<sub>``.

### Links

#### Get link status

```GET /internal/v1/links/{id}/status```

Unlike the public status endpoint, this is not scoped to the link's creator or audience client. It returns any link's status regardless of ownership, for ops/support lookups (e.g. debugging a customer-reported issue for a link the on-call engineer didn't create and isn't the audience for).

```json
{
    "linkId": "018f2f9a-7b3c-7000-8a2e-1c9d4e5f6a7b",
    "creatorClientId": "4b0f6d2e-9c1a-4f7e-8a51-2d3c4b5a6f70",
    "audience": "billing-service",
    "scope": "onboarding:continue",
    "platform": "WEB",
    "targetUrl": "https://billing.example.com/onboarding?deeplinkId=018f2f9a-7b3c-7000-8a2e-1c9d4e5f6a7b&private=true",
    "private": true,
    "maxUses": 1,
    "useCount": 0,
    "lastUsedAt": null,
    "expiresAt": "2026-04-28T16:00:00Z",
    "revokedAt": null,
    "revokedBy": null,
    "createdAt": "2026-04-28T15:00:00Z"
}
```

Includes ``targetUrl`` and ``revokedBy`` on top of the public status fields, since ops debugging benefits from seeing exactly where a link routes and who revoked it. ``userContext`` is deliberately left out even here: it's payload data the creator and audience exchange, and ops doesn't need its contents to debug routing, TTL, or usage-limit issues.

Errors: ```404``` ``LINK_NOT_FOUND`` — link not found.

#### Revoke link

```DELETE /internal/v1/links/{id}```

Same soft-revoke as the public revoke endpoint, but without the creator/audience ownership check, for ops-driven takedown (e.g. a reported-abuse link neither party will revoke themselves).

- ```204 No Content``` — revoked, or already inactive
- ```404``` ``LINK_NOT_FOUND`` — link not found

#### List links

```GET /internal/v1/links```

Cursor-based pagination (see Notes for why cursor over offset).

Query parameters:

- ```limit``` — page size (optional, capped at a server-defined max)
- ```cursor``` — opaque cursor from a previous response's ``nextCursor``; omit for the first page
- ```creatorClientId``` — filter to links created by this client (optional)
- ```audience``` — filter to links routed to this audience (optional)
- ```createdAfter``` / ```createdBefore``` — filter to links created within this time range, ISO 8601 (optional, either or both may be set)

```json
{
    "items": [
        {
            "linkId": "018f2f9a-7b3c-7000-8a2e-1c9d4e5f6a7b",
            "creatorClientId": "4b0f6d2e-9c1a-4f7e-8a51-2d3c4b5a6f70",
            "audience": "billing-service",
            "scope": "onboarding:continue",
            "platform": "WEB",
            "private": true,
            "maxUses": 1,
            "useCount": 0,
            "lastUsedAt": null,
            "expiresAt": "2026-04-28T16:00:00Z",
            "revokedAt": null,
            "createdAt": "2026-04-28T15:00:00Z"
        }
    ],
    "nextCursor": "01926f1e-0000-7000-8000-000000000000"
}
```

``nextCursor`` is null/omitted on the last page. Items are ordered by ``id`` ascending (creation order, since ``id`` is a UUID v7).

Errors: ```400``` ``MALFORMED_REQUEST`` — malformed ``cursor``, ``limit``, or date filter.

### Clients

#### Client representation

```json
{
    "id": "4b0f6d2e-9c1a-4f7e-8a51-2d3c4b5a6f70",
    "audience": "onboarding-service",
    "name": "Onboarding Service",
    "active": true,
    "deactivatedAt": null,
    "createdAt": "2026-04-01T10:00:00Z",
    "createdBy": "operator:7d1c…",
    "updatedAt": "2026-04-01T10:00:00Z",
    "updatedBy": "operator:7d1c…"
}
```

#### Endpoints

| Method and path | Purpose | Body | Success |
| --- | --- | --- | --- |
| ```POST /internal/v1/clients``` | Create a client | ``audience``, ``name`` | ```201``` with the client and its ``clientSecret`` |
| ```GET /internal/v1/clients``` | List clients (cursor pagination, optional ``active`` filter) | | ```200``` |
| ```GET /internal/v1/clients/{id}``` | Get one client | | ```200``` |
| ```PATCH /internal/v1/clients/{id}``` | Update ``name`` | ``name`` | ```200``` |
| ```POST /internal/v1/clients/{id}/secret/rotate``` | Issue a new secret, keeping the current one valid for a grace period | ``gracePeriodSeconds`` (optional, server default) | ```200``` with ``clientSecret`` and ``previousSecretExpiresAt`` |
| ```POST /internal/v1/clients/{id}/deactivate``` | Deactivate the client | | ```204```, idempotent |

- ``id`` is generated by the service. ``audience`` is set at creation and can't be changed (see [Data Model § ScopeMapping Notes](data_model.md#scopemapping-notes) for the rotation pattern).
- Deactivation is one-way. A deactivated client fails authentication from then on.
- ``clientSecret`` is only ever returned in the create and rotate responses. It can't be retrieved later; a lost secret is replaced by rotating.
- Rotating while a previous secret is still in its grace period ends that grace period immediately: only the current and the new secret remain valid. Secret hashes never appear in any response.

Errors:

- ```400``` ``MALFORMED_REQUEST`` / ``INVALID_FIELD`` — malformed request, or an attempt to change ``audience``
- ```404``` ``NOT_FOUND`` — client not found
- ```409``` ``CONFLICT`` — ``audience`` already in use

### Scope Mappings

#### Scope mapping representation

```json
{
    "id": "9a3e5c7b-1d2f-4e6a-8b0c-3f5d7e9a1b2c",
    "audience": "billing-service",
    "scope": "onboarding:continue",
    "platform": "WEB",
    "baseTargetUrl": "https://billing.example.com/onboarding",
    "createdAt": "2026-04-01T10:00:00Z",
    "createdBy": "operator:7d1c…",
    "updatedAt": "2026-04-01T10:00:00Z",
    "updatedBy": "operator:7d1c…"
}
```

#### Endpoints

| Method and path | Purpose | Body | Success |
| --- | --- | --- | --- |
| ```POST /internal/v1/scope-mappings``` | Create a mapping | ``audience``, ``scope``, ``platform``, ``baseTargetUrl`` | ```201``` with the mapping |
| ```GET /internal/v1/scope-mappings``` | List mappings (cursor pagination, optional ``audience`` filter) | | ```200``` |
| ```GET /internal/v1/scope-mappings/{id}``` | Get one mapping | | ```200``` |
| ```PATCH /internal/v1/scope-mappings/{id}``` | Update ``baseTargetUrl`` | ``baseTargetUrl`` | ```200``` |
| ```DELETE /internal/v1/scope-mappings/{id}``` | Remove the mapping | | ```204``` |

- The ``(audience, scope, platform)`` tuple is immutable; only ``baseTargetUrl`` can change.
- Updating or removing a mapping doesn't affect links already created from it (see [Architecture § No automatic revocation on scope change](architecture.md#no-automatic-revocation-on-scope-change)).

Errors:

- ```400``` ``MALFORMED_REQUEST`` / ``INVALID_FIELD`` — malformed request, invalid ``baseTargetUrl``, ``baseTargetUrl`` using a reserved query parameter (``deeplinkId``, ``private``), or an attempt to change the tuple
- ```400``` ``UNKNOWN_AUDIENCE`` — (on create) ``audience`` doesn't belong to an active client
- ```404``` ``NOT_FOUND`` — mapping not found
- ```409``` ``CONFLICT`` — a mapping for this ``(audience, scope, platform)`` already exists

### Audit Events

```GET /internal/v1/audit-events```

Lists audit events (see [Architecture § Audit](architecture.md#audit)), oldest first, with cursor pagination like the links list. Reading audit events is not itself audited.

Query parameters:

- ```from``` / ```to``` — time window, ISO 8601 (**required**). The span may not exceed a server-configured maximum (default 7 days). Since event ids are UUID v7, the window is applied as an id range on the primary key, which keeps every query bounded (see [Data Model § AuditEvent Indexes](data_model.md#auditevent-indexes))
- ```resourceType``` and ```resourceId``` — filter to one resource (optional; ``resourceId`` requires ``resourceType``)
- ```actor``` — filter to one actor, e.g. ``operator:<sub>`` or ``client:<id>`` (optional)
- ```action``` — filter to one action, e.g. ``link.revoked`` (optional)
- ```limit``` — page size (optional, capped at a server-defined max)
- ```cursor``` — opaque cursor from a previous response's ``nextCursor``

Filters other than the time window are applied to the events inside it without an index, so a narrow window is faster. To see a link's history, start the window at the link's ``createdAt``: no event about a link can come before it.

```json
{
    "items": [
        {
            "id": "018f2f9a-7b3d-7000-9c1e-2a3b4c5d6e7f",
            "occurredAt": "2026-04-28T15:00:00Z",
            "action": "link.created",
            "actor": "client:4b0f6d2e-9c1a-4f7e-8a51-2d3c4b5a6f70",
            "resourceType": "deeplink",
            "resourceId": "018f2f9a-7b3c-7000-8a2e-1c9d4e5f6a7b",
            "requestId": "c0a8012e-5f1d-4b6e-9a7c-3e2d1f0b9a8c",
            "changes": null
        }
    ],
    "nextCursor": null
}
```

Errors:

- ```400``` ``MALFORMED_REQUEST`` — malformed ``cursor``, ``limit``, or date
- ```400``` ``INVALID_FIELD`` — ``from``/``to`` missing, ``from`` after ``to``, window over the maximum span, or ``resourceId`` without ``resourceType``

## Notes

- Resolution is a ``POST``, not a ``GET``, even though it only reads a link id from the path and returns data. Resolving a link mutates state — it increments ``useCount`` (and, with ``maxUses`` allowing multiple resolutions, repeated calls visibly produce different outcomes over time: success, success, ..., then exhausted). That makes it neither safe nor idempotent in the HTTP sense, so it shouldn't be exposed as a cacheable, freely-retryable GET — an intermediary cache or an HTTP client's automatic retry-on-failure behavior could otherwise return a stale response or silently consume an extra use. Resolution also always requires client credentials, so it was never reachable as a plain clickable link in a browser — there's no clickability benefit lost by moving off GET.
- ``GET /internal/v1/links`` uses cursor (keyset) pagination — ``WHERE id > cursor ORDER BY id LIMIT n`` — rather than offset/``LIMIT-OFFSET``. ``DeepLink`` is the highest-volume, insert-heavy table in the model (see [Data Model](data_model.md)), and offset pagination degrades linearly with page depth on a table that size — the database still has to scan and discard every skipped row even with an index. It's also unstable under concurrent inserts: since new links are constantly being written, a position-based offset can shift underneath an in-progress pagination, causing skipped or duplicated rows across pages. Cursor pagination avoids both, at the cost of not supporting "jump to page N" or a cheap total count — an acceptable tradeoff for an ops tool that's mostly scrolling recent or filtered results rather than jumping to arbitrary pages. The usual hard part of cursor pagination — picking a unique, stable, orderable cursor column and handling tie-breaks — is already solved here: ``id`` is a UUID v7, unique and monotonically time-ordered by construction, so no composite tie-breaking column is needed.
- ``linkId`` is the ``DeepLink.id`` UUID v7 itself, not a separate opaque token — see [Data Model § DeepLink Notes](data_model.md#deeplink-notes) for why, and for the security trade-off that comes with reusing it as the public identifier.
