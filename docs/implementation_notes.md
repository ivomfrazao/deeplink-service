# Implementation Notes

This document is not part of the formal spec. It records design reasoning, alternatives that were
considered and set aside, and problems already thought through — so a developer implementing this
service, who was not part of designing it, doesn't have to re-derive (or accidentally re-litigate)
decisions that already have a documented reason behind them.

Unlike [Architecture § Key Architectural Decisions](architecture.md#key-architectural-decisions),
which records decisions that are part of the contract other documents depend on, this file is a
looser log: background, rejected alternatives, and open worries that don't rise to the level of a
formal decision yet.

## Storage backend for link state

The service needs a durable store for `Client`, `ScopeMapping`, and `DeepLink` (see
[Data Model](data_model.md)). A relational store is assumed throughout the docs, but it's worth
recording why, since an in-memory/TTL-native store (e.g. Redis) is a natural first thought given
that a large share of links are short-TTL and single-use.

Redis-style TTL-keyed storage is a good fit for that common case, but a poor fit as the *only*
store once the model has to support what it currently supports:

- **The internal ops list API needs real queries.** Filtering by `creatorClientId`, `audience`,
  and a created-at range with cursor pagination (see [API Contract](api_v1.md)) is either
  hand-built secondary indexing on top of a KV store, or it's what a relational store already does.
- **Post-expiry debuggability.** Ops needs to be able to look up a link's status after it's already
  expired or been used up. A hard TTL-evicted key has destroyed that evidence; a relational store
  can mark a row expired/exhausted instead of deleting it.

Conclusion: keep a durable relational (or relational-equivalent) store as the system of record.
An in-memory store may still be worth introducing later, but as a cache layer in front of the
system of record — for example, caching client records to avoid a database lookup on every
authenticated request (see
[Architecture § Availability](architecture.md#credential-verification-cost)) — not as a
replacement for it. Don't build that caching layer preemptively; add it only once request volume
actually makes client lookup a measured bottleneck.

The one piece of link-related state that *is* a good fit for a TTL-native store on its own merits,
rather than as an optimization: the ``Idempotency-Key`` cache for resolve (see
[API Contract](api_v1.md)). Unlike link records themselves, an idempotency cache entry is
genuinely short-lived by design (a bounded replay window, typically hours, not the lifetime of a
link), never needs ops querying, and its entire purpose is to disappear once the window passes —
that's a TTL-keyed store's native use case, not an exception to the reasoning above.

A stored idempotency outcome contains the link's ``userContext``, so that store holds the same
data as `DeepLinkContext`, for the length of the replay window. It needs the same access controls,
and encryption at rest if `userContext` encryption is ever introduced.

## Mobile (native) clients

Platform `ANDROID`/`IOS` in [Data Model](data_model.md) describes the target platform being routed
to, not that the mobile app itself holds client credentials. A native app embedding a
`client_secret` in its shipped binary isn't holding a secret at all — it's extractable. For the
current scope, the audience/creator client of record for a mobile integration must be that app's
own backend; the mobile app itself only handles the deep-link URI and hands off to its backend.

Native/public clients (no confidential backend involved) are explicitly out of scope for now — see
[Future Improvements](future_improvements.md#native-public-mobile-clients) for the tracked follow-up, and
note that it may never become a requirement if every integrating mobile app continues to have a
backend of its own.

## JWT and JWKS validation hygiene

Standard JWT-validator and JWKS-client correctness, not a decision specific to this service's
shape — recorded here so it isn't dropped, since it still needs to be built correctly. It applies to both user and operator tokens:

- Only an explicitly allowed set of signing algorithms is accepted (e.g. RS256/ES256). `alg: none`
  is always rejected, and a token's `alg` must match the key type actually found in JWKS for its
  `kid` — a token isn't accepted just because *some* algorithm validates it, to close
  algorithm-confusion attacks (e.g. an RSA public key being reinterpreted as an HMAC secret).
- `exp` is mandatory and enforced; `nbf`/`iat` are enforced when present; `iss` must match the
  configured IdP issuer.
- A small clock-skew tolerance (on the order of tens of seconds) is applied when comparing
  `exp`/`nbf` against current time, to absorb ordinary drift between the IdP and this service
  without meaningfully extending a token's real lifetime.
- Keys are cached with a TTL and refreshed in the background, not fetched on every request.
- An unrecognized `kid` (not in the current cache) triggers at most one on-demand refresh, not a
  refresh per request — otherwise a burst of tokens signed with a newly rotated key would cause a
  thundering herd of JWKS fetches against the IdP.

`aud` enforcement is *not* in this list — unlike the above, it's a decision specific to this
service's threat model, not generic hygiene, and lives in
[Architecture § Token Validation](architecture.md#token-validation). Likewise, the behavior when
the IdP is unreachable during a key refresh (serve stale keys for a bounded grace period rather
than failing closed) is a real architectural decision — see
[Architecture § Availability and Blast Radius](architecture.md#availability-and-blast-radius).

## Related Documents

- [Architecture](architecture.md)
- [Data Model](data_model.md)
- [Future Improvements](future_improvements.md)
