# Future Improvements

This document tracks work and open questions that are deliberately deferred past the first release. Nothing here is part of the current spec. Each entry states what is open, why it is deferred, and what would trigger revisiting it.

## Planned Improvements

### Rate Limiting

Rate limiting is expected to exist, but is not part of the first release. The mechanism (gateway middleware vs. in-service, token bucket vs. sliding window, etc.) is an infrastructure choice and is not prescribed.

What should be limited is fixed by the API's identity model:

- **Creation** is limited per creator client, not per source IP. A shared NAT/proxy in front of a legitimate creator would otherwise be throttled as a single caller, while IP-based limiting alone is easily routed around by an attacker.
- **Resolution** is limited per audience client at minimum, since that is the authenticated identity making the call. Limiting per `linkId` on top of that is worth considering: repeated resolve attempts against one link are the closest thing this API has to a guessing/enumeration surface (see [API Contract § Notes](api_v1.md#notes) on `linkId` entropy).
- Both are counted regardless of whether the request succeeds or fails (401/404 included). Otherwise a caller can probe freely as long as every guess is wrong.

### Audit event stream with outbox

Audit events are stored in an append-only table, written in the same transaction as the state change (see [Architecture § Audit](architecture.md#audit)). This costs an extra insert on every successful resolve, the hottest write path, and keeps audit data in the link database.

Moving to an event stream (log pipeline or message bus) would take that load off the link database and let other systems consume audit events. To keep the guarantee that a committed action always has its audit record, the move would use a transactional outbox: the event is still written in the same transaction, to an outbox table, and a relay publishes it to the stream afterwards.

Revisit if audit writes become a measured cost on resolve, or another system needs to consume audit events.

### Observability

Metrics, tracing, and request-level logging (including rejected requests, with caller identity and error reason). Not part of the first release, which focuses on the API and data model. Several decisions already lean on it: failed resolution attempts are deliberately not audited or counted per link (see below), on the assumption that request logs will cover them.

### Shared client SDK

A thin client library for integrators that applies consistent timeouts, exponential backoff with jitter, circuit breaking, and retries on resolve only with an `Idempotency-Key`. Without it, inconsistent retry behavior across independently-built integrations can turn a slow service into one that is fully down. Deferred until the API and data model are proven; most valuable once there are enough integrators for inconsistent retry behavior to matter.

## Open Decisions

### Deeplink broker / public redirect endpoint

Whether to introduce a public-facing redirect surface (e.g. `redirect.mydomain.com/{linkId}`) that resolves the target URL dynamically from the live `ScopeMapping` at click time, instead of the current model where the target URL is resolved once at creation and embedded directly by the creator. This would let long-lived links stay accurate as mappings change, but requires a new, largely unauthenticated public surface with its own risks (open redirect, bot/abuse exposure, no client-credential gate) that the current "does not redirect users" scope explicitly avoids.

Revisit if long-lived links (up to the one-year TTL cap) in practice suffer from stale target URLs.

### Creator-side authorization scoping

Whether to restrict which creator clients may create links for a given audience/scope/platform, rather than any authenticated creator being able to target any existing mapping. Today there is no ownership tie between a `Client` and the `ScopeMapping`s it may use, so nothing distinguishes "not allowed to use this mapping" from "mapping doesn't exist". A forbidden mapping would either be reported as `MAPPING_NOT_FOUND` (hiding that the mapping exists from creators not allowed to use it), or get its own `403` code for the authorized-but-forbidden case.

Revisit if creators ever need to be restricted to a subset of audiences.

### Native (public) mobile clients

Whether to support a mobile app itself as the client of record, without a backend intermediary (e.g. via a PKCE-style public-client flow or a backend-for-frontend token exchange). Not supported today, since every mobile integration so far has its own backend to act as the confidential client. See [Implementation Notes](implementation_notes.md#mobile-native-clients).

Revisit only if a mobile-only integration (no backend) actually shows up.

### Tracking failed resolution attempts per link

Whether `DeepLink` should carry an `attemptedUseCount` (or similar) alongside `useCount`, incremented on every resolve call regardless of outcome, as an analysis signal (e.g. links with unusual failed-attempt volume). Not added now:

- Resolve already requires valid audience-client credentials before any per-link logic runs, so the population able to generate such attempts on a given link is already narrow.
- Request logging (see [Observability](#observability)) would capture the same information with more context (caller identity, error reason, timing) than a bare counter.
- It would add a write to the highest-volume table on every failed attempt, real cost for an analytics-only field.

Revisit if a concrete analysis need appears that log aggregation doesn't serve. The column is additive and safe to introduce later.

## Related Documents

- [Architecture](architecture.md)
- [Data Model](data_model.md)
- [Implementation Notes](implementation_notes.md)
